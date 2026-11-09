use std::sync::Arc;
use std::time::Duration;

use serde_json::{Value, json};

use super::send_prompt::{SUBMIT_DELAY, send_signed_prompt};
use super::tool::{CallContext, Tool, ToolOutput};
use crate::backends::tmux::Tmux;

/// Answers a prompt another agent sent, signed with the caller's agent index.
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
        "Answer a prompt from another agent in the AOT session, i.e. a prompt tagged \"[from agent N]\": pass that N as index. The answer is delivered as \"[from agent M] <prompt>\" (M is your current window index) with the same hint, so the conversation can continue. Multi-line answers are fine. To start a new conversation, use send_prompt instead. Only works between agent windows; your own window is refused."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "index": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "N from the [from agent N] tag of the prompt being answered",
                },
                "prompt": {
                    "type": "string",
                    "description": "Answer to send",
                },
            },
            "required": ["index", "prompt"],
            "additionalProperties": false,
        })
    }

    fn call(&self, arguments: &Value, context: &CallContext) -> ToolOutput {
        match send_signed_prompt(self.tmux.as_ref(), arguments, context, self.submit_delay) {
            Ok(delivery) => ToolOutput::text(format!(
                "Sent answer to agent {} as agent {}",
                delivery.target, delivery.sender
            )),
            Err(error) => error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::tmux::MockTmux;
    use crate::frontends::mcp::tools::tool::tests::window;
    use mockall::predicate::eq;

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
    fn test_delivers_signed_answer() {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows().returning(|| {
            Ok(vec![
                window(2, "me", "claude", "/"),
                window(5, "asker", "codex", "/"),
            ])
        });
        tmux.expect_window_for_pane().returning(|_| Ok(2));
        tmux.expect_paste_text()
            .withf(|index, text| *index == 5 && text.starts_with("[from agent 2] hello\n"))
            .times(1)
            .return_once(|_, _| Ok(()));
        tmux.expect_send_keys()
            .with(eq(5), eq(""), eq(true))
            .times(1)
            .return_once(|_, _, _| Ok(()));

        let output = tool(tmux).call(&json!({ "index": 5, "prompt": "hello" }), &caller());

        assert_eq!(
            output,
            ToolOutput::text("Sent answer to agent 5 as agent 2")
        );
    }

    #[test]
    fn test_reports_errors() {
        let output = tool(MockTmux::new()).call(&json!({ "index": 5 }), &caller());
        assert!(output.is_error);
        assert!(output.text.starts_with("Missing required argument: prompt"));
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
        assert!(tool.description().contains("[from agent N]"));
        assert_eq!(tool.input_schema()["required"], json!(["index", "prompt"]));
    }
}
