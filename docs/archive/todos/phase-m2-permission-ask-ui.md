# Phase M2 — Mid-Tool Permission Ask UI

**Phase:** M2 (production polish, visible UX)
**Spec:** `docs/spec/harness-architecture.md` §5.4 (permission engine)
**Depends:** D (PermissionEngine), M1 (diff preview shares the modal pattern)
**Estimated PR size:** ~500 LoC

---

## Why

`PermissionEngine` already classifies tools as `Allow / Ask / Deny`,
but the agent loop auto-promotes `Ask` to `Allow` because there is no
modal yet. Real safety only kicks in once the user is in the loop for
write operations (shell, apply_patch on dangerous paths, network
fetches).

## Scope

### In scope

- Generic `Approval` modal for any tool — title, description, args
  preview, decision keys (`a`/`d`/`A` for allow-once/deny/allow-all).
- `[permissions]` config matrix:
  - `apply_patch = "ask"` (default)
  - `shell.read_only = "allow"`, `shell.default = "ask"`,
    `shell.dangerous = "deny"`
  - `network = "ask"` (future-proof for web tools)
  - `task = "allow"` (subagents inherit parent rules)
- "Allow this session" button — caches decision per `(tool_name,
  fingerprint)` until restart.
- "Always allow" button — persists into auth-store-style decisions
  table (`auth_decisions` schema already in Phase G plan).
- Subagents inherit decisions from parent at spawn time but cannot
  escalate beyond their derived ruleset.
- Statusline counter `(2 pending approvals)` when modal is queued.

### Out of scope

- Per-domain network allowlist (defer to web-tool phase).
- Approval policy bot / external review service.

## Acceptance criteria

- `shell rm -rf` shows the modal; `d` denies; agent receives
  `denied_by_user` result and continues.
- "Always allow" persists across restarts (DB row in `auth_decisions`).
- Plan-mode agent never sees the modal — engine returns `Deny` directly.
- Config `[permissions] shell = "deny"` short-circuits the modal too.
- Two queued approvals → second one renders after first decided.
- `cargo test` passes; tests cover allow/deny/always-allow paths.

## Files touched

| File | Change |
|---|---|
| `src/ui/approval.rs` (new) | Modal renderer |
| `src/permissions/decisions.rs` (new) | Persistent allow-list table |
| `src/permissions/mod.rs` | Wire modal channel |
| `src/agent/loop.rs` | `PermissionDecision::Ask(prompt)` blocks on oneshot |
| `src/app.rs` | New `UiMode::Approval`; pending queue |
| `src/config/schema.rs` | `[permissions]` per-tool matrix |
| `src/session/store.rs` | `auth_decisions` table migration |
| Tests | Approval queue, persistence, plan-mode short-circuit |

## Risks

- **Deadlock**: agent loop blocks on oneshot; if UI thread crashes the
  loop hangs forever. Wrap with `tokio::select!` + cancel token.
- **Always-allow is dangerous**: scope it tightly — `(tool, args
  fingerprint)`, not just `tool`. Hash the canonical args JSON.
- **Plan/Build mode interaction**: switching mid-turn shouldn't allow
  through pending approvals from the previous mode.

## References

- claude-code permissions doc
- opencode `permission/permission.ts`
- artui Phase E permission Ask sketch
