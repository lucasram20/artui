# Phase K — `task` sub-agent tool

**Status:** DONE (2026-05-21)
**Summary:** Implemented `task` tool that spawns subagents with isolated context. Supports `explore` (read-only: read_file/glob/search) and `general` (full minus task) types. No recursion (task tool excluded from subagent registry). Reduced step limit (10). Summary extracted and returned to parent. `ToolRegistry::for_subagent(read_only)` method added. 86 tests pass.

**Phase:** K
**Spec:** `docs/spec/harness-architecture.md` §6
**Depends:** A, B, C, D, G
**Estimated PR size:** ~500 LoC

---

## Why

Long-running research / exploration in the parent context wastes tokens. opencode's `tool/task.ts` and codex's multi-agent v1/v2 both spawn a fresh session with derived permissions, run it through the same loop, and return only the summary — keeping the parent context lean.

This is the first feature that needs a "session within a session" model, so it lands after Phase G persistence is in place (subagent sessions are real DB rows with `parent_id`).

## Scope

### In scope

- New `src/tools/task.rs` — `Tool` impl that:
  1. Validates the requested `subagent_type`.
  2. Creates a new `Session` row with `parent_id = parent_session_id`.
  3. Derives permissions: parent agent's permission ruleset minus `task` itself (no recursive spawn) and minus `todowrite` (model can't manage parent's todos).
  4. Runs `agent::loop::run_turn(...)` in a `tokio::spawn`'d future against the new session.
  5. Awaits the child (or returns immediately if `background: true`).
  6. Returns the last assistant text as the tool result.
- `task_status` polling tool when `background: true` (gated behind `experimental_background_subagents` config).
- Session picker UI shows parent → child relationship (tree view).
- TUI shows running subagent count in statusline (e.g. `ctx 12% · build · 1 task`).

### Out of scope

- Cross-machine subagents.
- Full graph of subagents (limit to depth 1 for v1; tree depth 2+ deferred).
- MCP-bridged subagents.

## Acceptance criteria

- "Use a subagent to research how AGENTS.md walks work in codex" → spawns a child session with explore-style permissions, runs to completion, returns a summary.
- Child session has its own row in `sessions` with `parent_id` set.
- Child session permissions are restricted (e.g. `apply_patch` denied for `explore`-type subagents).
- Subagent cannot recursively spawn another subagent unless `experimental_recursive_tasks = true`.
- `cargo test` passes; integration test that exercises the spawn → return-summary path.

## Files touched

| File | Change |
|---|---|
| `src/tools/task.rs` (new) | Task tool impl |
| `src/agent/subagent.rs` (new) | Derived permission helpers |
| `src/agent/primary.rs` | Add subagent variants: `Explore`, `General` (or use a separate `SubAgent` enum) |
| `src/session/persistence.rs` | Already supports `parent_id`; ensure `list_recent` shows tree |
| `src/ui/popups.rs` | Tree view in session picker |
| `src/config/schema.rs` | `[experimental] background_subagents = false`, `recursive_tasks = false` |
| Tests | Spawn → complete → result; recursive-spawn rejected |

## Tool spec

```rust
ToolSpec {
    name: "task",
    description: "Spawn a subagent to handle a focused task in an isolated context. Returns the subagent's summary.",
    parameters: {
        description: string (3-5 word summary, required),
        prompt: string (the task, required),
        subagent_type: string (e.g. "explore", "general"; required),
        background: bool (default false; if true, returns immediately and the model polls task_status)
    }
}
```

## Subagent types

| Type | Permissions | Use case |
|---|---|---|
| `explore` | read/glob/search only | Research, navigation |
| `general` | full minus `task` and `todowrite` | Multi-step focused work |
| `scout` (deferred) | read + webfetch | External docs |

## Risks

- **Spawn loop**: subagent calling `task` recursively could exhaust context. Default `recursive_tasks = false`; depth check in `subagent.rs`.
- **Permission inheritance bug**: child must NOT inherit "always" approvals from parent. Phase G's `auth_decisions` table is keyed by session_id, so this is automatic — verify with a test.
- **Cancellation propagation**: cancelling parent must cancel children. `CancellationToken::child_token()` handles it; verify in tests.
- **Storage growth**: every subagent creates a session row + messages. Add `/sessions tree --depth 2` command for inspection later.

## References

- Spec: `docs/spec/harness-architecture.md` §6 phase K
- opencode `tool/task.ts`: `/tmp/opencode/packages/opencode/src/tool/task.ts`
- codex multi-agent v1/v2: `codex-rs/core/src/tools/handlers/multi_agents*`
