use std::sync::atomic::{AtomicUsize, Ordering};

use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
use signal_hook::iterator::Signals;

use crate::backends::logger;

static RECEIVED: AtomicUsize = AtomicUsize::new(0);

/// What to do when a signal is received.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnSignal {
    /// Set the flag only; the caller polls `received()` and shuts down cleanly.
    Shutdown,
    /// Log the signal and call `process::exit(128 + signo)` immediately.
    Exit,
}

/// Registers handlers for SIGTERM, SIGINT, SIGHUP, and SIGQUIT.
/// Returns `Ok(())` on success; on failure, logs the error and returns it
/// (signals are best-effort — the caller may continue without them).
pub fn install(policy: OnSignal) -> std::io::Result<()> {
    let mut signals = Signals::new([SIGTERM, SIGINT, SIGHUP, SIGQUIT])?;

    std::thread::spawn(move || {
        for signo in signals.forever() {
            handle(signo, &policy, |code| std::process::exit(code));
        }
    });

    Ok(())
}

/// Records a received signal and applies the policy, calling `exit` with the
/// process exit code when the policy is `Exit`.
fn handle(signo: i32, policy: &OnSignal, exit: impl FnOnce(i32)) {
    if let Some(name) = name_of(signo) {
        logger::error!("signal: received {name} ({signo})");
    }
    RECEIVED.store(signo as usize, Ordering::SeqCst);

    if let OnSignal::Exit = policy {
        exit(128 + signo);
    }
}

/// Returns the signal number and name if a signal was received, or `None`.
pub fn received() -> Option<(i32, &'static str)> {
    let signo = RECEIVED.load(Ordering::SeqCst);
    if signo == 0 {
        None
    } else {
        name_of(signo as i32).map(|name| (signo as i32, name))
    }
}

/// Returns the name of a signal, or `None` if unknown.
fn name_of(signo: i32) -> Option<&'static str> {
    match signo {
        SIGTERM => Some("SIGTERM"),
        SIGINT => Some("SIGINT"),
        SIGHUP => Some("SIGHUP"),
        SIGQUIT => Some("SIGQUIT"),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, MutexGuard};

    static RECEIVED_LOCK: Mutex<()> = Mutex::new(());

    /// Serializes tests that touch the received signal and starts them cleared.
    fn cleared() -> MutexGuard<'static, ()> {
        let guard = RECEIVED_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        RECEIVED.store(0, Ordering::SeqCst);
        guard
    }

    #[test]
    fn test_name_of_known_signals() {
        assert_eq!(name_of(SIGTERM), Some("SIGTERM"));
        assert_eq!(name_of(SIGINT), Some("SIGINT"));
        assert_eq!(name_of(SIGHUP), Some("SIGHUP"));
        assert_eq!(name_of(SIGQUIT), Some("SIGQUIT"));
        assert_eq!(name_of(999), None);
    }

    #[test]
    fn test_received_none_by_default() {
        let _guard = cleared();
        assert_eq!(received(), None);
    }

    #[test]
    fn test_received_ignores_unknown_signal() {
        let _guard = cleared();
        RECEIVED.store(999, Ordering::SeqCst);
        assert_eq!(received(), None);
    }

    #[test]
    fn test_shutdown_records_signal_without_exiting() {
        let _guard = cleared();
        let mut exited = None;
        handle(SIGTERM, &OnSignal::Shutdown, |code| exited = Some(code));
        assert_eq!(received(), Some((SIGTERM, "SIGTERM")));
        assert_eq!(exited, None);
    }

    #[test]
    fn test_exit_records_signal_and_exits_with_shell_code() {
        let _guard = cleared();
        let mut exited = None;
        handle(SIGINT, &OnSignal::Exit, |code| exited = Some(code));
        assert_eq!(received(), Some((SIGINT, "SIGINT")));
        assert_eq!(exited, Some(128 + SIGINT));
    }

    #[test]
    fn test_handle_logs_received_signal() {
        let path = logger::tests::temp_log_path();
        let _log = logger::tests::init_for_test(&path);
        let _guard = cleared();

        handle(SIGHUP, &OnSignal::Shutdown, |_| {});

        let log = std::fs::read_to_string(&path).unwrap();
        assert!(log.contains("ERROR signal: received SIGHUP (1)"));
    }
}
