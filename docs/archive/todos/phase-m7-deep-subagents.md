# Phase M7 — Deep Subagent Trees

**Phase:** M7 (production polish, deeper agent capability)
**Spec:** Phase K extension (currently depth-1 only)
**Depends:** K (task tool), G (session store), H (compaction)
**Estimated PR size:** ~600 LoC

---

## Why

Phase K shipped the `task` subagent tool but capped it at depth 1.
Claude Code and Codex run depth-N subagent trees: a planner spawns
researchers, each researcher spawns small focused workers. Per-branch
context budgets prevent any single chain from blowing the parent
context.

## Scope

### In scope

- Remove depth-1 hard cap; default `[experimental] max_subagent_depth
  = 4` config.
- Per-branch token budget: each subagent gets `min(parent_budget *
  0.25, max_subagent_tokens)`.
- Per-branch compaction (re-uses Phase H module) so a long-running
  child doesn't OOM its slice.
- Tree visualization in `/sessions tree` and the chat UI (indent +
  tree glyphs).
- `task_status <id>` polling tool returns:
  - `"running" | "succeeded" | "failed"`
  - tail of latest assistant text
  - token usage so far
- Cycle detection: a session-id graph that rejects parent ↔ child
  recursion.

### Out of scope

- Cross-machine subagents (federated artui).
- Subagent orchestration via a DAG file (declarative pipelines).
- Streaming subagent output up to the parent in real time. Today the
  parent still waits for completion; streaming up-tree is a follow-up.

## Acceptance criteria

- `task subagent_type=general` from a depth-3 chain runs to
  completion; `/sessions tree` shows the 4-level hierarchy.
- Each subagent has its own context budget; parent unaffected by
  child overflow.
- Cancelling the root cancels the entire tree (CancellationToken
  child tokens).
- `cargo test` includes a depth-4 spawn integration test.

## Files touched

| File | Change |
|---|---|
| `src/agent/subagent.rs` | depth-N derived perms, cycle check |
| `src/tools/task.rs` | Drop depth cap, per-branch budgets |
| `src/agent/loop.rs` | Per-branch compaction call |
| `src/session/store.rs` | tree query helpers |
| `src/ui/popups.rs` | tree view in session picker |
| `src/config/schema.rs` | `max_subagent_depth`, `max_subagent_tokens` |
| Tests | Depth-N spawn, cancel cascade, cycle reject |

## Risks

- **Storage growth**: every subagent is a row plus messages. Add
  `/sessions archive --older-than 30d` to bound it.
- **Token blowout if budgets are wrong**: floor each child at 4k so a
  starved leaf can still emit a sentence.
- **Permission inheritance**: child must NOT inherit "always allow"
  decisions from the parent (auth_decisions table is keyed by
  session_id — verify with an integration test).

## References

- claude-code orchestrator pattern
- codex multi-agent v2 (`core/src/tools/handlers/multi_agents*`)
- Phase K original ticket
