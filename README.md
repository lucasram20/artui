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

Rust 2021 · ratatui + crossterm TUI · Tokio async · reqwest streaming · serde TOML config. Targets Linux, macOS, Windows (`x86_64` and `aarch64`).

## Releases

Releases are tag-driven — no automation pushes a release on every commit. To ship:

```bash
# Bump versions in lockstep
sed -i 's/^version = ".*"/version = "0.6.0"/' Cargo.toml
(cd npm && npm version --no-git-tag-version 0.6.0)
cargo update -p artui --offline

git commit -am "chore(release): 0.6.0"
git tag v0.6.0
git push origin main --tags
```

The tag triggers `.github/workflows/release.yml`, which cross-compiles for Linux / macOS / Windows × `x86_64` / `aarch64` and uploads archives to GitHub Releases + the R2 mirror.

If a build fails or you want to rebuild an existing tag:

```bash
gh workflow run release.yml -f tag=v0.6.0
```

artui auto-checks for new versions at startup and surfaces a banner when severity meets `[updates] notify_level` (default: major bumps only). Configure or disable in `~/.config/artui/config.toml`.

| Path | Upgrade |
|---|---|
| curl / PowerShell | re-run the install one-liner |
| `npm install -g artui-cli` | `npm install -g artui-cli@latest` |
| `cargo install` | `git pull && cargo install --path .` |

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
