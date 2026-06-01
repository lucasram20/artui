# M3 — Workspace Snapshots & Rollback (design)

**Status:** approved design, pending implementation plan
**Issue:** [#19](https://github.com/lucasram20/artui/issues/19) — Phase M3
**Date:** 2026-06-01
**Depends:** E (apply_patch) — present and wired. (Issue lists G/session store; see "Corrections" below — that dependency is dropped.)

---

## Why

When the agent runs a destructive `shell` command or chains several
`apply_patch` calls before the user verifies, recovery today is manual and
per-step. There is no workspace-level "rewind to a known-good state". Claude
Code and opencode both ship `git stash`-style snapshots so a single command
rewinds. M3 adds that safety net.

## Corrections to the issue's premises

The issue's "Why" and "Scope" rest on three assumptions that do **not** hold in
`main` as of this writing. The design below is written against the real code.

1. **No persisted per-patch rollback.** `src/tools/apply_patch.rs` performs only
   *in-memory, intra-call* rollback: `rollback_change` reverts already-applied
   hunks if a *later* hunk in the **same** `apply_patch` call fails
   (`apply_patch.rs:166-178`). Nothing is written to
   `.artui/session/<id>/patches/<id>.before.json` — that path does not exist.
   This widens the gap M3 fills rather than narrowing it.

2. **The shell classifier is deny-only.** `src/tools/shell.rs` exposes
   `classify_deny` (`shell.rs:234`), a binary deny/allow gate. There is **no**
   read-only classifier. M3 adds one (`shell::is_read_only`).

3. **The session store is not wired into the runtime.** `SessionStore`
   (`src/session/store.rs`) is defined and unit-tested but never constructed at
   runtime — there is no `open_default` / `create_session` / `append_message`
   call in `src/app.rs` or `src/lib.rs`, and no "current session id" flows
   through `AppRequest` → `run_turn` → `ToolContext`. `ToolContext`
   (`src/tools/mod.rs:25`) carries `workspace_root`/`cwd` only.

**Consequence / decision (user-approved):** snapshots are **workspace-keyed**,
not session-keyed. This matches the issue's own acceptance criterion ("stored
under `~/.local/share/artui/snapshots/<workspace_hash>/`") and avoids dragging a
full Phase-G integration into M3. The conflicting `.artui/session/<id>/...`
paths from the issue are dropped. The subagent `parent_id` cascade is deferred
(no live session/subagent id exists to key it on).

---

## Architecture

```
src/snapshots/
  mod.rs          SnapshotManager, SnapshotId, SnapshotMeta, Backend enum,
                  index load/save, retention/prune, public API
  git_backend.rs  git working-tree snapshot via write-tree (incl. untracked)
  tar_backend.rs  tar.zst fallback for non-git workspaces
```

`SnapshotManager` is the single entry point. It is constructed once per
workspace and handed to the agent loop (for auto-snapshots) and to `App` (for
the `/snapshot` slash command).

### Public API (`src/snapshots/mod.rs`)

```rust
pub struct SnapshotId(String);              // "snap_<ULID>"

pub enum Backend { Git, Tar }

pub enum Reason { PrePatch, PreShell, PerTurn, Manual }

pub struct SnapshotMeta {
    pub id: SnapshotId,
    pub created_at: String,                 // ISO-8601, reuse session::store::now_iso style
    pub backend: Backend,
    pub reason: Reason,
    pub git_tree: Option<String>,           // tree sha (git backend)
    pub git_head: Option<String>,           // HEAD sha at capture (git backend)
    pub tar_path: Option<PathBuf>,          // <root>/<id>.tar.zst (tar backend)
    pub command: Option<String>,            // shell command that triggered it, if any
}

pub struct SnapshotManager { /* workspace_root, snapshot_root, retain, … */ }

impl SnapshotManager {
    /// Resolve storage dir, load index. Detects git vs tar backend.
    pub fn for_workspace(workspace_root: &Path, cfg: &SnapshotsConfig) -> Result<Self>;

    pub fn take(&self, reason: Reason, command: Option<String>) -> Result<SnapshotId>;
    pub fn restore(&self, id: &SnapshotId) -> Result<()>;
    pub fn list(&self) -> Vec<SnapshotMeta>;            // newest first
    pub fn clear(&self) -> Result<()>;                  // wipe index + tar files
    fn prune(&self) -> Result<()>;                      // keep newest `retain`
}
```

`take` appends to the index then calls `prune`. `prune` deletes the
oldest-beyond-`retain` entries and removes their `.tar.zst` files (git trees are
left to normal git gc — they are unreferenced loose objects).

### Storage layout

- Root: `~/.local/share/artui/snapshots/<workspace_hash>/`
  via `directories::ProjectDirs::from("", "", "artui").data_local_dir()`.
- `workspace_hash = hex(sha256(canonicalized_workspace_path))[..16]` (`sha2`).
- `index.json` — `Vec<SnapshotMeta>` serialized with `serde_json`.
- `<id>.tar.zst` — one per tar-backed snapshot.
- Index dir created with `0o700` on unix (mirrors `store.rs` perm handling).

### Git backend (`git_backend.rs`)

Detection: `git rev-parse --is-inside-work-tree` exits 0.

**Why not `git stash create`:** it captures tracked modifications only —
**untracked files are excluded**. The agent's most common mutation is *creating
new files* via `apply_patch`; those are untracked until the user stages them.
`git stash create` would silently fail to capture them, so restore would not
remove an agent-created file. We use a temp-index `write-tree` instead.

**take:**
```
TMP=$(mktemp)
GIT_INDEX_FILE=$TMP git --git-dir=<ws>/.git --work-tree=<ws> add -A     # stages tracked + untracked, no touch to real index
TREE=$(GIT_INDEX_FILE=$TMP git write-tree)
HEAD=$(git rev-parse HEAD)        # may fail in a repo with no commits → store None
rm $TMP
# record { git_tree: TREE, git_head: HEAD }
```
This never modifies the user's real index, working tree, or stash list.

**restore (to snapshot tree `TREE`):**
```
TMP=$(mktemp)
GIT_INDEX_FILE=$TMP git read-tree <TREE>
GIT_INDEX_FILE=$TMP git checkout-index -a -f                 # writes snapshot's files into the worktree
# remove files that exist now but are absent from the snapshot tree:
for f in $(GIT_INDEX_FILE=$TMP git diff --name-only --diff-filter=D <TREE> -- .): delete f
rm $TMP
```
Implementation detail resolved during planning: the simplest correct deletion
pass is to diff the snapshot tree against the *current* working tree and remove
additions. The plan will pin the exact `git diff` invocation and cover the
empty-repo (no HEAD) case. Restore only ever rewinds artui's own snapshot tree;
the user's stashes and branches are untouched.

### Tar backend (`tar_backend.rs`)

For non-git workspaces. Adds crates `tar = "0.4"` and `zstd = "0.13"`.

**take:** walk the workspace with the `ignore` crate (already a dependency —
respects `.gitignore`/`.ignore`) plus a built-in exclude set
(`.git`, `target`, `node_modules`, the snapshot root itself). Stream entries
into a `tar::Builder` wrapped in a `zstd::Encoder` writing
`<root>/<id>.tar.zst`. If the uncompressed walk exceeds
`cfg.max_tar_mb`, skip the snapshot and surface a warning (return a
distinguished `Ok(None)`-style signal so callers can show a hint rather than
error the turn). The exact size-guard mechanics are pinned in the plan.

**restore:** delete the current non-excluded file set, then extract the archive
back into the workspace.

---

## Auto-snapshot integration (dispatch layer)

Auto-snapshots are taken in `src/agent/loop.rs::run_turn`, **not** inside the
tools — tools have no `SnapshotManager` handle and no session context, whereas
the loop owns `AgentLoopConfig`.

`AgentLoopConfig` gains:
```rust
pub snapshots: Option<std::sync::Arc<crate::snapshots::SnapshotManager>>,
pub snapshot_policy: SnapshotPolicy,   // { auto_pre_patch, auto_pre_shell, auto_per_turn }
```
Default `None` / all-false so existing tests and the no-config path are
unaffected.

In `run_turn`:
- **Per-turn:** if `auto_per_turn`, call `take(PerTurn, None)` once before the
  first step.
- **Before dispatch**, inspect the pending tool call:
  - name `apply_patch` and `auto_pre_patch` → `take(PrePatch, None)`.
  - name `shell` and `auto_pre_shell` and `!shell::is_read_only(cmd)` →
    `take(PreShell, Some(cmd))`.
- On a successful `take`, emit `AppEvent::SnapshotSaved { id }`.

A failed snapshot **must not** abort the turn — log via `tracing::warn!` and
continue (a snapshot is a safety net, not a gate).

### `shell::is_read_only` (new, in `src/tools/shell.rs`)

Leading-token allowlist; conservative (unknown ⇒ treated as mutating ⇒
snapshot). Initial set: `ls pwd cat head tail less more file stat wc grep rg
egrep fgrep find fd echo which type printenv env date whoami id du df tree`,
plus `git` only when the subcommand is read-only
(`status log diff show blame branch remote rev-parse ls-files describe`).
Pipelines / `&&` / `;` ⇒ treat as mutating (snapshot). Lives beside
`classify_deny` with its own unit tests.

---

## Config — `[snapshots]` (`src/config/schema.rs`)

```rust
#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct SnapshotsConfig {
    pub enabled: bool,           // master switch (default true)
    pub auto_pre_patch: bool,    // default true
    pub auto_pre_shell: bool,    // default true
    pub auto_per_turn: bool,     // default false
    pub retain: usize,           // keep newest N (default 20)
    pub max_tar_mb: u64,         // skip tar snapshot above this (default 512)
}
```
Added as `snapshots: SnapshotsConfig` on `AppConfig` with a `Default`. When
`enabled = false`, the manager is never constructed and the loop's
`snapshots` handle stays `None`.

---

## Slash command — `/snapshot` (`src/app.rs`)

Handled in `run_slash_command` (the existing `SlashCommandResult` pattern,
`app.rs:1674`), registered in the slash-command table (`app.rs:2326`).

- `/snapshot` or `/snapshot list` → render newest-first table:
  `snap_…  2m ago  pre_patch  git` (id, relative age, reason, backend).
- `/snapshot restore <id>` → `manager.restore(id)`; on success print
  `Rewound to snap_… (<reason>, <created_at>)`; on unknown id, an error line.
- `/snapshot clear` → `manager.clear()`; print count removed.

`App` holds an `Option<Arc<SnapshotManager>>` constructed at startup from
`SnapshotsConfig`; when `None` (disabled) the command prints
`snapshots are disabled ([snapshots] enabled = false)`.

## TUI hint

New `AppEvent::SnapshotSaved { id: String }`. The chat view renders a dim,
copy-able status line `Snapshot saved: snap_01HV…` (consistent with existing
status-line rendering). No new popup.

---

## Testing

Unit + integration, all under `cargo test` (no new external binaries beyond
`git`, which CI already has):

1. **git round-trip** — init repo, `apply_patch`-create `README`, `take`,
   manually edit + add an untracked file, `restore` → README content rewinds
   **and** the untracked file is removed.
2. **git restore of a tracked edit** — modify a committed file, take, change
   again, restore → file matches the snapshot.
3. **tar round-trip** — non-git temp dir, same create/mutate/restore flow.
4. **retention** — `retain = 20`; take 25 → 20 kept (newest), 5 oldest pruned,
   their `.tar.zst` files deleted from disk.
5. **read-only allowlist** — `is_read_only("ls -la")`,
   `is_read_only("git status")` true; `is_read_only("rm foo")`,
   `is_read_only("git commit")`, `is_read_only("ls && rm x")` false.
6. **workspace_hash stability + index persistence** — same path ⇒ same hash;
   re-opening a `SnapshotManager` loads the prior `index.json`.
7. **max_tar_mb guard** — workspace over the limit yields the skip signal, no
   archive written, turn not errored.

---

## Crates added

- `tar = "0.4"`
- `zstd = "0.13"`

(Git backend shells out to `git`; no crate needed. `ignore`, `sha2`,
`directories`, `serde_json`, `ulid` are already dependencies.)

## Files touched

| File | Change |
|---|---|
| `src/snapshots/mod.rs` (new) | `SnapshotManager`, types, index, retention, public API |
| `src/snapshots/git_backend.rs` (new) | temp-index `write-tree` capture + restore |
| `src/snapshots/tar_backend.rs` (new) | `ignore`-walked `tar.zst` capture + restore |
| `src/lib.rs` | register `snapshots` module |
| `src/agent/loop.rs` | `AgentLoopConfig` fields + auto-snapshot before risky dispatch + per-turn |
| `src/tools/shell.rs` | `is_read_only` + tests |
| `src/app.rs` | `/snapshot` slash command, `App` manager handle, `SnapshotSaved` event |
| `src/config/schema.rs` | `[snapshots]` block on `AppConfig` |
| `Cargo.toml` | `tar`, `zstd` deps |
| Tests | round-trip (git+tar), retention, read-only, hash/persistence, size guard |

## Out of scope (deferred)

- Subagent `parent_id` cascade for `clear` (no live session/subagent id — revisit
  when Phase G is wired into the runtime).
- Cross-machine snapshot sync.
- Snapshot diff browser (`git stash show` / manual diff suffices for now).
- Encryption at rest.

## Risks

- **Git dirty state.** `write-tree` from a temp index bundles whatever is in the
  working tree at capture time, including the user's own staged changes. Restore
  rewinds the working tree to that captured tree — it never touches the user's
  stash list or branches, but it *will* move tracked files. The `/snapshot
  restore` output names the snapshot so the action is explicit.
- **Tar size.** Large build artifacts blow disk; mitigated by `ignore`-based
  excludes + `max_tar_mb` guard.
- **Empty repo (no HEAD).** `git rev-parse HEAD` fails before the first commit;
  `git_head` is stored as `None` and restore relies on the tree alone.
