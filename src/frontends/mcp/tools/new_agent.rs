use std::path::PathBuf;
use std::sync::{Arc, LazyLock};

use serde_json::{Value, json};

use super::tool::{CallContext, Tool, ToolOutput, caller_window, dir_argument};
use crate::backends::agents::{self, Agent, GenericAgent};
use crate::backends::tmux::{NewWindow, Tmux};

static DESCRIPTION: LazyLock<String> = LazyLock::new(|| {
    format!(
        "Start a new coding agent in its own window of the AOT session, in the background (the user's view does not switch). Optional command: the agent command line, e.g. \"claude\" or \"codex --model o3\"; it must start with one of the {}, and defaults to the caller's own agent. Optional dir: absolute, or relative to AOT's working directory (the default). Returns {{\"index\", \"title\", \"agent\", \"cwd\"}}; use the index with read_agent and the other tools.",
        known_commands()
    )
});

/// Starts a new agent in its own window of the AOT session.
pub struct NewAgent {
    tmux: Arc<dyn Tmux + Send + Sync>,
    default_dir: PathBuf,
}

impl NewAgent {
    /// Creates the tool over the nested AOT tmux session; agents start in
    /// `default_dir` unless the caller passes a directory.
    pub fn new(tmux: Arc<dyn Tmux + Send + Sync>, default_dir: PathBuf) -> Self {
        Self { tmux, default_dir }
    }

    fn command(&self, arguments: &Value, context: &CallContext) -> Result<String, ToolOutput> {
        match arguments.get("command") {
            None => {
                let caller = caller_window(self.tmux.as_ref(), context)?;
                agents::is_agent(&caller.running_command)
                    .map(|agent| agent.command().to_string())
                    .ok_or_else(|| {
                        ToolOutput::error(format!(
                            "No command given and the calling window is not an agent; pass command ({})",
                            known_commands()
                        ))
                    })
            }
            Some(Value::String(command)) if !command.trim().is_empty() => {
                Ok(command.trim().to_string())
            }
            Some(value) => Err(ToolOutput::error(format!(
                "Invalid command {value}: pass an agent command line, e.g. \"claude\" ({})",
                known_commands()
            ))),
        }
    }
}

impl Tool for NewAgent {
    fn name(&self) -> &'static str {
        "new_agent"
    }

    fn description(&self) -> &'static str {
        &DESCRIPTION
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": format!(
                        "Agent command line, e.g. \"claude\"; defaults to the caller's agent. Must start with one of the {}",
                        known_commands()
                    ),
                },
                "dir": {
                    "type": "string",
                    "description": "Working directory; absolute or relative to AOT's working directory",
                },
            },
            "additionalProperties": false,
        })
    }

    fn call(&self, arguments: &Value, context: &CallContext) -> ToolOutput {
        let command = match self.command(arguments, context) {
            Ok(command) => command,
            Err(error) => return error,
        };
        let agent = match agent_for(&command) {
            Ok(agent) => agent,
            Err(error) => return error,
        };
        let dir = match dir_argument(arguments, &self.default_dir) {
            Ok(dir) => dir,
            Err(error) => return error,
        };
        let options = NewWindow {
            dir: Some(dir),
            command: Some(command),
            detached: true,
            ..NewWindow::default()
        };
        match self.tmux.create_window(&options) {
            Ok(window) => ToolOutput::json(json!({
                "index": window.id,
                "title": window.name,
                "agent": agent.name(),
                "cwd": window.current_dir,
            })),
            Err(error) => ToolOutput::error(format!("Failed to start agent: {error}")),
        }
    }
}

fn agent_for(command: &str) -> Result<GenericAgent, ToolOutput> {
    let program = command.split_whitespace().next().unwrap_or_default();
    agents::is_agent(program).ok_or_else(|| {
        ToolOutput::error(format!(
            "'{program}' is not a known agent command ({})",
            known_commands()
        ))
    })
}

fn known_commands() -> String {
    let commands: Vec<&str> = agents::agents()
        .iter()
        .map(|agent| agent.command())
        .collect();
    format!("known agent commands: {}", commands.join(", "))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::tmux::{MockTmux, TmuxError};
    use crate::frontends::mcp::tools::tool::tests::window;
    use mockall::predicate::eq;

    fn temp_dir() -> String {
        std::env::temp_dir().to_string_lossy().into_owned()
    }

    fn tool(tmux: MockTmux) -> NewAgent {
        NewAgent::new(Arc::new(tmux), std::env::temp_dir())
    }

    fn caller(pane: &str) -> CallContext {
        CallContext {
            pane: Some(pane.into()),
        }
    }

    fn expect_create(tmux: &mut MockTmux, command: &'static str) {
        let dir = temp_dir();
        let expected = NewWindow {
            name: None,
            dir: Some(dir.clone()),
            command: Some(command.into()),
            detached: true,
        };
        tmux.expect_create_window()
            .withf(move |options| *options == expected)
            .times(1)
            .return_once(move |_| Ok(window(6, "claude", "claude", &dir)));
    }

    #[test]
    fn test_starts_given_agent_command() {
        let mut tmux = MockTmux::new();
        expect_create(&mut tmux, "claude --continue");

        let output = tool(tmux).call(
            &json!({ "command": " claude --continue " }),
            &CallContext::default(),
        );

        assert_eq!(
            output.structured,
            Some(json!({ "index": 6, "title": "claude", "agent": "Claude", "cwd": temp_dir() }))
        );
    }

    #[test]
    fn test_defaults_to_the_callers_agent() {
        let mut tmux = MockTmux::new();
        tmux.expect_window_for_pane()
            .with(eq("%2"))
            .return_once(|_| Ok(1));
        tmux.expect_list_windows()
            .return_once(|| Ok(vec![window(1, "me", "claude", "/")]));
        expect_create(&mut tmux, "claude");

        let output = tool(tmux).call(&json!({}), &caller("%2"));

        assert!(!output.is_error, "{}", output.text);
    }

    #[test]
    fn test_default_requires_an_agent_caller() {
        let mut tmux = MockTmux::new();
        tmux.expect_window_for_pane().return_once(|_| Ok(1));
        tmux.expect_list_windows()
            .return_once(|| Ok(vec![window(1, "shell", "zsh", "/")]));

        let output = tool(tmux).call(&json!({}), &caller("%2"));

        assert!(output.is_error);
        assert!(
            output
                .text
                .starts_with("No command given and the calling window is not an agent")
        );
        assert!(output.text.contains("claude"));
    }

    #[test]
    fn test_default_requires_a_known_caller() {
        let output = tool(MockTmux::new()).call(&json!({}), &CallContext::default());
        assert!(output.is_error);
        assert!(output.text.starts_with("The calling window is unknown"));
    }

    #[test]
    fn test_rejects_unknown_agent_command() {
        let output =
            tool(MockTmux::new()).call(&json!({ "command": "vim file" }), &CallContext::default());
        assert!(output.is_error);
        assert!(
            output
                .text
                .starts_with("'vim' is not a known agent command")
        );
    }

    #[test]
    fn test_rejects_blank_or_non_string_command() {
        for command in [json!("  "), json!(1), json!(null)] {
            let output =
                tool(MockTmux::new()).call(&json!({ "command": command }), &CallContext::default());
            assert!(output.is_error);
            assert!(
                output.text.starts_with("Invalid command"),
                "{}",
                output.text
            );
        }
    }

    #[test]
    fn test_missing_dir_is_tool_error() {
        let output = tool(MockTmux::new()).call(
            &json!({ "command": "claude", "dir": "no-such-dir-aot" }),
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

        let output = tool(tmux).call(&json!({ "command": "codex" }), &CallContext::default());

        assert!(output.is_error);
        assert_eq!(output.text, "Failed to start agent: Window not found");
    }

    #[test]
    fn test_metadata() {
        let tool = tool(MockTmux::new());
        assert_eq!(tool.name(), "new_agent");
        assert!(tool.description().contains("index"));
        let command = &tool.input_schema()["properties"]["command"];
        assert_eq!(command["type"], "string");
        for agent in agents::agents() {
            assert!(tool.description().contains(agent.command()));
            assert!(
                command["description"]
                    .as_str()
                    .unwrap()
                    .contains(agent.command())
            );
        }
    }
}
