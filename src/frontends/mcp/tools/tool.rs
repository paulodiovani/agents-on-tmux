use serde_json::Value;

use crate::backends::tmux::{Tmux, Window};

/// An MCP tool exposed to agents through the AOT MCP server.
pub trait Tool: Send + Sync {
    /// Unique tool name used by clients in `tools/call`.
    fn name(&self) -> &'static str;
    /// Description written for the controlling LLM.
    fn description(&self) -> &'static str;
    /// JSON Schema of the tool arguments.
    fn input_schema(&self) -> Value;
    /// Runs the tool for the given arguments on behalf of the calling connection.
    fn call(&self, arguments: &Value, context: &CallContext) -> ToolOutput;
}

/// Identity of the agent connection issuing a request.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CallContext {
    /// tmux pane id (`%N`) of the agent that spawned the shim, when known.
    pub pane: Option<String>,
}

/// Result of a tool call, rendered into an MCP `CallToolResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    /// Text content returned to the LLM.
    pub text: String,
    /// Optional machine-readable result, sent as `structuredContent`.
    pub structured: Option<Value>,
    /// Marks a tool execution error the LLM can recover from.
    pub is_error: bool,
}

impl ToolOutput {
    /// Successful result carrying `value` both as structured content and as JSON text.
    pub fn json(value: Value) -> Self {
        Self {
            text: value.to_string(),
            structured: Some(value),
            is_error: false,
        }
    }

    /// Successful plain-text result.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            structured: None,
            is_error: false,
        }
    }

    /// Tool execution error with a message the LLM can act on.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            text: message.into(),
            structured: None,
            is_error: true,
        }
    }
}

/// Reads the required window `index` argument (an integer, or a numeric string).
pub fn index_argument(arguments: &Value) -> Result<u32, ToolOutput> {
    let value = arguments.get("index").ok_or_else(|| {
        ToolOutput::error(
            "Missing required argument: index (the integer window index from list_windows or list_agents)",
        )
    })?;
    let index = match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    };
    index
        .and_then(|index| u32::try_from(index).ok())
        .ok_or_else(|| {
            ToolOutput::error(format!(
                "Invalid index {value}: pass the integer window index (e.g. 3) from list_windows or list_agents, not a title"
            ))
        })
}

/// Finds the window with the given index in the AOT session.
pub fn find_window(tmux: &dyn Tmux, index: u32) -> Result<Window, ToolOutput> {
    let windows = tmux
        .list_windows()
        .map_err(|error| ToolOutput::error(format!("Failed to list windows: {error}")))?;
    windows
        .into_iter()
        .find(|window| window.id == index)
        .ok_or_else(|| {
            ToolOutput::error(format!(
                "No window with index {index}; call list_windows or list_agents for the current indexes"
            ))
        })
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::backends::tmux::{MockTmux, TmuxError};
    use serde_json::json;

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

    #[test]
    fn test_json_output_mirrors_value_as_text() {
        let output = ToolOutput::json(json!({ "a": 1 }));
        assert_eq!(output.text, r#"{"a":1}"#);
        assert_eq!(output.structured, Some(json!({ "a": 1 })));
        assert!(!output.is_error);
    }

    #[test]
    fn test_error_output() {
        let output = ToolOutput::error("boom");
        assert_eq!(output.text, "boom");
        assert_eq!(output.structured, None);
        assert!(output.is_error);
    }

    #[test]
    fn test_text_output() {
        let output = ToolOutput::text("hello");
        assert_eq!(output.text, "hello");
        assert_eq!(output.structured, None);
        assert!(!output.is_error);
    }

    #[test]
    fn test_index_argument_accepts_integer_and_numeric_string() {
        assert_eq!(index_argument(&json!({ "index": 3 })), Ok(3));
        assert_eq!(index_argument(&json!({ "index": " 7 " })), Ok(7));
    }

    #[test]
    fn test_index_argument_missing() {
        let error = index_argument(&json!({})).unwrap_err();
        assert!(error.is_error);
        assert!(error.text.starts_with("Missing required argument: index"));
    }

    #[test]
    fn test_index_argument_rejects_invalid_values() {
        for value in [
            json!("@3"),
            json!("shell"),
            json!(-1),
            json!(1.5),
            json!(null),
            json!(u64::MAX),
        ] {
            let error = index_argument(&json!({ "index": value })).unwrap_err();
            assert!(error.is_error);
            assert!(error.text.starts_with("Invalid index"), "{}", error.text);
        }
    }

    #[test]
    fn test_find_window_returns_matching_window() {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .return_once(|| Ok(vec![window(1, "a", "zsh", "/"), window(3, "b", "zsh", "/")]));

        assert_eq!(find_window(&tmux, 3).unwrap().name, "b");
    }

    #[test]
    fn test_find_window_unknown_index() {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .return_once(|| Ok(vec![window(1, "a", "zsh", "/")]));

        let error = find_window(&tmux, 9).unwrap_err();
        assert!(error.is_error);
        assert!(error.text.starts_with("No window with index 9"));
    }

    #[test]
    fn test_find_window_tmux_failure() {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .return_once(|| Err(TmuxError::WindowNotFound));

        let error = find_window(&tmux, 1).unwrap_err();
        assert_eq!(error.text, "Failed to list windows: Window not found");
    }
}
