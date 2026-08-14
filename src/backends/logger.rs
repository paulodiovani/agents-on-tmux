use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_FILE: OnceLock<Mutex<File>> = OnceLock::new();

/// Initializes logging to the given file. Subsequent calls are no-ops.
/// Until this is called, `debug`/`info`/`error` are silent no-ops — the TUI
/// must never log to stdout/stderr, which would corrupt the alternate screen.
pub fn init(path: &Path) -> std::io::Result<()> {
    if LOG_FILE.get().is_none() {
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let _ = LOG_FILE.set(Mutex::new(file));
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

fn write_line(level: &str, msg: &str) {
    if let Some(file) = LOG_FILE.get()
        && let Ok(mut file) = file.lock()
    {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let _ = writeln!(file, "{timestamp} {level} {msg}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // LOG_FILE is process-global (OnceLock), so a single test covers the whole
    // lifecycle in order: no-op before init, init, writes, idempotent re-init.
    #[test]
    fn test_logger_lifecycle() {
        // Before init: silent no-ops.
        debug("dropped");
        info("dropped");
        error("dropped");

        let dir = std::env::temp_dir().join("aot-logger-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("aot.log");
        init(&path).unwrap();

        debug("hello debug");
        info("hello info");
        error("hello error");

        // Re-init with another path is a no-op; writes keep going to the first file.
        init(&dir.join("other.log")).unwrap();
        debug("after reinit");

        let content = std::fs::read_to_string(&path).unwrap();
        assert!(!content.contains("dropped"));
        assert!(content.contains("DEBUG hello debug"));
        assert!(content.contains("INFO hello info"));
        assert!(content.contains("ERROR hello error"));
        assert!(content.contains("DEBUG after reinit"));
        assert!(!dir.join("other.log").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
