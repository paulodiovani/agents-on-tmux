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

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::backends::tmux::{KeyBinding, Tmux, TmuxError, Window};
    use serde_json::json;

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
}
