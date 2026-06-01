# Phase M1 — Diff Preview Popup

**Phase:** M1 (production polish, visible UX)
**Spec:** `docs/spec/harness-architecture.md` §6 Phase E (deviation note)
**Depends:** E (apply_patch already lands; this re-enables the deferred preview)
**Estimated PR size:** ~600 LoC

---

## Why

Phase E shipped `apply_patch` but skipped the streaming diff preview UI.
The model's edits hit disk immediately, no chance to inspect, no abort.
Claude Code, Codex, OpenCode all show the diff before applying. This is
the most visible UX gap when a real user compares artui side-by-side
with the others.

## Scope

### In scope

- New `UiMode::DiffPreview { patch_id, oneshot_tx, parsed_diff }`.
- `src/tools/apply_patch/streaming_parser.rs` already exists — wire it
  to emit incremental hunks as `ModelEvent::ToolCallArgsDelta` arrives,
  so the popup updates live as the model writes.
- TUI popup in `src/ui/diff.rs`: split-pane (file list left, hunk view
  right), syntax-highlighted, scrollable.
- Keybindings:
  - `a` apply
  - `d` deny
  - `j` / `k` cursor through hunks
  - `J` / `K` jump file
  - `q` / `Esc` cancel and deny
- Permission engine returns `PermissionDecision::Ask(prompt)` for
  `apply_patch`; agent loop blocks on a `tokio::sync::oneshot` waiting
  for the popup decision.
- Auto-allow override: `[permissions] apply_patch = "allow"` in config
  for users who trust the agent (matches today's behaviour).

### Out of scope

- Inline edit-the-diff (Claude Code allows tweaking; defer).
- Per-hunk staging (apply some hunks, skip others). Future phase.
- Cross-file refactoring preview (diff already supports multi-file;
  just need the file-tree pane).

## Acceptance criteria

- Model proposes a 3-file patch → popup opens before disk writes.
- Pressing `a` applies; pressing `d` records `denied_by_user` tool
  result; the agent sees the denial and continues.
- Esc cancels the entire turn cleanly (no dangling tool call).
- Popup live-updates as the model streams more hunks (test with a slow
  provider).
- `cargo test` passes; integration test with a fake provider that
  emits a multi-hunk patch.

## Files touched

| File | Change |
|---|---|
| `src/ui/diff.rs` | New popup renderer |
| `src/ui/popups.rs` | Wire diff preview into popup dispatch |
| `src/app.rs` | New `UiMode::DiffPreview` arm; keybindings |
| `src/permissions/policy.rs` | Default `apply_patch = "ask"` |
| `src/agent/loop.rs` | Honor `PermissionDecision::Ask`; oneshot wiring |
| `src/tools/apply_patch.rs` | Hook for streaming preview events |
| Tests | Multi-hunk preview, deny path, esc-cancel |

## Risks

- **Streaming parser races**: hunks may arrive partial. Buffer until a
  complete hunk delimiter is seen; only then update the popup.
- **Esc handling**: must cancel the oneshot AND the agent loop, not
  just close the popup.
- **Large patches**: a 500-line diff popup needs scroll + maybe
  pagination. Cap visible lines at 200; show "+N more lines" hint.

## References

- codex `core/src/tools/handlers/apply_patch.rs` preview path
- opencode `tool/edit.ts` interactive review
- artui spec `docs/spec/harness-architecture.md` §6 Phase E (deferred)
