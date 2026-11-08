use std::sync::Arc;

use serde_json::Value;

use super::read_window::ReadWindow;
use super::tool::{CallContext, Tool, ToolOutput};
use crate::backends::tmux::Tmux;

/// Reads the recent output of an agent window; an alias of `read_window` named for agents.
pub struct ReadAgent {
    read_window: ReadWindow,
}

impl ReadAgent {
    /// Creates the tool over the nested AOT tmux session.
    pub fn new(tmux: Arc<dyn Tmux + Send + Sync>) -> Self {
        Self {
            read_window: ReadWindow::new(tmux),
        }
    }
}

impl Tool for ReadAgent {
    fn name(&self) -> &'static str {
        "read_agent"
    }

    fn description(&self) -> &'static str {
        "Read the last lines of an agent's screen and scrollback in the AOT session, as plain text. Use it to see an agent's progress, answer, or a question it is waiting on. Takes the integer window index from list_agents (never a title) and an optional number of lines (default 50, max 2000). Trailing blank lines are dropped. Same as read_window."
    }

    fn input_schema(&self) -> Value {
        self.read_window.input_schema()
    }

    fn call(&self, arguments: &Value, context: &CallContext) -> ToolOutput {
        self.read_window.call(arguments, context)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::tmux::MockTmux;
    use crate::frontends::mcp::tools::tool::tests::window;
    use mockall::predicate::eq;
    use serde_json::json;

    #[test]
    fn test_reads_agent_window_like_read_window() {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .return_once(|| Ok(vec![window(4, "claude", "claude", "/")]));
        tmux.expect_capture_pane()
            .with(eq(4), eq(10))
            .times(1)
            .return_once(|_, _| Ok("> done\n\n".into()));
        let tool = ReadAgent::new(Arc::new(tmux));

        let output = tool.call(&json!({ "index": 4, "lines": 10 }), &CallContext::default());

        assert_eq!(output, ToolOutput::text("> done"));
    }

    #[test]
    fn test_metadata_matches_read_window_schema() {
        let tool = ReadAgent::new(Arc::new(MockTmux::new()));
        let read_window = ReadWindow::new(Arc::new(MockTmux::new()));
        assert_eq!(tool.name(), "read_agent");
        assert!(tool.description().contains("list_agents"));
        assert_eq!(tool.input_schema(), read_window.input_schema());
    }
}
