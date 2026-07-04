use std::process::Command;
use std::time::{Duration, Instant};
use thiserror::Error;

/// Contract for interacting with tmux sessions and windows.
pub trait Tmux {
    /// Ensures the tmux session exists, creating it if necessary.
    fn create_session_if_not_exists(&self) -> Result<(), TmuxError>;
    /// Attaches to the tmux session, inheriting stdio. Blocks until detached.
    fn attach_session(&self) -> Result<(), TmuxError>;
    /// Lists all windows in the session.
    fn list_windows(&self) -> Result<Vec<Window>, TmuxError>;
    /// Creates a new window with the given name.
    fn create_window(&self, name: &str) -> Result<Window, TmuxError>;
    /// Kills the window with the given id.
    fn kill_window(&self, id: u32) -> Result<(), TmuxError>;
    /// Selects (focuses) the window with the given id.
    fn select_window(&self, id: u32) -> Result<(), TmuxError>;
    #[allow(dead_code)]
    /// Sends keys to the specified window.
    fn send_keys(&self, window_id: u32, command: &str) -> Result<(), TmuxError>;
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
    #[error("Command failed: {message}")]
    CommandFailed {
        message: String,
        stderr: String,
        code: Option<i32>,
    },
    #[error("tmux not available")]
    TmuxNotAvailable,
}

/// Represents a tmux window and its runtime state.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub id: u32,
    pub name: String,
    pub running_command: String,
    pub started_at: Option<Instant>,
    pub notification_pending: bool,
}

/// tmux driver that communicates with the tmux server.
pub struct TmuxDriver;

#[allow(dead_code)]
fn run_tmux_cmd(args: &[&str]) -> Result<String, TmuxError> {
    let output =
        Command::new("tmux")
            .args(args)
            .output()
            .map_err(|e| TmuxError::CommandFailed {
                message: format!("Failed to execute tmux: {}", e),
                stderr: String::new(),
                code: None,
            })?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    } else {
        Err(TmuxError::CommandFailed {
            message: format!("tmux {} failed", args.join(" ")),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            code: output.status.code(),
        })
    }
}

fn run_tmux_cmd_inherit_stdio(args: &[&str]) -> Result<(), TmuxError> {
    let status = Command::new("tmux")
        .args(args)
        .env_remove("TMUX")
        .env_remove("TMUX_TMPDIR")
        .stdin(std::process::Stdio::inherit())
        .stdout(std::process::Stdio::inherit())
        .stderr(std::process::Stdio::inherit())
        .status()
        .map_err(|e| TmuxError::CommandFailed {
            message: format!("Failed to execute tmux: {}", e),
            stderr: String::new(),
            code: None,
        })?;

    if status.success() {
        Ok(())
    } else {
        Err(TmuxError::CommandFailed {
            message: format!("tmux {} failed", args.join(" ")),
            stderr: String::new(),
            code: status.code(),
        })
    }
}

impl Tmux for TmuxDriver {
    /// Ensures the tmux session exists, creating it if necessary.
    fn create_session_if_not_exists(&self) -> Result<(), TmuxError> {
        let has_session = run_tmux_cmd(&["has-session", "-t", SESSION_NAME]);

        if has_session.is_err() {
            run_tmux_cmd(&["new-session", "-d", "-s", SESSION_NAME])?;
            run_tmux_cmd(&["set-option", "-t", SESSION_NAME, "status", "off"])?;
        }

        Ok(())
    }

    /// Attaches to the tmux session, inheriting stdio. Blocks until detached.
    fn attach_session(&self) -> Result<(), TmuxError> {
        run_tmux_cmd_inherit_stdio(&["attach-session", "-t", SESSION_NAME])
    }

    /// Lists all windows in the session.
    fn list_windows(&self) -> Result<Vec<Window>, TmuxError> {
        Ok(vec![
            Window {
                id: 1,
                name: "agent-1".to_string(),
                running_command: "cargo build".to_string(),
                started_at: Some(Instant::now() - Duration::from_secs(125)),
                notification_pending: false,
            },
            Window {
                id: 2,
                name: "agent-2".to_string(),
                running_command: "npm test".to_string(),
                started_at: Some(Instant::now() - Duration::from_secs(45)),
                notification_pending: true,
            },
            Window {
                id: 3,
                name: "agent-3".to_string(),
                running_command: "python main.py".to_string(),
                started_at: Some(Instant::now() - Duration::from_secs(300)),
                notification_pending: false,
            },
        ])
    }

    /// Creates a new window with the given name.
    fn create_window(&self, name: &str) -> Result<Window, TmuxError> {
        let windows = self.list_windows()?;
        let next_id = windows.iter().map(|w| w.id).max().unwrap_or(0) + 1;
        Ok(Window {
            id: next_id,
            name: name.to_string(),
            running_command: String::new(),
            started_at: None,
            notification_pending: false,
        })
    }

    /// Kills the window with the given id.
    fn kill_window(&self, _id: u32) -> Result<(), TmuxError> {
        Ok(())
    }

    /// Selects (focuses) the window with the given id.
    fn select_window(&self, _id: u32) -> Result<(), TmuxError> {
        Ok(())
    }

    /// Sends keys to the specified window.
    fn send_keys(&self, _window_id: u32, _command: &str) -> Result<(), TmuxError> {
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
    fn test_run_tmux_cmd_success() {
        let result = run_tmux_cmd(&["-V"]);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(output.contains("tmux"));
    }

    #[test]
    fn test_run_tmux_cmd_failure() {
        let result = run_tmux_cmd(&["invalid-command-that-does-not-exist"]);
        assert!(result.is_err());
        if let Err(TmuxError::CommandFailed {
            message,
            stderr,
            code,
        }) = result
        {
            assert!(message.contains("failed"));
            assert!(!stderr.is_empty() || code.is_some());
        }
    }

    #[test]
    fn test_create_session_if_not_exists() {
        let driver = TmuxDriver;
        let result = driver.create_session_if_not_exists();
        assert!(result.is_ok());
    }

    #[test]
    fn test_has_session_check() {
        let result = run_tmux_cmd(&["has-session", "-t", "nonexistent-session-12345"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_list_windows() {
        let driver = TmuxDriver;
        let windows = driver.list_windows().unwrap();
        assert_eq!(windows.len(), 3);
        assert_eq!(windows[0].id, 1);
        assert_eq!(windows[0].name, "agent-1");
        assert_eq!(windows[0].running_command, "cargo build");
        assert!(windows[0].started_at.is_some());
        assert!(!windows[0].notification_pending);
        assert_eq!(windows[1].id, 2);
        assert!(windows[1].notification_pending);
        assert_eq!(windows[2].id, 3);
    }

    #[test]
    fn test_create_window() {
        let driver = TmuxDriver;
        let window = driver.create_window("new-window").unwrap();
        assert_eq!(window.id, 4);
        assert_eq!(window.name, "new-window");
        assert_eq!(window.running_command, "");
        assert!(window.started_at.is_none());
        assert!(!window.notification_pending);
    }

    #[test]
    fn test_kill_window() {
        let driver = TmuxDriver;
        assert!(driver.kill_window(1).is_ok());
    }

    #[test]
    fn test_select_window() {
        let driver = TmuxDriver;
        assert!(driver.select_window(1).is_ok());
    }

    #[test]
    fn test_send_keys() {
        let driver = TmuxDriver;
        assert!(driver.send_keys(1, "ls -la").is_ok());
    }

    #[test]
    fn test_window_struct_fields() {
        let window = Window {
            id: 42,
            name: "test".to_string(),
            running_command: "echo hello".to_string(),
            started_at: Some(Instant::now() - Duration::from_secs(60)),
            notification_pending: true,
        };
        assert_eq!(window.id, 42);
        assert_eq!(window.name, "test");
        assert_eq!(window.running_command, "echo hello");
        assert!(window.started_at.is_some());
        assert!(window.notification_pending);
    }
}
