<p align="center">
  <img src="src/assets/artui.svg" alt="artui logo" width="240">
</p>

<h1 align="center">artui</h1>

<p align="center">
  A Rust terminal coding-agent TUI built with ratatui.
</p>

artui is designed around explicit tool use, bounded output, approvals, diffs, and logs instead of a generic chat window.

## Quick Start

### Install (prebuilt binary)

```bash
# Linux / macOS
curl -fsSL https://raw.githubusercontent.com/lucasram20/artui/main/scripts/install.sh | sh

# Windows (PowerShell)
irm https://raw.githubusercontent.com/lucasram20/artui/main/scripts/install.ps1 | iex
```

The script detects your OS/arch (`x86_64` or `aarch64`), downloads the matching release archive from GitHub Releases, and installs the binary into `~/.local/bin` (Linux/macOS) or `%LOCALAPPDATA%\artui\bin` (Windows).

Pin a specific version:

```bash
curl -fsSL https://raw.githubusercontent.com/lucasram20/artui/main/scripts/install.sh | sh -s -- --version v0.0.1
```

Build from source instead:

```bash
ARTUI_FROM_SOURCE=1 curl -fsSL https://raw.githubusercontent.com/lucasram20/artui/main/scripts/install.sh | sh
```

### Install (npm)

```bash
npm install -g artui
# or one-off:
npx artui
```

The npm package downloads the matching native binary on `postinstall` (mirrors the `turbo` / `esbuild` pattern).

### Install (cargo, local checkout)

```bash
cargo install --path .
```

Run the TUI with either command:

```bash
art
# or
artui
```

For development without installing:

```bash
cargo run --bin artui
```

Optional local model setup:

```bash
ollama serve
ollama pull gemma4:e2b
```

By default, artui uses Ollama at `http://localhost:11434` with `gemma4:e2b`.

Useful checks:

```bash
cargo fmt --check
cargo check
cargo clippy -- -D warnings
cargo test
```

## Tech Stack

![Rust](https://img.shields.io/badge/Rust-000000?logo=rust&logoColor=white)
![Ratatui](https://img.shields.io/badge/Ratatui-Terminal%20UI-FFB000)
![Tokio](https://img.shields.io/badge/Tokio-Async-2C4F7C)
![Reqwest](https://img.shields.io/badge/Reqwest-HTTP-0088CC)
![Serde](https://img.shields.io/badge/Serde-JSON%2FTOML-3B82F6)
![Ollama](https://img.shields.io/badge/Ollama-Local%20LLM-111111)
![OpenAI Compatible](https://img.shields.io/badge/OpenAI--compatible-HTTP%20API-412991)

- **Language:** Rust 2021
- **Terminal UI:** ratatui + crossterm
- **Async runtime:** Tokio
- **HTTP streaming:** reqwest
- **Config:** TOML via serde
- **LLM providers:** Ollama first, OpenAI-compatible HTTP skeleton
- **Primary target:** Linux/Fedora first, with Windows/macOS compatibility where practical

## Configuration

Global config path:

```text
~/.config/artui/config.toml
```

Minimal example:

```toml
default_provider = "ollama"

[providers.ollama]
host = "http://localhost:11434"
default_model = "gemma4:e2b"
```

GitHub Copilot account login uses GitHub's OAuth device flow. Create a GitHub OAuth app with device flow enabled, then configure its client ID and the GitHub OAuth endpoints from the official GitHub docs:

```toml
[providers.copilot]
github_oauth_client_id = "your-oauth-app-client-id"
github_oauth_scope = ""
github_login_timeout_secs = 900
```

Then run `/login` inside artui and choose GitHub Copilot. The GitHub device-code and token URLs default to GitHub's official OAuth endpoints and can be overridden if needed. Tokens are stored in the platform data directory auth store unless `auth_storage_path` is configured. After login, artui exchanges the GitHub token for a Copilot session token when possible, fetches available Copilot models, and shows them under a GitHub Copilot section in `/model`.

## Releases

Releases are fully automated by [`semantic-release`](https://github.com/semantic-release/semantic-release) running in `.github/workflows/semantic-release.yml`. Every push to `main` is analyzed against [Conventional Commits](https://www.conventionalcommits.org/) and:

1. Computes the next semver bump (`fix:` → patch, `feat:` → minor, `BREAKING CHANGE:` → major).
2. Updates `Cargo.toml` and `docs/changelogs/CHANGELOG.md`.
3. Creates a git tag (`vX.Y.Z`) and pushes it back via the `github-actions[bot]`.
4. The tag triggers `release.yml`, which cross-compiles binaries for Linux, macOS, and Windows (`x86_64` + `aarch64`) and uploads them to the GitHub Release with checksums.

Use Conventional Commits when you push to `main`:

```
feat: add /skill picker
fix: handle empty oauth response from copilot
feat!: rename ProviderRequest fields  # major bump (BREAKING CHANGE)
chore: bump tachyonfx                 # no release
```

## Documentation

- [v1 Agentic Spec](docs/spec/artui_v1_agentic_spec.md)
- [Spec Index](docs/spec/README.md)
- [Changelog](docs/changelogs/CHANGELOG.md)
