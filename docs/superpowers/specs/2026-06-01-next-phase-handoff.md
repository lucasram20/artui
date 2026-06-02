# Next Phase Handoff — after Phase M9 / v0.7.0

**Date:** 2026-06-02  
**Repo:** `lucasram20/artui`  
**Branch:** `main`  
**Release:** [v0.7.0](https://github.com/lucasram20/artui/releases/tag/v0.7.0)  
**Completed phases:** M3–M9 (first slices)  
**Closed issues:** [#19](https://github.com/lucasram20/artui/issues/19) M3, [#39](https://github.com/lucasram20/artui/issues/39)–[#44](https://github.com/lucasram20/artui/issues/44) M4–M9, [#38](https://github.com/lucasram20/artui/issues/38) release cut

## Shipped on v0.7.0

- **M3:** Workspace snapshots (git + tar), auto-snapshot hooks, `/snapshot` commands.
- **M4:** Linux bubblewrap + macOS Seatbelt sandbox backends.
- **M5:** Windows Job Object shell isolation.
- **M6:** Workspace symbol/text index; `search` modes `symbol` / `semantic`.
- **M7:** `task` depth guard (max depth 2).
- **M8:** `messages.parent_call_id` schema + idempotent migration.
- **M9:** `web` tool (HTTP fetch; not full agent-browser stack).

See `docs/changelogs/CHANGELOG.md` § [0.7.0] for full notes.

## Verification

```bash
cargo fmt --all -- --check
cargo test --quiet
cargo clippy --all-targets -- -D warnings
```

## Follow-up (parking lot / xl-spec)

Open backlog items remain under `parking-lot` labels ([#24](https://github.com/lucasram20/artui/issues/24)–[#34](https://github.com/lucasram20/artui/issues/34)). Full M-phase xl parity (tree-sitter indexer, restricted Windows token, Vercel agent-browser, deep subagent tree UI) is tracked via new issues on the GitHub Project — not `docs/todos/`.

## Recommended next-session startup

1. Pull `main` and confirm `v0.7.0` release assets on R2/install scripts.
2. Triage parking-lot issues or file follow-ups for xl-spec gaps.
3. Re-run `graphify update .` after substantial code changes.