use std::cell::Cell;
use std::ffi::OsString;
use std::fmt;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::{SystemTime, UNIX_EPOCH};

/// Size above which the log file is set aside at startup.
const MAX_LOG_BYTES: u64 = 5 * 1024 * 1024;

static SINK: Mutex<Option<Box<dyn Write + Send>>> = Mutex::new(None);
static PATH: Mutex<Option<PathBuf>> = Mutex::new(None);
static ROLE: Mutex<&str> = Mutex::new("main");
static ENABLED: AtomicBool = AtomicBool::new(false);
static BROKEN: AtomicBool = AtomicBool::new(false);

/// Severity of a log line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Debug,
    Info,
    Error,
}

thread_local! {
    static THREAD_ROLE: Cell<Option<&'static str>> = const { Cell::new(None) };
}

/// Writes a `DEBUG`-level line; formats nothing when logging is disabled.
macro_rules! debug {
    ($($arg:tt)*) => {
        if $crate::backends::logger::enabled() {
            $crate::backends::logger::write(
                $crate::backends::logger::Level::Debug,
                format_args!($($arg)*),
            );
        }
    };
}

/// Writes an `INFO`-level line; formats nothing when logging is disabled.
macro_rules! info {
    ($($arg:tt)*) => {
        if $crate::backends::logger::enabled() {
            $crate::backends::logger::write(
                $crate::backends::logger::Level::Info,
                format_args!($($arg)*),
            );
        }
    };
}

/// Writes an `ERROR`-level line; formats nothing when logging is disabled.
macro_rules! error {
    ($($arg:tt)*) => {
        if $crate::backends::logger::enabled() {
            $crate::backends::logger::write(
                $crate::backends::logger::Level::Error,
                format_args!($($arg)*),
            );
        }
    };
}

pub(crate) use {debug, error, info};

impl Level {
    fn as_str(self) -> &'static str {
        match self {
            Level::Debug => "DEBUG",
            Level::Info => "INFO",
            Level::Error => "ERROR",
        }
    }
}

/// Initializes logging to the given file, tagging lines with the process
/// `role`. Subsequent calls are no-ops. Until this is called, logging is a
/// silent no-op — the TUI must never log to stdout/stderr, which would corrupt
/// the alternate screen.
pub fn init(path: &Path, role: &'static str) -> std::io::Result<()> {
    let mut sink = lock_sink();
    if sink.is_none() {
        rotate(path, MAX_LOG_BYTES);
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        *sink = Some(Box::new(file));
        drop(sink);
        *lock_path() = Some(path.to_path_buf());
        *ROLE.lock().unwrap_or_else(PoisonError::into_inner) = role;
        ENABLED.store(true, Ordering::Relaxed);
    }
    Ok(())
}

/// Tags lines written from the current thread with `role` instead of the
/// process role.
pub fn set_thread_role(role: &'static str) {
    THREAD_ROLE.with(|cell| cell.set(Some(role)));
}

/// Whether log lines are currently being written.
pub fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed) && !BROKEN.load(Ordering::Relaxed)
}

/// Writes one line at `level`; used by the `debug!`, `info!` and `error!` macros.
pub fn write(level: Level, args: fmt::Arguments<'_>) {
    if BROKEN.load(Ordering::Relaxed) {
        return;
    }

    let line = format_line(level, args);

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

fn format_line(level: Level, args: fmt::Arguments<'_>) -> String {
    let elapsed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let role = THREAD_ROLE
        .with(Cell::get)
        .unwrap_or_else(|| *ROLE.lock().unwrap_or_else(PoisonError::into_inner));
    format!(
        "{}.{:03} {} {role} {} {args}\n",
        elapsed.as_secs(),
        elapsed.subsec_millis(),
        std::process::id(),
        level.as_str(),
    )
}

fn rotate(path: &Path, max_bytes: u64) {
    let too_big = std::fs::metadata(path)
        .map(|meta| meta.len() > max_bytes)
        .unwrap_or(false);
    if too_big {
        let _ = std::fs::rename(path, old_path(path));
    }
}

fn old_path(path: &Path) -> PathBuf {
    let mut name = OsString::from(path.as_os_str());
    name.push(".old");
    PathBuf::from(name)
}

fn lock_sink() -> MutexGuard<'static, Option<Box<dyn Write + Send>>> {
    SINK.lock().unwrap_or_else(PoisonError::into_inner)
}

fn lock_path() -> MutexGuard<'static, Option<PathBuf>> {
    PATH.lock().unwrap_or_else(PoisonError::into_inner)
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
        ENABLED.store(false, Ordering::Relaxed);

        init(path, "test").expect("test log init must succeed");
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
        fn text(&self) -> String {
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
            ENABLED.store(false, Ordering::Relaxed);
            let _ = std::fs::remove_file(&self.path);
            let _ = std::fs::remove_file(old_path(&self.path));
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

    fn install_failing_sink() -> LogGuard {
        let lock = CAPTURE_LOCK.lock().unwrap();
        let mut sink = lock_sink();
        *sink = Some(Box::new(FailingWriter));
        drop(sink);
        let mut p = lock_path();
        *p = None;
        drop(p);
        BROKEN.store(false, Ordering::Relaxed);
        ENABLED.store(true, Ordering::Relaxed);

        LogGuard {
            path: PathBuf::new(),
            _lock: lock,
        }
    }

    #[test]
    fn test_logger_lifecycle() {
        let _lock = CAPTURE_LOCK.lock().unwrap();
        assert!(!enabled());
        debug!("dropped");
        info!("dropped");
        error!("dropped");
        drop(_lock);

        let path = temp_log_path();
        let _guard = init_for_test(&path);
        assert!(enabled());

        debug!("hello {}", "debug");
        info!("hello info");
        error!("hello error");

        let other = temp_log_path();
        init(&other, "other").unwrap();
        debug!("after reinit");

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains("dropped"));
        assert!(content.contains("test DEBUG hello debug"));
        assert!(content.contains("test INFO hello info"));
        assert!(content.contains("test ERROR hello error"));
        assert!(content.contains("test DEBUG after reinit"));
        assert!(!other.exists());
    }

    #[test]
    fn test_line_has_millis_timestamp_and_pid() {
        let path = temp_log_path();
        let guard = init_for_test(&path);

        info!("tagged");

        let text = guard.text();
        let line = text.lines().find(|line| line.ends_with("tagged")).unwrap();
        let mut fields = line.split(' ');
        let timestamp = fields.next().unwrap();
        let (secs, millis) = timestamp.split_once('.').unwrap();
        assert!(secs.parse::<u64>().is_ok());
        assert_eq!(millis.len(), 3);
        assert_eq!(fields.next().unwrap(), std::process::id().to_string());
        assert_eq!(fields.next().unwrap(), "test");
        assert_eq!(fields.next().unwrap(), "INFO");
    }

    #[test]
    fn test_thread_role_overrides_process_role() {
        let path = temp_log_path();
        let guard = init_for_test(&path);

        std::thread::spawn(|| {
            set_thread_role("mcp");
            info!("from thread");
        })
        .join()
        .unwrap();
        info!("from main");

        let text = guard.text();
        assert!(text.contains(" mcp INFO from thread"));
        assert!(text.contains(" test INFO from main"));
    }

    #[test]
    fn test_rotate_sets_aside_oversized_file() {
        let path = temp_log_path();
        std::fs::write(&path, "old line\n").unwrap();
        rotate(&path, 4);
        assert!(!path.exists());
        assert_eq!(
            std::fs::read_to_string(old_path(&path)).unwrap(),
            "old line\n"
        );
        let _ = std::fs::remove_file(old_path(&path));
    }

    #[test]
    fn test_init_keeps_small_file() {
        let path = temp_log_path();
        std::fs::write(&path, "line\n").unwrap();
        let guard = init_for_test(&path);
        info!("appended");
        let text = guard.text();
        assert!(text.starts_with("line\n"));
        assert!(text.contains("appended"));
        assert!(!old_path(&path).exists());
    }

    #[test]
    fn test_old_path_appends_suffix() {
        assert_eq!(
            old_path(Path::new("/tmp/aot.log")),
            PathBuf::from("/tmp/aot.log.old")
        );
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

        error!("after poison");
        assert!(_guard.text().contains("ERROR after poison"));
    }

    #[test]
    fn test_write_failure_reopens_and_writes() {
        let path = temp_log_path();
        let _guard = init_for_test(&path);

        let mut sink = lock_sink();
        *sink = Some(Box::new(FailingWriter));
        drop(sink);

        error!("trigger reopen");

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("ERROR trigger reopen"));
        assert!(!BROKEN.load(Ordering::Relaxed));
    }

    #[test]
    fn test_write_failure_marks_broken_when_reopen_fails() {
        let _guard = install_failing_sink();

        error!("first failure");
        assert!(BROKEN.load(Ordering::Relaxed));

        assert!(!enabled());
        error!("silenced");
    }
}
