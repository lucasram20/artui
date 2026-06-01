# Phase H — Compaction (token budget)

**Status:** DONE (2026-05-21)
**Summary:** Implemented `agent::compaction` module with `estimate_tokens` (chars/4), `needs_compaction` (0.835 threshold), and `compact_messages` (summarizes oldest 60% via provider). Compaction sub-prompt preserves decisions, file paths, task state. 86 tests pass.

**Phase:** H
**Spec:** `docs/spec/harness-architecture.md` §6, §8 (risks); `session-persistence.md` §9
**Depends:** A, B, C, G
**Estimated PR size:** ~400 LoC

---

## Why

Long sessions blow the context window. claude-code reserves a ~16.5% autocompact buffer; codex calls `run_auto_compact` mid-turn; opencode runs a hidden `compaction` agent. artui currently has no token budget at all — it just streams forever and the request eventually 4xxs.

## Scope

### In scope

- Token estimation: char-based (`chars / 4`) until a real tokenizer dep is justified.
- Per-model context window registry (`known_context_window_tokens` already exists; extend to all models).
- Trigger: when `tokens_used >= 0.835 * context_window`, run compaction before next provider call.
- Compaction sub-prompt: hidden, uses the active provider with a dedicated `compaction.md` system prompt.
- Replace oldest N messages (combined ≈ 60% of `tokens_used`) with a single `role=system` summary message.
- Write `compacted_at` timestamp on the originals (Phase G schema already supports this).
- TUI surface: `[N messages compacted]` marker in transcript when scrolled past compacted ranges.

### Out of scope

- Real tokenizer (tiktoken-rs, anthropic-tokenizer-rs) — defer until measurements show char-based is wrong by >20% for relevant workloads.
- Manual `/compact` slash command — easy to add but not required.
- Per-provider compaction prompts — start with one prompt and tune.

## Acceptance criteria

- 100-turn synthetic session that would normally exceed 200k context window completes without 4xx.
- Compaction runs at ~83.5% utilization; `messages` table shows `compacted_at` set on the summarized rows; one new `system` message added.
- Replay rebuilds correctly: compacted-range marker shown; unread-historical messages still queryable from DB.
- `cargo test` passes.

## Files touched

| File | Change |
|---|---|
| `src/agent/compaction.rs` (new) | `run_compaction(session, store, provider, cfg) -> Result<()>` |
| `src/agent/prompts/compaction.md` (new) | Hidden compaction system prompt |
| `src/agent/loop.rs` | Pre-flight check before each provider call: `if estimate(session) > 0.835 * context_window { run_compaction(...).await?; }` |
| `src/session/mod.rs` | `Session::estimate_tokens(&self) -> u32` (chars/4) |
| `src/config/schema.rs` | `[agent] compaction_threshold = 0.835` |
| `src/ui/chat.rs` | Render `[N messages compacted]` markers |
| Tests | Property: round-trip compaction preserves task spec; integration: forced overflow triggers compaction |

## Compaction sub-prompt sketch

```
You are summarizing a coding-agent session so the rest can continue past the context window.

Preserve VERBATIM:
- The user's original task / goal (highest priority).
- File paths modified and current state of each.
- Decisions made and why.
- Open items / what's next.
- Constraints stated by the user.

Drop:
- Exact variable names from early turns.
- Full tool outputs already superseded.
- Reasoning chains that did not change the outcome.

Output a single user message styled as a status report. Maximum 1500 tokens.
```

## Risks

- **Token estimation drift**: chars/4 is OK for English code, undercounts CJK, mis-estimates dense JSON. Document the approximation. Add a `--tokenizer=tiktoken` build flag later.
- **Compaction prompt quality**: if the summary drops critical state, the agent regresses post-compaction. Pin the *original task* in the system prompt (not history) so it survives compaction. claude-code does this.
- **Recursive compaction**: if the session is *already* compacted and overflows again, we need to compact-of-compaction. Schema supports it; just iterate.
- **Provider call cost**: compaction call uses the same model + tokens. Could be cheaper to use a fast/cheap model for compaction; defer optimization.

## References

- Spec: `docs/spec/harness-architecture.md` §6 phase H, §8
- codex `run_auto_compact`: `codex-rs/core/src/session/turn.rs:auto_compact_token_status`
- codex compaction prompt: `codex-rs/core/templates/compact/prompt.md`
- opencode `MessageV2.filterCompactedEffect`: `packages/opencode/src/session/prompt.ts`
