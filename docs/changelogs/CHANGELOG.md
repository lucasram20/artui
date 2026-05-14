# Changelog

All notable changes to artui will be documented in this file.

## 2026-05-14

### Added

- Added a provider auth foundation with platform auth storage, redacted provider status, `/providers`, `/login`, and `/logout` commands.
- Added an official GitHub OAuth device-flow login path for `/login copilot` with configurable OAuth client ID and endpoint URLs.
- Added a `/login` provider picker popup matching the existing `/theme` and `/model` modal style.
- Added a provider metadata registry for Ollama, OpenAI-compatible APIs, GitHub Copilot, and OpenAI account-backed provider paths.
- Added GitHub Copilot token exchange, model discovery, and chat streaming support.
- Added Copilot model discovery from the active auth store, configured GitHub token environment variables, and `gh auth token`, choosing the richest working model catalog.
- Added Copilot request routing for `/chat/completions`, `/responses`, and the Anthropic-compatible `/v1/messages` shim based on discovered model endpoint metadata.
- Added Copilot stream parsing for OpenAI chat deltas, Responses API deltas, and Anthropic-style `content_block_delta` events.
- Added Copilot model endpoint metadata caching while keeping the `/model` UI grouped by simple provider/model names.
- Added OAuth provider support todos under `docs/todos/` for future OpenAI account and GitHub Copilot subscription provider work.
- Added a configurable streaming/loading animation with spinner frames and rotating catch phrases for empty assistant responses.
- Added configurable reasoning-only loading phrases that are used only when the active model matches user-defined reasoning model patterns.
- Added the short `art` binary alongside `artui` so installed builds can be launched without `cargo run`.
- Added chat transcript scrolling with ↑/↓, PageUp/PageDown, and Ctrl+U/Ctrl+D while keeping the composer/statusline fixed.
- Added a slash-command suggestion list below the chat input when typing `/`.
- Added basic built-in slash commands for `/help`, `/theme`, `/model`, `/statusline`, `/clear`, `/quit`, and `/exit`.
- Added Tab completion for the first matching slash command.
- Added `/statusline` to configure which statusline items are shown.
- Added `/model` to switch the active model from discovered provider models or by entering `/model <name>`.
- Added keyboard navigation for slash-command suggestions with ↑/↓ and Enter selection.
- Added the `/theme` command with an overlay selector popup for switching palettes at runtime.
- Added built-in themes inspired by Monokai Blue, Tokyo Night, Catppuccin Mocha, Gruvbox, Nord, and Dracula.
- Added dynamic footer context usage display, git branch/status text, and compact model/project status in the main TUI.

### Changed

- Changed provider status/login UX to use `/login` instead of a separate `/providers` slash command.
- Changed `/model` to list connected Copilot models under a GitHub Copilot provider section and switch both provider and model on selection.
- Changed Copilot's default model behavior to prefer discovered models instead of a stale hardcoded fallback.
- Changed Copilot model listing to filter hidden or disabled picker models returned by the Copilot backend.
- Changed Copilot model fetches to honor the token exchange response's API endpoint for Business/Enterprise-style routing when using the default models URL.
- Changed GPT-5/Codex-style Copilot models to use `/responses` first, while Claude-style Copilot models use `/v1/messages`.
- Changed `/theme` and `/model` modal rendering to hide the main TUI while the selector is open, leaving only the centered popup visible.
- Changed `/theme` and `/model` selectors to use a shared modal popup-window style with a backdrop and shadow inspired by OpenCode command dialogs.
- Changed chat rendering to normalize compact markdown-like model output into readable headings and bullet lines without moving the composer/statusline layout.

- Reworked the TUI into a minimal Claude Code-inspired layout with a bordered welcome header, centered logo area, tips/dev-notes panel, conversation-first body, Claude-style prompt row, and footer below the input.
- Changed the chat input so it sits directly below the conversation, expands with multi-line input/history growth, and avoids duplicated separator lines and cursor misalignment.
- Replaced static placeholder text with live provider/model/project/git/status values where available.
- Changed the theme system to render from the selected runtime palette across the main layout, chat, tools panel, and popup UI.
- Changed the default theme to keep a dark Monokai-like base with a warm blue accent instead of warm orange.
- Changed the header logo from image rendering to a slim static terminal glyph and tightened responsive logo/status spacing.
- Changed transcript rendering to show only real messages and style user/assistant markers with the selected theme colors.
- Changed the body layout so the chat stream stays in the top pane while the composer/statusline remain pinned to the bottom, even for long conversations.
- Set the crate version to `0.0.1`.
- Upgraded `ratatui` to `0.30`.

### Removed

- Removed PNG terminal image rendering and the `image`/`ratatui-image` dependencies.
- Removed obsolete ASCII/PNG source logo assets that are no longer used by the TUI.

### Fixed

- Fixed Copilot account providers previously returning placeholder "streaming is not implemented" errors.
- Fixed Copilot `/responses` stream duplication by ignoring final aggregate output text and emitting only incremental deltas.
- Fixed newer Copilot models that reject `/chat/completions` with `unsupported_api_for_model` by routing them to `/responses`.
- Fixed Claude-family Copilot models being treated as OpenAI-compatible by routing models advertising `/v1/messages` through the messages shim.
- Fixed `/model` scrolling so long Copilot model lists remain navigable.
- Fixed the global ↑/↓ handling so it scrolls chat history outside picker popups instead of changing themes accidentally.
- Fixed the broken `include_str!("../assets/artui.txt")` logo path by moving away from compile-time ASCII logo rendering.
- Fixed oversized/noisy logo rendering by replacing it with a constrained static glyph.
- Fixed the prompt arrow spacing, input cursor placement, and footer placement regressions from the layout iterations.
- Fixed slash-command suggestions so they temporarily replace the statusline and the statusline returns when suggestions close.
- Fixed long transcripts by auto-scrolling the chat pane to the newest visible content without moving the bottom composer.
- Fixed the chat/input separator so it stays attached to the bottom composer instead of appearing below the header.
- Fixed the empty-chat composer position so it starts immediately below the header with a Claude-style one-row breathing gap and gradually moves down as messages accumulate.
- Fixed the empty-chat statusline placement so it follows the composer instead of leaving a large input-looking gap.

## 2026-05-13

### Added

- Initialized the Rust crate for artui.
- Added a ratatui/crossterm TUI skeleton with transcript, input, workspace, and session panels.
- Added crash-safe terminal restoration on panic.
- Added global config loading from `~/.config/artui/config.toml` with defaults.
- Added the provider abstraction.
- Added Ollama HTTP chat streaming support.
- Added an OpenAI-compatible provider placeholder.
- Added repository README and gitignore.

### Changed

- Changed chat rendering to normalize compact markdown-like model output into readable headings and bullet lines without moving the composer/statusline layout.

- Updated the default Ollama model to `gemma4:e2b`.
- Included Ollama HTTP status and response body details in provider errors.
