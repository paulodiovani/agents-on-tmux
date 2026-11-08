use crate::backends::tmux::{KeyBinding, Tmux, TmuxError, Window};

/// In-memory `Tmux` double for MCP tool tests.
pub struct MockTmux {
    windows: Option<Vec<Window>>,
}

impl MockTmux {
    /// A session holding the given windows.
    pub fn with_windows(windows: Vec<Window>) -> Self {
        Self {
            windows: Some(windows),
        }
    }

    /// A session whose tmux commands all fail.
    pub fn failing() -> Self {
        Self { windows: None }
    }

    fn windows(&self) -> Result<Vec<Window>, TmuxError> {
        self.windows
            .clone()
            .ok_or_else(|| TmuxError::CommandFailed {
                message: "mock failure".into(),
                stderr: String::new(),
                code: Some(1),
            })
    }
}

impl Tmux for MockTmux {
    fn session_name(&self) -> &str {
        "agents-on-tmux"
    }

    fn socket_name(&self) -> Option<&str> {
        Some("agents-on-tmux")
    }

    fn create_session_if_not_exists(&self, _: Option<&str>) -> Result<(), TmuxError> {
        unimplemented!()
    }

    fn attach_session(&self) -> Result<(), TmuxError> {
        unimplemented!()
    }

    fn list_windows(&self) -> Result<Vec<Window>, TmuxError> {
        self.windows()
    }

    fn create_window(&self, _: &str) -> Result<Window, TmuxError> {
        unimplemented!()
    }

    fn kill_window(&self, _: u32) -> Result<(), TmuxError> {
        unimplemented!()
    }

    fn select_window(&self, _: u32) -> Result<(), TmuxError> {
        unimplemented!()
    }

    fn last_pane(&self) -> Result<(), TmuxError> {
        unimplemented!()
    }

    fn split_window(&self, _: &str, _: u16) -> Result<String, TmuxError> {
        unimplemented!()
    }

    fn resize_pane(&self, _: &str, _: u16) -> Result<(), TmuxError> {
        unimplemented!()
    }

    fn list_keys(&self, _: &str) -> Result<Vec<KeyBinding>, TmuxError> {
        unimplemented!()
    }

    fn show_options(&self, _: &str, _: bool) -> Result<String, TmuxError> {
        unimplemented!()
    }

    fn rename_window(&self, _: u32, _: &str) -> Result<(), TmuxError> {
        unimplemented!()
    }
}

/// Builds an inactive window running `command` in `cwd`.
pub fn window(id: u32, name: &str, command: &str, cwd: &str) -> Window {
    Window {
        id,
        name: name.into(),
        pane_pid: 1000 + id,
        running_command: command.into(),
        current_dir: cwd.into(),
        is_active: false,
        notification_pending: false,
        started_at: None,
    }
}
