<h1 align="center">
<img src="https://raw.githubusercontent.com/xtop-cli/web/main/public/img/logo.png" width="100px" alt="Xtop logo" />Extensions</h1>

Official repository for xtop extensions — optional behaviors the kernel can
host. Today this means **server-style extensions**: long-running
integrations the kernel starts on demand, which drive the app through the
host contract in `xtop-extension-api`.

The contract crate provides exactly one shape today: an extension
implements the `Extension` trait (`manifest()` + `run_server(server_id,
ctx)`) and acts through an `ExtensionHost` view that offers `tick()` and
`execute_plugin()` — extensions reach kernel features through plugins, not
through direct hooks. The kernel works fully without any extension.

**Future work**: hook-style extensions that touch config, theme, layout or
rendering (pre/post-render hooks, config transforms, new commands) are
designed as they become needed — nothing in `xtop-extension-api` implements
them yet.

## Workspace

Each extension lives in its own folder under `extensions/`:

```
extensions/
  xtop-extension-<name>/
    Cargo.toml
    src/
    README.md
```

## Current extension: xtop-extension-mcp

- MCP (Model Context Protocol) server over JSON-RPC 2.0 stdio, launched by
  the kernel as `xtop mcp`.
- Exposes the samurai plugin (plugins repo) as 12 MCP tools; the tool table
  is generated from the plugin's exported `PLUGIN_ID` + `actions::*`
  constants, so the extension depends on the plugins repo at compile time.
- [crate README](extensions/xtop-extension-mcp/README.md) · protocol and
  tool mapping in [docs/mcp-protocol.md](docs/mcp-protocol.md) · extension
  architecture in [docs/architecture.md](docs/architecture.md).

## Getting started (development)

From this repo root:

```bash
cargo build --workspace
```

During active development all repos live side by side and use local path
dependencies:

```
xtop/           kernel
api/            API crates
plugins/
effects/
extensions/     this repo
```

## License

MIT
