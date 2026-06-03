# Authentication & credentials

artui talks to multiple providers; each one stores its credentials a little
differently. This page tells you exactly where they live, how to remove them,
and how to distinguish API-key providers from subscription/OAuth providers.

## Where credentials live

| Layer | Linux / macOS path | Windows path |
|---|---|---|
| artui auth store (OAuth, refresh tokens) | `~/.local/share/artui/auth.json` | `%LOCALAPPDATA%\artui\auth.json` |
| Copilot.vim / VS Code Copilot fallback | `~/.config/github-copilot/{hosts,apps}.json` | same path under `%APPDATA%` |
| Environment variables | shell rc / launchd / SSM | system or user env |

Override the artui store path via `auth_storage_path` in `~/.config/artui/config.toml`.

The artui store file is created with mode `0o600` on Unix. Permissions are
checked by `cargo test`'s `saved_auth_file_is_owner_only` integration test.

## Provider taxonomy

| Provider | Auth shape | Statusline label | Notes |
|---|---|---|---|
| `ollama` (default) | none (local) or `OLLAMA_API_KEY` | `local` / `gateway` | Local or remote Ollama. |
| `openai_compat` | `OPENAI_OAUTH_TOKEN` ▸ `OPENAI_API_KEY` | `api-key` | Bring-your-own key. |
| `anthropic` | `ANTHROPIC_OAUTH_TOKEN` ▸ `ANTHROPIC_API_KEY` | `api-key` | Same precedence as Pi. |
| `copilot` | GitHub OAuth device flow → Copilot session token | `subscription` | Linked to a Copilot Pro/Business plan. |
| `openai_account` | ChatGPT account OAuth (when supported) | `subscription` | Tied to ChatGPT Plus/Pro. |

Subscription providers are billed through the user's plan; API-key providers
bill the key holder per request. The `/login` picker prefixes subscription
providers with `★`.

## Removing credentials

### From inside artui

```text
/logout copilot
/logout openai_compat
/logout openai_account
```

This removes the provider's record from `auth.json` and clears any cached
session tokens.

### Manually (when you cannot launch artui)

```bash
# Linux / macOS — show what's stored, then trim by hand:
jq . ~/.local/share/artui/auth.json

# Wipe a single provider:
jq 'del(.records.copilot)' ~/.local/share/artui/auth.json | sponge ~/.local/share/artui/auth.json

# Or delete the whole store (you'll have to log in again):
rm ~/.local/share/artui/auth.json
```

```powershell
# Windows
Remove-Item "$env:LOCALAPPDATA\artui\auth.json"
```

To revoke the upstream credential too:

- **GitHub Copilot** → <https://github.com/settings/applications> → revoke artui's OAuth grant.
- **Anthropic** → <https://console.anthropic.com/settings/keys>.
- **OpenAI** → <https://platform.openai.com/api-keys>.

## CLI flags

```text
--copilot-vscode-compat        # use VSCode public OAuth client (Iv1.b507a08c87ecfe98)
--copilot-client-id <id>       # arbitrary GitHub OAuth client_id (GH Enterprise)
```

Both are useful when artui's bundled client_id is rate-limited or your
organization runs GitHub Enterprise.

## Quick error decoder

| Error | Likely cause | Fix |
|---|---|---|
| `Personal Access Tokens (ghp_) are not supported…` | You pasted a PAT into the auth store. Copilot's `/chat/completions` endpoint refuses PATs. | Run `/login copilot` to get an OAuth token, or remove the `ghp_` value from `auth.json`. |
| `GitHub Copilot is not connected` | No record in `auth.json`, no `GH_TOKEN`/`GITHUB_TOKEN` env, and no `gh auth token` available. | Run `/login copilot` once. |
| `GitHub Copilot returned HTTP 429 rate limit` | Either your Copilot plan rate-limited you (5-hour session quota) or GitHub throttled the OAuth client. | Wait a few minutes, or try `--copilot-vscode-compat`. |
| `unsupported_api_for_model` | The model expects `/responses` or `/v1/messages` but artui sent `/chat/completions`. | Refresh: `/model refresh`. The endpoint metadata is now cached in the auth store. |
| `GitHub Copilot models are not available yet` | Discovery hasn't run for this account. | `/login copilot` again, or `/model refresh`. |
