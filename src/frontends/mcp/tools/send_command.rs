use std::sync::Arc;

use serde_json::{Value, json};

use super::tool::{CallContext, Tool, ToolOutput, send_target, text_argument};
use crate::backends::tmux::Tmux;

/// Runs a shell command in a non-agent window of the AOT session.
pub struct SendCommand {
    tmux: Arc<dyn Tmux + Send + Sync>,
}

impl SendCommand {
    /// Creates the tool over the nested AOT tmux session.
    pub fn new(tmux: Arc<dyn Tmux + Send + Sync>) -> Self {
        Self { tmux }
    }
}

impl Tool for SendCommand {
    fn name(&self) -> &'static str {
        "send_command"
    }

    fn description(&self) -> &'static str {
        "Type a shell command into a non-agent window of the AOT session and press Enter. Takes the integer window index from list_windows (never a title) and the command. The command runs asynchronously: call read_window afterwards to see its output. Agent windows are refused; use send_prompt for them. Your own window is refused."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "index": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Window index from list_windows",
                },
                "command": {
                    "type": "string",
                    "description": "Shell command to type and run",
                },
            },
            "required": ["index", "command"],
            "additionalProperties": false,
        })
    }

    fn call(&self, arguments: &Value, context: &CallContext) -> ToolOutput {
        let command = match text_argument(arguments, "command") {
            Ok(command) => command,
            Err(error) => return error,
        };
        let window = match send_target(self.tmux.as_ref(), arguments, context, false) {
            Ok(window) => window,
            Err(error) => return error,
        };
        match self.tmux.send_keys(window.id, &command, true) {
            Ok(()) => ToolOutput::text(format!(
                "Sent command to window {}; use read_window to see its output",
                window.id
            )),
            Err(error) => ToolOutput::error(format!(
                "Failed to send command to window {}: {error}",
                window.id
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::tmux::{MockTmux, TmuxError, Window};
    use crate::frontends::mcp::tools::tool::tests::window;
    use mockall::predicate::eq;

    fn tmux_with(windows: Vec<Window>) -> MockTmux {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .returning(move || Ok(windows.clone()));
        tmux.expect_window_for_pane().returning(|_| Ok(9));
        tmux
    }

    fn caller() -> CallContext {
        CallContext {
            pane: Some("%1".into()),
        }
    }

    #[test]
    fn test_types_command_and_presses_enter() {
        let mut tmux = tmux_with(vec![window(3, "shell", "zsh", "/")]);
        tmux.expect_send_keys()
            .with(eq(3), eq("cargo test"), eq(true))
            .times(1)
            .return_once(|_, _, _| Ok(()));

        let output = SendCommand::new(Arc::new(tmux))
            .call(&json!({ "index": 3, "command": "cargo test" }), &caller());

        assert_eq!(
            output,
            ToolOutput::text("Sent command to window 3; use read_window to see its output")
        );
    }

    #[test]
    fn test_refuses_agent_windows() {
        let tmux = tmux_with(vec![window(3, "claude", "claude", "/")]);

        let output = SendCommand::new(Arc::new(tmux))
            .call(&json!({ "index": 3, "command": "ls" }), &caller());

        assert!(output.is_error);
        assert!(output.text.contains("use send_prompt"));
    }

    #[test]
    fn test_missing_command_is_tool_error() {
        let output =
            SendCommand::new(Arc::new(MockTmux::new())).call(&json!({ "index": 3 }), &caller());
        assert!(output.is_error);
        assert!(
            output
                .text
                .starts_with("Missing required argument: command")
        );
    }

    #[test]
    fn test_send_failure_is_tool_error() {
        let mut tmux = tmux_with(vec![window(3, "shell", "zsh", "/")]);
        tmux.expect_send_keys()
            .return_once(|_, _, _| Err(TmuxError::WindowNotFound));

        let output = SendCommand::new(Arc::new(tmux))
            .call(&json!({ "index": 3, "command": "ls" }), &caller());

        assert_eq!(
            output.text,
            "Failed to send command to window 3: Window not found"
        );
    }

    #[test]
    fn test_metadata() {
        let tool = SendCommand::new(Arc::new(MockTmux::new()));
        assert_eq!(tool.name(), "send_command");
        assert!(tool.description().contains("index"));
        assert_eq!(tool.input_schema()["required"], json!(["index", "command"]));
    }
}
