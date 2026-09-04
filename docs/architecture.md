# xtop-extension-mcp architecture

`xtop-extension-mcp` is a **server-style extension** built on the
`xtop-extension-api` contract crate. It never talks to the kernel directly:
the kernel hosts it behind its `mcp-extension` feature and launches it from
the `xtop mcp` command.

## Extension trait usage

- `McpExtension` implements the [`Extension`] trait:
  - `manifest()` declares id `mcp`, name `xtop MCP server`, version from the
    crate version (`env!("CARGO_PKG_VERSION")`), and the single server id
    `mcp`.
  - `run_server(server_id, ctx)` rejects every server id other than `"mcp"`
    with `ExtensionError::Unknown` and otherwise runs the stdio server loop.
- While running, the extension drives the kernel exclusively through the two
  methods of [`ExtensionContext`]:
  - `tick()` — advance the monitoring tick (also ticks hosted plugins); the
    server calls it before every `tools/call` execution.
  - `execute_plugin(plugin_id, action, params)` — run a plugin action; all
    tool calls funnel through it.

## Structure

- `src/lib.rs`:
  - `run_mcp_server(ctx)` — the stdio loop: reads one line at a time,
    skips blanks, serializes and writes the response, flushes.
  - `handle_line(line, ctx)` — parses one JSON-RPC line and dispatches to
    the handlers; malformed JSON becomes `ExtensionError::Recoverable`
    (server stops), everything else returns the response object. Extracted
    from the loop so the JSON-RPC layer is testable without a terminal.
  - `handle_initialize` / `handle_tools_list` / `handle_tools_call` — the
    three protocol handlers.
  - `TOOL_ACTIONS`, `tool_name()`, `action_for_tool()` — the tool table
    derived from the samurai constants.
- `src/tests.rs` — unit tests of the JSON-RPC layer against a mock
  [`ExtensionHost`] that records `execute_plugin` calls and returns canned
  results (`tick()` no-ops).

## Why it depends on the plugins repo

The tools this server advertises are the samurai plugin's actions. Before
DR-6 the plugin id `"samurai"` and the 12 action strings were duplicated by
hand in this crate, so renaming an action (or the plugin) broke the MCP
tools **silently at runtime**. Depending on `xtop-plugin-samurai` makes the
coupling compile-time consistent:

- `PLUGIN_ID` drives every `execute_plugin` call (there is no other
  hardcoded plugin id in the crate);
- `actions::*` drives both the tool list (`tools/list`, order and names)
  and the argument translation (`tools/call`): each tool name is resolved
  back to its action constant before dispatch;
- a rename on either side now fails the extension build instead of breaking
  an AI client later.

Trade-off: the extension build pulls in the plugin crate (and with it
ratatui and `xtop-plugin-api`). That is acceptable for a kernel-hosted
server extension; in exchange both repos stay in lockstep.

## Relationship to samurai docs

- The wire protocol and the per-tool params mapping live in
  [mcp-protocol.md](mcp-protocol.md).
- The samurai actions, lifecycle and params mini-syntax are documented in
  the plugins repo (`plugins/docs/architecture.md`); the heuristics behind
  `process.alerts` / `alerts.status` in `plugins/docs/rules.md`.

[`Extension`]: https://docs.rs/xtop-extension-api/latest/xtop_extension_api/trait.Extension.html
[`ExtensionContext`]: https://docs.rs/xtop-extension-api/latest/xtop_extension_api/struct.ExtensionContext.html
[`ExtensionHost`]: https://docs.rs/xtop-extension-api/latest/xtop_extension_api/trait.ExtensionHost.html
