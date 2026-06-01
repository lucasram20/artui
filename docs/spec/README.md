# Specs

This folder contains product and implementation specifications for artui.

## Current Specs

- [artui v1 agentic spec](artui_v1_agentic_spec.md) — v1 milestone cut for the Rust + ratatui coding agent TUI (product spec; what to build).
- [harness architecture](harness-architecture.md) — implementation blueprint; gap analysis vs current code; module decomposition; build order. **Start here for implementation.**
- [codex architecture](codex-architecture.md) — file:line audit of OpenAI codex-rs harness patterns we can port.
- [opencode architecture](opencode-architecture.md) — file:line audit of sst/opencode harness patterns we can port.
- [session persistence](session-persistence.md) — SQLite schema + `SessionStore` API for resumable sessions and per-workspace memory.
- [copilot OAuth](copilot-oauth.md) — zero-config Copilot device flow; replaces the current PAT-paste friction.
- [LSP support](lsp.md) — four-phase (N1–N4) Language Server Protocol roadmap: async-lsp client, vendored server registry, and the `lsp` tool. Shipped.

## Reading order for new contributors

1. `artui_v1_agentic_spec.md` — what artui is meant to be.
2. `harness-architecture.md` — current vs target architecture, build phases A–L.
3. `codex-architecture.md` and `opencode-architecture.md` — when implementing a phase, read the corresponding section in the reference impls first.
4. `session-persistence.md` — phase G.
5. `copilot-oauth.md` — phase A.5 (parallel with provider tool-call work).

The corresponding implementation tickets are archived in `../archive/todos/`.
