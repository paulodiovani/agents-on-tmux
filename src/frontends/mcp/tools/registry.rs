use std::path::PathBuf;
use std::sync::Arc;

use super::list_agents::ListAgents;
use super::list_windows::ListWindows;
use super::new_agent::NewAgent;
use super::new_window::NewWindow;
use super::read_agent::ReadAgent;
use super::read_window::ReadWindow;
use super::respond_prompt::RespondPrompt;
use super::send_command::SendCommand;
use super::send_prompt::SendPrompt;
use super::tool::Tool;
use crate::backends::tmux::Tmux;

/// Every MCP tool AOT serves, in the order `tools/list` reports them.
/// `default_dir` is where new windows open when the caller gives no directory.
pub fn all_tools(tmux: Arc<dyn Tmux + Send + Sync>, default_dir: PathBuf) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(ListWindows::new(Arc::clone(&tmux))),
        Box::new(ListAgents::new(Arc::clone(&tmux))),
        Box::new(ReadWindow::new(Arc::clone(&tmux))),
        Box::new(ReadAgent::new(Arc::clone(&tmux))),
        Box::new(NewWindow::new(Arc::clone(&tmux), default_dir.clone())),
        Box::new(NewAgent::new(Arc::clone(&tmux), default_dir)),
        Box::new(SendCommand::new(Arc::clone(&tmux))),
        Box::new(SendPrompt::new(Arc::clone(&tmux))),
        Box::new(RespondPrompt::new(tmux)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::tmux::MockTmux;

    #[test]
    fn test_all_tools_names_in_order() {
        let tools = all_tools(Arc::new(MockTmux::new()), PathBuf::from("/"));
        let names: Vec<&str> = tools.iter().map(|tool| tool.name()).collect();
        assert_eq!(
            names,
            [
                "list_windows",
                "list_agents",
                "read_window",
                "read_agent",
                "new_window",
                "new_agent",
                "send_command",
                "send_prompt",
                "respond_prompt",
            ]
        );
    }
}
