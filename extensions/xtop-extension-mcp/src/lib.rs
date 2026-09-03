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
//! plugins.

use std::io::{self, BufRead, Write};

use xtop_extension_api::{Extension, ExtensionContext, ExtensionError, ExtensionManifest};

const SERVER_NAME: &str = "xtop";
const SERVER_VERSION: &str = "0.1.0";
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
            version: SERVER_VERSION.to_string(),
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
// Server loop
// ---------------------------------------------------------------------------

fn run_mcp_server(ctx: &mut ExtensionContext) -> Result<(), ExtensionError> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut stdout_lock = stdout.lock();

    for line in stdin.lock().lines() {
        let line = line.map_err(|e| ExtensionError::Fatal(format!("stdin read error: {e}")))?;
        let line = line.trim().to_string();
        if line.is_empty() {
            continue;
        }

        let parsed: serde_json::Value = serde_json::from_str(&line)
            .map_err(|e| ExtensionError::Recoverable(format!("invalid JSON-RPC: {e}")))?;

        let id = parsed.get("id").cloned();
        let method = parsed.get("method").and_then(|m| m.as_str()).unwrap_or("");

        let params = parsed
            .get("params")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        let response = match method {
            "initialize" => handle_initialize(id, &params),
            "tools/list" => handle_tools_list(id),
            "tools/call" => handle_tools_call(id, &params, ctx),
            _ => make_error(id, -32601, format!("Method not found: {method}")),
        };

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
            "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION }
        }),
    )
}

// ---------------------------------------------------------------------------
// MCP: tools/list
// ---------------------------------------------------------------------------

fn handle_tools_list(id: Option<serde_json::Value>) -> serde_json::Value {
    make_result(
        id,
        serde_json::json!({
            "tools": [
                {
                    "name": "system_summary",
                    "description": "Get a high-level system health summary (CPU, memory, disks, network, uptime, hostname)",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "processes_top",
                    "description": "Get top N processes by CPU usage, with optional regex filter",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "count": { "type": "integer", "description": "Number of processes (default 10)", "default": 10 },
                            "filter": { "type": "string", "description": "Optional regex to filter by name or command" }
                        }
                    }
                },
                {
                    "name": "processes_search",
                    "description": "Search processes using regex. Fields: name, cmd, user, state, exe, cwd",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "pattern": { "type": "string", "description": "Regex pattern" },
                            "fields": { "type": "string", "description": "Fields to search: name,cmd,user,state,exe,cwd (default: name)" }
                        },
                        "required": ["pattern"]
                    }
                },
                {
                    "name": "process_info",
                    "description": "Get detailed info about a process by PID (includes exe, ppid, threads, cwd)",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "pid": { "type": "integer", "description": "Process ID" }
                        },
                        "required": ["pid"]
                    }
                },
                {
                    "name": "process_kill",
                    "description": "Terminate a process by PID",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "pid": { "type": "integer", "description": "Process ID to kill" }
                        },
                        "required": ["pid"]
                    }
                },
                {
                    "name": "threshold_set",
                    "description": "Set alert thresholds for CPU, memory, and disk (percentages)",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "cpu": { "type": "number", "description": "CPU threshold" },
                            "mem": { "type": "number", "description": "Memory threshold" },
                            "disk": { "type": "number", "description": "Disk threshold" }
                        },
                        "required": ["cpu", "mem", "disk"]
                    }
                },
                {
                    "name": "threshold_get",
                    "description": "Get current alert threshold values",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "config_get",
                    "description": "Get current xtop configuration",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "config_set",
                    "description": "Update configuration: interval_ms, theme, or layout",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "interval_ms": { "type": "integer", "description": "Update interval in milliseconds" },
                            "theme": { "type": "string", "description": "Theme name" },
                            "layout": { "type": "string", "description": "Layout name" }
                        }
                    }
                },
                {
                    "name": "process_alerts",
                    "description": "Get all heuristic alerts as a JSON array (suspicious_exe_path, masquerading, known_threat, pipe_download, orphan, privilege_escalation, browser_child, thread_anomaly, fd_anomaly, spawn_storm)",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "alerts_status",
                    "description": "Get alert summary with counts by severity (critical, warning, info)",
                    "inputSchema": { "type": "object", "properties": {} }
                },
                {
                    "name": "plugin_status",
                    "description": "Get Samurai plugin internal status",
                    "inputSchema": { "type": "object", "properties": {} }
                }
            ]
        }),
    )
}

// ---------------------------------------------------------------------------
// MCP: tools/call
// ---------------------------------------------------------------------------

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

    // Map MCP tool name -> hosted plugin action + params string.
    let (plugin_id, action, params_str): (&str, &str, String) = match name {
        "system_summary" => ("samurai", "system.summary", String::new()),

        "processes_top" => {
            let count = args.get("count").and_then(|c| c.as_i64()).unwrap_or(10);
            let filter = args.get("filter").and_then(|f| f.as_str());
            let p = match filter {
                Some(f) => format!("{count},filter={f}"),
                None => count.to_string(),
            };
            ("samurai", "processes.top", p)
        }

        "processes_search" => {
            let pattern = args.get("pattern").and_then(|p| p.as_str()).unwrap_or("");
            let fields = args.get("fields").and_then(|f| f.as_str());
            let p = match fields {
                Some(f) => format!("{pattern},fields={f}"),
                None => pattern.to_string(),
            };
            ("samurai", "processes.search", p)
        }

        "process_info" => {
            let pid = match args.get("pid").and_then(|p| p.as_i64()) {
                Some(p) => p.to_string(),
                None => return make_error(id, -32602, "missing required argument: pid".into()),
            };
            ("samurai", "process.info", pid)
        }

        "process_kill" => {
            let pid = match args.get("pid").and_then(|p| p.as_i64()) {
                Some(p) => p.to_string(),
                None => return make_error(id, -32602, "missing required argument: pid".into()),
            };
            ("samurai", "process.kill", pid)
        }

        "threshold_set" => {
            let cpu = match args.get("cpu").and_then(|c| c.as_f64()) {
                Some(v) => v.to_string(),
                None => return make_error(id, -32602, "missing required argument: cpu".into()),
            };
            let mem = match args.get("mem").and_then(|m| m.as_f64()) {
                Some(v) => v.to_string(),
                None => return make_error(id, -32602, "missing required argument: mem".into()),
            };
            let disk = match args.get("disk").and_then(|d| d.as_f64()) {
                Some(v) => v.to_string(),
                None => return make_error(id, -32602, "missing required argument: disk".into()),
            };
            ("samurai", "threshold.set", format!("{cpu},{mem},{disk}"))
        }

        "threshold_get" => ("samurai", "threshold.get", String::new()),
        "config_get" => ("samurai", "config.get", String::new()),

        "config_set" => {
            if let Some(ms) = args.get("interval_ms").and_then(|v| v.as_i64()) {
                ("samurai", "config.set", format!("interval_ms={ms}"))
            } else if let Some(theme) = args.get("theme").and_then(|v| v.as_str()) {
                ("samurai", "config.set", format!("theme={theme}"))
            } else if let Some(layout) = args.get("layout").and_then(|v| v.as_str()) {
                ("samurai", "config.set", format!("layout={layout}"))
            } else {
                return make_error(id, -32602, "expected interval_ms, theme, or layout".into());
            }
        }

        "process_alerts" => ("samurai", "process.alerts", String::new()),
        "alerts_status" => ("samurai", "alerts.status", String::new()),
        "plugin_status" => ("samurai", "plugin.status", String::new()),

        _ => return make_error(id, -32601, format!("Tool not found: {name}")),
    };

    // Tick to refresh data (also ticks plugins).
    ctx.tick();

    // Execute the action on the hosted plugin.
    match ctx.execute_plugin(plugin_id, action, &params_str) {
        Ok(json_str) => make_result(
            id,
            serde_json::json!({
                "content": [{"type": "text", "text": json_str}]
            }),
        ),
        Err(e) => make_error(id, -32000, e.to_string()),
    }
}
