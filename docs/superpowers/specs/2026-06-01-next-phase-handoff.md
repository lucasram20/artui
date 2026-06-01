# Next Phase Handoff — after Phase M3

**Date:** 2026-06-01  
**Repo:** `lucasram20/artui`  
**Branch pushed:** `main`  
**Last pushed commit before this handoff:** `88959a1 docs(lsp): mark render examples as text`  
**Completed phase:** M3 — Workspace Snapshots & Rollback  
**Closed issue:** [#19 Phase M3 — Workspace Snapshots & Rollback](https://github.com/lucasram20/artui/issues/19)  
**Project status:** `Done`

## Phase M3 shipped

Phase M3 is complete and pushed to `main`.

Delivered:

- Workspace-keyed snapshot storage under `~/.local/share/artui/snapshots/<workspace_hash>/`.
- `[snapshots]` config:
  - `enabled = true`
  - `auto_pre_patch = true`
  - `auto_pre_shell = true`
  - `auto_per_turn = false`
  - `retain = 20`
  - `max_tar_mb = 512`
- Git snapshot backend using a temporary `GIT_INDEX_FILE` + `git write-tree`, including untracked files without touching the user's real index/stash/branches.
- Tar fallback backend using compressed `tar.zst` archives for non-git workspaces.
- Conservative `shell::is_read_only` classifier for snapshot gating.
- Auto-snapshot integration in the agent loop before `apply_patch` and before non-read-only `shell` commands.
- `/snapshot`, `/snapshot list`, `/snapshot restore <id>`, and `/snapshot clear` slash commands.
- TUI status hint when auto-snapshots are saved.
- Snapshot initialization errors reported distinctly from explicit `[snapshots] enabled = false`.
- Snapshot integration tests for git restore, tar fallback, retention/pruning, and isolated snapshot storage.
- README and `docs/changelogs/CHANGELOG.md` updates.
- `docs/specs/` consolidated into `docs/spec/`.
- Stale LSP doctest examples fixed by fencing them as `text` blocks.

## Verification completed on `main`

Before pushing `main`, these passed:

```bash
cargo fmt --all -- --check
cargo test --quiet
cargo clippy --all-targets -- -D warnings
```

`cargo test --quiet` included:

- 258 lib tests
- 8 existing integration tests
- 3 snapshot integration tests
- doctests clean

## Graphify status

The updated `graphify-out/` output is being committed with this handoff so the next machine can reuse the current indexed graph nodes.

Expected graphify output files include:

- `graphify-out/graph.json`
- `graphify-out/graph.html`
- `graphify-out/manifest.json`
- `graphify-out/GRAPH_REPORT.md`
- `graphify-out/cache/stat-index.json`
- `graphify-out/.graphify_labels.json`
- `graphify-out/.graphify_python`
- `graphify-out/.graphify_root`
- `graphify-out/.vocab.txt`
- `graphify-out/cost.json`

Note: `graphify-out/` is ignored by `.gitignore`, so these files were intentionally force-added for handoff portability.

## Suggested next phase

Next open roadmap phase is:

- [#39 Phase M4 — macOS Seatbelt Sandbox](https://github.com/lucasram20/artui/issues/39)
  - Labels: `phase:m4`, `workstream:sandbox`, `priority:p2`, `size:s`

Following phases still open after that:

- [#40 Phase M5 — Windows Sandbox (Job Object + Restricted Token)](https://github.com/lucasram20/artui/issues/40)
- [#41 Phase M6 — Codebase Indexer](https://github.com/lucasram20/artui/issues/41)
- [#42 Phase M7 — Deep Subagent Trees](https://github.com/lucasram20/artui/issues/42)
- [#43 Phase M8 — Production Polish (1.0 Release)](https://github.com/lucasram20/artui/issues/43)
- [#44 Phase M9 — Web Browsing Tool (Vercel agent-browser)](https://github.com/lucasram20/artui/issues/44)

## Recommended next-session startup

1. Pull latest `main` on the main PC.
2. Confirm `graphify-out/` is present and current.
3. Start Phase M4 from issue #39.
4. Reuse the same workflow that worked for M3:
   - inspect issue and current code reality,
   - write/approve design if assumptions differ,
   - write implementation plan under `docs/superpowers/plans/`,
   - execute with subagent-driven development,
   - review gates after each task,
   - final verification + changelog update,
   - close issue and mark project item `Done`.

## Important context for M4 planning

M3 deliberately avoided changing the sandbox layer. Snapshot restore is now available as the safety net before mutating tools, but sandbox hardening is still future work.

Current sandbox-related next work should inspect:

- `src/sandbox/mod.rs`
- permission/tool dispatch paths in `src/agent/loop.rs`
- shell execution in `src/tools/shell.rs`
- any platform-specific gates in config and README

M4 should not assume Linux bubblewrap behavior maps directly to macOS. Treat macOS Seatbelt as a platform-specific backend and keep Linux behavior stable.
