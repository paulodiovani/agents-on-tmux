use std::sync::Arc;

use super::list_windows::ListWindows;
use super::tool::Tool;
use crate::backends::tmux::Tmux;

/// Every MCP tool AOT serves, in the order `tools/list` reports them.
pub fn all_tools(tmux: Arc<dyn Tmux + Send + Sync>) -> Vec<Box<dyn Tool>> {
    vec![Box::new(ListWindows::new(tmux))]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontends::mcp::mock::MockTmux;

    #[test]
    fn test_all_tools_names_in_order() {
        let tools = all_tools(Arc::new(MockTmux::with_windows(Vec::new())));
        let names: Vec<&str> = tools.iter().map(|tool| tool.name()).collect();
        assert_eq!(names, ["list_windows"]);
    }
}
