# Todos

Per-phase implementation tickets for the artui harness rebuild. Each todo is a small, mergeable unit of work that maps to one phase in `docs/spec/harness-architecture.md` §6.

## Reading order

Before starting: read `docs/spec/harness-architecture.md` end-to-end. The first PR shape is in §7 of that doc.

## Phase tickets

| Order | Phase | Ticket | Outcome |
|---|---|---|---|
| 1 | A | [phase-a-tool-events.md](phase-a-tool-events.md) | `ModelEvent` carries tool calls; `ToolSpec` defined |
| 2 | A.5 | [copilot-oauth-zero-config.md](copilot-oauth-zero-config.md) | `/login copilot` works with zero config |
| 3 | B | [phase-b-tool-registry.md](phase-b-tool-registry.md) | `Tool` trait + registry + first `read_file` tool |
| 4 | C | [phase-c-session-and-loop.md](phase-c-session-and-loop.md) | Real agent loop replaces single-shot chat |
| 5 | D | [phase-d-glob-search.md](phase-d-glob-search.md) | `glob` + `search` tools; permission scaffold |
| 6 | E | [phase-e-apply-patch.md](phase-e-apply-patch.md) | V4A patch tool + diff preview + Ask flow |
| 7 | F | [phase-f-shell-tool.md](phase-f-shell-tool.md) | `shell` tool, classifier, output caps |
| 8 | G | [phase-g-sqlite-persistence.md](phase-g-sqlite-persistence.md) | SQLite session DB; resume + memory |
| 9 | H | [phase-h-compaction.md](phase-h-compaction.md) | Token budget + compaction sub-prompt |
| 10 | I | [phase-i-build-plan-modes.md](phase-i-build-plan-modes.md) | Build/Plan agent switch alters tools + prompt |
| 11 | J | [phase-j-bwrap-sandbox.md](phase-j-bwrap-sandbox.md) | Linux bubblewrap sandbox |
| 12 | K | [phase-k-task-subagent.md](phase-k-task-subagent.md) | `task` tool spawns child Session |

## Backlog / parking lot

- [oauth-provider-support.md](oauth-provider-support.md) — original provider roadmap covering Copilot/OpenAI account/MCP. Phase 3 (Copilot) is partially done; remaining items deferred until harness phases A–G land.

## Conventions

- One PR per phase ticket where possible.
- Each ticket has: scope, acceptance criteria, files touched, test plan, risks.
- Phase A–G must land in order. H–L can interleave.
- Tickets are working docs: update them as the implementation evolves.
