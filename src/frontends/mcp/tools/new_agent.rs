use std::path::PathBuf;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use super::tool::{CallContext, Tool, ToolOutput, caller_window, dir_argument, name_argument};
use crate::backends::agents::{self, Agent, GenericAgent};
use crate::backends::tmux::{Tmux, TmuxError, Window};

const STARTUP_WAIT: Duration = Duration::from_millis(500);

/// Starts a new agent in its own window of the AOT session.
pub struct NewAgent {
    tmux: Arc<dyn Tmux + Send + Sync>,
    default_dir: PathBuf,
    startup_wait: Duration,
}

impl NewAgent {
    /// Creates the tool over the nested AOT tmux session; agents start in
    /// `default_dir` unless the caller passes a directory.
    pub fn new(tmux: Arc<dyn Tmux + Send + Sync>, default_dir: PathBuf) -> Self {
        Self {
            tmux,
            default_dir,
            startup_wait: STARTUP_WAIT,
        }
    }

    fn check_still_running(&self, window: &Window, agent: &GenericAgent) -> Result<(), ToolOutput> {
        thread::sleep(self.startup_wait);
        let windows = self.tmux.list_windows().map_err(|error| {
            ToolOutput::error(format!("Failed to check the agent window: {error}"))
        })?;
        let alive = windows
            .iter()
            .any(|w| w.id == window.id && w.pane_pid == window.pane_pid);
        if alive { Ok(()) } else { Err(exited(agent)) }
    }

    fn agent(&self, arguments: &Value, context: &CallContext) -> Result<GenericAgent, ToolOutput> {
        match arguments.get("command") {
            None => {
                let caller = caller_window(self.tmux.as_ref(), context)?;
                agents::is_agent(&caller.running_command).ok_or_else(|| {
                    ToolOutput::error(format!(
                        "No command given and you are not an agent; pass command, one of the {}",
                        known_commands()
                    ))
                })
            }

            Some(Value::String(command)) => agents::is_agent(command).ok_or_else(|| {
                ToolOutput::error(format!(
                    "'{command}' is not a known agent command; pass one of the {}",
                    known_commands()
                ))
            }),
            Some(value) => Err(ToolOutput::error(format!(
                "Invalid command {value}: pass one of the {}",
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
        "Start an AI coding agent in a new background window. Optional: command (the agent, defaults to your own), args (passed to the agent as-is), name, and dir (absolute or relative to AOT's working directory). Returns {\"index\", \"name\", \"agent\", \"current_dir\"}."
    }

    fn input_schema(&self) -> Value {
        let commands: Vec<&str> = agents::agents()
            .iter()
            .map(|agent| agent.command())
            .collect();
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "enum": commands,
                    "description": "Agent to start; defaults to your own agent",
                },
                "args": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Arguments passed to the agent as-is, e.g. [\"--model\", \"o3\"]",
                },
                "name": {
                    "type": "string",
                    "description": "Agent name, a single line",
                },
                "dir": {
                    "type": "string",
                    "description": "Working directory, absolute or relative to AOT's working directory",
                },
            },
            "additionalProperties": false,
        })
    }

    fn call(&self, arguments: &Value, context: &CallContext) -> ToolOutput {
        let agent = match self.agent(arguments, context) {
            Ok(agent) => agent,
            Err(error) => return error,
        };

        let args = match args_argument(arguments) {
            Ok(args) => args,
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

        let mut command = vec![agent.command().to_string()];
        command.extend(args);
        let window =
            match self
                .tmux
                .create_window(name.as_deref(), Some(&dir), Some(&command), true)
            {
                Ok(window) => window,
                Err(TmuxError::WindowNotFound) => return exited(&agent),
                Err(error) => return ToolOutput::error(format!("Failed to start agent: {error}")),
            };
        if let Err(error) = self.check_still_running(&window, &agent) {
            return error;
        }
        ToolOutput::json(json!({
            "index": window.id,
            "name": window.name,
            "agent": agent.name(),
            "current_dir": window.current_dir,
        }))
    }
}

fn args_argument(arguments: &Value) -> Result<Vec<String>, ToolOutput> {
    let Some(value) = arguments.get("args") else {
        return Ok(Vec::new());
    };
    let invalid = || ToolOutput::error(format!("Invalid args {value}: pass an array of strings"));
    value
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .map(|arg| arg.as_str().map(str::to_string).ok_or_else(invalid))
        .collect()
}

fn exited(agent: &GenericAgent) -> ToolOutput {
    ToolOutput::error(format!(
        "'{}' exited right after starting: it may not be installed, or it failed on startup (check its args)",
        agent.command()
    ))
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
        NewAgent {
            tmux: Arc::new(tmux),
            default_dir: std::env::temp_dir(),
            startup_wait: Duration::ZERO,
        }
    }

    fn expect_windows(tmux: &mut MockTmux, windows: Vec<Window>) {
        tmux.expect_list_windows()
            .returning(move || Ok(windows.clone()));
    }

    fn caller(pane: &str) -> CallContext {
        CallContext {
            pane: Some(pane.into()),
        }
    }

    fn expect_create(tmux: &mut MockTmux, command: &[&str]) {
        expect_create_named(tmux, None, command);
    }

    fn expect_create_named(
        tmux: &mut MockTmux,
        expected_name: Option<&'static str>,
        command: &[&str],
    ) {
        let dir = temp_dir();
        let expected_dir = dir.clone();
        let expected: Vec<String> = command.iter().map(|part| part.to_string()).collect();
        tmux.expect_create_window()
            .withf(move |name, dir, command, detached| {
                *name == expected_name
                    && *dir == Some(expected_dir.as_str())
                    && *command == Some(expected.as_slice())
                    && *detached
            })
            .times(1)
            .return_once(move |_, _, _, _| Ok(window(6, "claude", "claude", &dir)));
    }

    fn started() -> Window {
        window(6, "claude", "claude", &temp_dir())
    }

    #[test]
    fn test_starts_given_agent_command() {
        let mut tmux = MockTmux::new();
        expect_create(&mut tmux, &["claude", "--continue", "a b"]);
        expect_windows(&mut tmux, vec![started()]);

        let output = tool(tmux).call(
            &json!({ "command": "claude", "args": ["--continue", "a b"] }),
            &CallContext::default(),
        );

        assert_eq!(
            output.structured,
            Some(
                json!({ "index": 6, "name": "claude", "agent": "Claude", "current_dir": temp_dir() })
            )
        );
    }

    #[test]
    fn test_defaults_to_the_callers_agent() {
        let mut tmux = MockTmux::new();
        tmux.expect_window_for_pane()
            .with(eq("%2"))
            .return_once(|_| Ok(1));
        expect_windows(&mut tmux, vec![window(1, "me", "claude", "/"), started()]);
        expect_create(&mut tmux, &["claude", "--resume"]);

        let output = tool(tmux).call(&json!({ "args": ["--resume"] }), &caller("%2"));

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
                .starts_with("No command given and you are not an agent")
        );
        assert!(output.text.contains("claude"));
    }

    #[test]
    fn test_default_requires_a_known_caller() {
        let output = tool(MockTmux::new()).call(&json!({}), &CallContext::default());
        assert!(output.is_error);
        assert!(output.text.starts_with("You are not running inside AOT"));
    }

    #[test]
    fn test_rejects_unknown_agent_command() {
        let output =
            tool(MockTmux::new()).call(&json!({ "command": "vim" }), &CallContext::default());
        assert!(output.is_error);
        assert!(
            output
                .text
                .starts_with("'vim' is not a known agent command")
        );
        assert!(output.text.contains("claude"));
    }

    #[test]
    fn test_rejects_command_line_in_command() {
        let output = tool(MockTmux::new()).call(
            &json!({ "command": "claude --continue" }),
            &CallContext::default(),
        );
        assert!(output.is_error);
        assert!(
            output
                .text
                .starts_with("'claude --continue' is not a known agent command")
        );
    }

    #[test]
    fn test_rejects_non_string_command() {
        for command in [json!(1), json!(null), json!(["claude"])] {
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
    fn test_rejects_invalid_args() {
        for args in [json!("--continue"), json!([1]), json!(["ok", null])] {
            let output = tool(MockTmux::new()).call(
                &json!({ "command": "claude", "args": args }),
                &CallContext::default(),
            );
            assert!(output.is_error);
            assert!(output.text.starts_with("Invalid args"), "{}", output.text);
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
    fn test_agent_exiting_right_away_is_tool_error() {
        let mut tmux = MockTmux::new();
        expect_create(&mut tmux, &["claude"]);
        expect_windows(&mut tmux, Vec::new());

        let output = tool(tmux).call(&json!({ "command": "claude" }), &CallContext::default());

        assert!(output.is_error);
        assert!(
            output
                .text
                .starts_with("'claude' exited right after starting")
        );
    }

    #[test]
    fn test_reused_index_with_another_pane_counts_as_exited() {
        let mut tmux = MockTmux::new();
        expect_create(&mut tmux, &["claude"]);
        let mut other = started();
        other.pane_pid += 1;
        expect_windows(&mut tmux, vec![other]);

        let output = tool(tmux).call(&json!({ "command": "claude" }), &CallContext::default());

        assert!(output.is_error);
    }

    #[test]
    fn test_liveness_check_failure_is_tool_error() {
        let mut tmux = MockTmux::new();
        expect_create(&mut tmux, &["claude"]);
        tmux.expect_list_windows()
            .return_once(|| Err(TmuxError::WindowNotFound));

        let output = tool(tmux).call(&json!({ "command": "claude" }), &CallContext::default());

        assert_eq!(
            output.text,
            "Failed to check the agent window: Window not found"
        );
    }

    #[test]
    fn test_passes_name() {
        let mut tmux = MockTmux::new();
        expect_create_named(&mut tmux, Some("reviewer"), &["claude"]);
        expect_windows(&mut tmux, vec![started()]);

        let output = tool(tmux).call(
            &json!({ "command": "claude", "name": "reviewer" }),
            &CallContext::default(),
        );

        assert!(!output.is_error, "{}", output.text);
    }

    #[test]
    fn test_invalid_name_is_tool_error() {
        let output = tool(MockTmux::new()).call(
            &json!({ "command": "claude", "name": "a\nb" }),
            &CallContext::default(),
        );
        assert_eq!(output.text, "Invalid name: it must be a single line");
    }

    #[test]
    fn test_new_uses_default_startup_wait() {
        let tool = NewAgent::new(Arc::new(MockTmux::new()), PathBuf::from("/"));
        assert_eq!(tool.startup_wait, STARTUP_WAIT);
    }

    #[test]
    fn test_window_gone_before_lookup_counts_as_exited() {
        let mut tmux = MockTmux::new();
        tmux.expect_create_window()
            .return_once(|_, _, _, _| Err(TmuxError::WindowNotFound));

        let output = tool(tmux).call(&json!({ "command": "codex" }), &CallContext::default());

        assert!(output.is_error);
        assert!(
            output
                .text
                .starts_with("'codex' exited right after starting")
        );
    }

    #[test]
    fn test_tmux_failure_is_tool_error() {
        let mut tmux = MockTmux::new();
        tmux.expect_create_window().return_once(|_, _, _, _| {
            Err(TmuxError::CommandFailed {
                message: "boom".into(),
                stderr: String::new(),
                code: Some(1),
            })
        });

        let output = tool(tmux).call(&json!({ "command": "codex" }), &CallContext::default());

        assert!(output.is_error);
        assert_eq!(
            output.text,
            "Failed to start agent: Command failed: boom (exit 1)"
        );
    }

    #[test]
    fn test_metadata() {
        let tool = tool(MockTmux::new());
        assert_eq!(tool.name(), "new_agent");
        assert!(tool.description().contains("index"));
        let schema = tool.input_schema();
        let commands: Vec<&str> = agents::agents().iter().map(|a| a.command()).collect();
        assert_eq!(schema["properties"]["command"]["enum"], json!(commands));
        assert_eq!(schema["properties"]["args"]["items"]["type"], "string");
    }
}
