use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use super::send_prompt::{SUBMIT_DELAY, deliver_prompt};
use super::tool::{CallContext, Tool, ToolOutput, caller_window, send_target, text_argument};
use crate::backends::agents;
use crate::backends::tmux::Tmux;

/// Sends a prompt to another agent signed with the caller's window, for agent-to-agent conversations.
pub struct RespondPrompt {
    tmux: Arc<dyn Tmux + Send + Sync>,
    submit_delay: Duration,
}

impl RespondPrompt {
    /// Creates the tool over the nested AOT tmux session.
    pub fn new(tmux: Arc<dyn Tmux + Send + Sync>) -> Self {
        Self {
            tmux,
            submit_delay: SUBMIT_DELAY,
        }
    }
}

impl Tool for RespondPrompt {
    fn name(&self) -> &'static str {
        "respond_prompt"
    }

    fn description(&self) -> &'static str {
        "Send a prompt to another agent in the AOT session, signed with your window, for agent-to-agent conversations. The prompt is delivered as \"[from window N] <prompt>\" (N is your current window index) followed by a hint to answer with respond_prompt. Use it to answer a prompt tagged [from window N] (pass that N as index), or to ask another agent something you expect an answer to; the answer arrives later as a new prompt in your window. Takes the integer window index from list_agents or a [from window N] tag (never a title) and the prompt; multi-line prompts are fine. Only works between agent windows; use send_prompt for an untagged prompt."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "index": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Window index of the agent to answer, from a [from window N] tag or list_agents",
                },
                "prompt": {
                    "type": "string",
                    "description": "Prompt to send, signed with your window",
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
        let sender = match caller_window(self.tmux.as_ref(), context) {
            Ok(sender) => sender,
            Err(error) => return error,
        };
        if agents::is_agent(&sender.running_command).is_none() {
            return ToolOutput::error(
                "respond_prompt only works from an agent window; use send_prompt instead",
            );
        }
        let target = match send_target(self.tmux.as_ref(), arguments, context, true) {
            Ok(target) => target,
            Err(error) => return error,
        };
        let signed = sign(sender.id, &prompt);
        match deliver_prompt(self.tmux.as_ref(), target.id, &signed, self.submit_delay) {
            Ok(()) => ToolOutput::text(format!(
                "Sent prompt to the agent in window {} as window {}; its answer will arrive as a new prompt",
                target.id, sender.id
            )),
            Err(error) => error,
        }
    }
}

fn sign(sender: u32, prompt: &str) -> String {
    format!(
        "[from window {sender}] {prompt}\n\n(To answer, use the respond_prompt tool of the AOT MCP with index {sender}.)"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::tmux::{MockTmux, TmuxError, Window};
    use crate::frontends::mcp::tools::tool::tests::window;
    use mockall::predicate::eq;

    fn tmux_with(windows: Vec<Window>, caller_index: u32) -> MockTmux {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .returning(move || Ok(windows.clone()));
        tmux.expect_window_for_pane()
            .with(eq("%1"))
            .returning(move |_| Ok(caller_index));
        tmux
    }

    fn agents() -> Vec<Window> {
        vec![
            window(2, "me", "claude", "/"),
            window(5, "reviewer", "codex", "/"),
            window(7, "shell", "zsh", "/"),
        ]
    }

    fn tool(tmux: MockTmux) -> RespondPrompt {
        RespondPrompt {
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
    fn test_delivers_signed_prompt_with_reply_hint() {
        let mut tmux = tmux_with(agents(), 2);
        let expected = "[from window 2] looks good?\nsee diff\n\n(To answer, use the respond_prompt tool of the AOT MCP with index 2.)";
        tmux.expect_paste_text()
            .with(eq(5), eq(expected))
            .times(1)
            .return_once(|_, _| Ok(()));
        tmux.expect_send_keys()
            .with(eq(5), eq(""), eq(true))
            .times(1)
            .return_once(|_, _, _| Ok(()));

        let output = tool(tmux).call(
            &json!({ "index": 5, "prompt": "looks good?\nsee diff" }),
            &caller(),
        );

        assert_eq!(
            output,
            ToolOutput::text(
                "Sent prompt to the agent in window 5 as window 2; its answer will arrive as a new prompt"
            )
        );
    }

    #[test]
    fn test_requires_a_known_caller() {
        let output = tool(MockTmux::new()).call(
            &json!({ "index": 5, "prompt": "hi" }),
            &CallContext::default(),
        );
        assert!(output.is_error);
        assert!(output.text.starts_with("The calling window is unknown"));
    }

    #[test]
    fn test_requires_an_agent_caller() {
        let tmux = tmux_with(agents(), 7);

        let output = tool(tmux).call(&json!({ "index": 5, "prompt": "hi" }), &caller());

        assert_eq!(
            output.text,
            "respond_prompt only works from an agent window; use send_prompt instead"
        );
    }

    #[test]
    fn test_refuses_non_agent_target_and_own_window() {
        let output =
            tool(tmux_with(agents(), 2)).call(&json!({ "index": 7, "prompt": "hi" }), &caller());
        assert!(output.text.starts_with("Window 7 is not an agent window"));

        let output =
            tool(tmux_with(agents(), 2)).call(&json!({ "index": 2, "prompt": "hi" }), &caller());
        assert!(output.text.starts_with("Window 2 is your own window"));
    }

    #[test]
    fn test_missing_prompt_is_tool_error() {
        let output = tool(MockTmux::new()).call(&json!({ "index": 5 }), &caller());
        assert!(output.text.starts_with("Missing required argument: prompt"));
    }

    #[test]
    fn test_delivery_failure_is_tool_error() {
        let mut tmux = tmux_with(agents(), 2);
        tmux.expect_paste_text()
            .return_once(|_, _| Err(TmuxError::WindowNotFound));

        let output = tool(tmux).call(&json!({ "index": 5, "prompt": "hi" }), &caller());

        assert_eq!(
            output.text,
            "Failed to send prompt to window 5: Window not found"
        );
    }

    #[test]
    fn test_new_uses_default_submit_delay() {
        let tool = RespondPrompt::new(Arc::new(MockTmux::new()));
        assert_eq!(tool.submit_delay, SUBMIT_DELAY);
    }

    #[test]
    fn test_metadata() {
        let tool = tool(MockTmux::new());
        assert_eq!(tool.name(), "respond_prompt");
        assert!(tool.description().contains("[from window N]"));
        assert_eq!(tool.input_schema()["required"], json!(["index", "prompt"]));
    }
}
