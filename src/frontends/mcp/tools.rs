use serde_json::Value;

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
