# xtop MCP protocol

`xtop-extension-mcp` implements a minimal **Model Context Protocol** server
over **JSON-RPC 2.0**, line-delimited on stdin/stdout — one JSON object per
line, one response per request, no HTTP. The kernel launches it as
`xtop mcp` (`McpExtension::new().run_server("mcp", ctx)`).

## Transport framing

- Every input line is trimmed; empty lines are skipped.
- Malformed JSON terminates the server with a recoverable error (the kernel
  reports it) — no JSON-RPC response is emitted, mirroring the stdio loop.
- Every well-formed request produces exactly one response object with
  `"jsonrpc": "2.0"` and the request's `id` echoed.

## Handled methods

| Method | Purpose |
|---|---|
| `initialize` | Announces `protocolVersion 2024-11-05`, `capabilities.tools`, `serverInfo` (`xtop`, version = crate version). |
| `tools/list` | The 12 tools generated from the samurai plugin's `actions::*` constants. |
| `tools/call` | Executes one tool against the hosted plugin. |
| anything else | JSON-RPC `-32601 Method not found: <method>`. |

`notifications/initialized`, `ping`, `resources/*`, `logging`, `instructions`
and batching are not implemented — out of scope for this minimal server.

## Tool table

The tool table is **generated from the samurai plugin's exported constants**
(DR-6): this crate depends on `xtop-plugin-samurai` at compile time and maps
each of its 12 `actions::*` constants to an MCP tool whose **name is the
action with `.` replaced by `_`** (`system.summary` → `system_summary`,
`process.alerts` → `process_alerts`, …). There is no second hand-maintained
string list.

| Tool | Samurai action | MCP arguments → params string |
|---|---|---|
| `system_summary` | `system.summary` | — |
| `processes_top` | `processes.top` | `count` (default 10), `filter` → `count[,filter=<regex>]` |
| `processes_search` | `processes.search` | `pattern`, `fields` → `pattern[,fields=<fields>]` |
| `process_info` | `process.info` | `pid` → `"<pid>"` (required) |
| `process_kill` | `process.kill` | `pid` → `"<pid>"` (required) |
| `threshold_set` | `threshold.set` | `cpu`, `mem`, `disk` → `"<cpu>,<mem>,<disk>"` (all required) |
| `threshold_get` | `threshold.get` | — |
| `config_get` | `config.get` | — |
| `config_set` | `config.set` | one of `interval_ms` / `theme` / `layout` → `"interval_ms=<ms>"` / `"theme=<name>"` / `"layout=<name>"` |
| `process_alerts` | `process.alerts` | — |
| `alerts_status` | `alerts.status` | — |
| `plugin_status` | `plugin.status` | — |

Descriptions and JSON `inputSchema` objects advertised by `tools/list` are
maintained here per tool; the action strings, the plugin id and the
tool-name mapping come from the plugin crate.

## Tools/call behavior

For every tool call the server:

1. resolves the tool name back to its action constant (unknown name →
   `-32601 Tool not found: <name>`),
2. translates the typed MCP `arguments` object into the plugin's string
   params syntax (missing required arguments → `-32602` with the argument
   name in the message),
3. calls `ctx.tick()` (refreshes the kernel state and ticks hosted
   plugins),
4. calls `ctx.execute_plugin(PLUGIN_ID, <action>, <params>)` — always
   against the samurai plugin id imported from the plugin crate,
5. wraps a successful plugin JSON response as
   `{"content": [{"type": "text", "text": "<plugin json>"}]}`.

## Error mapping

| JSON-RPC code | Meaning | Origin |
|---|---|---|
| `-32601` | Method not found / tool not found | unknown `method` or unknown tool name |
| `-32602` | Invalid params | missing required tool argument (pid, cpu/mem/disk, or config key) |
| `-32000` | Server/execution error | `ExtensionError` from `execute_plugin` (plugin missing, capability denied, action failed) |

## Example session

```
-> {"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}
<- {"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{"tools":{}},"serverInfo":{"name":"xtop","version":"0.1.0"}}}

-> {"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"processes_search","arguments":{"pattern":"sshd","fields":"name,cmd"}}}
<- {"jsonrpc":"2.0","id":2,"result":{"content":[{"type":"text","text":"[...]"}]}}
```
