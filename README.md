# artui

artui is a terminal-based coding agent TUI built with Rust and ratatui. It is designed as a controlled agentic loop around deterministic tools: search, read, patch, shell, test, and recover.

The goal is not to be a generic chat window. artui lets a model work inside a repository through explicit tool calls while deterministic infrastructure enforces path boundaries, output caps, approvals, diffs, and logs.

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

## Current Status

Milestone 0 foundation is implemented:

- ratatui/crossterm TUI skeleton
- input box and transcript panel
- streamed assistant text path
- global config loading from `~/.config/artui/config.toml`
- provider abstraction
- Ollama chat streaming provider
- OpenAI-compatible provider placeholder

Upcoming v1 work includes repository search/read tools, the agent loop, permission-gated patching, shell verification, session logs, and release hardening.

## How to Run

### Prerequisites

Install Rust:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

Optional for local model chat:

```bash
ollama serve
ollama pull gemma4:e2b
```

### Build and check

```bash
cargo fmt -- --check
cargo clippy -- -D warnings
cargo test
cargo check
```

### Run the TUI

```bash
cargo run
```

By default, artui tries to use Ollama at `http://localhost:11434` with `gemma4:e2b`.

## Configuration

Global config path:

```text
~/.config/artui/config.toml
```

Example:

```toml
default_provider = "ollama"

[agent]
max_steps_per_turn = 12
max_patch_retries = 2
max_shell_retries = 2
max_tool_output_chars = 30000
max_search_output_chars = 20000
max_read_file_chars = 16000

[providers.ollama]
host = "http://localhost:11434"
default_model = "gemma4:e2b"

[providers.openai_compat]
base_url = "https://api.openai.com/v1"
api_key_env = "OPENAI_API_KEY"
default_model = "gpt-4o-mini"
```

Project-level config under `.artui/config.toml` is planned but must be explicitly trusted before it can affect behavior.

## v1 Design Principles

- The model never directly touches the filesystem or shell.
- File edits should go through structured patch tools with diff previews.
- Shell commands should be classified as allow, ask, or deny.
- Search and file reads should be bounded, line-numbered, and output-capped.
- Tool activity should be visible in the TUI.
- Safety policy should override repo guidance files.

## Documentation

- [v1 Agentic Spec](docs/spec/artui_v1_agentic_spec.md)
- [Spec Index](docs/spec/README.md)
- [Changelog](docs/changelogs/CHANGELOG.md)

## License

No license has been selected yet.
