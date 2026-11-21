#![cfg(test)]

use std::sync::{Arc, Mutex};

use crate::backends::tmux::{KeyBinding, MockTmux, TmuxError, Window};

/// A small tmux session for TUI tests: its state is shared with every `MockTmux`
/// built from it, so tests can change windows and inspect calls after handing
/// the mock to the app.
#[derive(Clone)]
pub struct FakeTmux {
    /// Windows returned by `list_windows`, grown and shrunk by create and kill.
    pub windows: Arc<Mutex<Vec<Window>>>,
    /// Calls to the methods tests assert on, in order.
    pub calls: Arc<Mutex<Vec<String>>>,
    /// Bindings returned by `list_keys`; `None` makes it fail.
    pub keys: Option<Vec<KeyBinding>>,
    /// Value of the `prefix` option; `None` makes `show_options` fail.
    pub prefix: Option<String>,
    /// Makes `resize_pane` fail.
    pub resize_fails: bool,
}

impl FakeTmux {
    pub fn with_windows(windows: Vec<Window>) -> Self {
        Self {
            windows: Arc::new(Mutex::new(windows)),
            calls: Arc::new(Mutex::new(Vec::new())),
            keys: Some(default_key_bindings()),
            prefix: Some("C-b".to_string()),
            resize_fails: false,
        }
    }

    /// Calls recorded so far.
    pub fn recorded(&self) -> Vec<String> {
        self.calls.lock().unwrap().clone()
    }

    /// A `MockTmux` backed by this session's state.
    pub fn mock(&self) -> MockTmux {
        let mut mock = MockTmux::new();

        let (windows, calls) = (self.windows.clone(), self.calls.clone());
        mock.expect_list_windows().returning(move || {
            record(&calls, "list_windows".to_string());
            Ok(windows.lock().unwrap().clone())
        });

        let (windows, calls) = (self.windows.clone(), self.calls.clone());
        mock.expect_create_window()
            .returning(move |name, _dir, _command, _detached| {
                record(&calls, "create_window".to_string());
                let mut windows = windows.lock().unwrap();
                let window = Window {
                    id: windows.iter().map(|w| w.id).max().unwrap_or(0) + 1,
                    name: name.unwrap_or_default().to_string(),
                    pane_pid: 12345,
                    running_command: String::new(),
                    current_dir: "/home/user".to_string(),
                    is_active: false,
                    notification_pending: false,
                    started_at: None,
                };
                windows.push(window.clone());
                Ok(window)
            });

        let windows = self.windows.clone();
        mock.expect_kill_window().returning(move |id| {
            windows.lock().unwrap().retain(|w| w.id != id);
            Ok(())
        });

        let calls = self.calls.clone();
        mock.expect_select_window().returning(move |id| {
            record(&calls, format!("select_window {id}"));
            Ok(())
        });

        let calls = self.calls.clone();
        mock.expect_last_pane().returning(move || {
            record(&calls, "last_pane".to_string());
            Ok(())
        });

        let (calls, fails) = (self.calls.clone(), self.resize_fails);
        mock.expect_resize_pane().returning(move |pane_id, width| {
            record(&calls, format!("resize_pane {pane_id} {width}"));
            if fails {
                Err(command_failed("resize-pane"))
            } else {
                Ok(())
            }
        });

        let calls = self.calls.clone();
        mock.expect_prompt_rename_window()
            .returning(move |id, name| {
                record(&calls, format!("prompt_rename_window {id} {name}"));
                Ok(())
            });

        let keys = self.keys.clone();
        mock.expect_list_keys()
            .returning(move |_| keys.clone().ok_or_else(|| command_failed("list-keys")));

        let prefix = self.prefix.clone();
        mock.expect_show_options()
            .returning(move |name, _| match name {
                "prefix" => prefix.clone().ok_or_else(|| command_failed("show-options")),
                _ => Err(command_failed("show-options")),
            });

        mock
    }
}

fn record(calls: &Mutex<Vec<String>>, call: String) {
    calls.lock().unwrap().push(call);
}

fn command_failed(command: &str) -> TmuxError {
    TmuxError::CommandFailed {
        message: format!("{command} failed"),
        stderr: String::new(),
        code: Some(1),
    }
}

/// The default tmux bindings for the commands the footer hints at.
pub fn default_key_bindings() -> Vec<KeyBinding> {
    [
        ("C-b", "send-prefix"),
        ("c", "new-window -c \"#{pane_current_path}\""),
        ("n", "next-window"),
        ("p", "previous-window"),
        ("l", "last-window"),
        (",", "command-prompt -I \"#W\" { rename-window \"%%\" }"),
    ]
    .into_iter()
    .map(|(key, command)| KeyBinding {
        key: key.to_string(),
        command: command.to_string(),
    })
    .collect()
}
