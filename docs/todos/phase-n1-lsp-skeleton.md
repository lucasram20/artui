# Phase N1 — LSP Skeleton + Minimal Tool

**Phase:** N1 (LSP support, foundation)
**Spec:** [docs/specs/lsp.md](../specs/lsp.md)
**Depends:** B (tool registry), F (shell tool spawn pattern)
**Estimated PR size:** ~1500 LoC (was ~1200 — bumped to account for the
~80-entry vendored registry and the sync script)
**Target release:** v0.5.0

---

## Why

The four CLIs we benchmark against (Codex, Claude Code, OpenCode, oh-my-pi)
all ship LSP. artui doesn't. Without it, the agent answers "where is `Foo`
defined?" by grepping — which is fine for symbol names but hopeless for
overloads, generics, re-exports, type aliases, and across-module references.

Phase N1 lays the foundation: a Tower-based LSP client wired into the tool
loop, a TOML server registry covering five languages, and three minimal
operations (definition, hover, status). N2–N4 build on top.

## Scope

### In scope

- New `src/lsp/` module:
  - `client.rs` — one `LspClient` per (server, root). Owns the child
    process, the `async_lsp::ServerSocket`, the `open_files` map, the
    `diagnostics` cache, and the request-id counter.
  - `manager.rs` — `LspManager`: workspace-wide cache of `Arc<LspClient>`
    keyed by `(server_id, root)`. Lazy spawn on first use. `warmup(cwd)`
    runs as a background task on artui startup. Graceful shutdown on
    `Drop` (sends `shutdown` + `exit`, then `kill_on_drop(true)`).
  - `registry.rs` — `ServerRegistry`: parses `defaults.toml` (embedded
    via `include_str!`) merged with `~/.config/artui/lsp.toml`. Exposes
    `resolve(path) -> Option<(server_id, root)>` driven by file extension
    + root-marker walk.
  - `render.rs` — turns `lsp_types::Location` / `lsp_types::Hover` into
    human-readable strings the model can act on.
  - `types.rs` — `ServerSpec`, `LspAction`, `LspToolArgs`,
    `LocationView`, `RootMarker`.
  - `defaults.toml` — **vendored from helix-editor's `languages.toml`**
    (MPL-2.0) and ported to artui's schema. ~80 servers across every
    mainstream language: rust-analyzer, gopls, pyright,
    typescript-language-server, clangd, zls, taplo, elixir-ls, gleam,
    nimlsp, dart, kotlin-language-server, lua-language-server, bashls,
    vimls, hls, ocaml-lsp, sourcekit-lsp, terraformls, ansiblels,
    yaml-language-server, jdtls, omnisharp, scalameta, perlnavigator,
    docker-langserver, marksman, vscode-css-languageserver,
    vscode-html-languageserver, vscode-json-languageserver, prismals,
    graphql-language-service, …
  - `NOTICE` — MPL-2.0 §3 attribution credit for the helix-derived
    portion of `defaults.toml`. License compliance audit trail.
- New `scripts/sync-helix-lsp.py` — pulls upstream
  `helix-editor/helix:languages.toml`, transforms to artui's schema,
  re-emits `defaults.toml`. Run manually before each release that
  refreshes the registry; not a build-time dep.
- New `src/tools/lsp.rs` — implements `Tool`. Single `lsp` tool with
  `action ∈ {definition, hover, status}` for this phase. Future actions
  return "not yet implemented in N1; see phase N2".
- `Cargo.toml`: add `async-lsp` (with `tokio` + `client-monitor` features)
  and `lsp-types` (matching the version `async-lsp` re-exports).
- `src/config/schema.rs`: new `LspConfig` struct with
  `enabled: bool` (default true), `warmup_on_startup: bool` (default
  true), `log_messages: bool` (default false), `request_timeout_secs:
  u32` (default 10).
- `src/app.rs`: build an `Arc<LspManager>` at startup (only if
  `cfg.lsp.enabled`), pass it through `ToolContext`. Spawn warmup as a
  background task.
- `src/tools/mod.rs`: `ToolContext` gains
  `lsp_manager: Option<Arc<LspManager>>`.
- `src/tools/registry.rs`: register `LspTool` only when the manager
  exists.

### Out of scope

- references, implementation, type_definition, symbols, diagnostics
  (Phase N2).
- Writethrough on apply_patch (Phase N3).
- rename, code_actions (Phase N4).
- Bundling language servers in artui's release archive — users install
  rust-analyzer / gopls etc. themselves the same way they would for any
  editor. The README documents this.
- A polyglot LSP-aggregator UI. The TUI surfaces tool output and that's
  enough for v1.

## Acceptance criteria

- [ ] `cargo build` clean with `async-lsp` + `lsp-types` added.
- [ ] `cargo test` ≥ 175 lib + 8 integration (24 new tests covering
      registry resolution, manager spawn-on-demand, render).
- [ ] `cargo clippy --all-targets -- -D warnings` clean.
- [ ] `cargo fmt --all -- --check` clean.
- [ ] On a fresh artui run inside a Cargo project, the model can call
      `{"action": "definition", "path": "src/lib.rs", "line": 12,
      "column": 8}` and get back a `LocationView` pointing at the right
      file:line:col.
- [ ] On the same project, `{"action": "hover", ...}` returns the
      symbol's hover doc (markdown stripped to plain text by default).
- [ ] `{"action": "status"}` lists running servers, their root, and
      indexing state. Works even when no servers are running.
- [ ] `defaults.toml` parses cleanly and loads all ~80 helix-derived
      server entries; spot-check rust-analyzer, gopls, pyright,
      typescript-language-server, clangd, zls, hls, lua-language-server,
      jdtls.
- [ ] `NOTICE` file shipped alongside `defaults.toml` crediting
      helix-editor under MPL-2.0 §3.
- [ ] `scripts/sync-helix-lsp.py` regenerates `defaults.toml` from
      upstream and produces a deterministic byte-identical output on
      repeat runs.
- [ ] When rust-analyzer is not on `$PATH`, calling `definition` on a
      `.rs` file returns a clear "rust-analyzer not found; install via
      `rustup component add rust-analyzer`" string. Not a panic, not a
      crash.
- [ ] Killing artui mid-call does not orphan child language servers.
      Verified by an integration test that asserts no surviving
      `rust-analyzer` PID after `panic!()`.
- [ ] `cfg.lsp.enabled = false` removes the tool from the registry
      entirely; the model never sees it.

## Files touched

```
Cargo.toml                               +2 deps
src/lib.rs                               +1 mod lsp
src/lsp/                                 NEW (8 files)
  ├── mod.rs                             public surface
  ├── client.rs                          ~350 LoC
  ├── manager.rs                         ~250 LoC
  ├── registry.rs                        ~200 LoC
  ├── render.rs                          ~80 LoC
  ├── types.rs                           ~120 LoC
  └── defaults.toml                      ~50 lines (5 servers)
src/tools/mod.rs                         +ToolContext.lsp_manager
src/tools/lsp.rs                         NEW ~250 LoC (3 actions)
src/tools/registry.rs                    +1 conditional register
src/config/schema.rs                     +LspConfig
src/app.rs                               +manager construction, warmup spawn
docs/changelogs/CHANGELOG.md             +0.5.0 entry
```

## Test plan

| Layer              | Tests                                                   |
| ------------------ | ------------------------------------------------------- |
| `ServerRegistry`   | extension → server resolution; root-marker walk; user override merging; missing server returns None |
| `LspClient` (mock) | feed canned JSON-RPC frames over a duplex pipe; assert `definition` / `hover` parse via `lsp_types`; assert `initialize` / `initialized` handshake fires before any request |
| `LspManager`       | `for_path` is idempotent for the same root; cache miss spawns once; shutdown kills children; missing executable returns clean error not panic |
| `lsp` tool         | dispatch happy paths via the registry; bad action enum returns error string; unsupported file type returns "no language server for `.zig`" |
| Integration (`cfg(integration)`) | feature-gated: spawn real `rust-analyzer` against a fixture crate, assert `definition` resolves a known symbol; gated on `cargo test --features lsp-integration`, skipped in normal CI |

Target: 24 new lib tests, 1 feature-gated integration test.

## Risks

- **Process leak**: a panicking artui must not orphan child language
  servers. Mitigation: `LspClient::Drop` sends `shutdown` + `exit`, then
  `kill_on_drop(true)` on the `Child`. Verified by integration test.
- **Binary size growth**: `async-lsp` + `lsp-types` adds ~600 KB. Release
  tarball goes from ~4 MB to ~4.6 MB. Acceptable.
- **rust-analyzer indexing window**: a fresh workspace blocks for ~30 s
  on rust-analyzer's cache priming. Mitigation: track
  `$/progress` notifications with the `rustAnalyzer/cachePriming`
  token; calls during the indexing window return "indexing, retry in N
  seconds" rather than a hung future. Detailed handling in N2; N1's
  status action just reports the indexing state.
- **Cross-platform path handling**: URIs (`file://`) and file paths need
  to round-trip cleanly on Windows. Mitigation: use
  `lsp_types::Url::from_file_path` end-to-end, no manual `file://`
  string construction.

## Crate choice rationale

| Option            | Verdict                                                 |
| ----------------- | ------------------------------------------------------- |
| `async-lsp`       | ✅ Tower-based, supports both client and server roles, ships an `omni_trait::LanguageServer` so we can call `socket.definition(params).await` directly. Pluggable middleware (concurrency cap, lifecycle, tracing). Active. |
| `tower-lsp`       | ❌ Server-only. Would need a custom client wrapper over `lsp-types` + `lsp-server`. |
| Hand-rolled       | ❌ ~2-3 kloc to match oh-my-pi's `client.ts`. We'd own the bug surface for no upside. |
| `lsp-server`      | ❌ Sync, intended for in-process language servers, not driving external children. |

`async-lsp` re-exports `lsp_types::*`, so request/notification types stay
strongly typed end-to-end — no `serde_json::Value` shims at the call site.
