# LSP support for artui

Plan to wire Language Server Protocol clients into artui so the agent can ask
"what's the type of `Foo`?", "where is this function defined?", "what calls this?",
"rename this symbol everywhere", and "is the file I just wrote diagnostic-clean?"
without re-implementing those queries with grep + Tree-sitter.

The reference is **oh-my-pi** (`packages/coding-agent/src/lsp/`). It has the
sharpest design of the four CLIs surveyed:

| CLI            | LSP shape                                                        |
| -------------- | ---------------------------------------------------------------- |
| **oh-my-pi**   | First-class `lsp` tool with 14 actions; writethrough-on-edit; defaults.json registry covering ~60 languages; warms servers at startup |
| **Claude Code**| Pluginised — `typescript-lsp` etc. ship as separate plugins under `~/.claude/plugins/`. Loose contract |
| **OpenCode**   | Built-in, per-language clients, surfaces diagnostics and definitions to the agent |
| **Codex CLI**  | Lightweight, mostly diagnostics + go-to-definition |

oh-my-pi's design is the one we're cloning, ported to Rust, sized for what artui
actually needs.

## Goals

1. Single `lsp` tool the model calls with `{action, params}`. Same dispatch
   pattern as the existing tools (`shell`, `read_file`, `apply_patch`, …).
2. A registry of language servers driven by a TOML/JSON file, with sensible
   defaults baked into the binary and user overrides via `~/.config/artui/`.
3. **Writethrough**: every successful `apply_patch` notifies the LSP, then
   pulls diagnostics for the edited files and feeds them back to the agent in
   the same tool result. The model sees its own breakage immediately.
4. Process lifecycle that survives the agent loop — servers spawn lazily on
   first use per workspace root, stay warm, and shut down on artui exit.
5. Read-only operations (definition, references, hover, symbols, diagnostics)
   bypass the approval engine — they're indistinguishable from `read_file`
   for safety. Mutating operations (rename, code actions, applyEdit) go
   through the same approval pipeline as `apply_patch`.
6. Crash isolation: a broken language server is a logged warning, not a fatal
   error. The agent falls back to grep + Tree-sitter the way it does today.

## Non-goals (for v1)

- Inline-completion / signature-help / inlay hints. The model writes code; it
  doesn't need autocomplete.
- A polyglot LSP-aggregator UI. The TUI surfaces diagnostics in tool output
  and that's it.
- Wire-protocol coverage parity with vscode-languageclient. We only implement
  the requests in the §"Operations" table.
- A bundled language-server distribution. Users install rust-analyzer / gopls
  / pyright themselves the same way they would for any editor; we discover
  them on `$PATH`.

## Architecture

```
src/
├── lsp/                            ← NEW
│   ├── mod.rs                      # public surface, re-exports
│   ├── client.rs                   # one LspClient per (server, root) — owns the
│   │                               #   child process, request id counter,
│   │                               #   open-file map, diagnostics cache
│   ├── registry.rs                 # ServerRegistry: languageId → ServerSpec
│   │                               #   loads defaults.toml + user overrides;
│   │                               #   resolves a file path to a server
│   ├── manager.rs                  # LspManager: workspace-wide map of
│   │                               #   (server_id, root) → Arc<LspClient>;
│   │                               #   spawn-on-demand, warmup at startup,
│   │                               #   graceful shutdown on Drop
│   ├── writethrough.rs             # didOpen/didChange/didSave + pullDiagnostics
│   │                               #   helper invoked from apply_patch
│   ├── render.rs                   # LSP types → human-readable strings the
│   │                               #   model can act on (paths, ranges,
│   │                               #   diagnostic severity tags)
│   ├── defaults.toml               # bundled server definitions (rust-analyzer,
│   │                               #   gopls, pyright, typescript-language-server,
│   │                               #   clangd, …)
│   └── types.rs                    # ServerSpec, LspAction, LspToolArgs,
│                                   #   LocationView, DiagnosticView, RootMarker
└── tools/
    └── lsp.rs                      ← NEW — implements `Tool` for the registry,
                                    #   thin shim that parses LspAction and
                                    #   dispatches to LspManager.
```

The split mirrors oh-my-pi's `client.ts` / `config.ts` / `index.ts` / `edits.ts`
/ `render.ts` layout, just spelled in idiomatic Rust modules.

### Crate choice: `async-lsp`

We pick **`async-lsp`** over `tower-lsp` and a hand-rolled JSON-RPC stack:

| Option            | Verdict                                                 |
| ----------------- | ------------------------------------------------------- |
| `async-lsp`       | ✅ Tower-based, supports both client and server roles, ships an `omni_trait::LanguageServer` so we can call `socket.definition(params).await` directly. Pluggable middleware (concurrency cap, lifecycle, tracing). Active. |
| `tower-lsp`       | ❌ Server-only. Would need a custom client wrapper over `lsp-types` + `lsp-server`. |
| Hand-rolled       | ❌ ~2-3kloc to match oh-my-pi's `client.ts`. We'd own the bug surface. |
| `lsp-server`      | ❌ Sync, intended for in-process language servers, not driving external children. |

We add to `Cargo.toml`:

```toml
async-lsp = { version = "<latest>", features = ["client-monitor", "tokio"] }
lsp-types = "<matching version that async-lsp re-exports>"
```

`async-lsp` re-exports `lsp_types::*`, so the request/notification types stay
strongly typed end-to-end — no `serde_json::Value` shims at the call site.

### `LspClient` (per server-process)

```rust
pub struct LspClient {
    server_id: String,                  // "rust-analyzer", "gopls", …
    root: PathBuf,                      // resolved workspace root
    socket: ServerSocket,               // async-lsp client handle
    capabilities: ServerCapabilities,   // populated after initialize
    open_files: Mutex<HashMap<PathBuf, OpenFile>>,
    diagnostics: Mutex<HashMap<PathBuf, Vec<Diagnostic>>>,
    _proc: Child,                       // killed on Drop
}

struct OpenFile { version: i32, language_id: String }
```

`LspClient::spawn(spec, root, events)` does:

1. `tokio::process::Command::new(spec.command).args(&spec.args).stdio(piped()).spawn()`.
2. Wires stdin/stdout into `async_lsp::MainLoop` running on a Tokio task.
3. Sends `initialize` with our client capabilities, awaits the response.
4. Sends `initialized`.
5. Returns the wired-up client. Errors on step 1 fall back to a `Disabled`
   placeholder so subsequent calls fail fast and loud.

Server-to-client notifications routed via a custom `LspService`:

- `publishDiagnostics` → updates `diagnostics` map, emits `AppEvent::LspDiagnostics`
  for the TUI footer.
- `window/showMessage` / `window/logMessage` → tracing log.
- `workspace/applyEdit` → bridge into the approval engine (out of v1; for now
  return `applied: false` and log a warning so we don't silently mutate the
  workspace).

### `ServerRegistry`

`defaults.toml`, embedded with `include_str!`. The seed registry is **vendored
from helix-editor's `languages.toml`** (MPL-2.0) and ported to artui's
schema. helix has the cleanest, most-curated language-server registry in the
ecosystem — ~80 language servers wired up across the languages oh-my-pi
covers and then some. Vendoring saves us from writing ~60 entries by hand
and gets us the same coverage on day one.

License compliance: we ship a `src/lsp/NOTICE` file crediting helix and
quoting the MPL-2.0 §3 attribution clause. The TOML data is not linked
code so we're not subject to the file-level copyleft, but we attribute as
a courtesy and to keep the audit trail clean. (If we ever modify entries,
the modifications go into a sibling `defaults.artui.toml` overlay merged
last so the upstream file stays untouched and re-syncable.)

The schema we port to (one entry per server, nesting language → servers
the way helix does it):

```toml
[server.rust-analyzer]
command = "rust-analyzer"
file_types = ["rs"]
root_markers = ["Cargo.toml", "rust-project.json"]
init_options = { checkOnSave = { command = "clippy" } }

[server.gopls]
command = "gopls"
file_types = ["go"]
root_markers = ["go.mod", "go.sum"]

[server.pyright]
command = "pyright-langserver"
args = ["--stdio"]
file_types = ["py", "pyi"]
root_markers = ["pyproject.toml", "setup.py", "setup.cfg", "pyrightconfig.json"]

[server.typescript-language-server]
command = "typescript-language-server"
args = ["--stdio"]
file_types = ["ts", "tsx", "js", "jsx", "mts", "cts"]
root_markers = ["package.json", "tsconfig.json"]

[server.clangd]
command = "clangd"
file_types = ["c", "cc", "cpp", "cxx", "h", "hpp"]
root_markers = ["compile_commands.json", "compile_flags.txt", ".clangd"]

# … ~75 more entries vendored from helix's languages.toml: zls, taplo,
# elixir-ls, gleam, nimlsp, dart, kotlin-language-server, lua-language-server,
# bashls, vimls, hls, ocaml-lsp, sourcekit-lsp, terraformls, ansiblels,
# yaml-language-server, jdtls, omnisharp, scalameta, perlnavigator, …
```

A `scripts/sync-helix-lsp.py` helper periodically pulls helix's
`languages.toml` and re-emits our schema so we can refresh the registry
without rewriting it.

User overrides live at `~/.config/artui/lsp.toml` and merge over the
defaults — same pattern as the existing `config.toml` / provider config.
This is also where users add servers we didn't bundle (deno, ruff, jujutsu,
…).

`ServerRegistry::resolve(path)`:

1. Map extension → `server_id` candidates.
2. Walk up from `path` looking for any `root_markers` entry; first hit wins.
3. Return `Some((server_id, root))` or `None` (file is unsupported, the
   tool returns a clear "no language server for `.zig`" error).

### `LspManager`

```rust
pub struct LspManager {
    registry: ServerRegistry,
    clients: tokio::sync::RwLock<HashMap<(String, PathBuf), Arc<LspClient>>>,
    events: mpsc::Sender<AppEvent>,
}

impl LspManager {
    pub async fn warmup(&self, cwd: &Path) -> WarmupReport;
    pub async fn for_path(&self, path: &Path) -> anyhow::Result<Arc<LspClient>>;
    pub async fn shutdown(&self);
}
```

`warmup` enumerates root markers under `cwd` and spawns clients for any
server that has at least one matching file. Bounded with a 5-second timeout
per server so a slow rust-analyzer initialise can't block artui startup —
the warmup runs as a background task and the agent can use LSP before it
finishes, just with a "warming up" placeholder.

`for_path` is the lazy spawn-on-demand path. First call for a (server, root)
pair builds the client; subsequent calls hit the cache.

### Operations exposed to the model

The single `lsp` tool with the schema:

```json
{
  "name": "lsp",
  "description": "Language-server-backed code intelligence: definition, references, hover, symbols, diagnostics, rename. Uses installed servers (rust-analyzer, gopls, pyright, typescript-language-server, clangd, …).",
  "parameters": {
    "type": "object",
    "required": ["action"],
    "properties": {
      "action": { "enum": ["definition", "references", "hover", "implementation", "type_definition", "document_symbols", "workspace_symbols", "diagnostics", "rename", "code_actions", "status"] },
      "path":   { "type": "string", "description": "workspace-relative file path (required for textDocument/* actions)" },
      "line":   { "type": "integer", "description": "1-based line number" },
      "column": { "type": "integer", "description": "1-based column number" },
      "query":  { "type": "string", "description": "for workspace_symbols" },
      "new_name": { "type": "string", "description": "for rename" }
    }
  }
}
```

| Action             | LSP request                          | Approval | Notes |
| ------------------ | ------------------------------------ | -------- | ----- |
| `definition`       | `textDocument/definition`            | none     | returns ≤8 hits |
| `references`       | `textDocument/references`            | none     | `includeDeclaration: true` |
| `hover`            | `textDocument/hover`                 | none     | strips markdown if no `pretty_print` |
| `implementation`   | `textDocument/implementation`        | none     | |
| `type_definition`  | `textDocument/typeDefinition`        | none     | |
| `document_symbols` | `textDocument/documentSymbol`        | none     | tree-rendered |
| `workspace_symbols`| `workspace/symbol`                   | none     | capped at 50 results |
| `diagnostics`      | reads cached `publishDiagnostics`    | none     | invalidated when the file is touched |
| `rename`           | `textDocument/rename` → `WorkspaceEdit` | YES   | rendered as a diff, run through the same approval flow as `apply_patch` |
| `code_actions`     | `textDocument/codeAction`            | YES if executed | listing is read-only; applying goes through approval |
| `status`           | none — internal                      | none     | reports which servers are running, stalled, or failed |

Read-only set is hard-coded; it's not a config knob.

### Writethrough on `apply_patch`

`tools/apply_patch.rs` already produces a list of `(path, before, after)`
triples after the patch lands. After the patch succeeds, the tool calls into
`lsp::writethrough::after_edit(&manager, &paths, ctx.events.clone()).await`,
which:

1. For each path: send `textDocument/didChange` (or `didOpen` if not yet
   tracked) with the new contents and bump the version.
2. Wait up to 750 ms for `publishDiagnostics` for each file (real work
   already started — most servers push within ~200 ms for incremental edits;
   rust-analyzer's check-on-save is the slow one and we bail).
3. Append the diagnostics to the `apply_patch` tool result, scoped to
   the changed lines plus a 3-line buffer.

That gives the model the loop: patch → see breakage → fix → see clean.

### Capability detection and graceful degradation

After `initialize`, we cache `ServerCapabilities`. Before each request we
gate-check:

```rust
match action {
    LspAction::Rename => caps.rename_provider.is_some(),
    LspAction::CodeActions => caps.code_action_provider.is_some(),
    // …
}
```

If a server doesn't support an action, the tool returns
`"server <id> does not advertise <action> capability"` — actionable feedback
the agent can route around.

Servers that take a long time to *first* start indexing (rust-analyzer
indexes a fresh workspace for ~30s) are tracked in `LspManager`; calls
during the indexing window return an "indexing, retry in N seconds" string
rather than a hung future. We watch `$/progress` notifications with the
`rustAnalyzer/cachePriming` token to drive this.

## Configuration

`~/.config/artui/config.toml`:

```toml
[lsp]
enabled = true                      # master switch
warmup_on_startup = true            # spawn detected servers eagerly
writethrough = true                 # diagnostics-on-edit feedback
diagnostics_timeout_ms = 750        # how long to wait after didChange
log_messages = false                # forward window/logMessage to tracing
```

Plus `~/.config/artui/lsp.toml` for server overrides:

```toml
[servers.rust-analyzer]
init_options = { checkOnSave = { command = "check" } }

[servers.deno]
command = "deno"
args = ["lsp"]
file_types = ["ts", "tsx", "js", "jsx"]
root_markers = ["deno.json", "deno.jsonc"]
```

Disabling all of LSP is `lsp.enabled = false` in `config.toml` — the tool
isn't registered, the model never sees it.

## Testing strategy

| Layer              | Tests                                                     |
| ------------------ | --------------------------------------------------------- |
| `ServerRegistry`   | extension → server resolution; root-marker walk; user override merging |
| `LspClient` (mock) | `wiremock`-style stdio harness — feed canned JSON-RPC frames; assert `definition` / `hover` / `references` parse |
| `LspManager`       | warmup spawns the right set; `for_path` is idempotent; shutdown kills children |
| `writethrough`     | apply_patch → didChange → diagnostics-pull happy path with mock client |
| Integration        | one feature-gated test that spawns real `rust-analyzer` against a fixture crate and asserts `definition` resolves a known symbol. Gated on `cfg(integration)` and `cargo test --features lsp-integration`, skipped in normal CI |

Target: 25–30 new lib tests, 1 integration test. No regressions in the 166
existing.

## Rollout phases

The work breaks into four phases I'd ship as separate releases:

| Phase | Scope | Ship as |
| ----- | ----- | ------- |
| **1. Skeleton** | `lsp/` module, `async-lsp` dep, `ServerRegistry`, `LspManager`, `LspClient` happy path, `lsp` tool with `definition` + `hover` + `status`. defaults.toml shipping rust-analyzer + gopls + pyright + typescript + clangd. | `0.5.0` |
| **2. Read-only completeness** | references, implementation, type_definition, document_symbols, workspace_symbols, diagnostics. Cached `publishDiagnostics`. status panel in TUI. | `0.5.x` patch |
| **3. Writethrough** | didOpen/didChange tracking, post-`apply_patch` diagnostic pull, render diagnostics into tool result, surface in chat footer. | `0.6.0` |
| **4. Mutating ops** | rename + code_actions, hooked into the approval engine the same way `apply_patch` is. workspace/applyEdit handling. | `0.7.0` |

Each phase is independently shippable and adds value on its own.

## Open questions

1. **Approval UX for rename**: should it render as a single `WorkspaceEdit`
   diff in the existing approval modal, or split into per-file approvals?
   The existing modal handles a single patch — rename can touch dozens of
   files. Lean: render aggregated diff, single approve/reject, but flag if
   >5 files affected.
2. **rust-analyzer warmup wait**: do we block `lsp` calls during cache
   priming, queue them, or fall back to "indexing" string immediately?
   Lean: queue with a 30s ceiling, then bail.
3. **Telemetry**: track LSP call count, error rate, p95 latency per server?
   `tracing` spans are free; aggregating numbers is the cost.
4. **Embedded Tree-sitter fallback**: for files no server is configured
   for, do we surface a graceful "use grep instead" message, or auto-fall
   back to a Tree-sitter-backed `definition` heuristic? v1: clear error.
5. **MCP overlap**: existing MCP servers can already shell-call language
   servers. Do we deprecate user-supplied LSP-via-MCP configurations once
   first-class LSP lands? Lean: leave alone, document the redundancy.

## Risk register

- **Process leak**: a panicking artui must not orphan child language servers.
  `LspClient::Drop` sends `shutdown` + `exit`, then `kill_on_drop(true)` on
  the `Child`. Verified by an integration test that asserts no
  `rust-analyzer` PID survives a deliberate `panic!()`.
- **Binary size growth**: `async-lsp` + `lsp-types` adds ~600KB to the
  release binary. Acceptable; release tarball goes from ~4MB to ~4.6MB.
- **Cross-platform pain**: process spawning on Windows is
  well-trodden in tokio (`Command::new` works), but path normalization
  (URI ↔ path) needs care. Use `lsp_types::Url::from_file_path` and friends
  end-to-end; no manual `file://` strings.
- **Server discovery**: a fresh artui install on a machine without
  rust-analyzer logs a friendly "no language server found for *.rs;
  install rust-analyzer to enable LSP". Not an error — the model just
  doesn't get the tool action for that file.

## Files this plan touches

```
Cargo.toml                          (+2 deps)
src/lib.rs                          (+1 mod declaration)
src/lsp/                            (NEW — 8 files, ~1.5kloc)
src/lsp/defaults.toml               (NEW — bundled server registry)
src/tools/mod.rs                    (+1 mod declaration)
src/tools/lsp.rs                    (NEW — ~250 lines)
src/tools/registry.rs               (+1 register call gated on config)
src/config/schema.rs                (+1 LspConfig struct, defaults)
src/app.rs                          (+ LspManager startup wire-up,
                                     pass through ToolContext)
src/tools/mod.rs (ToolContext)      (+ lsp_manager: Option<Arc<LspManager>>)
src/tools/apply_patch.rs            (+ writethrough call after a successful patch)
docs/specs/lsp.md                   (this file — already written as the plan)
docs/changelogs/CHANGELOG.md        (each phase entry)
```

Estimated total: ~1.8kloc of new Rust + ~150 lines of TOML defaults + tests.
