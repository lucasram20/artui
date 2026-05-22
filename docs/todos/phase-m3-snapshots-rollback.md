# Phase M3 — Workspace Snapshots & Rollback

**Phase:** M3 (production polish, safety net)
**Spec:** `docs/spec/harness-architecture.md` §6 (snapshots — deferred)
**Depends:** E (apply_patch), G (session store)
**Estimated PR size:** ~700 LoC

---

## Why

Today artui has per-patch rollback metadata (`.artui/session/<id>/
patches/<patch-id>.before.json`), but no workspace-level snapshot.
If the agent runs `shell rm -rf …` or chains 5 patches before
verifying, recovery means manually undoing each step. Claude Code and
opencode both ship `git stash`-style snapshots so a single command
rewinds to a known-good state.

## Scope

### In scope

- `Snapshot::take(workspace) -> SnapshotId` — captures via
  `git stash create` when the workspace is a git repo, falls back to
  a tarball into `.artui/session/<id>/snapshots/<sid>.tar.zst` when
  it's not.
- `Snapshot::restore(SnapshotId)` — `git stash apply` or tar extraction.
- Auto-snapshot before:
  1. `apply_patch` if `[snapshots] auto_pre_patch = true`.
  2. `shell` for non-read-only commands (classifier already labels them).
  3. Agent turn start when `[snapshots] auto_per_turn = true`.
- `/snapshot list` / `/snapshot restore <id>` / `/snapshot clear`
  slash commands.
- Snapshot retention: keep last N (default 20), auto-prune older.
- TUI hint after destructive ops: `Snapshot saved: snap_01HV…`
  with copy-able id.

### Out of scope

- Cross-machine snapshot sync.
- Snapshot diff browser (defer; for now `git stash show` works).
- Encryption at rest.

## Acceptance criteria

- `apply_patch` to README → `Snapshot saved` toast → manual edit →
  `/snapshot restore snap_01HV…` rewinds README.
- Non-git workspace falls back to tar; same restore command works.
- Snapshots persist across artui restarts (stored under
  `~/.local/share/artui/snapshots/<workspace_hash>/`).
- `cargo test` passes; integration test for git + non-git path.

## Files touched

| File | Change |
|---|---|
| `src/snapshots/mod.rs` (new) | Snapshot type, take/restore |
| `src/snapshots/git_backend.rs` (new) | git stash backend |
| `src/snapshots/tar_backend.rs` (new) | tar.zst fallback |
| `src/agent/loop.rs` | Auto-snapshot before risky tools |
| `src/app.rs` | `/snapshot` slash command |
| `src/config/schema.rs` | `[snapshots]` block |
| `src/session/store.rs` | snapshots table |
| Tests | Round-trip restore, retention, fallback |

## Risks

- **Git dirty state**: if user has staged changes, `git stash create`
  bundles them. Document this — `restore` only ever rewinds artui's
  own snapshots, never the user's manual stashes.
- **Tar size**: large repos (Cargo target/, node_modules) blow disk.
  Snapshot must respect `.gitignore`-style exclude list.
- **Subagents**: a child agent's snapshots should be tagged with
  parent_id so `clear` cascades.

## References

- opencode session snapshot doc
- claude-code "snapshot before risky op" pattern
- artui spec §6 (deferred snapshot system)
