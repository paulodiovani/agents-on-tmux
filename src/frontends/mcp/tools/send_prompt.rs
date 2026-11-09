use std::sync::Arc;
use std::thread;
use std::time::Duration;

use serde_json::{Value, json};

use super::tool::{CallContext, Tool, ToolOutput, caller_window, send_target, text_argument};
use crate::backends::agents;
use crate::backends::tmux::Tmux;

pub const SUBMIT_DELAY: Duration = Duration::from_millis(200);

/// Gives another agent of the AOT session a prompt signed with the caller's agent index.
pub struct SendPrompt {
    tmux: Arc<dyn Tmux + Send + Sync>,
    submit_delay: Duration,
}

/// Agents involved in a delivered prompt, by window index.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Delivery {
    /// Window index of the calling agent that signed the prompt.
    pub sender: u32,
    /// Window index of the agent that received the prompt.
    pub target: u32,
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
        "Give a prompt to another agent managed by AOT and submit it. The prompt is delivered as \"[from agent N] <prompt>\" (N is your own index) so the agent knows who sent it. Takes the agent's index (never its title) and the prompt; multi-line prompts are fine. Only works between agents, and never on yourself."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "index": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Index of the agent",
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
        match send_signed_prompt(self.tmux.as_ref(), arguments, context, self.submit_delay) {
            Ok(delivery) => ToolOutput::text(format!(
                "Sent prompt to the agent in window {} as agent {}",
                delivery.target, delivery.sender
            )),
            Err(error) => error,
        }
    }
}

/// Delivers the `prompt` argument to the agent at the `index` argument, signed
/// with the calling agent's window index.
pub fn send_signed_prompt(
    tmux: &dyn Tmux,
    arguments: &Value,
    context: &CallContext,
    delay: Duration,
) -> Result<Delivery, ToolOutput> {
    let prompt = text_argument(arguments, "prompt")?;
    let sender = caller_window(tmux, context)?;
    if agents::is_agent(&sender.running_command).is_none() {
        return Err(ToolOutput::error(
            "Only agents can send prompts, since prompts are signed with the sender agent's index",
        ));
    }
    let target = send_target(tmux, arguments, context, true)?;
    let failed = |error| {
        ToolOutput::error(format!(
            "Failed to send prompt to window {}: {error}",
            target.id
        ))
    };
    tmux.paste_text(target.id, &sign(sender.id, &prompt))
        .map_err(failed)?;
    thread::sleep(delay);
    tmux.send_keys(target.id, "", true).map_err(failed)?;
    Ok(Delivery {
        sender: sender.id,
        target: target.id,
    })
}

fn sign(sender: u32, prompt: &str) -> String {
    format!("[from agent {sender}] {prompt}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::tmux::{MockTmux, TmuxError, Window};
    use crate::frontends::mcp::tools::tool::tests::window;
    use mockall::Sequence;
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

    fn windows() -> Vec<Window> {
        vec![
            window(2, "me", "claude", "/"),
            window(4, "worker", "codex", "/"),
            window(7, "shell", "zsh", "/"),
        ]
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

    fn send(
        tmux: MockTmux,
        arguments: Value,
        context: &CallContext,
    ) -> Result<Delivery, ToolOutput> {
        send_signed_prompt(&tmux, &arguments, context, Duration::ZERO)
    }

    #[test]
    fn test_pastes_signed_prompt_then_presses_enter() {
        let mut tmux = tmux_with(windows(), 2);
        let mut sequence = Sequence::new();
        let expected = "[from agent 2] fix the tests\nthen commit";
        tmux.expect_paste_text()
            .with(eq(4), eq(expected))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_, _| Ok(()));
        tmux.expect_send_keys()
            .with(eq(4), eq(""), eq(true))
            .times(1)
            .in_sequence(&mut sequence)
            .return_once(|_, _, _| Ok(()));

        let delivery = send(
            tmux,
            json!({ "index": 4, "prompt": "fix the tests\nthen commit" }),
            &caller(),
        );

        assert_eq!(
            delivery,
            Ok(Delivery {
                sender: 2,
                target: 4
            })
        );
    }

    #[test]
    fn test_requires_a_known_caller() {
        let error = send(
            MockTmux::new(),
            json!({ "index": 4, "prompt": "hi" }),
            &CallContext::default(),
        )
        .unwrap_err();
        assert!(error.text.starts_with("You are not running inside AOT"));
    }

    #[test]
    fn test_requires_an_agent_caller() {
        let error = send(
            tmux_with(windows(), 7),
            json!({ "index": 4, "prompt": "hi" }),
            &caller(),
        )
        .unwrap_err();
        assert!(error.text.starts_with("Only agents can send prompts"));
    }

    #[test]
    fn test_refuses_non_agent_target_and_own_window() {
        let error = send(
            tmux_with(windows(), 2),
            json!({ "index": 7, "prompt": "hi" }),
            &caller(),
        )
        .unwrap_err();
        assert!(error.text.contains("only agents accept prompts"));

        let error = send(
            tmux_with(windows(), 2),
            json!({ "index": 2, "prompt": "hi" }),
            &caller(),
        )
        .unwrap_err();
        assert!(error.text.starts_with("Index 2 is yourself"));
    }

    #[test]
    fn test_missing_prompt_is_tool_error() {
        let error = send(MockTmux::new(), json!({ "index": 4 }), &caller()).unwrap_err();
        assert!(error.text.starts_with("Missing required argument: prompt"));
    }

    #[test]
    fn test_paste_failure_is_tool_error() {
        let mut tmux = tmux_with(windows(), 2);
        tmux.expect_paste_text()
            .return_once(|_, _| Err(TmuxError::WindowNotFound));

        let error = send(tmux, json!({ "index": 4, "prompt": "hi" }), &caller()).unwrap_err();

        assert_eq!(
            error.text,
            "Failed to send prompt to window 4: Window not found"
        );
    }

    #[test]
    fn test_enter_failure_is_tool_error() {
        let mut tmux = tmux_with(windows(), 2);
        tmux.expect_paste_text().return_once(|_, _| Ok(()));
        tmux.expect_send_keys()
            .return_once(|_, _, _| Err(TmuxError::WindowNotFound));

        assert!(send(tmux, json!({ "index": 4, "prompt": "hi" }), &caller()).is_err());
    }

    #[test]
    fn test_tool_confirms_delivery() {
        let mut tmux = tmux_with(windows(), 2);
        tmux.expect_paste_text().return_once(|_, _| Ok(()));
        tmux.expect_send_keys().return_once(|_, _, _| Ok(()));

        let output = tool(tmux).call(&json!({ "index": 4, "prompt": "hi" }), &caller());

        assert_eq!(
            output,
            ToolOutput::text("Sent prompt to the agent in window 4 as agent 2")
        );
    }

    #[test]
    fn test_tool_reports_errors() {
        let output = tool(MockTmux::new()).call(&json!({ "index": 4 }), &caller());
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
        assert!(tool.description().contains("[from agent N]"));
        assert_eq!(tool.input_schema()["required"], json!(["index", "prompt"]));
    }
}
