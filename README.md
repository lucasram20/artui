<p align="center">
  <img src="src/assets/artui.svg" alt="artui logo" width="240">
</p>

<h1 align="center">artui</h1>

<p align="center">
  Interactive coding-agent CLI written in Rust. Built with ratatui.
</p>

<p align="center">
  <a href="#install">Install</a> · <a href="#configure">Configure</a> · <a href="#run">Run</a> · <a href="#documentation">Docs</a>
</p>

artui is a TUI coding agent built around explicit tool use, bounded output, approvals, diffs, and logs — not a generic chat window. Think Claude Code / Codex / OpenCode, but Rust, terminal-first, and provider-agnostic.

## Install

One-liner installers fetch the matching prebuilt binary from a public Cloudflare R2 mirror — zero GitHub auth required.

**Linux / macOS**

```bash
curl -fsSL https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev/install.sh | sh
```

**Windows (PowerShell)**

```powershell
irm https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev/install.ps1 | iex
```

**npm**

```bash
npm install -g artui-cli
```

<details>
<summary>Other install options & flags</summary>

```bash
# Skip the confirmation prompt (CI, scripts, automation)
curl -fsSL https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev/install.sh | sh -s -- --yes

# Pin a specific version
curl -fsSL https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev/install.sh | sh -s -- --version v0.3.4 --yes

# Build from source
ARTUI_FROM_SOURCE=1 curl -fsSL https://pub-5f8bc1cacf17454481c6c01145aa3e98.r2.dev/install.sh | sh -s -- --yes

# npm without downloading the binary (just the wrapper)
npm install -g artui-cli --abort

# Clone + cargo install
cargo install --path .
```

The install script prints a logo, asks `Y/n`, then streams a real progress bar. Type `n` (or close the terminal) to abort cleanly. Override the mirror with `ARTUI_MIRROR_BASE` if you self-host the artifacts.

See [`docs/distribution.md`](docs/distribution.md) for how the R2 mirror is set up.

</details>

Run the TUI with either binary:

```bash
art        # short alias
artui      # canonical name
```

## Configure

Optional. Drop a TOML config at `~/.config/artui/config.toml` to change defaults:

```toml
default_provider = "ollama"

[providers.freemodel]
# Power-user override: bypass the artui Cloudflare relay and call
# api.freemodel.dev directly. Requires FREEMODEL_API_KEY in your env.
# base_url = "https://api.freemodel.dev/v1"

[providers.ollama]
host = "http://localhost:11434"
default_model = "gemma4:e2b"

[updates]
notify_level = "major"   # off | major | minor | all
```

artui ships with built-in Freemodel, Ollama, OpenAI-compatible, and GitHub Copilot providers. First launch can use a built-in hosted provider without setup — the binary routes through a tiny Cloudflare Worker that keeps the upstream API key server-side. Sign in to Copilot from inside the TUI: `/login` → choose GitHub Copilot. See [`docs/auth.md`](docs/auth.md) for credential paths and the full provider taxonomy, and [`cloudflare/README.md`](cloudflare/README.md) for how the freemodel relay is set up if you're forking artui.

## Run

```bash
ollama serve              # optional — for local models
ollama pull gemma4:e2b
artui
```

Inside the TUI:

| Command | Effect |
|---|---|
| `/help` | List all slash commands |
| `/model` | Switch active model |
| `/agent` | Toggle Build / Plan agent mode |
| `/system` | Print the active system prompt |
| `/skill list` | Manage skill overlays (Mattpocock / `skills.sh` compatible) |
| `/mcp` | Inspect MCP server connections |
| `/login` / `/logout` | Manage provider credentials |

Universal skill paths supported: `~/.agents/skills/`, `<workspace>/.agents/skills/`, plus artui-specific `<workspace>/.artui/skills/`.

## Tech stack

![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)
![Ratatui](https://img.shields.io/badge/Ratatui-Terminal%20UI-FFB000)
![Tokio](https://img.shields.io/badge/Tokio-Async-2C4F7C)
![Reqwest](https://img.shields.io/badge/Reqwest-HTTP-0088CC)
![Ollama](https://img.shields.io/badge/Ollama-Local%20LLM-111111)
![OpenAI Compatible](https://img.shields.io/badge/OpenAI--compatible-HTTP%20API-412991)
![Cloudflare Workers](https://img.shields.io/badge/Cloudflare%20Workers-Freemodel%20relay-F38020?logo=cloudflare&logoColor=white)
![Cloudflare R2](https://img.shields.io/badge/Cloudflare%20R2-Release%20mirror-F38020?logo=cloudflare&logoColor=white)

Rust 2021 · ratatui + crossterm TUI · Tokio async · reqwest streaming · serde TOML config. Cloudflare Workers fronts the freemodel provider relay (keeps the upstream API key server-side); Cloudflare R2 hosts the install-script binaries and `latest/` pointer so the curl/PowerShell one-liners work zero-auth. Prebuilt binaries ship for **Linux / Windows on `x86_64`**. macOS users (Intel + Apple Silicon) and Linux ARM users build from source via `cargo install --git https://github.com/lucasram20/artui` — the install scripts print clear instructions when they detect an unsupported target.

## Releases

Releases are **publish-driven** — building a release requires a human to click "Publish release" on GitHub. Tag pushes alone don't trigger anything; the `release-trigger.yml` GHA workflow listens for `release: published` and POSTs to CircleCI's API to start the cross-platform build.

```bash
# 1. Bump versions in lockstep
sed -i 's/^version = ".*"/version = "0.7.0"/' Cargo.toml
(cd npm && npm version --no-git-tag-version 0.7.0)
cargo update -p artui --offline

git commit -am "chore(release): 0.7.0"
git push origin main

# 2. Tag the commit and push the tag
git tag v0.7.0
git push origin v0.7.0

# 3. Draft a release. Edit notes, attach changelog, etc.
gh release create v0.7.0 --draft --notes-from-tag

# 4. When you're ready, publish it (or click "Publish release" in the
#    GitHub Releases UI). This fires CircleCI's release pipeline:
#    Linux + Windows builds → R2 upload → artifact attach.
gh release edit v0.7.0 --draft=false
```

Pre-releases (`gh release create v0.7.0-rc1 --prerelease`) are explicitly skipped by the trigger workflow — uncheck the pre-release flag to actually fire the build. This lets you draft, edit, and validate notes without burning CI credits.

CI runs on every branch push but skips docs-only commits (anything that only touches `*.md`, `docs/**`, `npm/**`, `cloudflare/**`, `.gitignore`, etc.). Add `[ci force]` to a commit subject to override the path filter, or `[ci skip]` to skip even build-relevant changes.

The GHA `release.yml` is preserved as a `workflow_dispatch`-only fallback — trigger via the Actions tab if CircleCI is unavailable:

```bash
gh workflow run release.yml -f tag=v0.7.0
```

artui auto-checks for new versions at startup and surfaces a banner when severity meets `[updates] notify_level` (default: major bumps only). Configure or disable in `~/.config/artui/config.toml`.

| Path | Upgrade |
|---|---|
| curl / PowerShell | re-run the install one-liner |
| `npm install -g artui-cli` | `npm install -g artui-cli@latest` |
| `cargo install` | `git pull && cargo install --path .` |

## Roadmap & backlog

Tracked on the **[artui project board](https://github.com/users/lucasram20/projects/2)** — phase tickets, parking-lot items, and live infra threads with Status / Phase / Workstream / Priority / Size fields. Historical phase docs are archived under [`docs/archive/todos/`](docs/archive/todos/) for spec-style reference.

## Documentation

- [v1 agentic spec](docs/spec/artui_v1_agentic_spec.md)
- [Distribution & R2 mirror](docs/distribution.md)
- [Authentication & credentials](docs/auth.md)
- [Spec index](docs/spec/README.md)
- [Changelog](docs/changelogs/CHANGELOG.md)

## Development

```bash
cargo fmt --check
cargo check
cargo clippy -- -D warnings
cargo test
```

The freemodel relay lives in [`cloudflare/`](cloudflare/) and deploys
separately as a Cloudflare Worker — fork-friendly, free-tier compatible.
See [`cloudflare/README.md`](cloudflare/README.md) for the deploy walkthrough.

## License

MIT.
