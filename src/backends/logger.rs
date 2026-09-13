use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

static SINK: Mutex<Option<Box<dyn Write + Send>>> = Mutex::new(None);
static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
static BROKEN: AtomicBool = AtomicBool::new(false);

/// Initializes logging to the given file. Subsequent calls are no-ops.
/// Until this is called, `debug`/`info`/`error` are silent no-ops — the TUI
/// must never log to stdout/stderr, which would corrupt the alternate screen.
pub fn init(path: &Path) -> std::io::Result<()> {
    let mut sink = lock_sink();
    if sink.is_none() {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        *sink = Some(Box::new(file));
        drop(sink);
        let mut p = lock_path();
        *p = Some(path.to_path_buf());
    }
    Ok(())
}

/// Writes a `DEBUG`-level line; no-op when logging is not initialized.
pub fn debug(msg: &str) {
    write_line("DEBUG", msg);
}

/// Writes an `INFO`-level line; no-op when logging is not initialized.
pub fn info(msg: &str) {
    write_line("INFO", msg);
}

/// Writes an `ERROR`-level line; no-op when logging is not initialized.
pub fn error(msg: &str) {
    write_line("ERROR", msg);
}

fn lock_sink() -> MutexGuard<'static, Option<Box<dyn Write + Send>>> {
    SINK.lock().unwrap_or_else(PoisonError::into_inner)
}

fn lock_path() -> MutexGuard<'static, Option<PathBuf>> {
    PATH.lock().unwrap_or_else(PoisonError::into_inner)
}

fn write_line(level: &str, msg: &str) {
    if BROKEN.load(Ordering::Relaxed) {
        return;
    }

    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let line = format!("{timestamp} {level} {msg}\n");

    let mut sink = lock_sink();
    let Some(writer) = sink.as_mut() else { return };
    if writer.write_all(line.as_bytes()).is_ok() {
        return;
    }

    drop(sink);
    if !try_reopen() {
        BROKEN.store(true, Ordering::Relaxed);
    } else {
        let mut sink = lock_sink();
        if let Some(writer) = sink.as_mut() {
            let _ = writer.write_all(line.as_bytes());
        }
    }
}

fn try_reopen() -> bool {
    let path = lock_path().clone();
    let Some(path) = path else { return false };
    match OpenOptions::new().create(true).append(true).open(&path) {
        Ok(file) => {
            *lock_sink() = Some(Box::new(file));
            true
        }
        Err(_) => false,
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;

    static CAPTURE_LOCK: Mutex<()> = Mutex::new(());
    static TEST_COUNTER: AtomicUsize = AtomicUsize::new(0);

    pub struct LogGuard {
        path: PathBuf,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    pub fn init_for_test(path: &Path) -> LogGuard {
        let lock = CAPTURE_LOCK.lock().unwrap();
        let mut sink = lock_sink();
        *sink = None;
        drop(sink);
        let mut p = lock_path();
        *p = None;
        drop(p);
        BROKEN.store(false, Ordering::Relaxed);

        init(path).expect("test log init must succeed");
        LogGuard {
            path: path.to_path_buf(),
            _lock: lock,
        }
    }

    pub fn temp_log_path() -> PathBuf {
        let n = TEST_COUNTER.fetch_add(1, Ordering::Relaxed);
        std::env::temp_dir().join(format!("aot-log-test-{}-{}", std::process::id(), n))
    }

    impl LogGuard {
        pub fn text(&self) -> String {
            std::fs::read_to_string(&self.path).unwrap_or_default()
        }
    }

    impl Drop for LogGuard {
        fn drop(&mut self) {
            let mut sink = lock_sink();
            *sink = None;
            drop(sink);
            *lock_path() = None;
            BROKEN.store(false, Ordering::Relaxed);
            let _ = std::fs::remove_file(&self.path);
        }
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buf: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("synthetic write failure"))
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    pub fn install_failing_sink() -> LogGuard {
        let lock = CAPTURE_LOCK.lock().unwrap();
        let mut sink = lock_sink();
        *sink = Some(Box::new(FailingWriter));
        drop(sink);
        let mut p = lock_path();
        *p = None;
        drop(p);
        BROKEN.store(false, Ordering::Relaxed);

        LogGuard {
            path: PathBuf::new(),
            _lock: lock,
        }
    }

    #[test]
    fn test_logger_lifecycle() {
        debug("dropped");
        info("dropped");
        error("dropped");

        let path = temp_log_path();
        let _guard = init_for_test(&path);

        debug("hello debug");
        info("hello info");
        error("hello error");

        let other = temp_log_path();
        init(&other).unwrap();
        debug("after reinit");

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains("dropped"));
        assert!(content.contains("DEBUG hello debug"));
        assert!(content.contains("INFO hello info"));
        assert!(content.contains("ERROR hello error"));
        assert!(content.contains("DEBUG after reinit"));
        assert!(!other.exists());
    }

    #[test]
    fn test_poisoned_mutex_continues_logging() {
        let path = temp_log_path();
        let _guard = init_for_test(&path);

        let _ = std::thread::spawn(|| {
            let _guard = SINK.lock();
            panic!("poison the sink");
        })
        .join();

        error("after poison");
        assert!(_guard.text().contains("ERROR after poison"));
    }

    #[test]
    fn test_write_failure_reopens_and_writes() {
        let path = temp_log_path();
        let _guard = init_for_test(&path);

        let mut sink = lock_sink();
        *sink = Some(Box::new(FailingWriter));
        drop(sink);

        error("trigger reopen");

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("ERROR trigger reopen"));
        assert!(!BROKEN.load(Ordering::Relaxed));
    }

    #[test]
    fn test_write_failure_marks_broken_when_reopen_fails() {
        let _guard = install_failing_sink();

        error("first failure");
        assert!(BROKEN.load(Ordering::Relaxed));

        error("silenced");
    }
}
