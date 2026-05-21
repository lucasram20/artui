# Session Persistence — SQLite Design

**Status:** v1 design (2026-05-21)
**Companion:** `harness-architecture.md` §3, `artui_v1_agentic_spec.md` §15
**Replaces:** the in-memory-only `Vec<Message>` transcript currently held in `App`.

This document specifies how artui persists session state to SQLite so sessions can be resumed across crashes, restarts, and explicit suspends — matching the model used by claude-code, sst/opencode (Drizzle SQLite), and OpenAI codex-rs (JSONL + SQLite index).

---

## 1. Goals

| Goal | Why |
|---|---|
| Resume a session after `q` / Ctrl-C / power-loss | Continuity across runs; never lose work mid-task |
| List recent sessions with title + workspace + last activity | `/sessions` command + statusline picker |
| Replay full transcript from disk | Audit trail; debugging; share-replay |
| Per-workspace memory (CLAUDE.md-style) | Project-scoped context that survives between sessions |
| Privacy: data stays on user's machine | No telemetry, no cloud |
| Crash-safe: never half-write a row | WAL mode + atomic transactions |
| Lightweight: no daemon, no migration tooling cost | sqlx + a single file |

Non-goals for v1: cross-machine sync, encryption-at-rest, multi-user. Those land later.

---

## 2. File layout

```
$XDG_DATA_HOME/artui/                              (Linux: ~/.local/share/artui/)
├── artui.db                                       (global: sessions, messages, memory)
├── artui.db-wal                                   (WAL journal)
├── artui.db-shm                                   (shared mem)
└── workspaces/
    └── <workspace-hash>/
        └── snapshots/                             (deferred to phase J — Snapshot system)
```

Platform paths (via `directories` crate, already a dep per spec §6):

| OS | Path |
|---|---|
| Linux | `~/.local/share/artui/artui.db` (or `$XDG_DATA_HOME/artui/artui.db`) |
| macOS | `~/Library/Application Support/artui/artui.db` |
| Windows | `%LOCALAPPDATA%\artui\artui.db` |

Workspace hash = `blake3(canonical_workspace_root)[..16]` so the same repo from different mount points still resolves consistently.

File mode `0o600` on Unix.

---

## 3. Schema

```sql
-- Created on first run. Migrated by sqlx-managed `_migrations` table.

CREATE TABLE sessions (
    id              TEXT    PRIMARY KEY,             -- ulid (sortable)
    title           TEXT    NOT NULL DEFAULT '',     -- model-generated short title
    workspace_root  TEXT    NOT NULL,                -- canonical absolute path
    workspace_hash  TEXT    NOT NULL,                -- blake3(canonical)[..16]
    agent_id        TEXT    NOT NULL,                -- "build" | "plan" | future
    provider_id     TEXT    NOT NULL,                -- "ollama" | "copilot" | …
    model           TEXT    NOT NULL,
    git_branch      TEXT,
    git_commit      TEXT,
    created_at      INTEGER NOT NULL,                -- unix seconds
    updated_at      INTEGER NOT NULL,
    archived_at     INTEGER,                         -- soft-delete; NULL = active
    parent_id       TEXT REFERENCES sessions(id)     -- subagent / task tool
);

CREATE INDEX sessions_workspace_idx ON sessions(workspace_hash, updated_at DESC);
CREATE INDEX sessions_active_idx ON sessions(archived_at, updated_at DESC);

CREATE TABLE messages (
    id              TEXT    PRIMARY KEY,             -- ulid
    session_id      TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    role            TEXT    NOT NULL,                -- "user" | "assistant" | "tool" | "system"
    seq             INTEGER NOT NULL,                -- monotonic per session
    created_at      INTEGER NOT NULL,
    finish_reason   TEXT,                            -- "stop" | "tool_calls" | "length" | NULL
    input_tokens    INTEGER,
    output_tokens   INTEGER,
    compacted_at    INTEGER                          -- if this message was summarized away
);

CREATE INDEX messages_session_idx ON messages(session_id, seq);

CREATE TABLE message_parts (
    id              TEXT    PRIMARY KEY,             -- ulid
    message_id      TEXT    NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    part_seq        INTEGER NOT NULL,
    kind            TEXT    NOT NULL,                -- "text" | "tool_call" | "tool_result" | "image" | "reasoning"
    payload         TEXT    NOT NULL,                -- JSON; schema in src/session/parts.rs
    created_at      INTEGER NOT NULL
);

CREATE INDEX message_parts_message_idx ON message_parts(message_id, part_seq);

CREATE TABLE tool_calls (
    call_id         TEXT    PRIMARY KEY,             -- model-supplied id
    session_id      TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    message_id      TEXT    NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    tool_name       TEXT    NOT NULL,
    arguments       TEXT    NOT NULL,                -- JSON
    result          TEXT,                            -- model-facing text
    error           TEXT,
    artifact_path   TEXT,                            -- if output was capped
    permission_decision TEXT,                        -- "allow" | "ask_approved" | "ask_denied" | "deny"
    duration_ms     INTEGER,
    started_at      INTEGER NOT NULL,
    finished_at     INTEGER
);

CREATE INDEX tool_calls_session_idx ON tool_calls(session_id, started_at);

CREATE TABLE patches (
    id              TEXT    PRIMARY KEY,             -- ulid
    session_id      TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    call_id         TEXT REFERENCES tool_calls(call_id),
    workspace_root  TEXT    NOT NULL,
    diff_text       TEXT    NOT NULL,                -- unified diff
    files_changed   TEXT    NOT NULL,                -- JSON array of paths
    applied         INTEGER NOT NULL DEFAULT 0,      -- 0/1
    rolled_back_at  INTEGER,
    created_at      INTEGER NOT NULL
);

CREATE INDEX patches_session_idx ON patches(session_id, created_at);

CREATE TABLE memory (
    id              TEXT    PRIMARY KEY,
    workspace_hash  TEXT    NOT NULL,
    scope           TEXT    NOT NULL,                -- "project" | "user" | "session"
    session_id      TEXT REFERENCES sessions(id) ON DELETE CASCADE,
    key             TEXT    NOT NULL,
    value           TEXT    NOT NULL,
    created_at      INTEGER NOT NULL,
    updated_at      INTEGER NOT NULL,
    UNIQUE(workspace_hash, scope, session_id, key)
);

CREATE INDEX memory_workspace_idx ON memory(workspace_hash, scope);

CREATE TABLE auth_decisions (
    id              TEXT    PRIMARY KEY,
    session_id      TEXT    NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    rule_pattern    TEXT    NOT NULL,                -- e.g. tool=shell, command="cargo test"
    decision        TEXT    NOT NULL,                -- "allow" | "deny"
    expires_at      INTEGER,                         -- NULL = session-scoped, else unix
    created_at      INTEGER NOT NULL
);

CREATE INDEX auth_decisions_session_idx ON auth_decisions(session_id);
```

**Why ULIDs not UUIDs**: lexicographically sortable by creation time, so listing sessions newest-first is `ORDER BY id DESC` with no extra index.

**Why JSON in `payload`/`arguments`**: SQLite's JSON1 is built-in. Avoids 5 separate part tables. Read perf is fine for ≤10k messages per session.

---

## 4. Pragmas

```sql
PRAGMA journal_mode = WAL;          -- crash-safe + concurrent reads while writing
PRAGMA synchronous = NORMAL;        -- fsync at checkpoint, not every txn (fine for desktop)
PRAGMA foreign_keys = ON;           -- ON DELETE CASCADE actually fires
PRAGMA temp_store = MEMORY;
PRAGMA mmap_size = 268435456;       -- 256 MiB
PRAGMA busy_timeout = 5000;         -- 5s
```

WAL is critical: it lets the TUI render thread read while the agent thread writes.

---

## 5. Rust crate choices

| Crate | Use | Why |
|---|---|---|
| `sqlx` (with `runtime-tokio` + `sqlite` features) | Connection, queries, migrations | Async, compile-time-checked queries via `query!` |
| `ulid` | ID generation | Sortable, no central allocator |
| `blake3` | Workspace-hash | Faster than sha2, no extra dep weight |
| `directories` | XDG paths | Already a dep per spec §6 |

**Migrations**: ship as `migrations/0001_init.sql`, etc. `sqlx::migrate!()` runs them at startup. Schema bumps require a new `0002_*.sql` — never edit a checked-in migration.

---

## 6. The `SessionStore` API

```rust
// src/session/persistence.rs

pub struct SessionStore {
    pool: sqlx::SqlitePool,
}

impl SessionStore {
    pub async fn open(path: &Path) -> Result<Self> { /* mkdir, set 0o600, run migrations */ }

    pub async fn create_session(&self, init: SessionInit) -> Result<SessionId>;
    pub async fn list_recent(&self, workspace_hash: &str, limit: u32) -> Result<Vec<SessionSummary>>;
    pub async fn list_active(&self, limit: u32) -> Result<Vec<SessionSummary>>;
    pub async fn load(&self, id: &SessionId) -> Result<Session>;
    pub async fn archive(&self, id: &SessionId) -> Result<()>;
    pub async fn delete(&self, id: &SessionId) -> Result<()>;

    pub async fn append_message(&self, session_id: &SessionId, msg: Message) -> Result<MessageId>;
    pub async fn append_tool_call(&self, /* … */) -> Result<()>;
    pub async fn record_patch(&self, /* … */) -> Result<PatchId>;

    pub async fn set_title(&self, id: &SessionId, title: &str) -> Result<()>;
    pub async fn set_compacted(&self, message_ids: &[MessageId]) -> Result<()>;

    pub async fn memory_get(&self, workspace_hash: &str, scope: MemoryScope, key: &str) -> Result<Option<String>>;
    pub async fn memory_set(&self, workspace_hash: &str, scope: MemoryScope, key: &str, value: &str) -> Result<()>;
    pub async fn memory_list(&self, workspace_hash: &str, scope: MemoryScope) -> Result<Vec<(String, String)>>;
}
```

Write-through: `Session::push(message)` calls `store.append_message(...)` synchronously inside the agent loop. There is no separate flush — every step's tool result is durable before the next prompt is built. Crash mid-tool-call leaves the call_id row with `finished_at = NULL`, which the resume path reports as "interrupted".

---

## 7. Resume flow

1. User runs `artui` (no args). TUI starts. Statusline shows last 5 sessions for current workspace.
2. User picks one (or presses `r` / runs `/resume`).
3. `SessionStore::load(id)` selects the session row, then all `messages` (ordered by `seq`) plus their `message_parts`, plus `tool_calls`, plus `patches`.
4. `Session` struct is rebuilt: transcript, tool_log, in-flight call status. Compacted messages are replaced by their summary.
5. TUI renders the rebuilt transcript.
6. Composer is enabled. Next user message kicks the agent loop with full context.

**Crash recovery**: any `tool_calls` row with `finished_at IS NULL` is shown to the user with `[interrupted]` and *not* fed back to the model. The model sees only completed tool results so it doesn't think it's mid-tool-call.

---

## 8. Memory (claude-code-style CLAUDE.md, but durable)

Three scopes:

- **`project`** — keyed by `workspace_hash`. Persists across sessions for the same repo. Use case: "this repo uses pnpm not npm". Shown in every prompt for that repo.
- **`user`** — keyed by `workspace_hash = "_global_"`. Shown in every prompt regardless of repo. Use case: "I prefer concise commit messages".
- **`session`** — keyed by `workspace_hash` + `session_id`. Shown only in this session. Use case: model's own working notes.

Surfaced into the system prompt by `agent::prompts::build_system_prompt`:

```
## Memory (project)
- This repo uses pnpm. `npm install` will fail because there is no package-lock.json.
- Tests are run with `pnpm test`, not `npm test`.
- Always run `pnpm typecheck` after edits.
```

Slash commands:

- `/memory list` — show all memory for current workspace
- `/memory set <key> <value>` — write a project-scoped memory
- `/memory delete <key>`
- `/memory user <key> <value>` — write user-global

The contents of `AGENTS.md` / `CLAUDE.md` / `.artui/rules.md` are still read at session start (per spec §11). DB-backed memory layers on top, so users can edit either source.

---

## 9. Compaction integration

When `tokens_used >= 0.835 * context_window`, the agent loop:

1. Picks the oldest N messages whose combined tokens ≈ 60% of `tokens_used`.
2. Sends them + a summarization prompt to a **hidden compaction sub-prompt** (codex pattern, `core/templates/compact/prompt.md`).
3. Receives a summary message.
4. Calls `store.set_compacted(&message_ids)` with the originals + writes a single new message of role=`system` with the summary.
5. Resumes the next loop iteration with the compacted history (originals are still on disk for replay; only the *next prompt* drops them).

`messages.compacted_at` lets the replay path show "[N messages compacted]" markers for fidelity.

---

## 10. Privacy

- DB file mode `0o600` on Unix.
- No network sync. No telemetry hooks.
- `/forget <pattern>` slash command removes matching messages **and** writes a tombstone so replay shows "[forgotten]" instead of the original text.
- `auth/store.rs` already encrypts nothing — credentials sit at `0o600`. The DB follows the same threat model: physical-access attackers are out of scope; OS-level user isolation suffices for v1.

---

## 11. Migration from current state

artui has no persistence today. There is nothing to migrate. First run on the new code creates `artui.db` and runs `0001_init.sql`. Existing in-memory sessions die at quit as they always have.

Optional: a `--import-rollout <path>` flag could read codex JSONL files and back-populate sessions, useful for cross-tool migration. Defer.

---

## 12. Testing

- Unit: round-trip `Session::push(...)` → `store.load(...)` is identity.
- Property: arbitrary message sequences serialize and deserialize correctly.
- Integration: `kill -9` mid-tool-call leaves DB consistent; resume reports interrupted call.
- Concurrency: TUI render thread + agent thread + persistence thread, soak test with 10k messages.

---

## 13. Open questions

- **Per-workspace DB vs single DB**: opencode does both. Single DB is simpler for v1; per-workspace can come later if global queries become slow.
- **Encryption at rest**: out of scope for v1. Schema doesn't preclude it. Future SQLCipher build flag.
- **Cross-machine sync**: out of scope.
- **Snapshot system**: deferred to phase J. opencode tracks workspace as content tree at every step-start. Powerful for revert but ≥10× the disk footprint.
