# xtop-extension-mcp

MCP (Model Context Protocol) server extension for
[xtop](https://github.com/xtop-cli/xtop). Exposes xtop's samurai plugin
(system monitoring, process queries and control, heuristic alerts) as MCP
tools over a JSON-RPC 2.0 stdio server. Any MCP-compatible AI — Claude
Desktop, Cline, etc. — can connect:

```json
{
  "mcpServers": {
    "xtop": {
      "command": "xtop",
      "args": ["mcp"]
    }
  }
}
```

## Protocol overview

- JSON-RPC 2.0 over stdin/stdout, one JSON object per line.
- Handles `initialize`, `tools/list` and `tools/call`; anything else is
  answered with `-32601`. Execution failures map to `-32000`, missing
  arguments to `-32602`.
- Malformed JSON stops the server with a recoverable error (no response is
  emitted).
- Wire details and an example session: [docs/mcp-protocol.md](../../docs/mcp-protocol.md).

## Tools

The server exposes the 12 samurai actions as 12 tools. The tool table is
generated from the constants exported by `xtop-plugin-samurai` (DR-6) —
tool name = action with `.` replaced by `_`:

| Tool | Samurai action | Arguments |
|---|---|---|
| `system_summary` | `system.summary` | — |
| `processes_top` | `processes.top` | `count` (default 10), `filter` (regex) |
| `processes_search` | `processes.search` | `pattern` (regex), `fields` (`name,cmd,user,state,exe,cwd`) |
| `process_info` | `process.info` | `pid` |
| `process_kill` | `process.kill` | `pid` |
| `threshold_set` | `threshold.set` | `cpu`, `mem`, `disk` |
| `threshold_get` | `threshold.get` | — |
| `config_get` | `config.get` | — |
| `config_set` | `config.set` | `interval_ms` / `theme` / `layout` |
| `process_alerts` | `process.alerts` | — |
| `alerts_status` | `alerts.status` | — |
| `plugin_status` | `plugin.status` | — |

Typed arguments are translated to the plugin's string params syntax before
every `execute_plugin` call (see [docs/mcp-protocol.md](../../docs/mcp-protocol.md)).

## Samurai coupling

This crate depends on `xtop-plugin-samurai` at compile time:

- every tool call targets `PLUGIN_ID` — there is no other plugin id;
- `actions::*` drives the tool table and the argument translation.

A rename of the plugin id or of an action now breaks this build instead of
breaking MCP clients at runtime. Architecture and rationale:
[docs/architecture.md](../../docs/architecture.md).

## How the kernel launches it

`xtop mcp` (kernel, `mcp-extension` feature) constructs
`McpExtension::new()` and calls `run_server("mcp", ctx)` with a host context
that resolves `execute_plugin` against the kernel's hosted plugins.

## Development

```bash
cargo build --workspace
./scripts/ci.sh        # fmt | clippy | check | test
```

## License

MIT
