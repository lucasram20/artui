# Phase G — SQLite session persistence + memory

**Status:** DONE (2026-05-21)
**Summary:** Implemented `SessionStore` with rusqlite (WAL mode, 0o600 perms, ULID keys). Full CRUD for sessions, messages, memory. Schema: sessions/messages/memory tables with CASCADE delete, interrupted-message flagging. 86 tests pass.

**Phase:** G
**Spec:** `docs/spec/session-persistence.md` (full schema and API)
**Blocks:** H (compaction reads/writes from DB)
**Depends:** A, B, C
**Estimated PR size:** ~1200 LoC + migrations

---

## Why

Sessions die on quit. No resume, no replay, no per-workspace memory. Both opencode (Drizzle SQLite) and codex (JSONL + SQLite index) persist locally. claude-code persists CLAUDE.md-style memory. This phase brings artui to parity.

## Scope

### In scope

- Add `sqlx` (with `runtime-tokio`, `sqlite`), `ulid`, `blake3` crates.
- New `src/session/persistence.rs` — `SessionStore` API per `docs/spec/session-persistence.md` §6.
- SQL migrations under `migrations/0001_init.sql` (full schema in spec §3).
- WAL mode + pragmas per spec §4.
- Write-through: `Session::push(...)` → `store.append_message(...)` synchronous.
- Resume flow: `SessionStore::load(id)` rebuilds `Session` from rows (spec §7).
- Memory: project / user / session scopes with slash commands `/memory list|set|delete|user`.
- Crash recovery: tool_calls with `finished_at IS NULL` flagged `[interrupted]` on resume.
- `/sessions` slash command opens a picker showing recent sessions for the current workspace.

### Out of scope

- Snapshot system (Phase J equivalent).
- Cross-machine sync.
- Encryption at rest (SQLCipher).
- Compaction (Phase H reads `messages.compacted_at` set by this phase).

## Acceptance criteria

- Run `artui`, send a message, quit, run `artui` again, pick the session — full transcript replayed.
- `kill -9` artui mid-tool-call; relaunch; resume shows `[interrupted]` for the dead call. Model is not fed the half-written tool result.
- `/memory set lang rust` → next session in same workspace shows it in system prompt.
- DB file is `~/.local/share/artui/artui.db` (Linux) with mode `0o600`.
- WAL mode confirmed via `PRAGMA journal_mode;`.
- Concurrent test: TUI render thread + agent thread + persistence thread, 10k messages, no SQLITE_BUSY.
- `cargo test` passes; integration tests cover resume, crash recovery, memory.

## Files touched

| File | Change |
|---|---|
| `Cargo.toml` | Add deps: sqlx, ulid, blake3 |
| `migrations/0001_init.sql` (new) | Full schema per spec §3 |
| `src/session/mod.rs` | Add `SessionId` (ulid wrapper), `MemoryScope` |
| `src/session/persistence.rs` (new) | `SessionStore` |
| `src/session/parts.rs` (new) | `MessagePart` JSON serde for `message_parts.payload` |
| `src/agent/loop.rs` | After every `Session::push`, also `store.append_message`. |
| `src/agent/prompts.rs` | Inject `## Memory (project)` block from `store.memory_list(workspace_hash, MemoryScope::Project)` |
| `src/app.rs` | At startup: `SessionStore::open(...)`, list recent for workspace, prompt user to resume or start fresh |
| `src/ui/popups.rs` | New `draw_session_picker(...)` |
| Tests | `tests/persistence.rs`: round-trip, crash, concurrency |

## Schema highlights

See `docs/spec/session-persistence.md` §3 for full schema. Key points:

- ULIDs for session/message ids: lexicographically sortable.
- JSON payload column for parts (avoids N tables).
- Foreign keys with `ON DELETE CASCADE` so `/forget session` is one DELETE.
- WAL mode is non-negotiable (concurrent reads + writes).

## Risks

- **First-run migration**: `sqlx::migrate!()` runs in-process at startup. If migration fails (corrupt DB), TUI must show a clear error and fall back to in-memory mode rather than crashing.
- **Schema drift**: never edit `migrations/0001_init.sql` after merge; always add `0002_*.sql`.
- **JSON1 availability**: `sqlx`'s default sqlite build includes JSON1. Verify in CI.
- **Disk usage**: 10k messages with avg 2KB part = 20MB per session. Add `/sessions archive --older-than 30d` slash command later.
- **Test isolation**: tests must use temp DB paths (`tempfile::NamedTempFile`); never touch user's real DB.

## References

- Spec: `docs/spec/session-persistence.md` (full)
- opencode `storage/db.ts`: `/tmp/opencode/packages/opencode/src/storage/db.ts:33`
- codex `RolloutRecorder`: `codex-rs/rollout/src/recorder.rs:73`
- sqlx docs: https://docs.rs/sqlx
- ULID rationale: https://github.com/ulid/spec
