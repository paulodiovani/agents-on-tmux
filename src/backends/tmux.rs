use std::process::Command;
use std::time::Instant;
use thiserror::Error;

use crate::backends::logger;

/// Contract for executing tmux commands.
pub trait CommandExecutor {
    /// Executes a tmux command and returns stdout on success.
    fn execute(&self, args: &[&str]) -> Result<String, TmuxError>;
    /// Executes a tmux command with inherited stdio (for attach).
    fn execute_inherit_stdio(&self, args: &[&str]) -> Result<(), TmuxError>;
}

/// Contract for interacting with tmux sessions and windows.
pub trait Tmux {
    /// Returns the session name this driver targets.
    fn session_name(&self) -> &str;
    /// Returns the socket name this driver targets, if any.
    fn socket_name(&self) -> Option<&str>;
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
    /// Switches to the last-active pane in the session.
    fn last_pane(&self) -> Result<(), TmuxError>;
    /// Splits the current window horizontally, creating a side pane of the
    /// given width in columns, and returns its pane id.
    fn split_window(&self, command: &str, width: u16) -> Result<String, TmuxError>;
    /// Resizes the given pane to an absolute width in columns.
    fn resize_pane(&self, pane_id: &str, width: u16) -> Result<(), TmuxError>;
    /// Lists the key bindings of the given key table (e.g. "prefix").
    fn list_keys(&self, table: &str) -> Result<Vec<KeyBinding>, TmuxError>;
    /// Returns the value of a single option: this driver's session by
    /// default, or the global scope when `global` is set.
    fn show_options(&self, name: &str, global: bool) -> Result<String, TmuxError>;
    /// Opens the tmux command prompt on the calling client, pre-filled with
    /// `initial`, running `template` on accept (`%%` expands to the input).
    fn command_prompt(&self, initial: &str, template: &str) -> Result<(), TmuxError>;
}

pub const SESSION_NAME: &str = "agents-on-tmux";
pub const SOCKET_NAME: &str = "agents-on-tmux";

/// Errors that can occur during tmux operations.
#[derive(Debug, Error)]
pub enum TmuxError {
    #[error("Command failed: {message}")]
    CommandFailed {
        message: String,
        stderr: String,
        code: Option<i32>,
    },
    #[error("Cannot run aot inside its own tmux server (socket '{0}')")]
    InsideOwnServer(String),
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

/// A tmux key binding: the bound key and the command it runs.
#[derive(Debug, Clone, PartialEq)]
pub struct KeyBinding {
    pub command: String,
    pub key: String,
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

/// Detects the parent tmux server socket by parsing the TMUX environment variable.
/// Returns the socket name (e.g., "default", "agents-on-tmux").
pub fn detect_parent_socket() -> Result<String, TmuxError> {
    check_inside_tmux()?;

    let tmux_env = std::env::var("TMUX").map_err(|_| TmuxError::NotInsideTmux)?;

    // TMUX format: <socket-path>,<server-pid>,<session-id>
    // Example: /tmp/tmux-1000/agents-on-tmux,12345,0
    let socket_path = tmux_env.split(',').next().ok_or(TmuxError::NotInsideTmux)?;

    // Extract socket name from path (last component)
    let socket_name = socket_path.rsplit('/').next().unwrap_or("default");

    Ok(socket_name.to_string())
}

/// Real tmux command executor that calls the tmux binary.
pub struct ShellCommandExecutor {
    socket: Option<String>,
}

impl ShellCommandExecutor {
    /// Creates a new executor targeting the default tmux server.
    pub fn new() -> Self {
        Self { socket: None }
    }

    /// Creates a new executor targeting a specific tmux server via `-L <socket>`.
    pub fn new_with_socket(socket: &str) -> Self {
        Self {
            socket: Some(socket.to_string()),
        }
    }
}

impl CommandExecutor for ShellCommandExecutor {
    fn execute(&self, args: &[&str]) -> Result<String, TmuxError> {
        let mut cmd = Command::new("tmux");
        if let Some(ref socket) = self.socket {
            cmd.args(["-L", socket]);
        }
        let output = cmd
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
        let mut cmd = Command::new("tmux");
        if let Some(ref socket) = self.socket {
            cmd.args(["-L", socket]);
        }
        let status = cmd
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
    socket: Option<String>,
}

impl TmuxDriver<ShellCommandExecutor> {
    /// Creates a new TmuxDriver with the real command executor targeting the default tmux server.
    pub fn new(session: &str) -> Self {
        Self {
            executor: ShellCommandExecutor::new(),
            session: session.to_string(),
            socket: None,
        }
    }

    /// Creates a new TmuxDriver with the real command executor targeting a specific tmux server.
    pub fn new_with_socket(session: &str, socket: &str) -> Self {
        Self {
            executor: ShellCommandExecutor::new_with_socket(socket),
            session: session.to_string(),
            socket: Some(socket.to_string()),
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
            socket: None,
        }
    }
}

fn parse_window_line(line: &str) -> Option<Window> {
    let parts: Vec<&str> = line.split('\t').collect();
    if parts.len() != 6 {
        return None;
    }

    let running_command = parts[0].to_string();
    let current_dir = parts[1].to_string();
    let notification_pending = parts[2] == "1";
    let is_active = parts[3] == "1";
    let id = parts[4].parse::<u32>().ok()?;
    let name = parts[5].to_string();

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

/// Parses one `list-keys` output line, shaped `bind-key [-r] -T <table>
/// <key> <command…>`. Fields split on any whitespace because list-keys pads
/// its columns; single quotes around escaped keys (e.g. '"') are stripped.
fn parse_key_line(line: &str) -> Option<KeyBinding> {
    let fields: Vec<&str> = line.split_whitespace().collect();
    let (key, command) = match fields.as_slice() {
        ["bind-key", "-T", _, key, cmd @ ..] | ["bind-key", "-r", "-T", _, key, cmd @ ..] => {
            (*key, cmd.join(" "))
        }
        _ => return None,
    };
    let key = key.trim_matches('\'');

    if key.is_empty() || command.is_empty() {
        return None;
    }
    Some(KeyBinding {
        command,
        key: key.to_string(),
    })
}

impl<E: CommandExecutor> Tmux for TmuxDriver<E> {
    /// Returns the session name this driver targets.
    fn session_name(&self) -> &str {
        &self.session
    }

    /// Returns the socket name this driver targets, if any.
    fn socket_name(&self) -> Option<&str> {
        self.socket.as_deref()
    }

    /// Ensures the tmux session exists, creating it if necessary.
    /// Also checks if we're already running on the same socket to prevent nested execution.
    fn create_session_if_not_exists(&self) -> Result<(), TmuxError> {
        // Check if we're already running on the same socket
        if let Some(ref driver_socket) = self.socket
            && let Ok(parent_socket) = detect_parent_socket()
        {
            logger::debug(&format!(
                "tmux: parent socket: {}, driver socket: {}",
                parent_socket, driver_socket
            ));
            if parent_socket == *driver_socket {
                return Err(TmuxError::InsideOwnServer(parent_socket));
            }
        }

        let has_session = self.executor.execute(&["has-session", "-t", &self.session]);

        if has_session.is_err() {
            logger::debug(&format!(
                "tmux: creating session '{}' on socket {:?}",
                self.session, self.socket
            ));
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
            "#{pane_current_command}\t#{pane_current_path}\t#{window_activity_flag}\t#{window_active}\t#{window_index}\t#{window_name}",
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

    /// Switches to the last-active pane in the session.
    fn last_pane(&self) -> Result<(), TmuxError> {
        self.executor.execute(&["last-pane", "-t", &self.session])?;
        Ok(())
    }

    /// Splits the current window horizontally, creating a side pane.
    fn split_window(&self, command: &str, width: u16) -> Result<String, TmuxError> {
        let width = width.to_string();
        self.executor
            .execute(&[
                "split-window",
                "-h",
                "-b",
                "-l",
                &width,
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

    /// Resizes the given pane to an absolute width in columns. The target must
    /// be explicit: without -t tmux resolves the default to the active pane.
    fn resize_pane(&self, pane_id: &str, width: u16) -> Result<(), TmuxError> {
        let width = width.to_string();
        self.executor
            .execute(&["resize-pane", "-t", pane_id, "-x", &width])?;
        Ok(())
    }

    /// Lists the key bindings of the given key table (e.g. "prefix"), as
    /// `bind-key [-r] -T <table> <key> <command>` lines.
    fn list_keys(&self, table: &str) -> Result<Vec<KeyBinding>, TmuxError> {
        let output = self.executor.execute(&["list-keys", "-T", table])?;
        Ok(output.lines().filter_map(parse_key_line).collect())
    }

    /// Returns the value of a single option: this driver's session by
    /// default, or the global scope when `global` is set.
    fn show_options(&self, name: &str, global: bool) -> Result<String, TmuxError> {
        let args = if global {
            vec!["show-options", "-gv", name]
        } else {
            vec!["show-options", "-v", "-t", self.session.as_str(), name]
        };
        self.executor.execute(&args).map(|s| s.trim().to_string())
    }

    /// Opens the tmux command prompt on the calling client, pre-filled with
    /// `initial`, running `template` on accept (`%%` expands to the input).
    fn command_prompt(&self, initial: &str, template: &str) -> Result<(), TmuxError> {
        self.executor
            .execute(&["command-prompt", "-I", initial, template])
            .map(|_| ())
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
        session_prefix: RefCell<Option<String>>,
        windows: RefCell<Vec<Window>>,
    }

    impl MockCommandExecutor {
        fn new() -> Self {
            Self {
                commands: RefCell::new(Vec::new()),
                pane_id: RefCell::new("%99".to_string()),
                session_exists: RefCell::new(false),
                session_prefix: RefCell::new(None),
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
                Some(&"list-windows") => {
                    let windows = self.windows.borrow();
                    let output: Vec<String> = windows
                        .iter()
                        .map(|w| {
                            format!(
                                "{}\t{}\t{}\t{}\t{}\t{}",
                                w.running_command,
                                w.current_dir,
                                if w.notification_pending { "1" } else { "0" },
                                if w.is_active { "1" } else { "0" },
                                w.id,
                                w.name
                            )
                        })
                        .collect();
                    Ok(output.join("\n"))
                }
                Some(&"new-session") => {
                    *self.session_exists.borrow_mut() = true;
                    Ok(String::new())
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
                Some(&"select-window") => Ok(String::new()),
                Some(&"last-pane") => Ok(String::new()),
                Some(&"send-keys") => Ok(String::new()),
                Some(&"set-option") => Ok(String::new()),
                Some(&"resize-pane") => Ok(String::new()),
                Some(&"command-prompt") => Ok(String::new()),
                Some(&"split-window") => Ok(self.pane_id.borrow().clone()),
                Some(&"list-keys") => Ok(concat!(
                    "bind-key -r -T prefix Up select-pane -U\n",
                    // Non-repeat rows pad the -r slot with blanks.
                    "bind-key    -T prefix C-b send-prefix\n",
                    "bind-key    -T prefix c new-window -c \"#{pane_current_path}\"\n",
                    "bind-key    -T prefix n next-window\n",
                    "bind-key    -T prefix p previous-window\n",
                    "bind-key    -T prefix l last-window\n",
                    "bind-key    -T prefix , command-prompt -I \"#W\" { rename-window \"%%\" }\n",
                )
                .to_string()),
                Some(&"show-options") => {
                    if !args.contains(&"prefix") {
                        return Err(TmuxError::CommandFailed {
                            message: format!("unknown option: {:?}", args),
                            stderr: String::new(),
                            code: Some(1),
                        });
                    }
                    if args.contains(&"-gv") {
                        Ok("C-b\n".to_string())
                    } else if let Some(prefix) = self.session_prefix.borrow().as_ref() {
                        Ok(format!("{prefix}\n"))
                    } else {
                        Err(TmuxError::CommandFailed {
                            message: format!("option not set: {:?}", args),
                            stderr: String::new(),
                            code: Some(1),
                        })
                    }
                }
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
    fn test_socket_name() {
        assert_eq!(SOCKET_NAME, "agents-on-tmux");
    }

    #[test]
    fn test_driver_session_name() {
        let driver = TmuxDriver::new("test-session");
        assert_eq!(driver.session_name(), "test-session");
    }

    #[test]
    fn test_driver_socket_name_none() {
        let driver = TmuxDriver::new("test-session");
        assert_eq!(driver.socket_name(), None);
    }

    #[test]
    fn test_driver_socket_name_some() {
        let driver = TmuxDriver::new_with_socket("test-session", "test-socket");
        assert_eq!(driver.socket_name(), Some("test-socket"));
    }

    #[test]
    fn test_create_session_if_not_exists_creates_new() {
        let executor = MockCommandExecutor::new();
        let driver = TmuxDriver::with_executor(executor);
        let result = driver.create_session_if_not_exists();
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_session_existing_skips_set_option() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        driver.create_session_if_not_exists().unwrap();

        let commands = driver.executor.commands.borrow();
        assert!(
            !commands
                .iter()
                .any(|cmd| cmd.first().map(|s| s.as_str()) == Some("set-option"))
        );
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
        let line = "bash\t/home/user/project\t0\t0\t1\tagent-1";
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
        let line = "zsh\t/home/user\t1\t0\t2\tagent-2";
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
        let line = "bash\t/tmp\t0\t1\t3\tagent-3";
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
        assert!(parse_window_line("bash\t/path\t0\t0\tnotanumber\tname").is_none());
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
        let pane_id = driver.split_window("aot --tui", 35).unwrap();
        assert_eq!(pane_id, "%99");
    }

    #[test]
    fn test_split_window_command_args() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        let _ = driver.split_window("aot --tui", 35);

        let commands = driver.executor.commands.borrow();
        let split_cmd = commands
            .iter()
            .find(|cmd| cmd.first().map(|s| s.as_str()) == Some("split-window"))
            .unwrap();

        assert!(split_cmd.contains(&"-h".to_string()));
        assert!(split_cmd.contains(&"-b".to_string()));
        assert!(split_cmd.contains(&"-l".to_string()));
        assert!(split_cmd.contains(&"35".to_string()));
        assert!(split_cmd.contains(&"-t".to_string()));
        assert!(split_cmd.contains(&SESSION_NAME.to_string()));
        assert!(split_cmd.contains(&"aot --tui".to_string()));
    }

    #[test]
    fn test_split_window_custom_width() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        let _ = driver.split_window("aot --tui", 50);

        let commands = driver.executor.commands.borrow();
        let split_cmd = commands
            .iter()
            .find(|cmd| cmd.first().map(|s| s.as_str()) == Some("split-window"))
            .unwrap();

        assert!(split_cmd.contains(&"50".to_string()));
    }

    #[test]
    fn test_resize_pane_command_args() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        driver.resize_pane("%5", 35).unwrap();

        let commands = driver.executor.commands.borrow();
        let resize_cmd = commands
            .iter()
            .find(|cmd| cmd.first().map(|s| s.as_str()) == Some("resize-pane"))
            .unwrap();

        // Explicit -t: without it tmux resolves the default target to the
        // active pane, not necessarily the calling one.
        assert!(resize_cmd.contains(&"-t".to_string()));
        assert!(resize_cmd.contains(&"%5".to_string()));
        assert!(resize_cmd.contains(&"-x".to_string()));
        assert!(resize_cmd.contains(&"35".to_string()));
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
    fn test_inside_own_server_error_message() {
        let error = TmuxError::InsideOwnServer("agents-on-tmux".to_string());
        let message = error.to_string();
        assert_eq!(
            message,
            "Cannot run aot inside its own tmux server (socket 'agents-on-tmux')"
        );
    }

    #[test]
    fn test_detect_parent_socket_custom() {
        unsafe { std::env::set_var("TMUX", "/tmp/tmux-1000/agents-on-tmux,1234,0") };
        let result = detect_parent_socket();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "agents-on-tmux");
        unsafe { std::env::remove_var("TMUX") };
    }

    #[test]
    fn test_detect_parent_socket_default() {
        unsafe { std::env::set_var("TMUX", "/tmp/tmux-1000/default,1234,0") };
        let result = detect_parent_socket();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "default");
        unsafe { std::env::remove_var("TMUX") };
    }

    #[test]
    fn test_detect_parent_socket_not_inside_tmux() {
        unsafe { std::env::remove_var("TMUX") };
        let result = detect_parent_socket();
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), TmuxError::NotInsideTmux));
    }

    #[test]
    fn test_parse_key_line_valid() {
        let binding = parse_key_line("bind-key -T prefix c new-window").unwrap();
        assert_eq!(binding.key, "c");
        assert_eq!(binding.command, "new-window");
    }

    #[test]
    fn test_parse_key_line_tolerates_column_padding() {
        // list-keys aligns columns; non-repeat rows pad the -r slot.
        let binding = parse_key_line("bind-key    -T prefix   C-b send-prefix").unwrap();
        assert_eq!(binding.key, "C-b");
        assert_eq!(binding.command, "send-prefix");
    }

    #[test]
    fn test_parse_key_line_repeat_binding() {
        let binding = parse_key_line("bind-key -r -T prefix Up select-pane -U").unwrap();
        assert_eq!(binding.key, "Up");
        assert_eq!(binding.command, "select-pane -U");
    }

    #[test]
    fn test_parse_key_line_strips_quotes_around_key() {
        let binding = parse_key_line("bind-key -T prefix '\"' split-window").unwrap();
        assert_eq!(binding.key, "\"");
        assert_eq!(binding.command, "split-window");
    }

    #[test]
    fn test_parse_key_line_keeps_command_spaces() {
        let binding = parse_key_line("bind-key -T prefix c new-window -c /tmp").unwrap();
        assert_eq!(binding.command, "new-window -c /tmp");
    }

    #[test]
    fn test_parse_key_line_invalid() {
        assert!(parse_key_line("no-bind-here").is_none());
        assert!(parse_key_line("bind-key").is_none());
        assert!(parse_key_line("bind-key -T").is_none());
        assert!(parse_key_line("bind-key -T prefix").is_none());
        assert!(parse_key_line("bind-key -T prefix c").is_none());
        assert!(parse_key_line("bind-key -X prefix c new-window").is_none());
    }

    #[test]
    fn test_list_keys_parses_bindings() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        let keys = driver.list_keys("prefix").unwrap();
        assert_eq!(keys.len(), 7);

        let find = |command: &str| {
            keys.iter()
                .find(|b| b.command == command)
                .unwrap_or_else(|| panic!("no binding for {command}"))
        };
        assert_eq!(find("send-prefix").key, "C-b");
        assert_eq!(find("new-window -c \"#{pane_current_path}\"").key, "c");
        assert_eq!(find("next-window").key, "n");
        assert_eq!(find("last-window").key, "l");
        assert_eq!(
            find("command-prompt -I \"#W\" { rename-window \"%%\" }").key,
            ","
        );
    }

    #[test]
    fn test_list_keys_command_args() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        let _ = driver.list_keys("prefix");

        let commands = driver.executor.commands.borrow();
        let list_keys_cmd = commands
            .iter()
            .find(|cmd| cmd.first().map(|s| s.as_str()) == Some("list-keys"))
            .unwrap();
        assert_eq!(
            list_keys_cmd.as_slice(),
            [
                "list-keys".to_string(),
                "-T".to_string(),
                "prefix".to_string()
            ]
        );
    }

    #[test]
    fn test_show_options_global_prefix() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        assert_eq!(driver.show_options("prefix", true).unwrap(), "C-b");
    }

    #[test]
    fn test_show_options_session_prefix_override() {
        let executor = MockCommandExecutor::with_session();
        *executor.session_prefix.borrow_mut() = Some("C-a".to_string());
        let driver = TmuxDriver::with_executor(executor);
        assert_eq!(driver.show_options("prefix", false).unwrap(), "C-a");
    }

    #[test]
    fn test_show_options_session_prefix_unset_fails() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        assert!(driver.show_options("prefix", false).is_err());
    }

    #[test]
    fn test_show_options_command_args() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        let _ = driver.show_options("prefix", false);
        let _ = driver.show_options("prefix", true);

        let commands = driver.executor.commands.borrow();
        let mut show_options_cmds = commands
            .iter()
            .filter(|cmd| cmd.first().map(|s| s.as_str()) == Some("show-options"));
        assert_eq!(
            show_options_cmds.next().unwrap().as_slice(),
            [
                "show-options".to_string(),
                "-v".to_string(),
                "-t".to_string(),
                SESSION_NAME.to_string(),
                "prefix".to_string(),
            ]
        );
        assert_eq!(
            show_options_cmds.next().unwrap().as_slice(),
            [
                "show-options".to_string(),
                "-gv".to_string(),
                "prefix".to_string(),
            ]
        );
    }

    #[test]
    fn test_show_options_trims_output() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        let value = driver.show_options("prefix", true).unwrap();
        assert!(!value.contains('\n'));
    }

    #[test]
    fn test_command_prompt_command_args() {
        let executor = MockCommandExecutor::with_session();
        let driver = TmuxDriver::with_executor(executor);
        driver
            .command_prompt("#W", "rename-window -t \"aot:1\" \"%%\"")
            .unwrap();

        let commands = driver.executor.commands.borrow();
        let prompt_cmd = commands
            .iter()
            .find(|cmd| cmd.first().map(|s| s.as_str()) == Some("command-prompt"))
            .unwrap();
        assert_eq!(
            prompt_cmd.as_slice(),
            [
                "command-prompt".to_string(),
                "-I".to_string(),
                "#W".to_string(),
                "rename-window -t \"aot:1\" \"%%\"".to_string(),
            ]
        );
    }

    #[test]
    fn test_key_binding_struct_fields() {
        let binding = KeyBinding {
            command: "next-window".to_string(),
            key: "n".to_string(),
        };
        assert_eq!(binding.key, "n");
        assert_eq!(binding.command, "next-window");
    }
}
