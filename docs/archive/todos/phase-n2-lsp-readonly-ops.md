# Phase N2 — Read-only LSP Operations

**Phase:** N2 (LSP support, read-only completeness)
**Spec:** [docs/spec/lsp.md](../../spec/lsp.md)
**Depends:** N1 (skeleton + manager + lsp tool)
**Estimated PR size:** ~500 LoC
**Target release:** v0.5.x patch

---

## Why

N1 ships definition + hover + status. The model still can't ask "what
calls this?", "what implementations of this trait exist?", "what symbols
does this file expose?", or "is this file diagnostic-clean right now?".
Those are bread-and-butter read-only ops every editor LSP integration has,
and they don't need approval — they're indistinguishable from `read_file`
for safety.

## Scope

### In scope

- Add to `LspAction` and dispatch in `tools/lsp.rs`:
  - `references` — `textDocument/references` with
    `includeDeclaration: true`. Cap at 50 hits, render as
    `path:line:col\n  preview line`.
  - `implementation` — `textDocument/implementation`.
  - `type_definition` — `textDocument/typeDefinition`.
  - `document_symbols` — `textDocument/documentSymbol`. Tree-render
    nested symbols (class → method → param).
  - `workspace_symbols` — `workspace/symbol` driven by `query` arg.
    Cap at 50 results.
  - `diagnostics` — read the `LspClient::diagnostics` cache populated
    by `publishDiagnostics`. Returns the diagnostics for the requested
    file (or all files if `path` omitted). Severity tags rendered as
    `[error]` / `[warn]` / `[info]` / `[hint]`.
- `LspClient`: route `publishDiagnostics` notifications into the
  `Mutex<HashMap<PathBuf, Vec<Diagnostic>>>` cache. Emit
  `AppEvent::LspDiagnostics(path, count)` so the TUI footer can show
  a "12 errors, 4 warnings in src/foo.rs" badge.
- `src/ui/`: minimal diagnostics-count badge in the bottom-right of the
  chat pane when any cached diagnostics exist. No new popup; clicking is
  out of scope.
- Capability gating: before each request, check
  `caps.references_provider`, `caps.implementation_provider`, etc.
  Servers that don't advertise the capability return
  `"server <id> does not advertise <action>"` rather than failing
  opaquely.

### Out of scope

- Writethrough — diagnostics in this phase only update when the user
  edits files outside the agent (or during indexing). Phase N3 wires
  the post-apply_patch loop.
- Mutating ops (rename, code_actions). Phase N4.
- Diagnostic-quick-fix popups in the TUI.

## Acceptance criteria

- [ ] All five new actions return useful results against a fixture
      Cargo project with rust-analyzer indexed.
- [ ] `references` on a function with 12 callers returns 12 entries
      formatted as `path:line:col\n  preview` and capped if >50.
- [ ] `document_symbols` on `src/lib.rs` returns the module's
      structs/fns/impls in nested form.
- [ ] `workspace_symbols query="LspManager"` finds the type.
- [ ] `diagnostics` on a file with a deliberate compile error reports
      the error inline.
- [ ] Calling `references` against a server that doesn't support it
      returns the capability-not-advertised string, not a hang.
- [ ] TUI badge shows error/warning counts when diagnostics are cached;
      clears when files are clean.
- [ ] +6 tests (one per action) using the mock client harness from N1.
- [ ] All N1 acceptance criteria still pass.

## Files touched

```
src/lsp/types.rs                     +5 LspAction variants
src/lsp/client.rs                    +publishDiagnostics routing,
                                       diagnostic cache
src/lsp/render.rs                    +DocumentSymbol tree, Diagnostic
                                       severity tags
src/tools/lsp.rs                     +5 action arms
src/ui/chat.rs (or equivalent)       +diagnostics badge
src/app.rs                           +AppEvent::LspDiagnostics handler
docs/changelogs/CHANGELOG.md         +0.5.x entry
```

## Test plan

| Layer        | Tests                                                          |
| ------------ | -------------------------------------------------------------- |
| Mock client  | `references` happy path; capability-missing path; `documentSymbol` parses to nested view |
| Integration  | `cfg(integration)`: workspace_symbols against fixture crate finds known symbol |
| TUI          | badge renders correctly with seeded diagnostics cache (snapshot test) |

## Risks

- **Reference explosion**: a `references` query against a popular trait
  (`Display`, `Iterator`) can return thousands of hits. Cap at 50,
  render a "+N more" footer; the model can paginate by narrowing the
  query.
- **Stale diagnostics**: if the file is modified outside artui, the
  cache is stale until rust-analyzer pushes a new
  `publishDiagnostics`. Acceptable — we display a `(cached <Ns ago>)`
  hint when older than 30 s.
- **Workspace symbols quality**: gopls returns workspace symbols
  sorted by relevance; rust-analyzer returns a flat list. We render
  whatever the server gives back, no client-side reranking.
