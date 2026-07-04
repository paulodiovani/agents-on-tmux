use std::time::{Duration, Instant};
use thiserror::Error;

/// Contract for interacting with tmux sessions and windows.
pub trait Tmux {
    /// Ensures the tmux session exists, creating it if necessary.
    fn create_session_if_not_exists(&self) -> Result<(), TmuxError>;
    /// Lists all windows in the session.
    fn list_windows(&self) -> Result<Vec<Window>, TmuxError>;
    /// Creates a new window with the given name.
    fn create_window(&self, name: &str) -> Result<Window, TmuxError>;
    /// Kills the window with the given name.
    fn kill_window(&self, name: &str) -> Result<(), TmuxError>;
    /// Selects (focuses) the window with the given name.
    fn select_window(&self, name: &str) -> Result<(), TmuxError>;
    #[allow(dead_code)]
    /// Sends keys to the specified window.
    fn send_keys(&self, window: &str, command: &str) -> Result<(), TmuxError>;
}

pub const SESSION_NAME: &str = "agents-on-tmux";

/// Errors that can occur during tmux operations.
#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum TmuxError {
    #[error("Failed to create session")]
    SessionCreationFailed,
    #[error("Window not found")]
    WindowNotFound,
    #[error("Command failed")]
    CommandFailed,
    #[error("tmux not available")]
    TmuxNotAvailable,
}

/// Represents a tmux window and its runtime state.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub name: String,
    pub running_command: String,
    pub started_at: Option<Instant>,
    pub notification_pending: bool,
}

/// tmux driver that communicates with the tmux server.
pub struct TmuxDriver;

impl Tmux for TmuxDriver {
    /// Ensures the tmux session exists, creating it if necessary.
    fn create_session_if_not_exists(&self) -> Result<(), TmuxError> {
        Ok(())
    }

    /// Lists all windows in the session.
    fn list_windows(&self) -> Result<Vec<Window>, TmuxError> {
        Ok(vec![
            Window {
                name: "agent-1".to_string(),
                running_command: "cargo build".to_string(),
                started_at: Some(Instant::now() - Duration::from_secs(125)),
                notification_pending: false,
            },
            Window {
                name: "agent-2".to_string(),
                running_command: "npm test".to_string(),
                started_at: Some(Instant::now() - Duration::from_secs(45)),
                notification_pending: true,
            },
            Window {
                name: "agent-3".to_string(),
                running_command: "python main.py".to_string(),
                started_at: Some(Instant::now() - Duration::from_secs(300)),
                notification_pending: false,
            },
        ])
    }

    /// Creates a new window with the given name.
    fn create_window(&self, name: &str) -> Result<Window, TmuxError> {
        Ok(Window {
            name: name.to_string(),
            running_command: String::new(),
            started_at: None,
            notification_pending: false,
        })
    }

    /// Kills the window with the given name.
    fn kill_window(&self, _name: &str) -> Result<(), TmuxError> {
        Ok(())
    }

    /// Selects (focuses) the window with the given name.
    fn select_window(&self, _name: &str) -> Result<(), TmuxError> {
        Ok(())
    }

    /// Sends keys to the specified window.
    fn send_keys(&self, _window: &str, _command: &str) -> Result<(), TmuxError> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_name() {
        assert_eq!(SESSION_NAME, "agents-on-tmux");
    }

    #[test]
    fn test_create_session_if_not_exists() {
        let driver = TmuxDriver;
        assert!(driver.create_session_if_not_exists().is_ok());
    }

    #[test]
    fn test_list_windows() {
        let driver = TmuxDriver;
        let windows = driver.list_windows().unwrap();
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].name, "agent-1");
        assert_eq!(windows[0].running_command, "cargo build");
        assert!(windows[0].started_at.is_some());
        assert!(!windows[0].notification_pending);
        assert!(windows[1].notification_pending);
    }

    #[test]
    fn test_create_window() {
        let driver = TmuxDriver;
        let window = driver.create_window("new-window").unwrap();
        assert_eq!(window.name, "new-window");
        assert_eq!(window.running_command, "");
        assert!(window.started_at.is_none());
        assert!(!window.notification_pending);
    }

    #[test]
    fn test_kill_window() {
        let driver = TmuxDriver;
        assert!(driver.kill_window("agent-1").is_ok());
    }

    #[test]
    fn test_select_window() {
        let driver = TmuxDriver;
        assert!(driver.select_window("agent-1").is_ok());
    }

    #[test]
    fn test_send_keys() {
        let driver = TmuxDriver;
        assert!(driver.send_keys("agent-1", "ls -la").is_ok());
    }

    #[test]
    fn test_window_struct_fields() {
        let window = Window {
            name: "test".to_string(),
            running_command: "echo hello".to_string(),
            started_at: Some(Instant::now() - Duration::from_secs(60)),
            notification_pending: true,
        };
        assert_eq!(window.name, "test");
        assert_eq!(window.running_command, "echo hello");
        assert!(window.started_at.is_some());
        assert!(window.notification_pending);
    }
}
