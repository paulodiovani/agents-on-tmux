use std::sync::Arc;

use serde_json::{Value, json};

use super::tool::{CallContext, Tool, ToolOutput};
use crate::backends::agents;
use crate::backends::tmux::Tmux;

/// Lists the plain (non-agent) windows of the AOT session.
pub struct ListWindows {
    tmux: Arc<dyn Tmux + Send + Sync>,
}

impl ListWindows {
    /// Creates the tool over the nested AOT tmux session.
    pub fn new(tmux: Arc<dyn Tmux + Send + Sync>) -> Self {
        Self { tmux }
    }
}

impl Tool for ListWindows {
    fn name(&self) -> &'static str {
        "list_windows"
    }

    fn description(&self) -> &'static str {
        "List the plain (non-agent) windows of the AOT session, such as shells, servers, or editors. Returns {\"windows\": [{\"index\", \"title\", \"command\", \"cwd\", \"active\"}]}. Address windows in other tools by their integer index (e.g. 3), never by title. Indexes can change when windows are moved or renumbered; list again if a window is not where you expect. Use list_agents for agent windows."
    }

    fn input_schema(&self) -> Value {
        json!({ "type": "object", "additionalProperties": false })
    }

    fn call(&self, _arguments: &Value, _context: &CallContext) -> ToolOutput {
        let windows = match self.tmux.list_windows() {
            Ok(windows) => windows,
            Err(error) => return ToolOutput::error(format!("Failed to list windows: {error}")),
        };
        let windows: Vec<Value> = windows
            .iter()
            .filter(|window| agents::is_agent(&window.running_command).is_none())
            .map(|window| {
                json!({
                    "index": window.id,
                    "title": window.name,
                    "command": window.running_command,
                    "cwd": window.current_dir,
                    "active": window.is_active,
                })
            })
            .collect();
        ToolOutput::json(json!({ "windows": windows }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontends::mcp::tools::tool::tests::{MockTmux, window};

    fn call(tmux: MockTmux) -> ToolOutput {
        ListWindows::new(Arc::new(tmux)).call(&json!({}), &CallContext::default())
    }

    #[test]
    fn test_lists_only_non_agent_windows() {
        let mut shell = window(1, "shell", "zsh", "/home/u");
        shell.is_active = true;
        let tmux = MockTmux::with_windows(vec![
            shell,
            window(2, "claude", "claude", "/home/u/project"),
            window(4, "server", "cargo", "/home/u/project"),
        ]);

        let output = call(tmux);

        assert!(!output.is_error);
        let expected = json!({ "windows": [
            { "index": 1, "title": "shell", "command": "zsh", "cwd": "/home/u", "active": true },
            { "index": 4, "title": "server", "command": "cargo", "cwd": "/home/u/project", "active": false },
        ] });
        assert_eq!(output.structured, Some(expected.clone()));
        assert_eq!(output.text, expected.to_string());
    }

    #[test]
    fn test_empty_session_returns_empty_list() {
        let output = call(MockTmux::with_windows(Vec::new()));
        assert_eq!(output.structured, Some(json!({ "windows": [] })));
    }

    #[test]
    fn test_tmux_failure_is_tool_error() {
        let output = call(MockTmux::failing());
        assert!(output.is_error);
        assert!(output.text.starts_with("Failed to list windows:"));
    }

    #[test]
    fn test_metadata() {
        let tool = ListWindows::new(Arc::new(MockTmux::with_windows(Vec::new())));
        assert_eq!(tool.name(), "list_windows");
        assert!(tool.description().contains("index"));
        assert_eq!(
            tool.input_schema(),
            json!({ "type": "object", "additionalProperties": false })
        );
    }
}
