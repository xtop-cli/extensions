# xtop extensions

Xtop extensions, hooks and add-ons for the kernel.

Extensions are optional behaviors wired around the kernel's lifecycle
(theme/layout overrides, config transforms, pre/post-render hooks, new
commands...) through `xtop-extension-api`. The kernel works fully without any
of them.

## Workspace

Each extension lives in its own folder under `extensions/`:

```
extensions/
  xtop-extension-<name>/
    Cargo.toml
    src/
    README.md
```

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
