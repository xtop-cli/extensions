//! Unit tests for the MCP JSON-RPC layer.
//!
//! The stdio loop is reduced to [`handle_line`], which parses one JSON-RPC
//! message against a mock `ExtensionHost`; wire behavior (method names,
//! error codes, text wrapping) is asserted exactly. No sleeps, no network.

use xtop_extension_api::{Extension, ExtensionContext, ExtensionError, ExtensionHost};
use xtop_plugin_samurai::actions;
use xtop_plugin_samurai::PLUGIN_ID;

use super::{
    action_for_tool, handle_initialize, handle_line, handle_tools_call, tool_name, McpExtension,
};

// ---------------------------------------------------------------------------
// Mock host
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct MockHost {
    /// Recorded `execute_plugin(plugin_id, action, params)` calls.
    calls: Vec<(String, String, String)>,
    /// Canned plugin response for the next execute_plugin call.
    result: Result<String, String>,
    ticks: usize,
}

impl Default for MockHost {
    fn default() -> Self {
        Self::with_result(Ok(String::new()))
    }
}

impl MockHost {
    fn with_result(result: Result<String, String>) -> Self {
        Self {
            calls: vec![],
            result,
            ticks: 0,
        }
    }
}

impl ExtensionHost for MockHost {
    fn tick(&mut self) {
        self.ticks += 1;
    }

    fn execute_plugin(
        &mut self,
        plugin_id: &str,
        action: &str,
        params: &str,
    ) -> Result<String, ExtensionError> {
        self.calls.push((
            plugin_id.to_string(),
            action.to_string(),
            params.to_string(),
        ));
        match &self.result {
            Ok(json) => Ok(json.clone()),
            Err(msg) => Err(ExtensionError::Recoverable(msg.clone())),
        }
    }
}

fn ctx(host: &mut MockHost) -> ExtensionContext<'_> {
    ExtensionContext::new(host)
}

fn call(line: &str, host: &mut MockHost) -> serde_json::Value {
    let mut context = ctx(host);
    handle_line(line, &mut context).expect("line should produce a response")
}

fn error_code(response: &serde_json::Value) -> i64 {
    response["error"]["code"].as_i64().expect("error code")
}

// ---------------------------------------------------------------------------
// Extension manifest / server id
// ---------------------------------------------------------------------------

#[test]
fn manifest_serves_the_mcp_server() {
    let m = McpExtension::new().manifest();
    assert_eq!(m.id, "mcp");
    assert_eq!(m.servers, vec!["mcp"]);
    assert_eq!(m.version, env!("CARGO_PKG_VERSION"));
}

// ---------------------------------------------------------------------------
// initialize
// ---------------------------------------------------------------------------

#[test]
fn initialize_responds_with_protocol_version_and_capabilities() {
    let response = handle_initialize(Some(serde_json::json!(1)), &serde_json::json!({}));
    assert_eq!(response["jsonrpc"], "2.0");
    assert_eq!(response["id"], 1);
    assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(response["result"]["serverInfo"]["name"], "xtop");
    assert!(response["result"]["capabilities"]["tools"].is_object());
}

#[test]
fn initialize_over_the_line_interface() {
    let mut host = MockHost::default();
    let response = call(
        r#"{"jsonrpc":"2.0","id":2,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"1"}}}"#,
        &mut host,
    );
    assert_eq!(response["id"], 2);
    assert_eq!(response["result"]["protocolVersion"], "2024-11-05");
}

// ---------------------------------------------------------------------------
// tools/list
// ---------------------------------------------------------------------------

#[test]
fn tools_list_exposes_all_twelve_samurai_actions() {
    let mut host = MockHost::default();
    let response = call(
        r#"{"jsonrpc":"2.0","id":3,"method":"tools/list","params":{}}"#,
        &mut host,
    );
    let tools = response["result"]["tools"].as_array().expect("tools array");
    assert_eq!(tools.len(), 12);

    // Names are the same 12 tools as before, in the same order.
    let expected = [
        "system_summary",
        "processes_top",
        "processes_search",
        "process_info",
        "process_kill",
        "threshold_set",
        "threshold_get",
        "config_get",
        "config_set",
        "process_alerts",
        "alerts_status",
        "plugin_status",
    ];
    for (tool, name) in tools.iter().zip(expected) {
        assert_eq!(tool["name"], name);
        assert!(tool["description"].is_string());
        assert!(tool["inputSchema"]["type"] == "object");
    }
}

#[test]
fn tool_names_are_actions_with_dots_replaced() {
    assert_eq!(tool_name(actions::SYSTEM_SUMMARY), "system_summary");
    assert_eq!(tool_name(actions::PROCESSES_TOP), "processes_top");
    assert_eq!(tool_name(actions::CONFIG_SET), "config_set");
    assert_eq!(
        action_for_tool("plugin_status"),
        Some(actions::PLUGIN_STATUS)
    );
    assert_eq!(
        action_for_tool("processes_search"),
        Some(actions::PROCESSES_SEARCH)
    );
    assert_eq!(action_for_tool("no_such_tool"), None);
}

// ---------------------------------------------------------------------------
// tools/call happy paths
// ---------------------------------------------------------------------------

#[test]
fn tools_call_system_summary_targets_the_plugin_id() {
    let mut host = MockHost::with_result(Ok(r#"{"cpu_avg":12.3}"#.to_string()));
    let response = call(
        r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"system_summary","arguments":{}}}"#,
        &mut host,
    );
    assert_eq!(response["id"], 4);
    assert_eq!(response["result"]["content"][0]["type"], "text");
    assert_eq!(
        response["result"]["content"][0]["text"],
        r#"{"cpu_avg":12.3}"#
    );
    // The recorded plugin call carries PLUGIN_ID + the constant action.
    assert_eq!(host.calls.len(), 1);
    let (plugin_id, action, params) = &host.calls[0];
    assert_eq!(plugin_id, PLUGIN_ID);
    assert_eq!(action, actions::SYSTEM_SUMMARY);
    assert_eq!(params, "");
    // The tool call ticks the host first.
    assert_eq!(host.ticks, 1);
}

#[test]
fn tools_call_maps_arguments_to_the_plugin_params_syntax() {
    let mut host = MockHost::with_result(Ok("[]".to_string()));

    // processes_search: pattern + fields -> "pattern,fields=name,cmd".
    call(
        r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"processes_search","arguments":{"pattern":"sshd","fields":"name,cmd"}}}"#,
        &mut host,
    );
    assert_eq!(host.calls[0].1, actions::PROCESSES_SEARCH);
    assert_eq!(host.calls[0].2, "sshd,fields=name,cmd");

    // processes_top: count + filter -> "5,filter=nginx".
    call(
        r#"{"jsonrpc":"2.0","id":6,"method":"tools/call","params":{"name":"processes_top","arguments":{"count":5,"filter":"nginx"}}}"#,
        &mut host,
    );
    assert_eq!(host.calls[1].1, actions::PROCESSES_TOP);
    assert_eq!(host.calls[1].2, "5,filter=nginx");

    // threshold_set -> "90,85,80".
    call(
        r#"{"jsonrpc":"2.0","id":7,"method":"tools/call","params":{"name":"threshold_set","arguments":{"cpu":90,"mem":85,"disk":80}}}"#,
        &mut host,
    );
    assert_eq!(host.calls[2].1, actions::THRESHOLD_SET);
    assert_eq!(host.calls[2].2, "90,85,80");

    // config_set -> "theme=miami".
    call(
        r#"{"jsonrpc":"2.0","id":8,"method":"tools/call","params":{"name":"config_set","arguments":{"theme":"miami"}}}"#,
        &mut host,
    );
    assert_eq!(host.calls[3].1, actions::CONFIG_SET);
    assert_eq!(host.calls[3].2, "theme=miami");

    // Every call targeted PLUGIN_ID.
    assert!(host.calls.iter().all(|(pid, _, _)| pid == PLUGIN_ID));
}

// ---------------------------------------------------------------------------
// tools/call + line interface error paths
// ---------------------------------------------------------------------------

#[test]
fn tools_call_unknown_tool_is_method_not_found() {
    let mut host = MockHost::default();
    let response = call(
        r#"{"jsonrpc":"2.0","id":9,"method":"tools/call","params":{"name":"system_magic","arguments":{}}}"#,
        &mut host,
    );
    assert_eq!(error_code(&response), -32601);
    assert_eq!(response["error"]["message"], "Tool not found: system_magic");
    assert!(host.calls.is_empty());
}

#[test]
fn tools_call_missing_required_argument_is_invalid_params() {
    let mut host = MockHost::default();
    let response = call(
        r#"{"jsonrpc":"2.0","id":10,"method":"tools/call","params":{"name":"process_kill","arguments":{}}}"#,
        &mut host,
    );
    assert_eq!(error_code(&response), -32602);
    assert_eq!(
        response["error"]["message"],
        "missing required argument: pid"
    );
}

#[test]
fn tools_call_failing_execution_is_internal_error() {
    let mut host = MockHost::with_result(Err("kill denied".to_string()));
    let response = call(
        r#"{"jsonrpc":"2.0","id":11,"method":"tools/call","params":{"name":"process_kill","arguments":{"pid":1234}}}"#,
        &mut host,
    );
    assert_eq!(error_code(&response), -32000);
    assert_eq!(response["error"]["message"], "kill denied");
}

#[test]
fn unknown_method_is_method_not_found() {
    let mut host = MockHost::default();
    let response = call(
        r#"{"jsonrpc":"2.0","id":12,"method":"ping","params":{}}"#,
        &mut host,
    );
    assert_eq!(error_code(&response), -32601);
    assert_eq!(response["error"]["message"], "Method not found: ping");
}

#[test]
fn malformed_json_terminates_with_recoverable_error() {
    // Mirrors the stdio loop: a parse failure is surfaced as
    // ExtensionError::Recoverable (the run_server call returns Err).
    let mut host = MockHost::default();
    let mut context = ctx(&mut host);
    let err = handle_line("this is not json", &mut context).unwrap_err();
    match err {
        ExtensionError::Recoverable(msg) => assert!(msg.contains("invalid JSON-RPC"), "{msg}"),
        other => panic!("expected Recoverable parse error, got {other:?}"),
    }
}

#[test]
fn tools_call_wraps_every_response_in_text_content() {
    let mut host = MockHost::with_result(Ok(r#"{"a":1}"#.to_string()));
    let response = call(
        r#"{"jsonrpc":"2.0","id":13,"method":"tools/call","params":{"name":"alerts_status","arguments":{}}}"#,
        &mut host,
    );
    assert_eq!(response["id"], 13);
    assert_eq!(error_code_if_any(&response), None);
    let content = &response["result"]["content"][0];
    assert_eq!(content["type"], "text");
    assert_eq!(content["text"], r#"{"a":1}"#);
}

fn error_code_if_any(response: &serde_json::Value) -> Option<i64> {
    response["error"]["code"].as_i64()
}

#[test]
fn handlers_agree_with_line_interface() {
    // handle_initialize / handle_tools_list / handle_tools_call are the same
    // functions the line parser dispatches to.
    let mut host = MockHost::with_result(Ok("{}".to_string()));
    let mut context = ctx(&mut host);

    let via_line = handle_line(
        r#"{"jsonrpc":"2.0","id":14,"method":"tools/call","params":{"name":"config_get","arguments":{}}}"#,
        &mut context,
    )
    .unwrap();
    let via_handler = handle_tools_call(
        Some(serde_json::json!(14)),
        &serde_json::json!({"name": "config_get", "arguments": {}}),
        &mut context,
    );
    assert_eq!(via_line, via_handler);
}
