use std::sync::Arc;

use serde_json::{Value, json};

use super::tool::{CallContext, Tool, ToolOutput};
use crate::backends::agents::{self, Agent};
use crate::backends::tmux::Tmux;

/// Lists the agent windows of the AOT session.
pub struct ListAgents {
    tmux: Arc<dyn Tmux + Send + Sync>,
}

impl ListAgents {
    /// Creates the tool over the nested AOT tmux session.
    pub fn new(tmux: Arc<dyn Tmux + Send + Sync>) -> Self {
        Self { tmux }
    }
}

impl Tool for ListAgents {
    fn name(&self) -> &'static str {
        "list_agents"
    }

    fn description(&self) -> &'static str {
        "List agents: windows running an AI coding agent. Returns {\"agents\": [{\"index\", \"name\", \"agent\", \"command\", \"current_dir\", \"active\"}]}."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "additionalProperties": false })
    }

    fn call(&self, _arguments: &Value, _context: &CallContext) -> ToolOutput {
        let windows = match self.tmux.list_windows() {
            Ok(windows) => windows,
            Err(error) => return ToolOutput::error(format!("Failed to list agents: {error}")),
        };

        let agents: Vec<Value> = windows
            .iter()
            .filter_map(|window| {
                let agent = agents::is_agent(&window.running_command)?;
                Some(json!({
                    "index": window.id,
                    "name": window.name,
                    "agent": agent.name(),
                    "command": window.running_command,
                    "current_dir": window.current_dir,
                    "active": window.is_active,
                }))
            })
            .collect();
        ToolOutput::json(json!({ "agents": agents }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::tmux::{MockTmux, TmuxError, Window};
    use crate::frontends::mcp::tools::tool::tests::window;

    fn tmux_listing(windows: Vec<Window>) -> MockTmux {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .times(1)
            .return_once(move || Ok(windows));
        tmux
    }

    fn call(tmux: MockTmux) -> ToolOutput {
        ListAgents::new(Arc::new(tmux)).call(&json!({}), &CallContext::default())
    }

    #[test]
    fn test_lists_only_agent_windows() {
        let mut claude = window(2, "reviewer", "claude", "/home/u/project");
        claude.is_active = true;
        let tmux = tmux_listing(vec![
            window(1, "shell", "zsh", "/home/u"),
            claude,
            window(3, "codex", "codex", "/home/u/other"),
        ]);

        let output = call(tmux);

        assert!(!output.is_error);
        let expected = json!({ "agents": [
            { "index": 2, "name": "reviewer", "agent": "Claude", "command": "claude", "current_dir": "/home/u/project", "active": true },
            { "index": 3, "name": "codex", "agent": "Codex", "command": "codex", "current_dir": "/home/u/other", "active": false },
        ] });
        assert_eq!(output.structured, Some(expected.clone()));
        assert_eq!(output.text, expected.to_string());
    }

    #[test]
    fn test_no_agents_returns_empty_list() {
        let output = call(tmux_listing(vec![window(1, "shell", "zsh", "/")]));
        assert_eq!(output.structured, Some(json!({ "agents": [] })));
    }

    #[test]
    fn test_tmux_failure_is_tool_error() {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .times(1)
            .return_once(|| Err(TmuxError::WindowNotFound));

        let output = call(tmux);

        assert!(output.is_error);
        assert_eq!(output.text, "Failed to list agents: Window not found");
    }

    #[test]
    fn test_metadata() {
        let tool = ListAgents::new(Arc::new(MockTmux::new()));
        assert_eq!(tool.name(), "list_agents");
        assert!(tool.description().contains("index"));
        assert_eq!(
            tool.input_schema(),
            json!({ "type": "object", "additionalProperties": false })
        );
    }
}
