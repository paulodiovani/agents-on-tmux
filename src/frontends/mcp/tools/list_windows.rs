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

    fn read_only(&self) -> bool {
        true
    }

    fn description(&self) -> &'static str {
        "List windows running a shell or a command. Returns {\"windows\": [{\"index\", \"name\", \"command\", \"current_dir\", \"active\"}]}."
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
                    "name": window.name,
                    "command": window.running_command,
                    "current_dir": window.current_dir,
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
        ListWindows::new(Arc::new(tmux)).call(&json!({}), &CallContext::default())
    }

    #[test]
    fn test_lists_only_non_agent_windows() {
        let mut shell = window(1, "shell", "zsh", "/home/u");
        shell.is_active = true;
        let tmux = tmux_listing(vec![
            shell,
            window(2, "claude", "claude", "/home/u/project"),
            window(4, "server", "cargo", "/home/u/project"),
        ]);

        let output = call(tmux);

        assert!(!output.is_error);
        let expected = json!({ "windows": [
            { "index": 1, "name": "shell", "command": "zsh", "current_dir": "/home/u", "active": true },
            { "index": 4, "name": "server", "command": "cargo", "current_dir": "/home/u/project", "active": false },
        ] });
        assert_eq!(output.structured, Some(expected.clone()));
        assert_eq!(output.text, expected.to_string());
    }

    #[test]
    fn test_empty_session_returns_empty_list() {
        let output = call(tmux_listing(Vec::new()));
        assert_eq!(output.structured, Some(json!({ "windows": [] })));
    }

    #[test]
    fn test_tmux_failure_is_tool_error() {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .times(1)
            .return_once(|| Err(TmuxError::WindowNotFound));

        let output = call(tmux);

        assert!(output.is_error);
        assert_eq!(output.text, "Failed to list windows: Window not found");
    }

    #[test]
    fn test_metadata() {
        let tool = ListWindows::new(Arc::new(MockTmux::new()));
        assert_eq!(tool.name(), "list_windows");
        assert!(tool.read_only());
        assert!(tool.description().contains("index"));
        assert_eq!(
            tool.input_schema(),
            json!({ "type": "object", "additionalProperties": false })
        );
    }
}
