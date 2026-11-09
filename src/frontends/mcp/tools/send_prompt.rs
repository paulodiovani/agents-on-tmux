use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use super::tool::{CallContext, Tool, ToolOutput, send_target, text_argument};
use crate::backends::tmux::Tmux;

const SUBMIT_DELAY: Duration = Duration::from_millis(200);

/// Gives a prompt to an agent window of the AOT session.
pub struct SendPrompt {
    tmux: Arc<dyn Tmux + Send + Sync>,
    submit_delay: Duration,
}

impl SendPrompt {
    /// Creates the tool over the nested AOT tmux session.
    pub fn new(tmux: Arc<dyn Tmux + Send + Sync>) -> Self {
        Self {
            tmux,
            submit_delay: SUBMIT_DELAY,
        }
    }
}

impl Tool for SendPrompt {
    fn name(&self) -> &'static str {
        "send_prompt"
    }

    fn description(&self) -> &'static str {
        "Give a prompt to an agent in the AOT session and submit it, as if the user typed it (the agent is not told who sent it). Takes the integer window index from list_agents (never a title) and the prompt; multi-line prompts are fine. The agent works asynchronously: call read_agent afterwards to follow its progress. Non-agent windows are refused; use send_command for them. Your own window is refused."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "index": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Window index from list_agents",
                },
                "prompt": {
                    "type": "string",
                    "description": "Prompt to submit to the agent",
                },
            },
            "required": ["index", "prompt"],
            "additionalProperties": false,
        })
    }

    fn call(&self, arguments: &Value, context: &CallContext) -> ToolOutput {
        let prompt = match text_argument(arguments, "prompt") {
            Ok(prompt) => prompt,
            Err(error) => return error,
        };
        let window = match send_target(self.tmux.as_ref(), arguments, context, true) {
            Ok(window) => window,
            Err(error) => return error,
        };
        match deliver_prompt(self.tmux.as_ref(), window.id, &prompt, self.submit_delay) {
            Ok(()) => ToolOutput::text(format!(
                "Sent prompt to the agent in window {}; use read_agent to follow its progress",
                window.id
            )),
            Err(error) => error,
        }
    }
}

/// Pastes `prompt` into the agent window and submits it with Enter after `delay`.
pub fn deliver_prompt(
    tmux: &dyn Tmux,
    index: u32,
    prompt: &str,
    delay: Duration,
) -> Result<(), ToolOutput> {
    let failed =
        |error| ToolOutput::error(format!("Failed to send prompt to window {index}: {error}"));
    tmux.paste_text(index, prompt).map_err(failed)?;
    thread::sleep(delay);
    tmux.send_keys(index, "", true).map_err(failed)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::tmux::{MockTmux, TmuxError, Window};
    use crate::frontends::mcp::tools::tool::tests::window;
    use mockall::Sequence;
    use mockall::predicate::eq;

    fn tmux_with(windows: Vec<Window>) -> MockTmux {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .returning(move || Ok(windows.clone()));
        tmux.expect_window_for_pane().returning(|_| Ok(9));
        tmux
    }

    fn tool(tmux: MockTmux) -> SendPrompt {
        SendPrompt {
            tmux: Arc::new(tmux),
            submit_delay: Duration::ZERO,
        }
    }

    fn caller() -> CallContext {
        CallContext {
            pane: Some("%1".into()),
        }
    }

    #[test]
    fn test_pastes_prompt_then_presses_enter() {
        let mut tmux = tmux_with(vec![window(4, "claude", "claude", "/")]);
        let mut sequence = Sequence::new();
        tmux.expect_paste_text()
            .with(eq(4), eq("fix the tests\nthen commit"))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_, _| Ok(()));
        tmux.expect_send_keys()
            .with(eq(4), eq(""), eq(true))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_, _, _| Ok(()));

        let output = tool(tmux).call(
            &json!({ "index": 4, "prompt": "fix the tests\nthen commit" }),
            &caller(),
        );

        assert_eq!(
            output,
            ToolOutput::text(
                "Sent prompt to the agent in window 4; use read_agent to follow its progress"
            )
        );
    }

    #[test]
    fn test_refuses_non_agent_windows() {
        let tmux = tmux_with(vec![window(4, "shell", "zsh", "/")]);

        let output = tool(tmux).call(&json!({ "index": 4, "prompt": "hi" }), &caller());

        assert!(output.is_error);
        assert!(output.text.contains("use send_command"));
    }

    #[test]
    fn test_missing_prompt_is_tool_error() {
        let output = tool(MockTmux::new()).call(&json!({ "index": 4 }), &caller());
        assert!(output.is_error);
        assert!(output.text.starts_with("Missing required argument: prompt"));
    }

    #[test]
    fn test_paste_failure_is_tool_error() {
        let mut tmux = tmux_with(vec![window(4, "claude", "claude", "/")]);
        tmux.expect_paste_text()
            .return_once(|_, _| Err(TmuxError::WindowNotFound));

        let output = tool(tmux).call(&json!({ "index": 4, "prompt": "hi" }), &caller());

        assert_eq!(
            output.text,
            "Failed to send prompt to window 4: Window not found"
        );
    }

    #[test]
    fn test_enter_failure_is_tool_error() {
        let mut tmux = tmux_with(vec![window(4, "claude", "claude", "/")]);
        tmux.expect_paste_text().return_once(|_, _| Ok(()));
        tmux.expect_send_keys()
            .return_once(|_, _, _| Err(TmuxError::WindowNotFound));

        let output = tool(tmux).call(&json!({ "index": 4, "prompt": "hi" }), &caller());

        assert!(output.is_error);
    }

    #[test]
    fn test_new_uses_default_submit_delay() {
        let tool = SendPrompt::new(Arc::new(MockTmux::new()));
        assert_eq!(tool.submit_delay, SUBMIT_DELAY);
    }

    #[test]
    fn test_metadata() {
        let tool = tool(MockTmux::new());
        assert_eq!(tool.name(), "send_prompt");
        assert!(tool.description().contains("index"));
        assert_eq!(tool.input_schema()["required"], json!(["index", "prompt"]));
    }
}
