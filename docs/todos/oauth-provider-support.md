# OAuth Provider Support Todo

Goal: add account-based model providers so `artui` can use a user's existing OpenAI/ChatGPT and GitHub Copilot subscriptions after a one-time login, while keeping Ollama and API-key providers working.

## Research summary

- OpenCode exposes subscription-backed providers through `/connect`; its docs list OpenAI ChatGPT Plus/Pro and GitHub Copilot account login flows, with credentials stored locally in an auth file.
- Pi Agent documents `/login` and `/logout` for subscription providers, storing and refreshing provider tokens locally.
- OpenAI Codex supports ChatGPT login and API-key login; its docs warn that local auth files contain access tokens and must be treated like passwords.
- GitHub Copilot's official SDK guidance uses GitHub OAuth so each user is billed through their own Copilot subscription and the app does not need to handle model API keys directly.
- OpenCode's public source shows a Copilot compatibility path that exchanges a GitHub OAuth token for a Copilot API token, then talks to the Copilot OpenAI-compatible endpoint. Treat this as implementation reference, not a guarantee of stable public API behavior.

## Design constraints

- Do not hardcode user-specific credentials, local model names, or subscription state.
- Prefer official provider auth flows and documented SDKs where available.
- Store secrets in one auth layer, not scattered provider config fields.
- Make endpoint URLs and provider behavior configurable so private/internal endpoints can change without code edits.
- Keep Ollama provider behavior unchanged and available without login.
- Never commit auth files, access tokens, refresh tokens, or browser callback secrets.

## Proposed file structure

```text
src/auth/
  mod.rs
  store.rs          # local credential read/write, permissions, schema migrations
  browser.rs        # open browser/device-flow helpers
  openai.rs         # OpenAI/ChatGPT auth strategy
  copilot.rs        # GitHub Copilot auth strategy

src/providers/
  mod.rs
  ollama.rs
  openai.rs         # API-key OpenAI provider remains supported
  openai_account.rs # subscription/account-backed provider when allowed
  copilot.rs
```

## Implementation phases

### Phase 1 — Auth foundation

- [x] Add an `AuthStore` abstraction with provider-scoped records.
- [x] Store credentials under the platform data/config directory, for example `~/.local/share/artui/auth.json` on Linux.
- [x] Create files with owner-only permissions on Unix.
- [x] Add config for auth storage path override for tests and advanced users.
- [x] Add `/login`, `/logout`, and `/providers` slash commands.
- [x] Add redacted provider status in the TUI, e.g. `connected`, `expired`, `not connected`.
- [x] Add tests for auth-file serialization, missing files, corrupt files, and secret redaction.

### Phase 2 — Provider registry

- [x] Introduce a provider registry that can merge Ollama, API-key providers, and account-backed providers.
- [x] Extend `/model` so provider/model pairs are displayed without hardcoding local machines.
- [x] Add provider metadata: `id`, display name, auth requirement, model list strategy, and streaming capability.
- [x] Keep existing `default_provider` and `default_model` config compatible.

### Phase 3 — GitHub Copilot provider

- [x] Prefer official GitHub OAuth / Copilot SDK guidance for user authentication.
- [x] Support token discovery from environment variables only as explicit fallback, e.g. `GITHUB_TOKEN` / `GH_TOKEN` when documented.
- [x] If using a Copilot-token exchange compatibility path, make the token endpoint and API base URL configurable.
- [ ] Refresh Copilot access tokens before expiry and retry once on `401`.
- [x] Fetch or configure available Copilot models dynamically when possible; avoid freezing a long hardcoded list in core UI.
- [x] Route Copilot models by discovered endpoint metadata, including OpenAI-compatible chat, Responses API, and Anthropic-compatible messages paths.
- [x] Filter Copilot model picker results using backend picker flags and disabled policy state.
- [x] Add unit coverage for Copilot stream parsing, API routing heuristics, model filtering, and endpoint validation.
- [ ] Add integration tests with mocked token exchange, model listing, streaming, expiry, and refresh failure.
- [ ] Persist token exchange expiry metadata and avoid exchanging a GitHub token on every Copilot request when a session token is still valid.
- [ ] Retry Copilot requests once with a fresh session token after `401` or an expired Copilot session token response.
- [ ] Surface model-specific capability hints in `/model`, such as messages/responses routing, context window, vision, and reasoning support.
- [ ] Add a configurable Copilot request timeout for model discovery and streaming setup.

### Phase 4 — OpenAI account-backed provider

- [x] First keep official OpenAI API-key support as the stable path.
- [x] Only add ChatGPT subscription OAuth if there is an official documented flow or permitted integration path for third-party apps.
- [ ] If an account-backed flow is added, keep token storage, refresh, and logout in the shared auth layer.
- [ ] Do not reuse or scrape another tool's private auth tokens.
- [ ] Add clear UX copy distinguishing API billing from ChatGPT subscription/account usage.

### Phase 5 — UX polish

- [ ] Add `/login <provider>` and `/logout <provider>` command completions.
- [x] Show connected providers in `/login` using the same centered popup style as `/model` and `/theme`.
- [x] Add actionable error messages for expired auth, missing browser, denied OAuth, and unsupported subscription provider.
- [ ] Add docs for setup, logout, and how to delete local auth state manually.
- [ ] Show Copilot entitlement or model-unavailable errors with a direct hint to refresh `/model` and check GitHub Copilot plan settings.
- [ ] Add a manual `/model refresh` command for account-backed providers.
- [ ] Add statusline copy that distinguishes API-key providers from account-backed subscription providers.

## Security checklist

- [x] Auth files are ignored by git and never logged.
- [x] Tokens are redacted from errors, debug logs, and panic output.
- [x] Logout removes provider tokens and cached derived tokens.
- [x] Refresh-token writes are atomic to avoid corrupting auth state.
- [x] Tests verify permissions on Unix where possible.
- [x] Provider endpoints are validated and not silently redirected to untrusted hosts.

## References

- OpenCode providers: https://opencode.ai/docs/providers/
- GitHub Copilot + OpenCode announcement: https://github.blog/changelog/2026-01-16-github-copilot-now-supports-opencode/
- Pi Agent providers: https://pi.dev/docs/latest/providers
- OpenAI Codex auth: https://developers.openai.com/codex/auth
- OpenAI Codex CLI reference: https://developers.openai.com/codex/cli/reference
- GitHub Copilot SDK OAuth setup: https://docs.github.com/en/copilot/how-tos/copilot-sdk/set-up-copilot-sdk/github-oauth
- GitHub Copilot SDK authentication: https://docs.github.com/en/copilot/how-tos/copilot-sdk/authenticate-copilot-sdk/authenticate-copilot-sdk
