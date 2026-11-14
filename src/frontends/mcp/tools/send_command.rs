use std::sync::Arc;

use serde_json::{Value, json};

use super::tool::{CallContext, Tool, ToolOutput, command_argument, send_target};
use crate::backends::shell;
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
        "Run a command in a window running a shell. Takes the window index and the command as an array: the program followed by its arguments, e.g. [\"cargo\", \"test\"]; each element is passed as one argument. Returns once the command is sent."
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
                "command": {
                    "type": "array",
                    "items": { "type": "string" },
                    "minItems": 1,
                    "description": "Program followed by its arguments, e.g. [\"ls\", \"-la\"]",
                },
            },
            "required": ["index", "command"],
            "additionalProperties": false,
        })
    }

    fn call(&self, arguments: &Value, context: &CallContext) -> ToolOutput {
        let command = match command_line(arguments) {
            Ok(command) => command,
            Err(error) => return error,
        };

        let window = match send_target(self.tmux.as_ref(), arguments, context, false) {
            Ok(window) => window,
            Err(error) => return error,
        };

        match self.tmux.send_keys(window.id, &command, true) {
            Ok(()) => ToolOutput::text(format!("Sent command to window {}", window.id)),
            Err(error) => ToolOutput::error(format!(
                "Failed to send command to window {}: {error}",
                window.id
            )),
        }
    }
}

fn command_line(arguments: &Value) -> Result<String, ToolOutput> {
    let parts = command_argument(arguments)?
        .ok_or_else(|| ToolOutput::error("Missing required argument: command"))?;
    Ok(shell::join(&parts))
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
            .with(eq(3), eq("cargo test --lib"), eq(true))
            .times(1)
            .return_once(|_, _, _| Ok(()));

        let output = SendCommand::new(Arc::new(tmux)).call(
            &json!({ "index": 3, "command": ["cargo", "test", "--lib"] }),
            &caller(),
        );

        assert_eq!(output, ToolOutput::text("Sent command to window 3"));
    }

    #[test]
    fn test_refuses_agent_windows() {
        let tmux = tmux_with(vec![window(3, "claude", "claude", "/")]);

        let output = SendCommand::new(Arc::new(tmux))
            .call(&json!({ "index": 3, "command": ["ls"] }), &caller());

        assert!(output.is_error);
        assert!(
            output
                .text
                .contains("commands only run in non-agent windows")
        );
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
    fn test_quotes_arguments_only_when_needed() {
        let command = command_line(&json!({
            "command": ["grep", "-r", "it's here", "a b", "", "$HOME", "src/main.rs"]
        }));
        assert_eq!(
            command,
            Ok(r#"grep -r 'it'\''s here' 'a b' '' '$HOME' src/main.rs"#.to_string())
        );
    }

    #[test]
    fn test_rejects_invalid_commands() {
        for command in [
            json!("ls -la"),
            json!([]),
            json!(["ls", 1]),
            json!([""]),
            json!(null),
        ] {
            let error = command_line(&json!({ "command": command })).unwrap_err();
            assert!(error.is_error);
            assert!(error.text.starts_with("Invalid command"), "{}", error.text);
        }
    }

    #[test]
    fn test_send_failure_is_tool_error() {
        let mut tmux = tmux_with(vec![window(3, "shell", "zsh", "/")]);
        tmux.expect_send_keys()
            .return_once(|_, _, _| Err(TmuxError::WindowNotFound));

        let output = SendCommand::new(Arc::new(tmux))
            .call(&json!({ "index": 3, "command": ["ls"] }), &caller());

        assert_eq!(
            output.text,
            "Failed to send command to window 3: Window not found"
        );
    }

    #[test]
    fn test_metadata() {
        let tool = SendCommand::new(Arc::new(MockTmux::new()));
        assert_eq!(tool.name(), "send_command");
        assert!(!tool.read_only());
        assert!(tool.description().contains("index"));
        assert_eq!(tool.input_schema()["required"], json!(["index", "command"]));
        assert_eq!(
            tool.input_schema()["properties"]["command"]["type"],
            "array"
        );
    }
}
