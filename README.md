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
# Linux / macOS — interactive, asks before installing
curl -fsSL https://raw.githubusercontent.com/lucasram20/artui/main/scripts/install.sh | sh

# Linux / macOS — non-interactive (CI, scripts, opt-in upgrades)
curl -fsSL https://raw.githubusercontent.com/lucasram20/artui/main/scripts/install.sh | sh -s -- --yes

# Windows (PowerShell)
irm https://raw.githubusercontent.com/lucasram20/artui/main/scripts/install.ps1 | iex
# Non-interactive PowerShell
irm https://raw.githubusercontent.com/lucasram20/artui/main/scripts/install.ps1 | iex -ArgumentList -Yes
```

The script prints an artui banner, asks for a one-key confirmation, then streams a real progress bar while the matching binary downloads from GitHub Releases. Installs into `~/.local/bin` (Linux/macOS) or `%LOCALAPPDATA%\artui\bin` (Windows). Set `ARTUI_INSTALL_YES=1` (or pass `--yes`/`-Yes`) to skip the prompt — type `n` at the prompt to abort cleanly without downloading anything.

Pin a specific version:

```bash
curl -fsSL https://raw.githubusercontent.com/lucasram20/artui/main/scripts/install.sh | sh -s -- --version v0.3.4 --yes
```

Build from source instead:

```bash
ARTUI_FROM_SOURCE=1 curl -fsSL https://raw.githubusercontent.com/lucasram20/artui/main/scripts/install.sh | sh -s -- --yes
```

### Install (npm)

```bash
npm install -g artui-cli           # downloads the prebuilt binary on postinstall
npm install -g artui-cli --abort   # skip the download (just install the wrapper)
ARTUI_SKIP_POSTINSTALL=1 npm install -g artui-cli   # same, env-flavoured
npx artui-cli                      # one-off run
```

The package name is `artui-cli` because the bare `artui` slot on npm is held by an unrelated 2017 package. Bin entries stay as `artui` and `art`, so commands stay unchanged.

The npm package's postinstall renders an [`ink`](https://github.com/vadimdemedes/ink)-driven progress UI on TTY (the same React-for-CLI library Claude Code uses) and falls back to plain logs in CI. The wrapper binary is the native artui release picked from GitHub Releases. Pass `--abort`/`--no-install`/`--skip-binary` to install the wrapper without fetching the binary; rebuild later with `npm rebuild artui-cli`.

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

Releases are tag-driven — no automation pushes a release on every commit. When you're ready to ship a new version:

```bash
# Bump versions in lockstep
sed -i 's/^version = ".*"/version = "0.4.0"/' Cargo.toml
(cd npm && npm version --no-git-tag-version 0.4.0)
cargo update -p artui --offline

# Commit, tag, push
git commit -am "chore(release): 0.4.0"
git tag v0.4.0
git push origin main --tags
```

`.github/workflows/release.yml` fires on the tag push, cross-compiles for Linux/macOS/Windows × `x86_64`/`aarch64`, and publishes a GitHub Release with checksums.

If a build fails (or you bumped versions but forgot to push the tag), trigger the workflow manually:

```bash
gh workflow run release.yml -f tag=v0.4.0
```

Use [Conventional Commits](https://www.conventionalcommits.org/) in your day-to-day commits if you like (`feat:`, `fix:`, `chore:`); they don't auto-trigger anything but they keep the changelog tidy and make it easy to decide the next semver bump by hand.

## Auto-update

artui polls GitHub Releases at startup and shows a banner when a meaningful update is available. The default policy follows the user's request: **only major bumps are surfaced** — patch and minor releases stay silent.

```toml
[updates]
repo          = "lucasram20/artui"   # "<owner>/<name>" to poll
notify_level  = "major"              # "off" | "major" | "minor" | "all"
auto_check    = true                  # set false to disable the network poll entirely
timeout_secs  = 5                     # GitHub API timeout
```

How upgrades happen — modeled on Claude Code, OpenCode, and Codex:

| Install path | Upgrade command |
|---|---|
| `curl \| sh` / `irm \| iex` | re-run the same one-liner; the script always pulls the latest binary from the latest release |
| `npm install -g artui-cli` | `npm install -g artui-cli@latest` (postinstall picks up the matching binary) |
| `cargo install --git` | `ARTUI_FROM_SOURCE=1 curl ... \| sh` re-runs `cargo install` |
| Source clone | `git pull && cargo install --path .` |

The install scripts are hosted on `main`, so each new push automatically updates the canonical script — but **existing installs do not auto-rewrite themselves**. The startup banner exists precisely so you don't have to remember to run the script. Set `notify_level = "off"` or `auto_check = false` if you'd rather opt out.

## Documentation

- [v1 Agentic Spec](docs/spec/artui_v1_agentic_spec.md)
- [Spec Index](docs/spec/README.md)
- [Distribution & R2 mirror setup](docs/distribution.md)
- [Authentication & credentials](docs/auth.md)
- [Changelog](docs/changelogs/CHANGELOG.md)
