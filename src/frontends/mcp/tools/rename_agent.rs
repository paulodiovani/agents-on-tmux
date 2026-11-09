use std::sync::Arc;

use serde_json::Value;

use super::rename_window::RenameWindow;
use super::tool::{CallContext, Tool, ToolOutput};
use crate::backends::tmux::Tmux;

/// Changes the title of an agent managed by AOT; an alias of `rename_window` named for agents.
pub struct RenameAgent {
    rename_window: RenameWindow,
}

impl RenameAgent {
    /// Creates the tool over the nested AOT tmux session.
    pub fn new(tmux: Arc<dyn Tmux + Send + Sync>) -> Self {
        Self {
            rename_window: RenameWindow::new(tmux),
        }
    }
}

impl Tool for RenameAgent {
    fn name(&self) -> &'static str {
        "rename_agent"
    }

    fn description(&self) -> &'static str {
        "Change the title of an agent managed by AOT, including yourself. Takes the agent's index (never its title) and the new title, a single line. Returns {\"index\", \"title\"}."
    }

    fn input_schema(&self) -> Value {
        self.rename_window.input_schema()
    }

    fn call(&self, arguments: &Value, context: &CallContext) -> ToolOutput {
        self.rename_window.call(arguments, context)
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
    fn test_renames_like_rename_window() {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .return_once(|| Ok(vec![window(2, "claude", "claude", "/")]));
        tmux.expect_set_window_name()
            .with(eq(2), eq("reviewer"))
            .times(1)
            .return_once(|_, _| Ok(()));
        let tool = RenameAgent::new(Arc::new(tmux));

        let output = tool.call(
            &json!({ "index": 2, "title": "reviewer" }),
            &CallContext::default(),
        );

        assert_eq!(
            output.structured,
            Some(json!({ "index": 2, "title": "reviewer" }))
        );
    }

    #[test]
    fn test_metadata_matches_rename_window_schema() {
        let tool = RenameAgent::new(Arc::new(MockTmux::new()));
        let rename_window = RenameWindow::new(Arc::new(MockTmux::new()));
        assert_eq!(tool.name(), "rename_agent");
        assert!(tool.description().contains("agent's index"));
        assert_eq!(tool.input_schema(), rename_window.input_schema());
    }
}
