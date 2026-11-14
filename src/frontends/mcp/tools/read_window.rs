use std::sync::Arc;

use serde_json::{Value, json};

use super::tool::{CallContext, Tool, ToolOutput, find_window, index_argument};
use crate::backends::tmux::Tmux;

const DEFAULT_LINES: u64 = 50;
const MAX_LINES: u64 = 2000;

/// Reads the recent output of a window in the AOT session.
pub struct ReadWindow {
    tmux: Arc<dyn Tmux + Send + Sync>,
}

impl ReadWindow {
    /// Creates the tool over the nested AOT tmux session.
    pub fn new(tmux: Arc<dyn Tmux + Send + Sync>) -> Self {
        Self { tmux }
    }
}

impl Tool for ReadWindow {
    fn name(&self) -> &'static str {
        "read_window"
    }

    fn read_only(&self) -> bool {
        true
    }

    fn description(&self) -> &'static str {
        "Read the latest output of a window as plain text. Takes the window index and an optional number of lines (default 50, max 2000)."
    }

    fn input_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "index": {
                    "type": "integer",
                    "minimum": 0,
                    "description": "Window index",
                },
                "lines": {
                    "type": "integer",
                    "minimum": 1,
                    "maximum": MAX_LINES,
                    "default": DEFAULT_LINES,
                    "description": "How many of the latest lines to return",
                },
            },
            "required": ["index"],
            "additionalProperties": false,
        })
    }

    fn call(&self, arguments: &Value, _context: &CallContext) -> ToolOutput {
        let index = match index_argument(arguments) {
            Ok(index) => index,
            Err(error) => return error,
        };

        let lines = match lines_argument(arguments) {
            Ok(lines) => lines,
            Err(error) => return error,
        };

        if let Err(error) = find_window(self.tmux.as_ref(), index) {
            return error;
        }

        match self.tmux.capture_pane(index, lines) {
            Ok(capture) => ToolOutput::text(last_lines(&capture, lines as usize)),
            Err(error) => ToolOutput::error(format!("Failed to read window {index}: {error}")),
        }
    }
}

fn lines_argument(arguments: &Value) -> Result<u32, ToolOutput> {
    let Some(value) = arguments.get("lines") else {
        return Ok(DEFAULT_LINES as u32);
    };
    value
        .as_u64()
        .map(|lines| lines.clamp(1, MAX_LINES) as u32)
        .ok_or_else(|| {
            ToolOutput::error(format!(
                "Invalid lines {value}: pass a positive integer (max {MAX_LINES})"
            ))
        })
}

fn last_lines(capture: &str, count: usize) -> String {
    let lines: Vec<&str> = capture.lines().map(str::trim_end).collect();
    let end = lines
        .iter()
        .rposition(|line| !line.is_empty())
        .map_or(0, |last| last + 1);
    lines
        .get(end.saturating_sub(count)..end)
        .unwrap_or_default()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backends::tmux::{MockTmux, TmuxError};
    use crate::frontends::mcp::tools::tool::tests::window;
    use mockall::predicate::eq;

    fn tmux_with_window(index: u32) -> MockTmux {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .return_once(move || Ok(vec![window(index, "shell", "zsh", "/")]));
        tmux
    }

    fn call(tmux: MockTmux, arguments: Value) -> ToolOutput {
        ReadWindow::new(Arc::new(tmux)).call(&arguments, &CallContext::default())
    }

    #[test]
    fn test_reads_last_lines_with_default_count() {
        let mut tmux = tmux_with_window(2);
        tmux.expect_capture_pane()
            .with(eq(2), eq(50))
            .times(1)
            .return_once(|_, _| Ok("$ ls  \nsrc\n\n\n".into()));

        let output = call(tmux, json!({ "index": 2 }));

        assert_eq!(output, ToolOutput::text("$ ls\nsrc"));
    }

    #[test]
    fn test_reads_requested_number_of_lines() {
        let mut tmux = tmux_with_window(1);
        tmux.expect_capture_pane()
            .with(eq(1), eq(2))
            .times(1)
            .return_once(|_, _| Ok("a\nb\nc\n".into()));

        let output = call(tmux, json!({ "index": 1, "lines": 2 }));

        assert_eq!(output.text, "b\nc");
    }

    #[test]
    fn test_lines_are_clamped() {
        let mut tmux = tmux_with_window(1);
        tmux.expect_capture_pane()
            .with(eq(1), eq(2000))
            .times(1)
            .return_once(|_, _| Ok("a\n".into()));

        assert_eq!(call(tmux, json!({ "index": 1, "lines": 99999 })).text, "a");

        let mut tmux = tmux_with_window(1);
        tmux.expect_capture_pane()
            .with(eq(1), eq(1))
            .times(1)
            .return_once(|_, _| Ok("a\nb\n".into()));

        assert_eq!(call(tmux, json!({ "index": 1, "lines": 0 })).text, "b");
    }

    #[test]
    fn test_invalid_lines_is_tool_error() {
        let output = call(MockTmux::new(), json!({ "index": 1, "lines": "many" }));
        assert!(output.is_error);
        assert!(output.text.starts_with("Invalid lines"));
    }

    #[test]
    fn test_invalid_index_is_tool_error() {
        let output = call(MockTmux::new(), json!({ "index": "@1" }));
        assert!(output.is_error);
        assert!(output.text.starts_with("Invalid index"));
    }

    #[test]
    fn test_unknown_window_is_tool_error() {
        let output = call(tmux_with_window(1), json!({ "index": 5 }));
        assert!(output.is_error);
        assert!(output.text.starts_with("No window or agent with index 5"));
    }

    #[test]
    fn test_capture_failure_is_tool_error() {
        let mut tmux = tmux_with_window(1);
        tmux.expect_capture_pane()
            .return_once(|_, _| Err(TmuxError::WindowNotFound));

        let output = call(tmux, json!({ "index": 1 }));

        assert!(output.is_error);
        assert_eq!(output.text, "Failed to read window 1: Window not found");
    }

    #[test]
    fn test_empty_capture_returns_empty_text() {
        assert_eq!(last_lines("\n  \n", 50), "");
        assert_eq!(last_lines("", 50), "");
    }

    #[test]
    fn test_metadata() {
        let tool = ReadWindow::new(Arc::new(MockTmux::new()));
        assert_eq!(tool.name(), "read_window");
        assert!(tool.read_only());
        assert!(tool.description().contains("index"));
        assert_eq!(tool.input_schema()["required"], json!(["index"]));
        assert_eq!(tool.input_schema()["properties"]["lines"]["maximum"], 2000);
    }
}
