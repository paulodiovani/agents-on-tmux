use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{Value, json};

use super::tool::{CallContext, Tool, ToolOutput, dir_argument};
use crate::backends::tmux::{NewWindow as WindowOptions, Tmux};

/// Opens a new shell window in the AOT session.
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
        "Open a new shell window in AOT, in the background (the user's view does not switch). Optional dir: absolute, or relative to AOT's working directory (the default). Returns {\"index\", \"title\", \"cwd\"}."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "dir": {
                    "type": "string",
                    "description": "Working directory; absolute or relative to AOT's working directory",
                },
            },
            "additionalProperties": false,
        })
    }

    fn call(&self, arguments: &Value, _context: &CallContext) -> ToolOutput {
        let dir = match dir_argument(arguments, &self.default_dir) {
            Ok(dir) => dir,
            Err(error) => return error,
        };
        let options = WindowOptions {
            dir: Some(dir),
            detached: true,
            ..WindowOptions::default()
        };
        match self.tmux.create_window(&options) {
            Ok(window) => ToolOutput::json(json!({
                "index": window.id,
                "title": window.name,
                "cwd": window.current_dir,
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
    fn test_creates_detached_window_in_default_dir() {
        let dir = temp_dir();
        let expected = WindowOptions {
            name: None,
            dir: Some(dir.clone()),
            command: Vec::new(),
            detached: true,
        };
        let mut tmux = MockTmux::new();
        let created = window(5, "zsh", "zsh", &dir);
        tmux.expect_create_window()
            .withf(move |options| *options == expected)
            .times(1)
            .return_once(move |_| Ok(created));

        let output = tool(tmux).call(&json!({}), &CallContext::default());

        assert_eq!(
            output.structured,
            Some(json!({ "index": 5, "title": "zsh", "cwd": dir }))
        );
    }

    #[test]
    fn test_passes_requested_dir() {
        let dir = temp_dir();
        let expected_dir = dir.clone();
        let mut tmux = MockTmux::new();
        tmux.expect_create_window()
            .withf(move |options| options.dir.as_deref() == Some(expected_dir.as_str()))
            .times(1)
            .return_once(|_| Ok(window(1, "zsh", "zsh", "/")));

        let output = NewWindow::new(Arc::new(tmux), PathBuf::from("/nonexistent"))
            .call(&json!({ "dir": dir }), &CallContext::default());

        assert!(!output.is_error);
    }

    #[test]
    fn test_missing_dir_is_tool_error() {
        let output = tool(MockTmux::new()).call(
            &json!({ "dir": "no-such-dir-aot" }),
            &CallContext::default(),
        );
        assert!(output.is_error);
        assert!(output.text.starts_with("Directory not found:"));
    }

    #[test]
    fn test_tmux_failure_is_tool_error() {
        let mut tmux = MockTmux::new();
        tmux.expect_create_window()
            .return_once(|_| Err(TmuxError::WindowNotFound));

        let output = tool(tmux).call(&json!({}), &CallContext::default());

        assert!(output.is_error);
        assert_eq!(output.text, "Failed to create window: Window not found");
    }

    #[test]
    fn test_metadata() {
        let tool = tool(MockTmux::new());
        assert_eq!(tool.name(), "new_window");
        assert!(tool.description().contains("index"));
        assert_eq!(tool.input_schema()["properties"]["dir"]["type"], "string");
    }
}
