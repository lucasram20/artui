# Phase I — Build / Plan agent modes

**Status:** DONE (2026-05-21)
**Summary:** Wired `PrimaryAgent` into `PermissionEngine` — Plan mode denies write tools (apply_patch, shell), Build mode allows them. Permission engine now takes agent as constructor param with `set_agent` for runtime switching. 86 tests pass.

**Phase:** I
**Spec:** `docs/spec/harness-architecture.md` §5.4, §6; `artui_v1_agentic_spec.md` §11
**Depends:** A, B, C, D, E
**Estimated PR size:** ~300 LoC

---

## Why

`PrimaryAgent::{Build, Plan}` already exists as an enum in `src/agent/mod.rs` with a `system_prompt()` method, but is never invoked. Per `graphify-out/GRAPH_REPORT.md` C24/C38, it's dead code. This phase wires it in: Tab cycles agent, agent overlays the system prompt, and overlays the permission policy.

## Scope

### In scope

- Tab key cycles `PrimaryAgent::{Build, Plan}` (currently Tab cycles `reasoning_effort` per CHANGELOG; move that to Shift+Tab where it already lives, or pick another binding for agents).
- `agent::prompts::build_system_prompt(...)` injects `agent.system_prompt()` after the env block.
- `permissions::policy::for_agent(agent)` overlays per-agent rules:
  - `Build`: defaults from config.
  - `Plan`: forces `apply_patch = "deny"`, `shell = "deny"`, all writes denied.
- TUI shows current agent in statusline (e.g. `ctx 5% · GitHub Copilot · gpt-5 · plan`).
- `/agent <name>` slash command for explicit selection.

### Out of scope

- Custom user-defined agents (config-driven).
- Subagents (Phase K).
- `compaction`/`title`/`summary` hidden agents (lands as part of Phase H/G).

## Acceptance criteria

- Tab → statusline updates `· build` ↔ `· plan`.
- In `plan` mode, the model attempting `apply_patch` gets a `denied` tool result.
- In `plan` mode, the system prompt includes the Plan agent's specific instructions ("you are in plan mode; do not edit files").
- Agent persists with the session (Phase G column `sessions.agent_id`).
- `cargo test` passes.

## Files touched

| File | Change |
|---|---|
| `src/agent/primary.rs` (move from `mod.rs`) | `PrimaryAgent` enum + `system_prompt()` + `description()` |
| `src/agent/mod.rs` | Re-exports |
| `src/agent/prompts.rs` | Inject `agent.system_prompt()` |
| `src/permissions/policy.rs` | `for_agent(agent: PrimaryAgent) -> PermissionPolicy` overlays |
| `src/app.rs` | Tab handler; `cycle_agent`; statusline update |
| `src/ui/layout.rs` | Statusline shows agent |
| `src/session/mod.rs` | `Session.agent_id` |
| Phase G schema already has `agent_id` column |
| Tests | Permission table tests with each agent overlay; transcript test that Plan refuses apply_patch |

## Plan agent system prompt addendum

```
You are in PLAN mode. Your role is to:
- Read code and understand the task.
- Propose a plan as a numbered list.
- Identify risks and alternatives.

You MUST NOT modify files, run shell commands, or apply patches.
If asked to make a change, return a plan and ask the user to switch to Build mode.

Available tools: read_file, glob, search, git_status.
Denied tools: apply_patch, shell.
```

## Risks

- **Tab binding conflict**: CHANGELOG 2026-05-14 says Shift+Tab cycles `reasoning_effort`. Confirm Tab is free (not currently bound to focus cycling per spec §13 keybinding table). If bound, pick a different key (e.g. `Ctrl+G`).
- **Permission overlay precedence**: agent overlay must take priority over user config. Test that `[permissions] shell = "allow"` in config is overridden by Plan agent's `shell = "deny"`.
- **Mid-turn agent switch**: forbid switching agent during an active turn. UI greys out Tab while `AgentRunner` is busy.

## References

- Spec: `docs/spec/artui_v1_agentic_spec.md` §11
- opencode `agent/agent.ts:120-280` (Build/Plan/General/Explore)
- Existing code: `src/agent/mod.rs::PrimaryAgent`
