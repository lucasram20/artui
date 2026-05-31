# Phase N3 — Writethrough on apply_patch

**Phase:** N3 (LSP support, the killer feature)
**Spec:** [docs/specs/lsp.md](../specs/lsp.md)
**Depends:** N1 (skeleton), N2 (diagnostics cache), E (apply_patch)
**Estimated PR size:** ~400 LoC
**Target release:** v0.6.0

---

## Why

This is the headline feature. Every time the agent edits a file, the LSP
sees the new content, computes diagnostics, and pushes them back to artui.
Phase N3 surfaces those diagnostics inside the same `apply_patch` tool
result so the model sees its breakage *immediately* in the same turn it
caused it. That short feedback loop is what makes oh-my-pi's `lsp wired
into every write` headline real — and it's the gap between "agent that
writes code" and "agent that writes code that compiles".

## Scope

### In scope

- `src/lsp/writethrough.rs` — new module:
  - `track(path, contents) -> didOpen-or-didChange`. Maintains a per-file
    version counter inside `LspClient::open_files`.
  - `untrack(path) -> didClose`. Called on file deletion.
  - `await_diagnostics(paths, timeout) -> HashMap<PathBuf,
    Vec<Diagnostic>>`. Polls the diagnostics cache until either every
    requested path has fresh diagnostics (`version == current`) or the
    timeout fires.
- `tools/apply_patch.rs` — after a successful patch, if
  `ctx.lsp_manager.is_some()` and `cfg.lsp.writethrough`:
  1. For each `(path, after)` in the patch, call
     `manager.for_path(path)` and run `track(path, after)`.
  2. Spawn an `await_diagnostics` task with the configured
     `diagnostics_timeout_ms` (default 750 ms).
  3. Format the diagnostics into a section appended to the tool result:

     ```
     ── LSP diagnostics ──
     src/foo.rs:42:8 [error] expected `;`, found `}`
     src/foo.rs:51:1 [warn] unused import: `std::fmt::Display`
     ```

  4. Scope output to the changed lines plus a 3-line buffer so the model
     doesn't see unrelated diagnostics from elsewhere in the file.
- `src/config/schema.rs`: extend `LspConfig` with
  `writethrough: bool` (default true) and
  `diagnostics_timeout_ms: u32` (default 750).
- Render budget: cap the diagnostics block at
  `[agent].max_tool_output_chars / 4`; truncate with a
  `… N more diagnostics …` footer.

### Out of scope

- Auto-fixing diagnostics (that's N4 — the agent decides whether to call
  `code_actions`).
- Diagnostics for files the agent only *read* but didn't write. Those
  remain accessible via the explicit `lsp.diagnostics` action.
- Pre-flight check before writing — only the *post*-write loop.
- Format-on-save. We don't push `textDocument/formatting`; the agent's
  own formatter (`apply_patch` is exact text) is authoritative.

## Acceptance criteria

- [ ] After `apply_patch` on a Rust file with an introduced compile
      error, the tool result includes the rust-analyzer diagnostic
      within ~1 s.
- [ ] After `apply_patch` on a file with no resulting issues, the tool
      result includes `── LSP diagnostics ── (clean)` so the model
      knows the check ran.
- [ ] If the LSP server doesn't respond within
      `diagnostics_timeout_ms`, the tool result appends
      `── LSP diagnostics ── (timeout)` instead of hanging.
- [ ] `cfg.lsp.writethrough = false` disables the post-patch hook
      entirely; tool result is identical to v0.5.x.
- [ ] +5 tests (track, untrack, await happy/timeout/clean,
      out-of-scope-diagnostic-filtering).
- [ ] Manual smoke: agent introduces a typo, sees the diagnostic in
      the same tool turn, fixes it on the next turn. Logged in the
      changelog as a transcript.

## Files touched

```
src/lsp/writethrough.rs              NEW ~250 LoC
src/lsp/client.rs                    +open_files version bump on track
src/tools/apply_patch.rs             +writethrough hook (~50 LoC)
src/config/schema.rs                 +LspConfig.writethrough,
                                       diagnostics_timeout_ms
docs/changelogs/CHANGELOG.md         +0.6.0 entry
```

## Test plan

| Layer       | Tests                                                          |
| ----------- | -------------------------------------------------------------- |
| Mock client | track sends didOpen on first call, didChange on subsequent;    |
|             | await returns when version matches; timeout returns partial    |
| apply_patch | success → writethrough invoked; failure → not invoked;         |
|             | writethrough=false → not invoked                               |
| Render      | diagnostics outside changed-line ± 3 buffer dropped;           |
|             | clean state renders "(clean)" footer                           |
| Integration | `cfg(integration)`: real rust-analyzer; introduce a typo;      |
|             | assert diagnostic appears in tool result                       |

## Risks

- **Latency tail**: rust-analyzer can stall up to 5 s on first-edit-of-day.
  Mitigation: 750 ms default timeout, configurable; emit
  `(timeout — try `lsp diagnostics` later)` so the model knows the
  feedback might be incomplete.
- **Diagnostic scoping false negatives**: a 3-line buffer around changed
  lines might miss errors caused by the edit but reported far away
  (e.g. a deleted import causing 20 errors elsewhere). Mitigation: when
  >5 diagnostics are pushed for an unchanged region, include a
  `(N elsewhere — run lsp.diagnostics for full list)` footer.
- **didChange ordering**: must send before `await_diagnostics` polls,
  must include the entire post-patch contents not the diff. Use full
  content (`TextDocumentSyncKind::Full`) for v1; incremental sync is a
  later optimization.
- **Concurrent edits**: two simultaneous `apply_patch` calls (subagents)
  to the same file. Mitigation: serialise per-path through
  `Mutex<HashMap<PathBuf, version>>`; the writethrough is part of the
  patch tool's critical section, not parallelizable.
