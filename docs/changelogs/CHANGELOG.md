# Changelog

All notable changes to artui will be documented in this file.

## 2026-05-21

### Added

- Added `ModelEvent` tool-call arms: `ToolCallStart`, `ToolCallArgsDelta`, `ToolCallEnd`, `ReasoningDelta`, `Usage` for streaming tool-call support from any provider.
- Added `ToolSpec`, `ToolChoice`, and `ToolCall` types to the provider protocol.
- Added per-provider tool serialization (`tool_serialization.rs`) for OpenAI Chat, OpenAI Responses, Anthropic Messages, and Ollama formats.
- Added full OpenAI-compatible SSE streaming implementation with tool-call parsing in `openai_compat.rs`.
- Added `Tool` trait, `ToolContext`, `ToolResult` types in `src/tools/mod.rs`.
- Added `ToolRegistry` with dispatch-by-name and spec collection for `ModelRequest.tools`.
- Added `read_file` tool with line numbers, line range, path traversal rejection, binary detection, and output truncation.
- Added `glob` tool using `ignore::WalkBuilder` with .gitignore respect and max_results cap.
- Added `search` tool wrapping ripgrep with case sensitivity, file glob filter, context lines, and output caps.
- Added `apply_patch` tool implementing V4A patch format (Add/Delete/Update File operations) with atomic rollback, fuzzy hunk matching, and context-aware error messages.
- Added `shell` tool with command classifier (denies sudo, rm -rf /, curl-pipe injection patterns), output caps (30k chars), timeout support, stderr capture, and kill_on_drop.
- Added `PermissionEngine` scaffold classifying read-only tools as Allow and write tools as Ask.
- Added `agent::loop::run_turn` — multi-step agent loop that streams model responses, collects tool calls, dispatches them, feeds results back, and iterates up to `max_steps_per_turn` (25).
- Added `CancellationToken` (tokio-util) threading through the agent loop for clean Esc cancellation.
- Added `tokio-util`, `ignore`, `glob`, `which` dependencies; added `process` feature to tokio.
- Added `tempfile` dev-dependency for tool tests.

### Changed

- Renamed `ModelEvent::Token` to `ModelEvent::TextDelta` across all providers.
- Changed `ModelEvent::Done` to `ModelEvent::Done { end_turn: bool }` for codex-compatible semantics.
- Extended `ModelRequest` with `tools`, `tool_choice`, and `max_output_tokens` fields.
- Changed `spawn_app_request` to route provider requests through the agent loop instead of direct `stream_turn`.
- Wired `ToolRegistry` into `App` so `ModelRequest.tools` is populated from registered tools on every turn.
- Added `SessionStore` (rusqlite, WAL mode, ULID keys, 0o600 perms) with sessions, messages, and memory tables. Supports create/list/resume/delete sessions, append/load messages, flag interrupted tool calls, and scoped memory CRUD.
- Added `agent::compaction` module with token estimation (chars/4), compaction threshold (0.835), and `compact_messages` that summarizes oldest 60% of context via a dedicated compaction sub-prompt.
- Added `PermissionEngine` agent-aware classification: read-only tools always allowed, write tools (apply_patch, shell) allowed in Build mode, denied in Plan mode.
- Added `src/sandbox/mod.rs` with bubblewrap (`bwrap`) command builder: read-only system mounts, writable workspace, optional network isolation, die-with-parent, graceful fallback when bwrap unavailable.
- Added `task` subagent tool that spawns isolated child agent loops with `explore` (read-only) or `general` (full minus task) tool sets. Prevents recursion by excluding task tool from subagent registries.
- Added `rusqlite` (bundled) and `ulid` dependencies.
- Added zero-config Copilot OAuth with hard-coded artui client_id (`Ov23liSsh5cnZv6yAz4X`) — no user OAuth App registration needed.
- Added VSCode-shaped Copilot API headers (`vscode/1.99.2`, `copilot-chat/0.26.3`, `Copilot-Integration-Id: vscode-chat`).
- Added `L` (login) and `D` (disconnect) keybinds to the `/model` picker for inline provider management.
- Added token source labels to `/login` picker (e.g. "connected via github-device-flow").
- Added `@file` mentions in chat input — type `@path/to/file` to attach file content on submit.
- Added bracketed paste support with smart content handling: large pastes (>5 lines) show as `[Pasted text #N +M lines]` tag, short pastes inline directly.
- Added `[Image #N]` tag display for pasted images (stored for future multimodal API support).
- Added Windows PowerShell support to shell tool: prefers `pwsh` (PS7+) → `powershell.exe` → `cmd.exe /C` fallback.

### Fixed

- Fixed Copilot login on student/free plans: use OAuth token directly when `/copilot_internal/v2/token` exchange returns 404 (matches opencode's approach).
- Fixed macOS CI failures: canonicalize workspace_root before path traversal comparison (resolves `/private` symlink).
- Fixed login picker text overlap: truncated long provider names, shortened status labels to "ready"/"key needed"/"sign in needed".
- Fixed `/logout copilot` not clearing cached models from `/model` picker.
- Fixed Copilot `is_connected` detection after adding token source labels.
- Removed silent PAT (`ghp_`) fallback — now rejects with friendly error pointing to `/login copilot`.

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
- Added Copilot session-token caching with expiry metadata, once-per-request `401` refresh retry, and configurable request timeouts.
- Added `/model refresh` for manual GitHub Copilot model discovery refresh.
- Added stronger artui identity prompts with exact active provider/model disclosure.
- Added `/model` capability hints from Copilot endpoint metadata.
- Added Shift+Tab reasoning effort control with provider request wiring for OpenAI-compatible Copilot routes.
- Added provider usage statusline item with Copilot premium/free quota caching, Ollama local/cloud labels, and API/account fallbacks.
- Changed the bottom status UX to a compact prompt-frame style with context/provider name in the left title, model/reasoning in the right title, and workspace/git state below.
- Added color-coded reasoning effort in the prompt title and prompt border: auto muted, low green, medium yellow, high pink/bold, xhigh purple/bold.
- Made Shift+Tab reasoning cycling provider/model-aware so unsupported efforts are skipped and xhigh is only offered for extended reasoning model families.
- Stabilized GitHub Copilot model refresh by selecting the richest available Copilot credential, then keeping model discovery, endpoint metadata, and usage tied to that same credential instead of mixing sources.
- Persisted Copilot endpoint-advertised reasoning effort metadata so model-specific low/medium/high/xhigh support can be honored when available.
- Fixed context usage display to estimate tokens against the active provider/model context window instead of comparing chat characters to the tool-output character limit; sub-1% usage now displays as `ctx 0%`.
- Changed GitHub Copilot HTTP 429 display to a neutral provider rate-limit message.
- Replaced prompt-title provider usage/quota text with the provider display name, e.g. `ctx 0% · GitHub Copilot`.
- Added Copilot endpoint context-window metadata support with provider/model fallback windows for local, OpenAI-compatible, account, and Copilot models.
- Expanded `/model` popup height and model discovery parsing so more plan-available Copilot models are visible.
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
