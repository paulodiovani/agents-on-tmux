use std::sync::Arc;

use serde_json::{Value, json};

use super::tool::{CallContext, Tool, ToolOutput, find_window, index_argument, name_argument};
use crate::backends::tmux::Tmux;

/// Renames a window managed by AOT.
pub struct RenameWindow {
    tmux: Arc<dyn Tmux + Send + Sync>,
}

impl RenameWindow {
    /// Creates the tool over the nested AOT tmux session.
    pub fn new(tmux: Arc<dyn Tmux + Send + Sync>) -> Self {
        Self { tmux }
    }
}

impl Tool for RenameWindow {
    fn name(&self) -> &'static str {
        "rename_window"
    }

    fn description(&self) -> &'static str {
        "Rename a window. Takes the window index and the new name, a single line. Returns {\"index\", \"name\"}."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "index": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Window index",
                },
                "name": {
                    "type": "string",
                    "description": "New name, a single line",
                },
            },
            "required": ["index", "name"],
            "additionalProperties": false,
        })
    }

    fn call(&self, arguments: &Value, _context: &CallContext) -> ToolOutput {
        let name = match name_argument(arguments) {
            Ok(Some(name)) => name,
            Ok(None) => return ToolOutput::error("Missing required argument: name"),
            Err(error) => return error,
        };

        let index = match index_argument(arguments) {
            Ok(index) => index,
            Err(error) => return error,
        };

        if let Err(error) = find_window(self.tmux.as_ref(), index) {
            return error;
        }

        match self.tmux.rename_window(index, &name) {
            Ok(()) => ToolOutput::json(json!({ "index": index, "name": name })),
            Err(error) => ToolOutput::error(format!("Failed to rename window {index}: {error}")),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::tmux::{MockTmux, TmuxError};
    use crate::frontends::mcp::tools::tool::tests::window;
    use mockall::predicate::eq;

    fn tmux_with_window(index: u32) -> MockTmux {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .return_once(move || Ok(vec![window(index, "zsh", "zsh", "/")]));
        tmux
    }

    fn call(tmux: MockTmux, arguments: Value) -> ToolOutput {
        RenameWindow::new(Arc::new(tmux)).call(&arguments, &CallContext::default())
    }

    #[test]
    fn test_renames_window() {
        let mut tmux = tmux_with_window(3);
        tmux.expect_rename_window()
            .with(eq(3), eq("tests"))
            .times(1)
            .return_once(|_, _| Ok(()));

        let output = call(tmux, json!({ "index": 3, "name": "  tests " }));

        assert_eq!(
            output.structured,
            Some(json!({ "index": 3, "name": "tests" }))
        );
    }

    #[test]
    fn test_rejects_multi_line_or_blank_name() {
        let output = call(MockTmux::new(), json!({ "index": 3, "name": "a\nb" }));
        assert_eq!(output.text, "Invalid name: it must be a single line");

        let output = call(MockTmux::new(), json!({ "index": 3, "name": " " }));
        assert!(output.text.starts_with("Invalid name"));
    }

    #[test]
    fn test_unknown_index_is_tool_error() {
        let output = call(tmux_with_window(3), json!({ "index": 4, "name": "x" }));
        assert!(output.is_error);
        assert!(output.text.starts_with("No window or agent with index 4"));
    }

    #[test]
    fn test_invalid_index_is_tool_error() {
        let output = call(MockTmux::new(), json!({ "index": "x", "name": "x" }));
        assert!(output.text.starts_with("Invalid index"));
    }

    #[test]
    fn test_rename_failure_is_tool_error() {
        let mut tmux = tmux_with_window(3);
        tmux.expect_rename_window()
            .return_once(|_, _| Err(TmuxError::WindowNotFound));

        let output = call(tmux, json!({ "index": 3, "name": "x" }));

        assert_eq!(output.text, "Failed to rename window 3: Window not found");
    }

    #[test]
    fn test_metadata() {
        let tool = RenameWindow::new(Arc::new(MockTmux::new()));
        assert_eq!(tool.name(), "rename_window");
        assert!(!tool.read_only());
        assert!(tool.description().contains("index"));
        assert_eq!(tool.input_schema()["required"], json!(["index", "name"]));
    }
}
