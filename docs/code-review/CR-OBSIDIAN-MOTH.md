# Code Review: CR-OBSIDIAN-MOTH

| Field | Value |
|-------|-------|
| Reviewer | GPT-5.5 Codex |
| Review date | 2026-06-02 |
| Scope commits (original) | `4e88196..9e347ab` |
| Remediation commit | [`d937b59`](https://github.com/lucasram20/artui/commit/d937b59381d5d535415c06bb5c30261bfcc7436b) |
| Changelog | `docs/changelogs/CHANGELOG.md` → **Unreleased → Fixed** |

Scope reviewed:
- `gh`/git: commits `4e88196..9e347ab`, release/docs alignment, M5–M9 implementation diff.
- Graphify: queried indexed graph for M3–M9, sandbox/index/web/release/distribution relationships.
- Changelog: `docs/changelogs/CHANGELOG.md` v0.7.0 notes.

## Agent Performance Notes

### What the coding agent did best
- Delivered a broad M5–M9 feature set with clear release/docs alignment and changelog coverage.
- Added useful sandbox/index/web-tool capabilities in cohesive areas instead of scattering unrelated changes.
- Preserved enough structure that the defects are fixable with targeted patches, not a rewrite.

### What it did not do well
- Missed Windows lifecycle semantics: the Job Object handle must stay alive while the child process runs.
- Treated public web-fetch validation as scheme-only, leaving local/private-network targets reachable.
- Added workspace indexing without a post-mutation refresh/staleness path.
- Documented/used semantic search as FTS-like while the storage table is plain SQLite, causing fallback behavior.

### How to improve without breaking current changes
- Keep fixes narrow and compatibility-preserving: patch behavior behind the existing APIs/config flags.
- Windows sandbox: store the job handle with the child/sandbox guard and drop it only after `wait_with_output()` completes.
- Web tool: add DNS/IP validation before request dispatch; reject loopback, private, link-local, multicast, unspecified, and cloud metadata targets while preserving normal `http`/`https` public fetches.
- Index: after successful file mutations, either rebuild affected entries or mark the index stale and rebuild before `symbol`/`semantic` search.
- Semantic search: migrate `chunks` to FTS5 or split modes clearly (`substring` vs `fts`) so docs match behavior.
- Add regression tests for each fix before refactoring broader architecture.

---

## Findings (original)

### 🔴 P1 — Windows Job Object sandbox kills commands immediately

- Location: `src/sandbox/win_jobobject.rs:85-94` (pre-fix)
- Problem: `assign_pid()` closes the job handle right after assigning the child. Because the job has `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, closing the last job handle terminates the assigned process before `wait_with_output()` can complete normally.
- Impact: On Windows with `sandbox.mode = auto`/`win_job`, most shell commands can be killed immediately or behave flakily, breaking the shell tool in the default Windows sandbox path.
- Fix: Keep the job handle alive until after `wait_with_output()` returns; only close it at the end/error path.

### 🔴 P1 — `web` tool allows localhost/private-network SSRF

- Location: `src/tools/web.rs:31-49` (pre-fix)
- Problem: URL validation only checks `http://`/`https://`; it does not reject `localhost`, loopback, RFC1918/link-local IPs, or DNS names resolving to private addresses.
- Impact: A model/tool call can fetch `http://127.0.0.1:*`, LAN admin panels, or cloud metadata endpoints from the user's machine, despite the tool description saying “public HTTP(S) URL”.
- Fix: Resolve and block loopback/private/link-local/metadata ranges; ideally allowlist public hosts or route through a safer browser/fetch layer.

### 🟡 P2 — Workspace index becomes stale after edits

- Location: `src/app.rs:578-586`, `src/tools/apply_patch.rs` integration area (pre-fix)
- Problem: `WorkspaceIndex::open()` rebuilds once during app startup, but there is no refresh after `apply_patch`, shell writes, LSP workspace edits, or file changes.
- Impact: `search mode=symbol|semantic` can return deleted symbols/old lines and miss newly created code for the rest of the session, which is especially misleading for agents relying on indexed codebase context.
- Fix: Rebuild/incrementally update the index after successful file mutations, or expose staleness and a refresh command before indexed search.

### 🟡 P2 — `semantic` search is not backed by FTS5

- Location: `src/index/mod.rs:47-51`, `src/index/text.rs:51-67` (pre-fix)
- Problem: `chunks` is created as a normal table, but `search_fts()` queries `body MATCH ?1`, which only works on FTS virtual tables; the code then falls back to `LIKE`.
- Impact: `mode=semantic` is effectively substring search, not FTS/semantic search, so multi-token/ranked queries and the documented index behavior are much weaker than advertised.
- Fix: Create `chunks` as an FTS5 virtual table (or rename the mode/documentation to reflect simple LIKE search).

---

## Remediation (for Codex re-review)

Implemented in `d937b59` on `main`. Changelog mirror: `docs/changelogs/CHANGELOG.md` **Unreleased**.

| ID | Status | Summary |
|----|--------|---------|
| P1 Windows job | **Fixed** | Job handle lifecycle corrected |
| P1 Web SSRF | **Fixed** | Pre-fetch DNS/IP + hostname guards |
| P2 Index staleness | **Partial** | Refresh after `apply_patch` only |
| P2 FTS5 semantic | **Fixed** | FTS5 virtual table + migration |

### P1 — Windows Job Object → **Fixed**

- **Files:** `src/sandbox/win_jobobject.rs`
- **Change:** `assign_pid()` only closes the process handle, not the job. `run_command()` holds `job_handle: Option<HANDLE>` until `child.wait_with_output()` returns, then `close_job()`.
- **Regression tests:** None on Linux CI (Windows-only path). Manual re-review on Windows with `sandbox.mode = auto` and a long-running `shell` command recommended.
- **Codex check:** Confirm no `CloseHandle(job)` between assign and wait; confirm error paths still close the job to avoid handle leaks.

### P1 — Web SSRF → **Fixed**

- **Files:** `src/tools/url_safety.rs` (new), `src/tools/web.rs`, `src/tools/mod.rs`, `Cargo.toml` (`url = "2"`)
- **Change:** `validate_public_http_url()` runs before `client.get(url)`. Blocks schemes other than http/https, blocked hostnames, literal blocked IPs, and post-DNS resolved addresses in private/metadata ranges.
- **Regression tests:** `url_safety::tests::*` (5 tests, incl. `rejects_loopback_url_before_fetch`, `allows_public_host` with live DNS to example.com).
- **Residual risk:** DNS rebinding between validation and fetch not mitigated (single-point resolve, no pinned connection). Redirects to private URLs not followed explicitly by validator (reqwest redirect policy should be reviewed). No port allowlist beyond blocked IP checks.

### P2 — Index staleness → **Partial**

- **Files:** `src/index/mod.rs` (`refresh_after_mutation`, `max_size_mb` stored on struct), `src/tools/apply_patch.rs`
- **Change:** After successful patch apply, `spawn_blocking(|| index.refresh_after_mutation(&workspace_root))`; failures log `tracing::warn!` only.
- **Not done (documented gap):** Shell mutating commands, LSP `apply_workspace_edit`, external file watchers, UI `/index refresh` command.
- **Codex check:** Apply patch in a session, run `search mode=semantic` for a symbol added only in that patch — should hit without restart.

### P2 — FTS5 semantic → **Fixed**

- **Files:** `src/index/text.rs`, `src/index/mod.rs`
- **Change:** `ensure_chunks_fts()` drops legacy plain `chunks` and creates `fts5(path UNINDEXED, line UNINDEXED, body)`. `search_fts()` uses `chunks MATCH ?` with per-token quoting; empty FTS result falls back to `LIKE`.
- **Regression tests:** `index::text::tests::fts5_match_beats_like_fallback`, `index::tests::fts5_semantic_search_finds_token`.
- **Residual risk:** Existing user index DBs are migrated on next open (table drop + recreate — full re-index on rebuild). Not ranked vector/embedding search; name `semantic` remains FTS5 text match.

---

## Verification commands

```bash
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
# Targeted:
cargo test url_safety
cargo test fts5
```

All passed on Linux at remediation time (`d937b59`).

---

## Notes

- Release docs/changelog now consistently say v0.7.0 ships Linux x86_64 + Windows x86_64 release assets and source install for macOS/ARM.
- Graphify index was present and queried successfully; graph also reflects the M6/M9 risks around index freshness and web-tool scope.
- **Re-review ask (Codex):** Confirm P1/P2 rows marked Fixed/Partial match code on `main` at or after `d937b59`; file Windows sandbox on a host if P1 cannot be validated from Linux CI alone.