# GitHub Copilot OAuth — Zero-Config Device Flow

**Status:** v1 design (2026-05-21)
**Companion:** `harness-architecture.md` §9, `oauth-provider-support.md` Phase 3
**Replaces:** the current friction-heavy flow that requires users to register their own GitHub OAuth App and paste the client_id into config, *or* fall back to pasting a Personal Access Token.

This doc specifies how artui acquires, stores, refreshes, and uses GitHub Copilot credentials so the user experience matches `gh auth login`, `opencode auth login github-copilot`, and Copilot.vim — i.e. the user runs `/login copilot`, opens a browser, types an 8-char code, and is done.

---

## 1. The friction problem

What artui does today (`src/auth/github.rs`, `src/providers/copilot.rs`, `src/config/schema.rs`):

- `github_oauth_client_id` defaults to **empty string** (`src/config/schema.rs:194`). The user has to register their own GitHub OAuth App and paste the client_id into `~/.config/artui/config.toml`.
- If the user can't be bothered, they paste a **Personal Access Token** (`ghp_…`). artui's `exchange_github_token` has a fallback that returns the PAT as a "session token", which silently fails at `/chat/completions` because PATs lack the IDE entitlement Copilot's internal endpoint expects.
- `editor_version` and `editor_plugin_version` default to `"artui"` (`src/config/schema.rs:194-195`), which **some Copilot tenants reject** with `400 invalid editor`.
- `Copilot-Integration-Id` header is missing from chat requests, which makes the API return 400 on stricter accounts.

Result: most users either give up or paste a PAT and get cryptic 400s. Both opencode and Copilot.vim ship a working zero-config flow. We can match it.

---

## 2. The four-tier model (industry-converged)

Every real Copilot client (VSCode, Copilot.vim, opencode, neovim/copilot.lua, JetBrains Copilot, copilot-proxy, litellm, freegpt) uses the same shape:

```
device-code  →  gho_ user OAuth token  →  copilot session token  →  /chat/completions
                (long-lived, on disk)     (~30 min, in-mem)         (per-request)
```

artui already has the right shape for the last three steps (`exchange_github_token` calls `/copilot_internal/v2/token` correctly with `Authorization: token <gho_...>`). The first step — *acquiring* the `gho_…` token without forcing the user to register an OAuth App — is what this doc fixes.

---

## 3. Client ID

Two valid public client IDs are observed in the wild. **Both work for individual Copilot subscribers; neither requires the user to register their own GitHub OAuth App.**

| Client ID | Source | Notes |
|---|---|---|
| `Iv1.b507a08c87ecfe98` | VSCode Copilot extension. Reused by litellm, copilot-proxy, freegpt, neovim/copilot.lua, JetBrains Copilot, Copilot.vim default | Most widely used. Required for parity with VSCode subscriber tier. Some policy risk: it's technically VSCode's IP. |
| `Ov23li8tweQw6odWQebz` | sst/opencode — `packages/opencode/src/plugin/github-copilot/copilot.ts:12` | opencode's own registered GitHub App (newer `Ov23li…` prefix is the 2024 GitHub Apps client_id format) |

**Recommendation for artui**: register an **artui-owned GitHub OAuth App** at `https://github.com/settings/developers` ("New OAuth App" → enable Device Flow → callback URL is unused but required, set to `http://localhost`). The device flow does not require a client secret. Hard-code the resulting `Ov23li…` client_id in the binary, the way opencode does:

```rust
// src/auth/github.rs (target)
const ARTUI_COPILOT_CLIENT_ID: &str = "Ov23li__ARTUI_TBD__";  // register on first release
const VSCODE_COPILOT_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98"; // fallback
```

Add a `--copilot-vscode-compat` flag (and a `[providers.copilot] vscode_compat = true` config knob) to fall back to the VSCode ID for users on VSCode-only entitlements.

The `github_oauth_client_id` config field is kept as an override for users who want their own app, but defaults to *neither* empty nor required — the binary has working defaults.

---

## 4. The device flow

Confirmed identical across opencode, Copilot.vim, copilot-proxy, and the GitHub published spec.

### 4.1 Request a device code

```http
POST https://github.com/login/device/code
Accept: application/json
Content-Type: application/json
{
  "client_id": "Ov23li__ARTUI_TBD__",
  "scope": "read:user"
}
```

Response:

```json
{
  "device_code": "abcd-1234-...",
  "user_code": "ABCD-1234",
  "verification_uri": "https://github.com/login/device",
  "interval": 5,
  "expires_in": 900
}
```

Show `user_code` and `verification_uri` to the user. Open the browser via `open_browser()` (already exists in artui).

### 4.2 Poll for the access token

```http
POST https://github.com/login/oauth/access_token
Accept: application/json
Content-Type: application/json
{
  "client_id": "Ov23li__ARTUI_TBD__",
  "device_code": "abcd-1234-...",
  "grant_type": "urn:ietf:params:oauth:grant-type:device_code"
}
```

Three error states:

| `error` field | Meaning | Action |
|---|---|---|
| `authorization_pending` | User hasn't completed the flow yet | Wait `interval` seconds, retry |
| `slow_down` | Polling too fast | Add 5s to interval, retry |
| `expired_token` | User took longer than `expires_in` | Tell user to run `/login copilot` again |
| `access_denied` | User denied | Stop |
| (other) | Anything else | Surface as error |

**Polling safety margin**: opencode adds 3 seconds to every poll to absorb clock skew. Mirror this:

```rust
const OAUTH_POLLING_SAFETY_MARGIN: Duration = Duration::from_secs(3);
```

Successful response:

```json
{
  "access_token": "gho_...",
  "token_type": "bearer",
  "scope": "read:user"
}
```

**Scope is `read:user` only.** Not `repo`, not `copilot`. The Copilot entitlement is checked server-side based on the OAuth App identity, not the OAuth scope.

### 4.3 Persist the token

artui's `AuthStore` (`src/auth/store.rs`) already handles this correctly:

- File: `~/.local/share/artui/auth.json` (or platform equivalent via `directories`).
- Mode: `0o600` on Unix.
- Atomic write: tmp file + `rename`.

Stored shape (one entry per provider):

```json
{
  "version": 1,
  "providers": {
    "copilot": {
      "type": "oauth",
      "access_token": "gho_...",
      "refresh_token": "gho_...",
      "expires_at": null,
      "metadata": {
        "client_id": "Ov23li...",
        "enterprise_url": null,
        "session_token": null,
        "session_expires_at": null,
        "endpoints": null
      }
    }
  }
}
```

The gho_ token does not expire under the device-flow grant (until the user revokes it via `https://github.com/settings/applications`), so `expires_at: null`. Refresh tokens are not issued for device flow; we copy the access token into the refresh slot for AuthStore shape compatibility.

---

## 5. Token exchange (gho_ → Copilot session token)

This is what artui already does correctly. Reaffirming the contract:

```http
GET https://api.github.com/copilot_internal/v2/token
Authorization: token gho_...                 ← lowercase "token", NOT "Bearer"
Editor-Version: vscode/1.99.2
Editor-Plugin-Version: copilot-chat/0.27.2025041102
User-Agent: GithubCopilot/1.155.0
```

Response (the parts artui must consume):

```json
{
  "token": "tid=...;exp=1716...;sku=...",
  "expires_at": 1716308341,
  "refresh_in": 1500,
  "endpoints": {
    "api": "https://api.githubcopilot.com",
    "proxy": "https://copilot-proxy.githubusercontent.com",
    "telemetry": "https://copilot-telemetry.githubusercontent.com",
    "origin-tracker": "https://origin-tracker.githubusercontent.com"
  },
  "chat_enabled": true,
  "chat_jetbrains_enabled": true,
  "code_quote_enabled": true,
  "individual": true,
  "sku": "monthly_subscriber",
  "tracking_id": "..."
}
```

Two changes needed:

1. **Trust `expires_at`**, fall back to `now + 25*60` only when missing. artui currently always uses the 25-min hard fallback (`unix_timestamp() + 25*60`).
2. **Honor `endpoints.api`** for Business / Enterprise. For GHE the `/copilot_internal/v2/token` URL itself moves to `https://<ghe-host>/api/v3/copilot_internal/v2/token`.

Cache the session token in `metadata.session_token` + `metadata.session_expires_at` in AuthStore. Refresh proactively when `session_expires_at - now < 60`.

---

## 6. Chat-completion request headers

Required on every `/chat/completions`, `/responses`, `/v1/messages` request to Copilot:

| Header | Value | Notes |
|---|---|---|
| `Authorization` | `Bearer <copilot-session-token>` | NOT the gho_ token |
| `Editor-Version` | `vscode/1.99.2` | Default; do not use `"artui"` |
| `Editor-Plugin-Version` | `copilot-chat/0.27.2025041102` | Match a real VSCode Copilot Chat release |
| `Copilot-Integration-Id` | `vscode-chat` | **REQUIRED** — without this header the API returns 400 |
| `User-Agent` | `GithubCopilot/1.155.0` | |
| `Openai-Intent` | `conversation-edits` | opencode sets this for chat |
| `x-initiator` | `user` or `agent` | Toggle based on whether last msg is a user msg |
| `Copilot-Vision-Request` | `true` | only when sending images |

artui's current defaults of `Editor-Version: artui` and `Editor-Plugin-Version: artui` (`src/config/schema.rs:194-195`) are rejected by some Copilot tenants. Override defaults to VSCode-shaped values; allow user override only via config, not flag, and warn when overridden.

`Copilot-Integration-Id: vscode-chat` is **non-overridable**.

---

## 7. gh CLI / Copilot.vim compatibility

Power users already have one of these set up:

- `gh auth login` writes to `~/.config/gh/hosts.yml` with a `oauth_token`.
- Copilot.vim writes to `~/.config/github-copilot/hosts.json` and `~/.config/github-copilot/apps.json`.

artui should **read these as a third token source** (after env var and AuthStore), giving zero-login UX for users who already authenticated elsewhere.

Schema (Copilot.vim format):

```json
// ~/.config/github-copilot/hosts.json
{
  "github.com:Iv1.b507a08c87ecfe98": {
    "user": "octocat",
    "oauth_token": "gho_...",
    "github_app_id": "Iv1.b507a08c87ecfe98"
  }
}
```

Read order:

1. `GH_TOKEN` / `GITHUB_TOKEN` env var (only if it's a `gho_…`; reject `ghp_…`).
2. `AuthStore::get(provider="copilot")`.
3. `~/.config/github-copilot/hosts.json` and `apps.json`.
4. `gh auth token` shell-out (already has dedupe logic in `src/providers/copilot.rs`).
5. Else: prompt user to run `/login copilot`.

---

## 8. Enterprise / GHE

Plumb `enterprise_url` through:

- `GitHubDeviceFlowConfig` (already partly present in `src/config/schema.rs`)
- `CopilotConfig`
- `AuthRecord.metadata.enterprise_url`

URLs become:

| Concern | github.com | GHE host = `company.ghe.com` |
|---|---|---|
| Device code | `https://github.com/login/device/code` | `https://company.ghe.com/login/device/code` |
| Token exchange | `https://api.github.com/copilot_internal/v2/token` | `https://company.ghe.com/api/v3/copilot_internal/v2/token` |
| Chat | from response's `endpoints.api` | from response's `endpoints.api` |

The chat URL always comes from `endpoints.api`, so artui doesn't have to hard-code the Business/Enterprise host.

---

## 9. UX flow

1. Fresh install. User runs `artui`. No auth configured. Provider list shows Copilot grayed out.
2. User types `/login copilot`. Modal appears:
   ```
   ┌──── GitHub Copilot Login ─────────────────────────────┐
   │                                                       │
   │   Open this URL in your browser:                      │
   │                                                       │
   │       https://github.com/login/device                 │
   │                                                       │
   │   Then enter this code:                               │
   │                                                       │
   │       ABCD-1234                                       │
   │                                                       │
   │   Waiting for authorization... [Esc to cancel]        │
   │                                                       │
   └───────────────────────────────────────────────────────┘
   ```
3. artui calls `open::that(verification_uri)` to open the browser automatically. If that fails, the URL is copyable from the modal.
4. User authorizes in browser. artui's poller succeeds. Modal closes. Statusline shows `ctx 0% · GitHub Copilot · gpt-5`.
5. Provider list shows Copilot active.

No client_id paste. No PAT. No config edit. No restart.

---

## 10. Migration

Existing users with PATs:

- On startup, if `AuthStore::get("copilot")` exists with `access_token` starting with `ghp_`, show a one-time banner:
  > Your stored Copilot credential is a Personal Access Token, which Copilot's chat endpoint does not accept. Run `/login copilot` to upgrade to OAuth (no setup required).
- `exchange_github_token` no longer silently treats `ghp_` as a session token. It returns a clear error pointing at `/login copilot`.

Existing users with their own client_id in config:

- The `[providers.copilot] github_oauth_client_id` field is still honored if non-empty.
- Otherwise the bundled artui client_id is used.
- No migration action required.

---

## 11. Implementation deltas

What changes vs current code:

| Area | File | Change |
|---|---|---|
| client_id default | `src/config/schema.rs:194` | Default to `Some(ARTUI_COPILOT_CLIENT_ID)` not `None` |
| Editor-Version default | `src/config/schema.rs:194-195` | Default to `vscode/1.99.2` not `artui` |
| Editor-Plugin-Version default | `src/config/schema.rs` | Default to `copilot-chat/0.27.2025041102` |
| `Copilot-Integration-Id` | `src/providers/copilot.rs` (request builders) | Always send `vscode-chat`. Non-overridable. |
| Device-flow poll body | `src/auth/github.rs:194-203` | Switch from form-encoded to JSON body (matches GitHub spec, opencode, Copilot.vim) |
| Polling safety margin | `src/auth/github.rs` | Add 3s to every poll interval |
| `slow_down` handling | `src/auth/github.rs` | Use server-supplied `interval` when present |
| Drop PAT fallback | `src/providers/copilot.rs:541-548` | Remove the silent "use the PAT as session token" branch |
| Trust `expires_at` | `src/providers/copilot.rs` | Use response `expires_at`; only fall back to `+25min` when missing |
| `endpoints.api` honor | `src/providers/copilot.rs:563-569` | Already correct; verify enterprise path |
| gh-CLI compat reader | `src/auth/github.rs` (new) | Add `read_copilot_vim_hosts()` returning the gho_ token if present |
| One-time PAT banner | `src/app.rs` startup hook | Detect `ghp_` and show upgrade prompt |
| `--copilot-vscode-compat` flag | `src/main.rs` clap | Toggle to use `Iv1.b507a08c87ecfe98` instead |
| `--copilot-client-id` flag | `src/main.rs` clap | Override for advanced users |

All changes are additive or default-changing. No breaking API.

---

## 12. Security notes

- **Never log the gho_ or session token.** `tracing` filter already redacts. Add tests.
- **Never include the gho_ token in error messages** surfaced to the model. Use `[redacted]` placeholders.
- **Mode 0o600** on `auth.json` (already correct in `auth/store.rs`).
- **Validate `endpoints.api`** is HTTPS and matches a domain allowlist (`*.githubcopilot.com`, `*.githubusercontent.com`, `<enterprise_host>`). Reject http: or unrelated domains.
- **OAuth App registration**: when artui registers its production OAuth App, the GitHub App owner is the artui project. App settings should disable "request user authorization (OAuth) during installation" and enable Device Flow only.

---

## 13. References

- GitHub OAuth Device Flow spec: https://docs.github.com/en/apps/oauth-apps/building-oauth-apps/authorizing-oauth-apps#device-flow
- RFC 8628 §3.5 (slow_down): https://datatracker.ietf.org/doc/html/rfc8628#section-3.5
- opencode device flow: `/tmp/opencode/packages/opencode/src/plugin/github-copilot/copilot.ts:212-326`
- opencode runtime header injection: `copilot.ts:92-169`
- opencode auth file shape: `/tmp/opencode/packages/opencode/src/auth/index.ts:7-46`
- Copilot.vim hosts.json compat reference
- codex device flow Rust idiom (different IdP, same shape): `/tmp/codex/codex-rs/login/src/device_code_auth.rs`
- Copilot internal API shape: deepwiki / freegpt mirror
