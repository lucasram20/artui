# Phase M — Production Polish Gaps

Roadmap to close the gap between artui's harness shape and the daily-driver
quality of Claude Code / Codex / OpenCode / pi.

Current state (v0.3.x): architecture parity, alpha quality. Goal of this
phase set: feature parity + production polish.

## Reading order

Each ticket maps to one phase. Phases M1–M3 are quick wins (visible UX gaps).
M4–M7 are larger structural work. M8 is documentation and release polish.

| Order | Phase | Ticket | Outcome | Size |
|---|---|---|---|---|
| 1 | M1 | [phase-m1-diff-preview-popup.md](phase-m1-diff-preview-popup.md) | Pre-apply diff preview popup; user sees changes before they hit disk | ~600 LoC |
| 2 | M2 | [phase-m2-permission-ask-ui.md](phase-m2-permission-ask-ui.md) | Mid-tool Ask modal; permission engine is wired but UI auto-allows today | ~500 LoC |
| 3 | M3 | [phase-m3-snapshots-rollback.md](phase-m3-snapshots-rollback.md) | Workspace-level snapshot before agent run; one-command rollback | ~700 LoC |
| 4 | M4 | [phase-m4-macos-seatbelt-sandbox.md](phase-m4-macos-seatbelt-sandbox.md) | macOS sandbox-exec parity with Linux bwrap | ~500 LoC |
| 5 | M5 | [phase-m5-windows-restricted-token.md](phase-m5-windows-restricted-token.md) | Windows job-object + restricted-token sandbox | ~700 LoC |
| 6 | M6 | [phase-m6-codebase-indexer.md](phase-m6-codebase-indexer.md) | Tree-sitter or BM25 index; search tool gains semantic mode | ~1500 LoC |
| 7 | M7 | [phase-m7-deep-subagents.md](phase-m7-deep-subagents.md) | Depth-N subagent trees; per-branch context budgets | ~600 LoC |
| 8 | M8 | [phase-m8-production-polish.md](phase-m8-production-polish.md) | Telemetry opt-in, crash reporter, docs site, accessibility | ~800 LoC |

## Conventions

- Phases M1–M3 ship under v0.4.x (visible UX bumps).
- Phases M4–M5 ship under v0.5.x (sandbox completeness).
- Phases M6–M7 ship under v0.6.x (deeper agent capabilities).
- Phase M8 ships under v0.9.x → v1.0.0 release.
- Every phase ticket follows the same shape as Phases A–L: scope,
  acceptance criteria, files touched, test plan, risks.
- Out-of-scope items end up in [parking-lot.md](parking-lot.md).
