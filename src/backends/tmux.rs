use std::process::Command;
use std::time::Instant;
use thiserror::Error;

/// Contract for executing tmux commands.
pub trait CommandExecutor {
    /// Executes a tmux command and returns stdout on success.
    fn execute(&self, args: &[&str]) -> Result<String, TmuxError>;
    /// Executes a tmux command with inherited stdio (for attach).
    fn execute_inherit_stdio(&self, args: &[&str]) -> Result<(), TmuxError>;
}

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
    /// Sends keys to the specified window.
    fn split_window(&self, command: &str) -> Result<String, TmuxError>;
}

pub const SESSION_NAME: &str = "agents-on-tmux";

/// Errors that can occur during tmux operations.
#[derive(Debug, Error)]
pub enum TmuxError {
    #[error("Command failed: {message}")]
    CommandFailed {
        message: String,
        stderr: String,
        code: Option<i32>,
    },
    #[error("Cannot run aot inside its own dedicated session '{0}'.")]
    InsideOwnSession(String),
    #[error("Not running inside a tmux session")]
    NotInsideTmux,
    #[error("Window not found")]
    WindowNotFound,
}

/// Represents a tmux window and its runtime state.
#[derive(Debug, Clone, PartialEq)]
pub struct Window {
    pub current_dir: String,
    pub id: u32,
    pub is_active: bool,
    pub name: String,
    pub notification_pending: bool,
    pub running_command: String,
    pub started_at: Option<Instant>,
}

pub fn check_inside_tmux() -> Result<(), TmuxError> {
    if let Err(_err) = std::env::var("TMUX") {
        Err(TmuxError::NotInsideTmux)
    } else {
        Ok(())
    }
}

/// Detects the parent tmux session by querying tmux for the current session name.
pub fn detect_parent_session() -> Result<String, TmuxError> {
    check_inside_tmux()?;

    let output = Command::new("tmux")
        .args(["display-message", "-p", "#S"])
        .output()
        .map_err(|e| TmuxError::CommandFailed {
            message: format!("Failed to execute tmux: {}", e),
            stderr: String::new(),
            code: None,
        })?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    } else {
        Err(TmuxError::NotInsideTmux)
    }
}

/// Real tmux command executor that calls the tmux binary.
pub struct ShellCommandExecutor;

impl CommandExecutor for ShellCommandExecutor {
    fn execute(&self, args: &[&str]) -> Result<String, TmuxError> {
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

    fn execute_inherit_stdio(&self, args: &[&str]) -> Result<(), TmuxError> {
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
}

/// tmux driver that communicates with the tmux server.
pub struct TmuxDriver<E: CommandExecutor = ShellCommandExecutor> {
    executor: E,
    session: String,
}

impl TmuxDriver<ShellCommandExecutor> {
    /// Creates a new TmuxDriver with the real command executor.
    pub fn new(session: &str) -> Self {
        Self {
            executor: ShellCommandExecutor,
            session: session.to_string(),
        }
    }
}

impl Default for TmuxDriver<ShellCommandExecutor> {
    fn default() -> Self {
        Self::new(SESSION_NAME)
    }
}

impl<E: CommandExecutor> TmuxDriver<E> {
    /// Creates a new TmuxDriver with a custom command executor.
    #[cfg(test)]
    pub fn with_executor(executor: E) -> Self {
        Self {
            executor,
            session: SESSION_NAME.to_string(),
        }
    }
}

fn parse_window_line(line: &str) -> Option<Window> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() != 6 {
        return None;
    }

    let id = parts[0].parse::<u32>().ok()?;
    let name = parts[1].to_string();
    let notification_pending = parts[2] == "1";
    let running_command = parts[3].to_string();
    let is_active = parts[4] == "1";
    let current_dir = parts[5].to_string();

    Some(Window {
        current_dir,
        id,
        is_active,
        name,
        notification_pending,
        running_command,
        started_at: None,
    })
}

impl<E: CommandExecutor> Tmux for TmuxDriver<E> {
    /// Ensures the tmux session exists, creating it if necessary.
    fn create_session_if_not_exists(&self) -> Result<(), TmuxError> {
        let has_session = self.executor.execute(&["has-session", "-t", &self.session]);

        if has_session.is_err() {
            self.executor
                .execute(&["new-session", "-d", "-s", &self.session])?;
            self.executor
                .execute(&["set-option", "-t", &self.session, "status", "off"])?;
        }

        Ok(())
    }

    /// Attaches to the tmux session, inheriting stdio. Blocks until detached.
    fn attach_session(&self) -> Result<(), TmuxError> {
        self.executor
            .execute_inherit_stdio(&["attach-session", "-t", &self.session])
    }

    /// Lists all windows in the session.
    fn list_windows(&self) -> Result<Vec<Window>, TmuxError> {
        let output = self.executor.execute(&[
            "list-windows",
            "-t",
            &self.session,
            "-F",
            "#{window_index}\t#{window_name}\t#{window_activity_flag}\t#{pane_current_command}\t#{window_active}\t#{pane_current_path}",
        ])?;

        let windows: Vec<Window> = output.lines().filter_map(parse_window_line).collect();

        Ok(windows)
    }

    /// Creates a new window with the given name.
    fn create_window(&self, name: &str) -> Result<Window, TmuxError> {
        self.executor
            .execute(&["new-window", "-t", &self.session, "-n", name])?;

        let windows = self.list_windows()?;
        windows
            .into_iter()
            .find(|w| w.name == name)
            .ok_or(TmuxError::WindowNotFound)
    }

    /// Kills the window with the given id.
    fn kill_window(&self, id: u32) -> Result<(), TmuxError> {
        let target = format!("{}:{}", self.session, id);
        self.executor.execute(&["kill-window", "-t", &target])?;
        Ok(())
    }

    /// Selects (focuses) the window with the given id.
    fn select_window(&self, id: u32) -> Result<(), TmuxError> {
        let target = format!("{}:{}", self.session, id);
        self.executor.execute(&["select-window", "-t", &target])?;
        Ok(())
    }

    /// Splits the current window horizontally, creating a side pane.
    fn split_window(&self, command: &str) -> Result<String, TmuxError> {
        self.executor
            .execute(&[
                "split-window",
                "-h",
                "-b",
                "-l",
                "30",
                "-d",
                "-P",
                "-F",
                "#{pane_id}",
                "-t",
                &self.session,
                command,
            ])
            .map(|s| s.trim().to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::time::Duration;

    /// Mock command executor for testing.
    struct MockCommandExecutor {
        commands: RefCell<Vec<Vec<String>>>,
        pane_id: RefCell<String>,
        session_exists: RefCell<bool>,
        windows: RefCell<Vec<Window>>,
    }

    impl MockCommandExecutor {
        fn new() -> Self {
            Self {
                commands: RefCell::new(Vec::new()),
                pane_id: RefCell::new("%99".to_string()),
                session_exists: RefCell::new(false),
                windows: RefCell::new(Vec::new()),
            }
        }

        fn with_session() -> Self {
            let mock = Self::new();
            *mock.session_exists.borrow_mut() = true;
            mock
        }
    }

    impl CommandExecutor for MockCommandExecutor {
        fn execute(&self, args: &[&str]) -> Result<String, TmuxError> {
            self.commands
                .borrow_mut()
                .push(args.iter().map(|s| s.to_string()).collect());

            match args.first() {
                Some(&"has-session") => {
                    if *self.session_exists.borrow() {
                        Ok(String::new())
                    } else {
                        Err(TmuxError::CommandFailed {
                            message: "session not found".to_string(),
                            stderr: String::new(),
                            code: Some(1),
                        })
                    }
                }
                Some(&"new-session") => {
                    *self.session_exists.borrow_mut() = true;
                    Ok(String::new())
                }
                Some(&"set-option") => Ok(String::new()),
                Some(&"list-windows") => {
                    let windows = self.windows.borrow();
                    let output: Vec<String> = windows
                        .iter()
                        .map(|w| {
                            format!(
                                "{}\t{}\t{}\t{}\t{}\t{}",
                                w.id,
                                w.name,
                                if w.notification_pending { "1" } else { "0" },
                                w.running_command,
                                if w.is_active { "1" } else { "0" },
                                w.current_dir
                            )
                        })
                        .collect();
                    Ok(output.join("\n"))
                }
                Some(&"new-window") => {
                    let name = args
                        .windows(2)
                        .find(|w| w[0] == "-n")
                        .map(|w| w[1].to_string())
                        .unwrap_or_else(|| "unnamed".to_string());
                    let mut windows = self.windows.borrow_mut();
                    let id = windows.iter().map(|w| w.id).max().unwrap_or(0) + 1;
                    let window = Window {
                        current_dir: "/home/user".to_string(),
                        id,
                        is_active: false,
                        name,
                        notification_pending: false,
                        running_command: "bash".to_string(),
                        started_at: None,
                    };
                    windows.push(window.clone());
                    Ok(String::new())
                }
                Some(&"kill-window") => {
                    let id_str = args
                        .windows(2)
                        .find(|w| w[0] == "-t")
                        .and_then(|w| w[1].split(':').nth(1))
                        .unwrap_or("0");
                    let id: u32 = id_str.parse().unwrap_or(0);
                    self.windows.borrow_mut().retain(|w| w.id != id);
                    Ok(String::new())
                }
                Some(&"select-window") => Ok(String::new()),
                Some(&"send-keys") => Ok(String::new()),
                Some(&"split-window") => Ok(self.pane_id.borrow().clone()),
                _ => Err(TmuxError::CommandFailed {
                    message: format!("unknown command: {:?}", args),
                    stderr: String::new(),
                    code: Some(1),
                }),
            }
        }

        fn execute_inherit_stdio(&self, args: &[&str]) -> Result<(), TmuxError> {
            self.commands
                .borrow_mut()
                .push(args.iter().map(|s| s.to_string()).collect());
            Ok(())
        }
    }

    #[test]
    fn test_session_name() {
        assert_eq!(SESSION_NAME, "agents-on-tmux");
    }

    #[test]
    fn test_create_session_if_not_exists_creates_new() {
        let executor = MockCommandExecutor::new();
        let driver = TmuxDriver::with_executor(executor);
        let result = driver.create_session_if_not_exists();
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_session_if_not_exists_existing() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        let result = driver.create_session_if_not_exists();
        assert!(result.is_ok());
    }

    #[test]
    fn test_parse_window_line_valid() {
        let line = "1\tagent-1\t0\tbash\t0\t/home/user/project";
        let window = parse_window_line(line).unwrap();
        assert_eq!(window.id, 1);
        assert_eq!(window.name, "agent-1");
        assert!(!window.notification_pending);
        assert_eq!(window.running_command, "bash");
        assert!(window.started_at.is_none());
        assert!(!window.is_active);
        assert_eq!(window.current_dir, "/home/user/project");
    }

    #[test]
    fn test_parse_window_line_with_notification() {
        let line = "2\tagent-2\t1\tzsh\t0\t/home/user";
        let window = parse_window_line(line).unwrap();
        assert_eq!(window.id, 2);
        assert_eq!(window.name, "agent-2");
        assert!(window.notification_pending);
        assert_eq!(window.running_command, "zsh");
        assert!(!window.is_active);
        assert_eq!(window.current_dir, "/home/user");
    }

    #[test]
    fn test_parse_window_line_active() {
        let line = "3\tagent-3\t0\tbash\t1\t/tmp";
        let window = parse_window_line(line).unwrap();
        assert_eq!(window.id, 3);
        assert!(window.is_active);
        assert_eq!(window.current_dir, "/tmp");
    }

    #[test]
    fn test_parse_window_line_invalid_format() {
        assert!(parse_window_line("invalid").is_none());
        assert!(parse_window_line("1\tname").is_none());
        assert!(parse_window_line("1\tname\t0").is_none());
        assert!(parse_window_line("1\tname\t0\tbash").is_none());
        assert!(parse_window_line("1\tname\t0\tbash\t0").is_none());
        assert!(parse_window_line("notanumber\tname\t0\tbash\t0\t/path").is_none());
    }

    #[test]
    fn test_list_windows_empty() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        let windows = driver.list_windows().unwrap();
        assert!(windows.is_empty());
    }

    #[test]
    fn test_list_windows_with_windows() {
        let executor = MockCommandExecutor::with_session();
        executor.windows.borrow_mut().push(Window {
            current_dir: "/home/user".to_string(),
            id: 1,
            is_active: false,
            name: "test-window".to_string(),
            notification_pending: false,
            running_command: "bash".to_string(),
            started_at: None,
        });
        let driver = TmuxDriver::with_executor(executor);
        let windows = driver.list_windows().unwrap();
        assert_eq!(windows.len(), 1);
        assert_eq!(windows[0].name, "test-window");
    }

    #[test]
    fn test_create_window() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        let window = driver.create_window("test-window").unwrap();
        assert_eq!(window.name, "test-window");
        assert!(window.started_at.is_none());
        assert!(!window.notification_pending);
    }

    #[test]
    fn test_kill_window() {
        let executor = MockCommandExecutor::with_session();
        executor.windows.borrow_mut().push(Window {
            current_dir: "/home/user".to_string(),
            id: 1,
            is_active: false,
            name: "to-kill".to_string(),
            notification_pending: false,
            running_command: "bash".to_string(),
            started_at: None,
        });
        let driver = TmuxDriver::with_executor(executor);
        assert!(driver.kill_window(1).is_ok());
        let windows = driver.list_windows().unwrap();
        assert!(windows.is_empty());
    }

    #[test]
    fn test_select_window() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        assert!(driver.select_window(1).is_ok());
    }

    #[test]
    fn test_attach_session() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        assert!(driver.attach_session().is_ok());
    }

    #[test]
    fn test_window_struct_fields() {
        let window = Window {
            current_dir: "/home/user/project".to_string(),
            id: 42,
            is_active: true,
            name: "test".to_string(),
            notification_pending: true,
            running_command: "echo hello".to_string(),
            started_at: Some(Instant::now() - Duration::from_secs(60)),
        };
        assert_eq!(window.id, 42);
        assert_eq!(window.name, "test");
        assert_eq!(window.running_command, "echo hello");
        assert!(window.started_at.is_some());
        assert!(window.notification_pending);
        assert!(window.is_active);
        assert_eq!(window.current_dir, "/home/user/project");
    }

    #[test]
    fn test_split_window() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        let pane_id = driver.split_window("aot --tui").unwrap();
        assert_eq!(pane_id, "%99");
    }

    #[test]
    fn test_split_window_command_args() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        let _ = driver.split_window("aot --tui");

        let commands = driver.executor.commands.borrow();
        let split_cmd = commands
            .iter()
            .find(|cmd| cmd.first().map(|s| s.as_str()) == Some("split-window"))
            .unwrap();

        assert!(split_cmd.contains(&"-h".to_string()));
        assert!(split_cmd.contains(&"-b".to_string()));
        assert!(split_cmd.contains(&"-l".to_string()));
        assert!(split_cmd.contains(&"30".to_string()));
        assert!(split_cmd.contains(&"-d".to_string()));
        assert!(split_cmd.contains(&"-t".to_string()));
        assert!(split_cmd.contains(&SESSION_NAME.to_string()));
        assert!(split_cmd.contains(&"aot --tui".to_string()));
    }

    #[test]
    fn test_check_inside_tmux_set() {
        unsafe { std::env::set_var("TMUX", "/tmp/tmux-1000/default,1234,0") };
        assert!(check_inside_tmux().is_ok());
        unsafe { std::env::remove_var("TMUX") };
    }

    #[test]
    fn test_check_inside_tmux_unset() {
        unsafe { std::env::remove_var("TMUX") };
        let result = check_inside_tmux();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TmuxError::NotInsideTmux));
    }

    #[test]
    fn test_inside_own_session_error_message() {
        let error = TmuxError::InsideOwnSession("agents-on-tmux".to_string());
        let message = error.to_string();
        assert_eq!(
            message,
            "Cannot run aot inside its own dedicated session 'agents-on-tmux'."
        );
    }
}
