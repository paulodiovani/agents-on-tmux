use serde_json::Value;

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

    /// Tool execution error with a message the LLM can act on.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            text: message.into(),
            structured: None,
            is_error: true,
        }
    }
}

/// Renders an AOT window id the way tools present it to agents (`@N`).
pub fn format_window_id(id: u32) -> String {
    format!("@{id}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
    fn test_format_window_id() {
        assert_eq!(format_window_id(3), "@3");
    }
}
