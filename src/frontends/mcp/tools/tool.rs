use std::path::Path;

use serde_json::Value;

use crate::backends::agents::{self, Agent};
use crate::backends::tmux::{Tmux, Window};

/// An MCP tool exposed to agents through the AOT MCP server.
pub trait Tool: Send + Sync {
    /// Unique tool name used by clients in `tools/call`.
    fn name(&self) -> &'static str;
    /// Description written for the controlling LLM.
    fn description(&self) -> &'static str;
    /// JSON Schema of the tool arguments.
    fn input_schema(&self) -> Value;
    /// Runs the tool for the given arguments on behalf of the calling connection.
    fn call(&self, arguments: &Value, context: &CallContext) -> ToolOutput;
}

/// Identity of the agent connection issuing a request.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CallContext {
    /// tmux pane id (`%N`) of the agent that spawned the shim, when known.
    pub pane: Option<String>,
}

/// Result of a tool call, rendered into an MCP `CallToolResult`.
#[derive(Debug, Clone, PartialEq)]
pub struct ToolOutput {
    /// Text content returned to the LLM.
    pub text: String,
    /// Optional machine-readable result, sent as `structuredContent`.
    pub structured: Option<Value>,
    /// Marks a tool execution error the LLM can recover from.
    pub is_error: bool,
}

impl ToolOutput {
    /// Successful result carrying `value` both as structured content and as JSON text.
    pub fn json(value: Value) -> Self {
        Self {
            text: value.to_string(),
            structured: Some(value),
            is_error: false,
        }
    }

    /// Successful plain-text result.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            structured: None,
            is_error: false,
        }
    }

    /// Tool execution error with a message the LLM can act on.
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            text: message.into(),
            structured: None,
            is_error: true,
        }
    }
}

/// Reads the required window `index` argument (an integer, or a numeric string).
pub fn index_argument(arguments: &Value) -> Result<u32, ToolOutput> {
    let value = arguments.get("index").ok_or_else(|| {
        ToolOutput::error(
            "Missing required argument: index (the integer window index from list_windows or list_agents)",
        )
    })?;
    let index = match value {
        Value::Number(number) => number.as_u64(),
        Value::String(text) => text.trim().parse::<u64>().ok(),
        _ => None,
    };
    index
        .and_then(|index| u32::try_from(index).ok())
        .ok_or_else(|| {
            ToolOutput::error(format!(
                "Invalid index {value}: pass the integer window index (e.g. 3) from list_windows or list_agents, not a title"
            ))
        })
}

/// Reads a required, non-blank string argument.
pub fn text_argument(arguments: &Value, name: &str) -> Result<String, ToolOutput> {
    match arguments.get(name) {
        None => Err(ToolOutput::error(format!(
            "Missing required argument: {name}"
        ))),
        Some(Value::String(text)) if !text.trim().is_empty() => Ok(text.clone()),
        Some(value) => Err(ToolOutput::error(format!(
            "Invalid {name} {value}: pass a non-empty string"
        ))),
    }
}

/// Resolves the optional `dir` argument to an existing directory: relative paths
/// are taken from `default_dir`, and a missing argument means `default_dir`.
pub fn dir_argument(arguments: &Value, default_dir: &Path) -> Result<String, ToolOutput> {
    let dir = match arguments.get("dir") {
        None => default_dir.to_path_buf(),
        Some(Value::String(dir)) => default_dir.join(dir),
        Some(value) => {
            return Err(ToolOutput::error(format!(
                "Invalid dir {value}: pass a directory path"
            )));
        }
    };
    if !dir.is_dir() {
        return Err(ToolOutput::error(format!(
            "Directory not found: {}",
            dir.display()
        )));
    }
    Ok(dir.to_string_lossy().into_owned())
}

/// Finds the window with the given index in the AOT session.
pub fn find_window(tmux: &dyn Tmux, index: u32) -> Result<Window, ToolOutput> {
    let windows = tmux
        .list_windows()
        .map_err(|error| ToolOutput::error(format!("Failed to list windows: {error}")))?;
    windows
        .into_iter()
        .find(|window| window.id == index)
        .ok_or_else(|| {
            ToolOutput::error(format!(
                "No window with index {index}; call list_windows or list_agents for the current indexes"
            ))
        })
}

/// Finds the window of the agent issuing the call, from its tmux pane.
pub fn caller_window(tmux: &dyn Tmux, context: &CallContext) -> Result<Window, ToolOutput> {
    let pane = context.pane.as_deref().ok_or_else(|| {
        ToolOutput::error("The calling window is unknown: the MCP client did not run inside an AOT window (TMUX_PANE unset)")
    })?;
    let index = tmux.window_for_pane(pane).map_err(|error| {
        ToolOutput::error(format!(
            "Failed to find the calling window (pane {pane}): {error}"
        ))
    })?;
    find_window(tmux, index)
}

/// Resolves and validates the `index` target of a send tool: the window must
/// exist, be an agent window exactly when `agent` is set, and not be the caller's own.
pub fn send_target(
    tmux: &dyn Tmux,
    arguments: &Value,
    context: &CallContext,
    agent: bool,
) -> Result<Window, ToolOutput> {
    let index = index_argument(arguments)?;
    let window = find_window(tmux, index)?;
    match (agent, agents::is_agent(&window.running_command)) {
        (true, None) => {
            return Err(ToolOutput::error(format!(
                "Window {index} is not an agent window; use send_command to run commands in it"
            )));
        }
        (false, Some(found)) => {
            return Err(ToolOutput::error(format!(
                "Window {index} runs an agent ({}); use send_prompt to give it a prompt",
                found.name()
            )));
        }
        _ => {}
    }
    let own_index = context
        .pane
        .as_deref()
        .and_then(|pane| tmux.window_for_pane(pane).ok());
    if own_index == Some(index) {
        return Err(ToolOutput::error(format!(
            "Window {index} is your own window; send to another window"
        )));
    }
    Ok(window)
}

#[cfg(test)]
pub(super) mod tests {
    use super::*;
    use crate::backends::tmux::{MockTmux, TmuxError};
    use serde_json::json;

    /// Builds an inactive window running `command` in `cwd`.
    pub fn window(id: u32, name: &str, command: &str, cwd: &str) -> Window {
        Window {
            id,
            name: name.into(),
            pane_pid: 1000 + id,
            running_command: command.into(),
            current_dir: cwd.into(),
            is_active: false,
            notification_pending: false,
            started_at: None,
        }
    }

    #[test]
    fn test_json_output_mirrors_value_as_text() {
        let output = ToolOutput::json(json!({ "a": 1 }));
        assert_eq!(output.text, r#"{"a":1}"#);
        assert_eq!(output.structured, Some(json!({ "a": 1 })));
        assert!(!output.is_error);
    }

    #[test]
    fn test_error_output() {
        let output = ToolOutput::error("boom");
        assert_eq!(output.text, "boom");
        assert_eq!(output.structured, None);
        assert!(output.is_error);
    }

    #[test]
    fn test_text_output() {
        let output = ToolOutput::text("hello");
        assert_eq!(output.text, "hello");
        assert_eq!(output.structured, None);
        assert!(!output.is_error);
    }

    #[test]
    fn test_index_argument_accepts_integer_and_numeric_string() {
        assert_eq!(index_argument(&json!({ "index": 3 })), Ok(3));
        assert_eq!(index_argument(&json!({ "index": " 7 " })), Ok(7));
    }

    #[test]
    fn test_index_argument_missing() {
        let error = index_argument(&json!({})).unwrap_err();
        assert!(error.is_error);
        assert!(error.text.starts_with("Missing required argument: index"));
    }

    #[test]
    fn test_index_argument_rejects_invalid_values() {
        for value in [
            json!("@3"),
            json!("shell"),
            json!(-1),
            json!(1.5),
            json!(null),
            json!(u64::MAX),
        ] {
            let error = index_argument(&json!({ "index": value })).unwrap_err();
            assert!(error.is_error);
            assert!(error.text.starts_with("Invalid index"), "{}", error.text);
        }
    }

    #[test]
    fn test_find_window_returns_matching_window() {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .return_once(|| Ok(vec![window(1, "a", "zsh", "/"), window(3, "b", "zsh", "/")]));

        assert_eq!(find_window(&tmux, 3).unwrap().name, "b");
    }

    #[test]
    fn test_find_window_unknown_index() {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .return_once(|| Ok(vec![window(1, "a", "zsh", "/")]));

        let error = find_window(&tmux, 9).unwrap_err();
        assert!(error.is_error);
        assert!(error.text.starts_with("No window with index 9"));
    }

    #[test]
    fn test_find_window_tmux_failure() {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .return_once(|| Err(TmuxError::WindowNotFound));

        let error = find_window(&tmux, 1).unwrap_err();
        assert_eq!(error.text, "Failed to list windows: Window not found");
    }

    #[test]
    fn test_dir_argument_defaults_to_default_dir() {
        let default_dir = std::env::temp_dir();
        assert_eq!(
            dir_argument(&json!({}), &default_dir),
            Ok(default_dir.to_string_lossy().into_owned())
        );
    }

    #[test]
    fn test_dir_argument_resolves_relative_and_absolute_paths() {
        let base = std::env::temp_dir();
        let absolute = base.to_string_lossy().into_owned();
        assert_eq!(
            dir_argument(&json!({ "dir": absolute }), Path::new("/nonexistent")),
            Ok(absolute.clone())
        );
        assert_eq!(
            dir_argument(&json!({ "dir": "." }), &base),
            Ok(base.join(".").to_string_lossy().into_owned())
        );
    }

    #[test]
    fn test_dir_argument_rejects_missing_directory() {
        let error =
            dir_argument(&json!({ "dir": "no-such-dir-aot" }), &std::env::temp_dir()).unwrap_err();
        assert!(error.is_error);
        assert!(error.text.starts_with("Directory not found:"));
    }

    #[test]
    fn test_dir_argument_rejects_non_string() {
        let error = dir_argument(&json!({ "dir": 1 }), Path::new("/")).unwrap_err();
        assert!(error.text.starts_with("Invalid dir"));
    }

    fn caller_context() -> CallContext {
        CallContext {
            pane: Some("%3".into()),
        }
    }

    #[test]
    fn test_caller_window_resolves_pane_to_window() {
        let mut tmux = MockTmux::new();
        tmux.expect_window_for_pane()
            .with(mockall::predicate::eq("%3"))
            .return_once(|_| Ok(2));
        tmux.expect_list_windows()
            .return_once(|| Ok(vec![window(2, "me", "claude", "/")]));

        assert_eq!(caller_window(&tmux, &caller_context()).unwrap().id, 2);
    }

    #[test]
    fn test_caller_window_without_pane() {
        let error = caller_window(&MockTmux::new(), &CallContext::default()).unwrap_err();
        assert!(error.is_error);
        assert!(error.text.starts_with("The calling window is unknown"));
    }

    #[test]
    fn test_caller_window_pane_lookup_failure() {
        let mut tmux = MockTmux::new();
        tmux.expect_window_for_pane()
            .return_once(|_| Err(TmuxError::WindowNotFound));

        let error = caller_window(&tmux, &caller_context()).unwrap_err();
        assert_eq!(
            error.text,
            "Failed to find the calling window (pane %3): Window not found"
        );
    }

    fn send_tmux(windows: Vec<Window>, own_index: Option<u32>) -> MockTmux {
        let mut tmux = MockTmux::new();
        tmux.expect_list_windows()
            .returning(move || Ok(windows.clone()));
        tmux.expect_window_for_pane()
            .returning(move |_| own_index.ok_or(TmuxError::WindowNotFound));
        tmux
    }

    fn shell_and_agent() -> Vec<Window> {
        vec![
            window(1, "shell", "zsh", "/"),
            window(2, "claude", "claude", "/"),
        ]
    }

    #[test]
    fn test_send_target_accepts_matching_kind() {
        let tmux = send_tmux(shell_and_agent(), Some(5));
        let context = caller_context();
        assert_eq!(
            send_target(&tmux, &json!({ "index": 1 }), &context, false)
                .unwrap()
                .id,
            1
        );
        assert_eq!(
            send_target(&tmux, &json!({ "index": 2 }), &context, true)
                .unwrap()
                .id,
            2
        );
    }

    #[test]
    fn test_send_target_rejects_wrong_kind() {
        let tmux = send_tmux(shell_and_agent(), None);
        let context = CallContext::default();

        let error = send_target(&tmux, &json!({ "index": 1 }), &context, true).unwrap_err();
        assert!(
            error
                .text
                .starts_with("Window 1 is not an agent window; use send_command")
        );

        let error = send_target(&tmux, &json!({ "index": 2 }), &context, false).unwrap_err();
        assert!(
            error
                .text
                .starts_with("Window 2 runs an agent (Claude); use send_prompt")
        );
    }

    #[test]
    fn test_send_target_rejects_own_window() {
        let tmux = send_tmux(shell_and_agent(), Some(2));
        let error =
            send_target(&tmux, &json!({ "index": 2 }), &caller_context(), true).unwrap_err();
        assert_eq!(
            error.text,
            "Window 2 is your own window; send to another window"
        );
    }

    #[test]
    fn test_send_target_allows_unknown_caller() {
        let tmux = send_tmux(shell_and_agent(), None);
        assert!(send_target(&tmux, &json!({ "index": 2 }), &caller_context(), true).is_ok());
    }

    #[test]
    fn test_send_target_propagates_index_and_lookup_errors() {
        let tmux = send_tmux(shell_and_agent(), None);
        let context = CallContext::default();
        assert!(
            send_target(&tmux, &json!({}), &context, false)
                .unwrap_err()
                .text
                .starts_with("Missing")
        );
        assert!(
            send_target(&tmux, &json!({ "index": 9 }), &context, false)
                .unwrap_err()
                .text
                .starts_with("No window")
        );
    }

    #[test]
    fn test_text_argument() {
        assert_eq!(
            text_argument(&json!({ "a": "x y" }), "a"),
            Ok("x y".to_string())
        );
        assert_eq!(
            text_argument(&json!({}), "a").unwrap_err().text,
            "Missing required argument: a"
        );
        for value in [json!(""), json!("  "), json!(1)] {
            let error = text_argument(&json!({ "a": value }), "a").unwrap_err();
            assert!(error.text.starts_with("Invalid a"), "{}", error.text);
        }
    }
}
