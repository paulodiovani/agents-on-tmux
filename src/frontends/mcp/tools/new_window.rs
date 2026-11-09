use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};

use super::tool::{CallContext, Tool, ToolOutput, command_argument, dir_argument, name_argument};
use crate::backends::tmux::Tmux;

/// Opens a new window in the AOT session, running a shell or a given command.
pub struct NewWindow {
    tmux: Arc<dyn Tmux + Send + Sync>,
    default_dir: PathBuf,
}

impl NewWindow {
    /// Creates the tool over the nested AOT tmux session; windows open in
    /// `default_dir` unless the caller passes a directory.
    pub fn new(tmux: Arc<dyn Tmux + Send + Sync>, default_dir: PathBuf) -> Self {
        Self { tmux, default_dir }
    }
}

impl Tool for NewWindow {
    fn name(&self) -> &'static str {
        "new_window"
    }

    fn description(&self) -> &'static str {
        "Open a window in the background running a shell, or the given command until it exits. Optional: command (program and arguments), name, and dir (absolute or relative to AOT's working directory). Returns {\"index\", \"name\", \"current_dir\"}."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 1,
                    "description": "Program followed by its arguments",
                },
                "name": {
                    "type": "string",
                    "description": "Window name, a single line",
                },
                "dir": {
                    "type": "string",
                    "description": "Working directory, absolute or relative to AOT's working directory",
                },
            },
            "additionalProperties": false,
        })
    }

    fn call(&self, arguments: &Value, _context: &CallContext) -> ToolOutput {
        let command = match command_argument(arguments) {
            Ok(command) => command,
            Err(error) => return error,
        };

        let name = match name_argument(arguments) {
            Ok(name) => name,
            Err(error) => return error,
        };

        let dir = match dir_argument(arguments, &self.default_dir) {
            Ok(dir) => dir,
            Err(error) => return error,
        };

        match self
            .tmux
            .create_window(name.as_deref(), Some(&dir), command.as_deref(), true)
        {
            Ok(window) => ToolOutput::json(json!({
                "index": window.id,
                "name": window.name,
                "current_dir": window.current_dir,
            })),
            Err(error) => ToolOutput::error(format!("Failed to create window: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::tmux::{MockTmux, TmuxError};
    use crate::frontends::mcp::tools::tool::tests::window;

    fn temp_dir() -> String {
        std::env::temp_dir().to_string_lossy().into_owned()
    }

    fn tool(tmux: MockTmux) -> NewWindow {
        NewWindow::new(Arc::new(tmux), std::env::temp_dir())
    }

    #[test]
    fn test_creates_detached_shell_window_in_default_dir() {
        let dir = temp_dir();
        let expected_dir = dir.clone();
        let mut tmux = MockTmux::new();
        let created = window(5, "zsh", "zsh", &dir);
        tmux.expect_create_window()
            .withf(move |name, dir, command, detached| {
                name.is_none()
                    && *dir == Some(expected_dir.as_str())
                    && command.is_none()
                    && *detached
            })
            .times(1)
            .return_once(move |_, _, _, _| Ok(created));

        let output = tool(tmux).call(&json!({}), &CallContext::default());

        assert_eq!(
            output.structured,
            Some(json!({ "index": 5, "name": "zsh", "current_dir": dir }))
        );
    }

    #[test]
    fn test_passes_name_command_and_dir() {
        let dir = temp_dir();
        let expected_dir = dir.clone();
        let mut tmux = MockTmux::new();
        tmux.expect_create_window()
            .withf(move |name, dir, command, detached| {
                *name == Some("watcher")
                    && *dir == Some(expected_dir.as_str())
                    && *command == Some(&["cargo".to_string(), "watch".to_string()][..])
                    && *detached
            })
            .times(1)
            .return_once(|_, _, _, _| Ok(window(1, "watcher", "cargo", "/")));

        let output = NewWindow::new(Arc::new(tmux), PathBuf::from("/nonexistent")).call(
            &json!({ "dir": dir, "name": " watcher ", "command": ["cargo", "watch"] }),
            &CallContext::default(),
        );

        assert!(!output.is_error, "{}", output.text);
    }

    #[test]
    fn test_invalid_arguments_are_tool_errors() {
        for (arguments, prefix) in [
            (json!({ "command": "cargo watch" }), "Invalid command"),
            (json!({ "name": "a\nb" }), "Invalid name"),
            (json!({ "dir": "no-such-dir-aot" }), "Directory not found:"),
        ] {
            let output = tool(MockTmux::new()).call(&arguments, &CallContext::default());
            assert!(output.is_error);
            assert!(output.text.starts_with(prefix), "{}", output.text);
        }
    }

    #[test]
    fn test_tmux_failure_is_tool_error() {
        let mut tmux = MockTmux::new();
        tmux.expect_create_window()
            .return_once(|_, _, _, _| Err(TmuxError::WindowNotFound));

        let output = tool(tmux).call(&json!({}), &CallContext::default());

        assert!(output.is_error);
        assert_eq!(output.text, "Failed to create window: Window not found");
    }

    #[test]
    fn test_metadata() {
        let tool = tool(MockTmux::new());
        let schema = tool.input_schema();
        assert_eq!(tool.name(), "new_window");
        assert!(!tool.read_only());
        assert!(tool.description().contains("shell"));
        assert_eq!(schema["properties"]["command"]["type"], "array");
        assert_eq!(schema["properties"]["name"]["type"], "string");
        assert_eq!(schema["properties"]["dir"]["type"], "string");
    }
}
