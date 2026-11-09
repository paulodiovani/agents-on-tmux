use serde_json::{Map, Value, json};

use super::tools::{CallContext, Tool};

/// Answers MCP JSON-RPC messages, one line at a time.
pub trait Protocol {
    /// Handles one JSON-RPC message; `None` when no reply is due (notifications).
    fn handle(&self, line: &str, context: &CallContext) -> Option<String>;
}

const INSTRUCTIONS: &str = "AOT orchestrates windows and AI coding agents. A window is a terminal running a shell or a command; an agent is a window running an AI coding agent. Windows and agents are referenced by their index (e.g. 3), which can change when windows move. Prompts between agents are signed \"[from agent N]\" with the sender's index N.";
const INVALID_PARAMS: i64 = -32602;
const INVALID_REQUEST: i64 = -32600;
const LEGACY_VERSIONS: [&str; 4] = ["2025-11-25", "2025-06-18", "2025-03-26", "2024-11-05"];
const META_CLIENT_CAPABILITIES: &str = "io.modelcontextprotocol/clientCapabilities";
const META_PROTOCOL_VERSION: &str = "io.modelcontextprotocol/protocolVersion";
const META_SERVER_INFO: &str = "io.modelcontextprotocol/serverInfo";
const METHOD_NOT_FOUND: i64 = -32601;
const MODERN_VERSIONS: [&str; 1] = ["2026-07-28"];
const PARSE_ERROR: i64 = -32700;
const SERVER_NAME: &str = "aot";
const UNSUPPORTED_PROTOCOL_VERSION: i64 = -32022;

struct RpcError {
    code: i64,
    message: String,
    data: Option<Value>,
}

/// MCP server logic (legacy `initialize` handshake and modern per-request metadata).
pub struct McpHandler {
    tools: Vec<Box<dyn Tool>>,
}

impl RpcError {
    fn new(code: i64, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
}

impl McpHandler {
    /// Creates a handler serving the given tools.
    pub fn new(tools: Vec<Box<dyn Tool>>) -> Self {
        Self { tools }
    }

    fn dispatch(
        &self,
        method: &str,
        params: &Map<String, Value>,
        context: &CallContext,
    ) -> Result<Value, RpcError> {
        if method == "initialize" {
            return Ok(initialize(params));
        }

        check_modern_meta(params)?;

        match method {
            "ping" => Ok(json!({})),
            "server/discover" => Ok(discover()),
            "tools/list" => Ok(self.list_tools()),
            "tools/call" => self.call_tool(params, context),
            _ => Err(RpcError::new(
                METHOD_NOT_FOUND,
                format!("Method not found: {method}"),
            )),
        }
    }

    fn list_tools(&self) -> Value {
        let tools: Vec<Value> = self
            .tools
            .iter()
            .map(|tool| {
                json!({
                    "name": tool.name(),
                    "description": tool.description(),
                    "inputSchema": tool.input_schema(),
                    "annotations": { "readOnlyHint": tool.read_only() },
                })
            })
            .collect();
        json!({ "tools": tools })
    }

    fn call_tool(
        &self,
        params: &Map<String, Value>,
        context: &CallContext,
    ) -> Result<Value, RpcError> {
        let name = params
            .get("name")
            .and_then(Value::as_str)
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, "Missing tool name"))?;

        let tool = self
            .tools
            .iter()
            .find(|tool| tool.name() == name)
            .ok_or_else(|| RpcError::new(INVALID_PARAMS, format!("Unknown tool: {name}")))?;

        let arguments = match params.get("arguments") {
            None => json!({}),
            Some(arguments) if arguments.is_object() => arguments.clone(),
            Some(_) => {
                return Err(RpcError::new(
                    INVALID_PARAMS,
                    "Tool arguments must be an object",
                ));
            }
        };

        let output = tool.call(&arguments, context);
        let mut result = json!({
            "content": [{ "type": "text", "text": output.text }],
            "isError": output.is_error,
        });
        if let Some(structured) = output.structured {
            result["structuredContent"] = structured;
        }

        Ok(result)
    }
}

impl Protocol for McpHandler {
    fn handle(&self, line: &str, context: &CallContext) -> Option<String> {
        let Ok(message) = serde_json::from_str::<Value>(line) else {
            return Some(error_response(
                &Value::Null,
                RpcError::new(PARSE_ERROR, "Parse error"),
            ));
        };

        let Some(message) = message.as_object() else {
            return Some(error_response(&Value::Null, invalid_request()));
        };

        let id = message.get("id");
        let method = message.get("method").and_then(Value::as_str);
        let is_jsonrpc = message.get("jsonrpc").and_then(Value::as_str) == Some("2.0");
        let (Some(method), true) = (method, is_jsonrpc) else {
            let id = id.filter(|id| is_valid_id(id)).unwrap_or(&Value::Null);
            return Some(error_response(id, invalid_request()));
        };

        let id = id?;

        if !is_valid_id(id) {
            return Some(error_response(&Value::Null, invalid_request()));
        }

        let empty = Map::new();
        let params = match message.get("params") {
            None => &empty,
            Some(Value::Object(params)) => params,
            Some(_) => {
                let error = RpcError::new(INVALID_PARAMS, "Params must be an object");
                return Some(error_response(id, error));
            }
        };

        Some(match self.dispatch(method, params, context) {
            Ok(result) => success_response(id, result),
            Err(error) => error_response(id, error),
        })
    }
}

fn invalid_request() -> RpcError {
    RpcError::new(INVALID_REQUEST, "Invalid Request")
}

fn is_valid_id(id: &Value) -> bool {
    id.is_string() || id.is_i64() || id.is_u64()
}

fn server_info() -> Value {
    json!({ "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") })
}

fn supported_versions() -> Vec<&'static str> {
    MODERN_VERSIONS
        .iter()
        .chain(LEGACY_VERSIONS.iter())
        .copied()
        .collect()
}

fn check_modern_meta(params: &Map<String, Value>) -> Result<(), RpcError> {
    let Some(meta) = params.get("_meta").and_then(Value::as_object) else {
        return Ok(());
    };
    let Some(version) = meta.get(META_PROTOCOL_VERSION) else {
        return Ok(());
    };
    let Some(version) = version.as_str() else {
        return Err(RpcError::new(INVALID_PARAMS, "Invalid protocol version"));
    };
    if !MODERN_VERSIONS.contains(&version) {
        return Err(RpcError {
            code: UNSUPPORTED_PROTOCOL_VERSION,
            message: "Unsupported protocol version".into(),
            data: Some(json!({ "supported": supported_versions(), "requested": version })),
        });
    }
    if !meta.contains_key(META_CLIENT_CAPABILITIES) {
        return Err(RpcError::new(
            INVALID_PARAMS,
            format!("Missing _meta field: {META_CLIENT_CAPABILITIES}"),
        ));
    }
    Ok(())
}

fn initialize(params: &Map<String, Value>) -> Value {
    let requested = params.get("protocolVersion").and_then(Value::as_str);
    let version = requested
        .filter(|version| LEGACY_VERSIONS.contains(version))
        .unwrap_or(LEGACY_VERSIONS[0]);
    json!({
        "protocolVersion": version,
        "capabilities": { "tools": {} },
        "serverInfo": server_info(),
        "instructions": INSTRUCTIONS,
    })
}

fn discover() -> Value {
    json!({
        "supportedVersions": supported_versions(),
        "capabilities": { "tools": {} },
        "instructions": INSTRUCTIONS,
    })
}

fn success_response(id: &Value, mut result: Value) -> String {
    result["resultType"] = json!("complete");
    result["_meta"] = json!({ META_SERVER_INFO: server_info() });
    json!({ "jsonrpc": "2.0", "id": id, "result": result }).to_string()
}

fn error_response(id: &Value, error: RpcError) -> String {
    let mut body = json!({ "code": error.code, "message": error.message });
    if let Some(data) = error.data {
        body["data"] = data;
    }
    json!({ "jsonrpc": "2.0", "id": id, "error": body }).to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontends::mcp::tools::ToolOutput;

    struct EchoTool;

    impl Tool for EchoTool {
        fn name(&self) -> &'static str {
            "echo"
        }

        fn description(&self) -> &'static str {
            "Echoes the arguments"
        }

        fn input_schema(&self) -> Value {
            json!({ "type": "object" })
        }

        fn call(&self, arguments: &Value, context: &CallContext) -> ToolOutput {
            let fail = arguments.get("fail").and_then(Value::as_bool) == Some(true);
            ToolOutput {
                text: format!("pane={:?}", context.pane),
                structured: (!fail).then(|| arguments.clone()),
                is_error: fail,
            }
        }
    }

    fn handler() -> McpHandler {
        McpHandler::new(vec![Box::new(EchoTool)])
    }

    fn reply(handler: &McpHandler, line: &str) -> Value {
        let context = CallContext {
            pane: Some("%1".into()),
        };
        let reply = handler.handle(line, &context).expect("reply expected");
        assert!(!reply.contains('\n'));
        serde_json::from_str(&reply).unwrap()
    }

    fn modern_meta() -> Value {
        json!({
            META_PROTOCOL_VERSION: "2026-07-28",
            META_CLIENT_CAPABILITIES: {},
        })
    }

    #[test]
    fn test_initialize_echoes_supported_legacy_version() {
        let response = reply(
            &handler(),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"c","version":"1"}}}"#,
        );
        assert_eq!(response["id"], 1);
        assert_eq!(response["result"]["protocolVersion"], "2025-06-18");
        assert_eq!(response["result"]["capabilities"], json!({ "tools": {} }));
        assert_eq!(response["result"]["serverInfo"]["name"], "aot");
        assert_eq!(response["result"]["instructions"], INSTRUCTIONS);
    }

    #[test]
    fn test_initialize_unknown_version_falls_back_to_latest_legacy() {
        let response = reply(
            &handler(),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"1900-01-01"}}"#,
        );
        assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
    }

    #[test]
    fn test_initialize_without_params_uses_latest_legacy() {
        let response = reply(
            &handler(),
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize"}"#,
        );
        assert_eq!(response["result"]["protocolVersion"], "2025-11-25");
    }

    #[test]
    fn test_notifications_get_no_reply() {
        let context = CallContext::default();
        let line = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
        assert_eq!(handler().handle(line, &context), None);
    }

    #[test]
    fn test_ping_returns_complete_result_with_server_info() {
        let response = reply(&handler(), r#"{"jsonrpc":"2.0","id":"a","method":"ping"}"#);
        assert_eq!(response["id"], "a");
        assert_eq!(response["result"]["resultType"], "complete");
        assert_eq!(
            response["result"]["_meta"][META_SERVER_INFO]["name"],
            SERVER_NAME
        );
    }

    #[test]
    fn test_discover_lists_all_supported_versions() {
        let line = json!({
            "jsonrpc": "2.0", "id": 1, "method": "server/discover",
            "params": { "_meta": modern_meta() },
        });
        let response = reply(&handler(), &line.to_string());
        assert_eq!(
            response["result"]["supportedVersions"],
            json!([
                "2026-07-28",
                "2025-11-25",
                "2025-06-18",
                "2025-03-26",
                "2024-11-05"
            ])
        );
        assert_eq!(response["result"]["capabilities"], json!({ "tools": {} }));
        assert_eq!(response["result"]["instructions"], INSTRUCTIONS);
    }

    #[test]
    fn test_modern_unsupported_version_is_rejected() {
        let line = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/list",
            "params": { "_meta": {
                META_PROTOCOL_VERSION: "2025-11-25",
                META_CLIENT_CAPABILITIES: {},
            } },
        });
        let response = reply(&handler(), &line.to_string());
        assert_eq!(response["error"]["code"], UNSUPPORTED_PROTOCOL_VERSION);
        assert_eq!(response["error"]["data"]["requested"], "2025-11-25");
        assert_eq!(response["error"]["data"]["supported"][0], "2026-07-28");
    }

    #[test]
    fn test_modern_non_string_version_is_invalid_params() {
        let line = json!({
            "jsonrpc": "2.0", "id": 1, "method": "ping",
            "params": { "_meta": { META_PROTOCOL_VERSION: 7 } },
        });
        let response = reply(&handler(), &line.to_string());
        assert_eq!(response["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn test_modern_missing_capabilities_is_invalid_params() {
        let line = json!({
            "jsonrpc": "2.0", "id": 1, "method": "ping",
            "params": { "_meta": { META_PROTOCOL_VERSION: "2026-07-28" } },
        });
        let response = reply(&handler(), &line.to_string());
        assert_eq!(response["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn test_meta_without_protocol_version_is_served_as_legacy() {
        let line =
            r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":{"_meta":{"progressToken":1}}}"#;
        let response = reply(&handler(), line);
        assert_eq!(response["result"]["resultType"], "complete");
    }

    #[test]
    fn test_tools_list_returns_registered_tools() {
        let line = json!({
            "jsonrpc": "2.0", "id": 1, "method": "tools/list",
            "params": { "_meta": modern_meta() },
        });
        let response = reply(&handler(), &line.to_string());
        assert_eq!(
            response["result"]["tools"],
            json!([{
                "name": "echo",
                "description": "Echoes the arguments",
                "inputSchema": { "type": "object" },
                "annotations": { "readOnlyHint": false },
            }])
        );
    }

    #[test]
    fn test_tools_list_empty_registry() {
        let response = reply(
            &McpHandler::new(Vec::new()),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#,
        );
        assert_eq!(response["result"]["tools"], json!([]));
    }

    #[test]
    fn test_tools_call_returns_text_structured_content_and_context() {
        let response = reply(
            &handler(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"echo","arguments":{"x":1}}}"#,
        );
        let result = &response["result"];
        assert_eq!(
            result["content"],
            json!([{ "type": "text", "text": "pane=Some(\"%1\")" }])
        );
        assert_eq!(result["structuredContent"], json!({ "x": 1 }));
        assert_eq!(result["isError"], false);
    }

    #[test]
    fn test_tools_call_without_arguments_passes_empty_object() {
        let response = reply(
            &handler(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"echo"}}"#,
        );
        assert_eq!(response["result"]["structuredContent"], json!({}));
    }

    #[test]
    fn test_tools_call_tool_error_sets_is_error() {
        let response = reply(
            &handler(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"echo","arguments":{"fail":true}}}"#,
        );
        assert_eq!(response["result"]["isError"], true);
        assert!(response["result"].get("structuredContent").is_none());
    }

    #[test]
    fn test_tools_call_unknown_tool() {
        let response = reply(
            &handler(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"nope"}}"#,
        );
        assert_eq!(response["error"]["code"], INVALID_PARAMS);
        assert_eq!(response["error"]["message"], "Unknown tool: nope");
    }

    #[test]
    fn test_tools_call_missing_name() {
        let response = reply(
            &handler(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{}}"#,
        );
        assert_eq!(response["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn test_tools_call_non_object_arguments() {
        let response = reply(
            &handler(),
            r#"{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{"name":"echo","arguments":[1]}}"#,
        );
        assert_eq!(response["error"]["code"], INVALID_PARAMS);
    }

    #[test]
    fn test_unknown_method() {
        let response = reply(&handler(), r#"{"jsonrpc":"2.0","id":1,"method":"nope"}"#);
        assert_eq!(response["error"]["code"], METHOD_NOT_FOUND);
    }

    #[test]
    fn test_parse_error() {
        let response = reply(&handler(), "{not json");
        assert_eq!(response["error"]["code"], PARSE_ERROR);
        assert_eq!(response["id"], Value::Null);
    }

    #[test]
    fn test_non_object_message_is_invalid_request() {
        let response = reply(&handler(), "[1,2]");
        assert_eq!(response["error"]["code"], INVALID_REQUEST);
    }

    #[test]
    fn test_missing_method_is_invalid_request_with_id() {
        let response = reply(&handler(), r#"{"jsonrpc":"2.0","id":3}"#);
        assert_eq!(response["error"]["code"], INVALID_REQUEST);
        assert_eq!(response["id"], 3);
    }

    #[test]
    fn test_wrong_jsonrpc_version_is_invalid_request() {
        let response = reply(&handler(), r#"{"jsonrpc":"1.0","id":3,"method":"ping"}"#);
        assert_eq!(response["error"]["code"], INVALID_REQUEST);
    }

    #[test]
    fn test_null_id_is_invalid_request() {
        let response = reply(&handler(), r#"{"jsonrpc":"2.0","id":null,"method":"ping"}"#);
        assert_eq!(response["error"]["code"], INVALID_REQUEST);
        assert_eq!(response["id"], Value::Null);
    }

    #[test]
    fn test_non_object_params_is_invalid_params() {
        let response = reply(
            &handler(),
            r#"{"jsonrpc":"2.0","id":1,"method":"ping","params":[1]}"#,
        );
        assert_eq!(response["error"]["code"], INVALID_PARAMS);
    }
}
