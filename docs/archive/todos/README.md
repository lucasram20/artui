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
- [parking-lot.md](parking-lot.md) — items punted out of Phase M (cross-machine sync, vector embeddings, GUI port, etc).

## Phase M — Production polish (post-v0.3, road to v1.0)

artui at v0.3.x has architecture parity with Claude Code / Codex / OpenCode / pi but lacks production polish. Phase M closes that gap. See [phase-m-README.md](phase-m-README.md) for ordering and milestone targets.

| Order | Phase | Ticket | Outcome |
|---|---|---|---|
| 1 | M1 | [phase-m1-diff-preview-popup.md](phase-m1-diff-preview-popup.md) | Pre-apply diff preview popup |
| 2 | M2 | [phase-m2-permission-ask-ui.md](phase-m2-permission-ask-ui.md) | Mid-tool Ask modal (permission engine wired) |
| 3 | M3 | [phase-m3-snapshots-rollback.md](phase-m3-snapshots-rollback.md) | Workspace snapshots + rollback |
| 4 | M4 | [phase-m4-macos-seatbelt-sandbox.md](phase-m4-macos-seatbelt-sandbox.md) | macOS Seatbelt parity with bwrap |
| 5 | M5 | [phase-m5-windows-restricted-token.md](phase-m5-windows-restricted-token.md) | Windows Job Object + restricted token |
| 6 | M6 | [phase-m6-codebase-indexer.md](phase-m6-codebase-indexer.md) | Tree-sitter symbols + BM25 chunks |
| 7 | M7 | [phase-m7-deep-subagents.md](phase-m7-deep-subagents.md) | Depth-N subagent trees |
| 8 | M8 | [phase-m8-production-polish.md](phase-m8-production-polish.md) | Telemetry, crash reporter, a11y, mdBook docs |
| 9 | M9 | [phase-m9-agent-browser-web-tool.md](phase-m9-agent-browser-web-tool.md) | Bundle Vercel agent-browser as a `web_*` tool family |

## Phase N — Language Server Protocol support (v0.5.x → v0.7.x)

LSP integration so the agent can call into rust-analyzer / gopls / pyright /
typescript-language-server / clangd for definitions, references, hover,
diagnostics, rename, and code actions. Reference design: oh-my-pi
`packages/coding-agent/src/lsp/`. Full architecture in
[`docs/specs/lsp.md`](../specs/lsp.md). See
[phase-n-README.md](phase-n-README.md) for ordering.

| Order | Phase | Ticket | Outcome |
|---|---|---|---|
| 1 | N1 | [phase-n1-lsp-skeleton.md](phase-n1-lsp-skeleton.md) | `lsp/` module + `async-lsp` + `lsp` tool with definition/hover/status; vendored ~80-server registry from helix-editor |
| 2 | N2 | [phase-n2-lsp-readonly-ops.md](phase-n2-lsp-readonly-ops.md) | references, implementation, type_definition, document_symbols, workspace_symbols, cached diagnostics |
| 3 | N3 | [phase-n3-lsp-writethrough.md](phase-n3-lsp-writethrough.md) | Post-`apply_patch` `publishDiagnostics` pull; diagnostics rendered into the same tool result |
| 4 | N4 | [phase-n4-lsp-mutating-ops.md](phase-n4-lsp-mutating-ops.md) | rename + code_actions through approval engine; `workspace/applyEdit` handling |

## Conventions

- One PR per phase ticket where possible.
- Each ticket has: scope, acceptance criteria, files touched, test plan, risks.
- Phase A–G must land in order. H–L can interleave.
- Tickets are working docs: update them as the implementation evolves.
