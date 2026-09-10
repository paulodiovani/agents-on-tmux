use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, LazyLock};

use signal_hook::consts::{SIGHUP, SIGINT, SIGQUIT, SIGTERM};
use signal_hook::iterator::Signals;

use crate::backends::logger;

/// What to do when a signal is received.
pub enum OnSignal {
    /// Set the flag only; the caller polls `received()` and shuts down cleanly.
    Shutdown,
    /// Log the signal and call `process::exit(128 + signo)` immediately.
    Exit,
}

static RECEIVED: LazyLock<Arc<AtomicUsize>> = LazyLock::new(|| Arc::new(AtomicUsize::new(0)));

/// Registers handlers for SIGTERM, SIGINT, SIGHUP, and SIGQUIT.
/// Returns `Ok(())` on success; on failure, logs the error and returns it
/// (signals are best-effort — the caller may continue without them).
pub fn install(policy: OnSignal) -> std::io::Result<()> {
    let mut signals = Signals::new([SIGTERM, SIGINT, SIGHUP, SIGQUIT])?;

    std::thread::spawn(move || {
        for signo in signals.forever() {
            if let Some(name) = name_of(signo) {
                logger::error(&format!("signal: received {name} ({signo})"));
            }
            RECEIVED.store(signo as usize, Ordering::SeqCst);

            match policy {
                OnSignal::Shutdown => {}
                OnSignal::Exit => {
                    std::process::exit(128 + signo);
                }
            }
        }
    });

    Ok(())
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
pub fn name_of(signo: i32) -> Option<&'static str> {
    match signo {
        SIGTERM => Some("SIGTERM"),
        SIGINT => Some("SIGINT"),
        SIGHUP => Some("SIGHUP"),
        SIGQUIT => Some("SIGQUIT"),
        _ => None,
    }
}

#[cfg(test)]
pub fn set_for_test(signo: i32) {
    RECEIVED.store(signo as usize, Ordering::SeqCst);
}

#[cfg(test)]
pub fn clear_for_test() {
    RECEIVED.store(0, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

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
        clear_for_test();
        assert_eq!(received(), None);
    }

    #[test]
    fn test_received_after_set() {
        clear_for_test();
        set_for_test(SIGTERM);
        assert_eq!(received(), Some((SIGTERM, "SIGTERM")));
        clear_for_test();
    }
}
