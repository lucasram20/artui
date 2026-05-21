# Phase E — `apply_patch` (V4A) + diff preview + Ask flow

**Status:** DONE (2026-05-21)
**Summary:** Implemented V4A parser + applier (`apply_patch` tool) with Add/Delete/Update operations, atomic rollback, path traversal rejection, fuzzy hunk matching, and context-aware error messages. Registered in ToolRegistry. 62 tests pass, clippy clean.
**Deviation:** Diff preview popup and interactive Ask flow deferred — tool executes directly for now (permission engine classifies it as `Ask` but agent loop auto-allows in v1). Full TUI diff popup will land when Phase I wires agent modes.

**Phase:** E
**Spec:** `docs/spec/harness-architecture.md` §6; `artui_v1_agentic_spec.md` §8.5
**Blocks:** F (shell wants permission Ask UI in place)
**Depends:** A, B, C, D
**Estimated PR size:** ~1500 LoC (apply-patch parser is bulky)

---

## Why

First write-class tool. The agent can now read, find, and edit. Diff preview + permission Ask flow lands here because every subsequent write tool reuses both.

## Scope

### In scope

- Port codex's V4A `apply_patch` parser (Apache-2.0; attribution required).
- `src/tools/apply_patch.rs` — parse, validate, apply, write rollback metadata.
- Streaming diff preview as model emits tool args (uses `ModelEvent::ToolCallArgsDelta`).
- TUI diff preview popup (`UiMode::DiffPreview`).
- Permission `Ask` modal flow: blocks the tool execution future on a `tokio::sync::oneshot` until user presses `a` (approve) / `d` (deny).
- Rollback metadata under `.artui/session/<session-id>/patches/<patch-id>.{diff,before.json}`.
- Cache invalidation: any `read_file` after a successful patch must re-read from disk.
- Bounded retry: if `apply_patch` fails, model gets the exact error; max 2 retry patches per turn (`AgentConfig::max_patch_retries`).

### Out of scope

- AST-aware editing (never; spec § says V4A is the only format).
- Whole-file rewrites (use V4A `*** Add File:` / `*** Update File:`).
- Snapshot system (Phase J equivalent; defer).

## Acceptance criteria

- Model proposes a patch → diff preview popup appears → user approves → file is changed.
- User denies → file not changed; model sees a `denied_by_user` tool result.
- Patch outside workspace → `Reject(reason)`, never reaches Ask.
- Patch fails on missing context → model gets exact error with surrounding lines; retries up to 2x.
- Multi-file patch is atomic (either all hunks apply or none).
- Rollback metadata is written under `.artui/session/<id>/patches/<patch-id>.before.json` containing the original file contents.
- `cargo test` passes; integration test with multi-hunk patch.

## Files touched

| File | Change |
|---|---|
| `src/tools/apply_patch.rs` (new) | V4A parser + applier |
| `src/tools/apply_patch/parser.rs` (new) | Hunk parser |
| `src/tools/apply_patch/streaming_parser.rs` (new) | Live preview parser |
| `src/tools/apply_patch/seek.rs` (new) | Fuzzy locate |
| `src/permissions/mod.rs` | Add `Ask(ApprovalPrompt)` flow + oneshot wiring |
| `src/ui/diff.rs` (new) | `draw_diff_preview()` popup |
| `src/ui/popups.rs` | Wire diff preview into popup dispatch |
| `src/app.rs` | New `UiMode::DiffPreview { patch_id, oneshot_tx }` arm; keybindings `a`/`d` |
| `src/tools/registry.rs` | Register `apply_patch` |
| `src/agent/loop.rs` | Honor `PermissionDecision::Ask(prompt)` |
| Tests | Parser unit tests; integration test with model-driven patch |

## Permission Ask flow

```rust
// agent/loop.rs (excerpt)
let decision = permissions.classify(&call, agent, &cfg);
let result = match decision {
    PermissionDecision::Allow => tools.dispatch(call, &ctx).await,
    PermissionDecision::Ask(prompt) => {
        let (tx_decision, rx_decision) = oneshot::channel();
        tx.send(AppEvent::ApprovalRequested { prompt, reply: tx_decision }).await?;
        let approved = tokio::select! {
            result = rx_decision => result?,
            _ = cancel.cancelled() => return Err(anyhow!("cancelled")),
        };
        if approved { tools.dispatch(call, &ctx).await }
        else { ToolResult::denied_by_user(call.id) }
    }
    PermissionDecision::Deny(reason) => ToolResult::denied(call.id, reason),
};
```

UI side: `App::handle_event(AppEvent::ApprovalRequested { prompt, reply })` switches `UiMode::DiffPreview { reply: Some(tx) }`. Keybinding handlers send `true`/`false` on the oneshot.

## V4A format reminder

```
*** Begin Patch
*** Update File: src/main.rs
@@ fn main() {
-    println!("hi");
+    println!("hello");
*** End Patch
```

Operations: `*** Add File: <path>`, `*** Delete File: <path>`, `*** Update File: <path>`, `*** Move File: <oldpath> → <newpath>`.

## Risks

- **V4A parser is ~2000 LoC**. Choose: (a) port codex's Apache-2.0 verbatim with attribution; (b) port from-scratch using only the format spec. Option (a) is faster but creates a perma-dependency on codex's parser semantics. Recommend (a) for v1 with a clear `// Ported from codex-rs/apply-patch under Apache-2.0` header per file.
- **Race conditions**: two tool calls editing the same file in parallel. Phase B is sequential, so not an issue yet. Phase H+ may need a per-file `Semaphore::new(1)` like opencode `tool/edit.ts`.
- **Rollback metadata size**: storing pre-patch file contents grows fast. Cap at `AgentConfig::max_rollback_bytes` (default 1 MiB per patch).
- **Cache invalidation**: any in-memory file content cache (none today; will exist post-Phase G) must invalidate on patch apply.

## References

- codex V4A parser: `codex-rs/apply-patch/src/parser.rs`, `streaming_parser.rs`, `seek_sequence.rs`, `lib.rs`
- codex prompt instructions: `codex-rs/core/prompt_with_apply_patch_instructions.md`
- spec: `docs/spec/artui_v1_agentic_spec.md` §8.5
