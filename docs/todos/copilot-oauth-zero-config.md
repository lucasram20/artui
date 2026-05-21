# Copilot OAuth — Zero-Config Device Flow

**Status:** DONE (2026-05-21)
**Summary:** Hard-coded artui OAuth App client_id (`Ov23liSsh5cnZv6yAz4X`). Defaulted editor_version to `vscode/1.99.2` and editor_plugin_version to `copilot-chat/0.26.3` for VSCode-shaped headers. Removed silent PAT fallback — `ghp_` tokens now rejected with friendly error pointing to `/login copilot`. 86 tests pass.
**Remaining:** `hosts.json` reader for gh-CLI compat and `--copilot-vscode-compat` flag deferred to follow-up.

**Phase:** A.5 (parallel with Phase A; not blocking)
**Spec:** `docs/spec/copilot-oauth.md`
**Replaces friction:** users no longer need to register their own GitHub OAuth App or paste a PAT.
**Estimated PR size:** ~250 LoC

---

## Why

Today's Copilot flow forces users to:

1. Register a GitHub OAuth App at `github.com/settings/developers`,
2. Paste the resulting `client_id` into `~/.config/artui/config.toml`,
3. Then run `/login copilot`.

Or alternatively, paste a Personal Access Token (`ghp_...`) which artui silently accepts but **does not work** at `/chat/completions` because PATs lack the IDE entitlement Copilot's internal endpoint expects. The user gets cryptic 400s.

Both opencode and Copilot.vim ship a working zero-config flow. We can match it by hard-coding a public client_id in the binary.

## Scope

### In scope

- Hard-code an artui-owned GitHub OAuth App client_id (`ARTUI_COPILOT_CLIENT_ID`).
- Default `editor_version` and `editor_plugin_version` to VSCode-shaped values.
- Always send `Copilot-Integration-Id: vscode-chat` (non-overridable).
- Switch device-flow poll body from form-encoded to JSON.
- Add 3-second polling safety margin; honor server-supplied `interval`.
- Drop the silent PAT-as-session-token fallback.
- Trust `expires_at` from `/copilot_internal/v2/token` response; fall back to `+25min` only if missing.
- Read `~/.config/github-copilot/hosts.json` and `apps.json` as a third token source (gh CLI / Copilot.vim users get zero-login).
- Show a one-time banner if stored credential is `ghp_...` pointing user at `/login copilot`.
- Add `--copilot-vscode-compat` CLI flag (uses `Iv1.b507a08c87ecfe98` instead).

### Out of scope

- Enterprise/GHE plumbing (separate ticket later).
- Encrypting `auth.json` (out of v1 entirely).

## Acceptance criteria

- `artui` (fresh install, no config) → `/login copilot` → 8-char code → done. No client_id paste.
- `artui` with `~/.config/github-copilot/hosts.json` set up → `/login copilot` is unnecessary; Copilot is active immediately.
- `artui` with stored `ghp_...` → one-time banner explaining how to upgrade.
- All existing Copilot chat/Responses/Anthropic-shim routes pass through with new headers.
- `cargo test` passes.

## Files touched

| File | Change |
|---|---|
| `src/auth/github.rs` | Add `ARTUI_COPILOT_CLIENT_ID` const; switch poll body to JSON; add 3s safety margin; add `slow_down` server-interval handling; new `read_copilot_vim_hosts()` reader |
| `src/config/schema.rs` | Default `github_oauth_client_id` to bundled const, not empty; default `editor_version` to `"vscode/1.99.2"`; default `editor_plugin_version` to a real Copilot Chat release |
| `src/providers/copilot.rs` | Always send `Copilot-Integration-Id: vscode-chat`; remove silent PAT fallback at lines 541-548; trust `expires_at` from response; reject `ghp_` access tokens with friendly error |
| `src/app.rs` | Startup hook: detect `ghp_` in stored credential, push one-time banner |
| `src/main.rs` | Add `--copilot-vscode-compat` and `--copilot-client-id` CLI flags |
| Tests in `auth/github.rs` and `providers/copilot.rs` | New cases for JSON poll body, slow_down handling, hosts.json reader, expires_at trust |

## Client ID registration (one-time)

Before this PR can ship to users, the maintainer registers a production GitHub OAuth App:

1. Go to `https://github.com/settings/developers` → "New OAuth App".
2. Application name: `artui`.
3. Homepage URL: artui repo URL.
4. Authorization callback URL: `http://localhost` (unused but required).
5. Enable Device Flow.
6. Copy the resulting `Ov23li...` client_id into `ARTUI_COPILOT_CLIENT_ID` const.

Until registration: tests can use `Iv1.b507a08c87ecfe98` (VSCode's public ID) as a placeholder; the test build must not ship to users.

## Risks

- **OAuth App rate limits**: GitHub limits OAuth Apps to 5k req/hr/IP. Should never hit. Document for issue triage.
- **Editor-Version policy drift**: GitHub may tighten what `Editor-Version` strings are accepted. If `vscode/1.99.2` stops working, bump to a newer release ID. Worst case: implement `--copilot-editor-version` flag.
- **Copilot ToS**: using VSCode's public client_id is a gray area. Default to artui's own ID; only fall back to VSCode's via explicit flag.
- **Token leakage**: `tracing` filter must redact gho_ and session tokens. Add tests for this.

## References

- Spec: `docs/spec/copilot-oauth.md`
- opencode device flow: `/tmp/opencode/packages/opencode/src/plugin/github-copilot/copilot.ts:212-326`
- GitHub Device Flow spec: https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow
