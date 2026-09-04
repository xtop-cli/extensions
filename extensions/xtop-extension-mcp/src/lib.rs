//! MCP (Model Context Protocol) server extension for xtop.
//!
//! Runs on stdio transport and exposes xtop's plugins (the `samurai`
//! plugin) as MCP tools through the `xtop-extension-api` host contract.
//! Any MCP-compatible AI (Claude Desktop, Cline, etc.) can connect via:
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "xtop": {
//!       "command": "xtop",
//!       "args": ["mcp"]
//!     }
//!   }
//! }
//! ```
//!
//! Protocol: JSON-RPC 2.0 over stdin/stdout (one JSON object per line).
//!
//! The extension is kernel-agnostic: every tool call is executed through
//! [`ExtensionContext`], which the kernel resolves against its hosted
//! plugins. It depends on `xtop-plugin-samurai` at compile time so the
//! plugin id (`PLUGIN_ID`) and the 12 action names (`actions::*`) are
//! single-sourced (DR-6): the MCP tool table is generated from those
//! constants instead of a hand-maintained string list.

use std::io::{self, BufRead, Write};

use xtop_extension_api::{Extension, ExtensionContext, ExtensionError, ExtensionManifest};
use xtop_plugin_samurai::actions;
use xtop_plugin_samurai::PLUGIN_ID;

const SERVER_NAME: &str = "xtop";
const PROTOCOL_VERSION: &str = "2024-11-05";

/// The MCP extension: provides the `mcp` server.
#[derive(Debug, Default)]
pub struct McpExtension;

impl McpExtension {
    pub fn new() -> Self {
        Self
    }
}

impl Extension for McpExtension {
    fn manifest(&self) -> ExtensionManifest {
        ExtensionManifest {
            id: "mcp".to_string(),
            name: "xtop MCP server".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            description: "Exposes xtop system monitoring and its plugins as MCP tools".to_string(),
            servers: vec!["mcp".to_string()],
        }
    }

    fn run_server(
        &mut self,
        server_id: &str,
        ctx: &mut ExtensionContext,
    ) -> Result<(), ExtensionError> {
        if server_id != "mcp" {
            return Err(ExtensionError::Unknown(format!(
                "server '{server_id}' is not provided by this extension"
            )));
        }
        run_mcp_server(ctx)
    }
}

// ---------------------------------------------------------------------------
// Tool table: generated from the samurai plugin's exported action constants.
// Tool name = action with '.' replaced by '_' (system.summary -> system_summary).
// ---------------------------------------------------------------------------

/// The samurai actions exposed as MCP tools, in tools/list order.
const TOOL_ACTIONS: [&str; 12] = [
    actions::SYSTEM_SUMMARY,
    actions::PROCESSES_TOP,
    actions::PROCESSES_SEARCH,
    actions::PROCESS_INFO,
    actions::PROCESS_KILL,
    actions::THRESHOLD_SET,
    actions::THRESHOLD_GET,
    actions::CONFIG_GET,
    actions::CONFIG_SET,
    actions::PROCESS_ALERTS,
    actions::ALERTS_STATUS,
    actions::PLUGIN_STATUS,
];

/// MCP tool name for a samurai action.
fn tool_name(action: &str) -> String {
    action.replace('.', "_")
}

/// Resolve an MCP tool name back to its samurai action constant.
fn action_for_tool(tool: &str) -> Option<&'static str> {
    TOOL_ACTIONS
        .iter()
        .copied()
        .find(|a| a.replace('.', "_") == tool)
}

fn tool_description(action: &str) -> &'static str {
    match action {
        actions::SYSTEM_SUMMARY => {
            "Get a high-level system health summary (CPU, memory, disks, network, uptime, hostname)"
        }
        actions::PROCESSES_TOP => "Get top N processes by CPU usage, with optional regex filter",
        actions::PROCESSES_SEARCH => {
            "Search processes using regex. Fields: name, cmd, user, state, exe, cwd"
        }
        actions::PROCESS_INFO => {
            "Get detailed info about a process by PID (includes exe, ppid, threads, cwd)"
        }
        actions::PROCESS_KILL => "Terminate a process by PID",
        actions::THRESHOLD_SET => "Set alert thresholds for CPU, memory, and disk (percentages)",
        actions::THRESHOLD_GET => "Get current alert threshold values",
        actions::CONFIG_GET => "Get current xtop configuration",
        actions::CONFIG_SET => "Update configuration: interval_ms, theme, or layout",
        actions::PROCESS_ALERTS => {
            "Get all heuristic alerts as a JSON array (suspicious_exe_path, masquerading, known_threat, pipe_download, orphan, privilege_escalation, browser_child, thread_anomaly, fd_anomaly, spawn_storm)"
        }
        actions::ALERTS_STATUS => "Get alert summary with counts by severity (critical, warning, info)",
        actions::PLUGIN_STATUS => "Get Samurai plugin internal status",
        _ => "Execute a Samurai plugin action",
    }
}

fn tool_input_schema(action: &str) -> serde_json::Value {
    match action {
        actions::SYSTEM_SUMMARY
        | actions::THRESHOLD_GET
        | actions::CONFIG_GET
        | actions::PROCESS_ALERTS
        | actions::ALERTS_STATUS
        | actions::PLUGIN_STATUS => {
            serde_json::json!({ "type": "object", "properties": {} })
        }
        actions::PROCESSES_TOP => serde_json::json!({
            "type": "object",
            "properties": {
                "count": { "type": "integer", "description": "Number of processes (default 10)", "default": 10 },
                "filter": { "type": "string", "description": "Optional regex to filter by name or command" }
            }
        }),
        actions::PROCESSES_SEARCH => serde_json::json!({
            "type": "object",
            "properties": {
                "pattern": { "type": "string", "description": "Regex pattern" },
                "fields": { "type": "string", "description": "Fields to search: name,cmd,user,state,exe,cwd (default: name)" }
            },
            "required": ["pattern"]
        }),
        actions::PROCESS_INFO | actions::PROCESS_KILL => serde_json::json!({
            "type": "object",
            "properties": {
                "pid": { "type": "integer", "description": "Process ID" }
            },
            "required": ["pid"]
        }),
        actions::THRESHOLD_SET => serde_json::json!({
            "type": "object",
            "properties": {
                "cpu": { "type": "number", "description": "CPU threshold" },
                "mem": { "type": "number", "description": "Memory threshold" },
                "disk": { "type": "number", "description": "Disk threshold" }
            },
            "required": ["cpu", "mem", "disk"]
        }),
        actions::CONFIG_SET => serde_json::json!({
            "type": "object",
            "properties": {
                "interval_ms": { "type": "integer", "description": "Update interval in milliseconds" },
                "theme": { "type": "string", "description": "Theme name" },
                "layout": { "type": "string", "description": "Layout name" }
            }
        }),
        _ => serde_json::json!({ "type": "object", "properties": {} }),
    }
}

// ---------------------------------------------------------------------------
// Server loop
// ---------------------------------------------------------------------------

fn run_mcp_server(ctx: &mut ExtensionContext) -> Result<(), ExtensionError> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| ExtensionError::Fatal(format!("stdin read error: {e}")))?;
        if line.trim().is_empty() {
            continue;
        }

        let response = handle_line(&line, ctx)?;
        let response_line = serde_json::to_string(&response)
            .map_err(|e| ExtensionError::Fatal(format!("serialize error: {e}")))?;
        writeln!(stdout_lock, "{response_line}")
            .map_err(|e| ExtensionError::Fatal(format!("stdout write error: {e}")))?;
        stdout_lock
            .flush()
            .map_err(|e| ExtensionError::Fatal(format!("stdout flush error: {e}")))?;
    }

    Ok(())
}

/// Parse and answer one non-empty JSON-RPC line.
///
/// Mirrors the stdio loop's error behavior: malformed JSON yields
/// `ExtensionError::Recoverable` (the server stops with an error), while
/// every well-formed request yields a JSON-RPC response object.
fn handle_line(
    line: &str,
    ctx: &mut ExtensionContext,
) -> Result<serde_json::Value, ExtensionError> {
    let parsed: serde_json::Value = serde_json::from_str(line)
        .map_err(|e| ExtensionError::Recoverable(format!("invalid JSON-RPC: {e}")))?;

    let id = parsed.get("id").cloned();
    let method = parsed.get("method").and_then(|m| m.as_str()).unwrap_or("");
    let params = parsed
        .get("params")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    Ok(match method {
        "initialize" => handle_initialize(id, &params),
        "tools/list" => handle_tools_list(id),
        "tools/call" => handle_tools_call(id, &params, ctx),
        _ => make_error(id, -32601, format!("Method not found: {method}")),
    })
}

// ---------------------------------------------------------------------------
// JSON-RPC helpers
// ---------------------------------------------------------------------------

fn make_result(id: Option<serde_json::Value>, result: serde_json::Value) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    })
}

fn make_error(id: Option<serde_json::Value>, code: i32, message: String) -> serde_json::Value {
    serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message }
    })
}

// ---------------------------------------------------------------------------
// MCP: initialize
// ---------------------------------------------------------------------------

fn handle_initialize(
    id: Option<serde_json::Value>,
    _params: &serde_json::Value,
) -> serde_json::Value {
    make_result(
        id,
        serde_json::json!({
            "protocolVersion": PROTOCOL_VERSION,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": SERVER_NAME, "version": env!("CARGO_PKG_VERSION") }
        }),
    )
}

// ---------------------------------------------------------------------------
// MCP: tools/list
// ---------------------------------------------------------------------------

fn handle_tools_list(id: Option<serde_json::Value>) -> serde_json::Value {
    let tools: Vec<serde_json::Value> = TOOL_ACTIONS
        .iter()
        .map(|action| {
            serde_json::json!({
                "name": tool_name(action),
                "description": tool_description(action),
                "inputSchema": tool_input_schema(action),
            })
        })
        .collect();

    make_result(id, serde_json::json!({ "tools": tools }))
}

// ---------------------------------------------------------------------------
// MCP: tools/call
// ---------------------------------------------------------------------------

/// Translate typed MCP arguments into the plugin's string params syntax.
/// Returns the params string, or a JSON-RPC error message for bad arguments.
fn build_params(action: &str, args: &serde_json::Value) -> Result<String, String> {
    match action {
        actions::PROCESSES_TOP => {
            let count = args.get("count").and_then(|c| c.as_i64()).unwrap_or(10);
            let filter = args.get("filter").and_then(|f| f.as_str());
            Ok(match filter {
                Some(f) => format!("{count},filter={f}"),
                None => count.to_string(),
            })
        }

        actions::PROCESSES_SEARCH => {
            let pattern = args.get("pattern").and_then(|p| p.as_str()).unwrap_or("");
            let fields = args.get("fields").and_then(|f| f.as_str());
            Ok(match fields {
                Some(f) => format!("{pattern},fields={f}"),
                None => pattern.to_string(),
            })
        }

        actions::PROCESS_INFO | actions::PROCESS_KILL => {
            match args.get("pid").and_then(|p| p.as_i64()) {
                Some(pid) => Ok(pid.to_string()),
                None => Err("missing required argument: pid".to_string()),
            }
        }

        actions::THRESHOLD_SET => {
            let cpu = match args.get("cpu").and_then(|c| c.as_f64()) {
                Some(v) => v,
                None => return Err("missing required argument: cpu".to_string()),
            };
            let mem = match args.get("mem").and_then(|m| m.as_f64()) {
                Some(v) => v,
                None => return Err("missing required argument: mem".to_string()),
            };
            let disk = match args.get("disk").and_then(|d| d.as_f64()) {
                Some(v) => v,
                None => return Err("missing required argument: disk".to_string()),
            };
            Ok(format!("{cpu},{mem},{disk}"))
        }

        actions::CONFIG_SET => {
            if let Some(ms) = args.get("interval_ms").and_then(|v| v.as_i64()) {
                Ok(format!("interval_ms={ms}"))
            } else if let Some(theme) = args.get("theme").and_then(|v| v.as_str()) {
                Ok(format!("theme={theme}"))
            } else if let Some(layout) = args.get("layout").and_then(|v| v.as_str()) {
                Ok(format!("layout={layout}"))
            } else {
                Err("expected interval_ms, theme, or layout".to_string())
            }
        }

        actions::SYSTEM_SUMMARY
        | actions::THRESHOLD_GET
        | actions::CONFIG_GET
        | actions::PROCESS_ALERTS
        | actions::ALERTS_STATUS
        | actions::PLUGIN_STATUS => Ok(String::new()),

        _ => Err(format!("no parameter mapping for action: {action}")),
    }
}

fn handle_tools_call(
    id: Option<serde_json::Value>,
    params: &serde_json::Value,
    ctx: &mut ExtensionContext,
) -> serde_json::Value {
    let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
    let args = params
        .get("arguments")
        .cloned()
        .unwrap_or(serde_json::Value::Null);

    let action = match action_for_tool(name) {
        Some(action) => action,
        None => return make_error(id, -32601, format!("Tool not found: {name}")),
    };

    let params_str = match build_params(action, &args) {
        Ok(p) => p,
        Err(e) => return make_error(id, -32602, e),
    };

    // Tick to refresh data (also ticks plugins).
    ctx.tick();

    // Execute the action on the hosted plugin.
    match ctx.execute_plugin(PLUGIN_ID, action, &params_str) {
        Ok(json_str) => make_result(
            id,
            serde_json::json!({
                "content": [{"type": "text", "text": json_str}]
            }),
        ),
        Err(e) => make_error(id, -32000, e.to_string()),
    }
}

#[cfg(test)]
mod tests;
