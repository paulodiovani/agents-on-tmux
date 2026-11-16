use std::time::{Duration, Instant, SystemTime};

use sysinfo::{Pid, ProcessesToUpdate, System};

/// Contract for looking up running processes.
#[cfg_attr(test, mockall::automock)]
pub trait Processes {
    /// Returns when the process with the given pid started, or `None` when
    /// there is no such process.
    fn started_at(&self, pid: u32) -> Option<Instant>;
}

/// Looks processes up in the operating system's process table.
#[derive(Debug, Default)]
pub struct SystemProcesses;

impl Processes for SystemProcesses {
    fn started_at(&self, pid: u32) -> Option<Instant> {
        let pid = Pid::from_u32(pid);
        let mut system = System::new().ok()?;
        system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        let start_secs = system.process(pid)?.start_time();
        instant_at(start_secs, SystemTime::now())
    }
}

/// The instant matching `epoch_secs` seconds since the Unix epoch, given the
/// current wall-clock time.
fn instant_at(epoch_secs: u64, now: SystemTime) -> Option<Instant> {
    let now_secs = now.duration_since(SystemTime::UNIX_EPOCH).ok()?.as_secs();
    let elapsed = Duration::from_secs(now_secs.saturating_sub(epoch_secs));
    Instant::now().checked_sub(elapsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_started_at_current_process() {
        let started_at = SystemProcesses.started_at(std::process::id()).unwrap();
        assert!(started_at <= Instant::now());
    }

    #[test]
    fn test_started_at_unknown_pid() {
        assert!(SystemProcesses.started_at(999_999_999).is_none());
    }

    #[test]
    fn test_instant_at_past_time() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let started_at = instant_at(900, now).unwrap();
        let elapsed = started_at.elapsed();
        assert!(elapsed >= Duration::from_secs(100));
        assert!(elapsed < Duration::from_secs(110));
    }

    #[test]
    fn test_instant_at_future_time_is_now() {
        let now = SystemTime::UNIX_EPOCH + Duration::from_secs(1_000);
        let started_at = instant_at(2_000, now).unwrap();
        assert!(started_at.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn test_instant_at_clock_before_epoch() {
        let now = SystemTime::UNIX_EPOCH - Duration::from_secs(1);
        assert!(instant_at(0, now).is_none());
    }
}
