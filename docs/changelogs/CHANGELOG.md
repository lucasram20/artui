# Changelog

All notable changes to artui will be documented in this file.

## 2026-05-14

### Added

- Added the `/theme` command with an overlay selector popup for switching palettes at runtime.
- Added built-in themes inspired by Monokai Blue, Tokyo Night, Catppuccin Mocha, Gruvbox, Nord, and Dracula.
- Added dynamic footer context usage display, git branch/status text, and compact model/project status in the main TUI.

### Changed

- Reworked the TUI into a minimal Claude Code-inspired layout with a bordered welcome header, centered logo area, tips/dev-notes panel, conversation-first body, Claude-style prompt row, and footer below the input.
- Changed the chat input so it sits directly below the conversation, expands with multi-line input/history growth, and avoids duplicated separator lines and cursor misalignment.
- Replaced static placeholder text with live provider/model/project/git/status values where available.
- Changed the theme system to render from the selected runtime palette across the main layout, chat, tools panel, and popup UI.
- Changed the default theme to keep a dark Monokai-like base with a warm blue accent instead of warm orange.
- Changed the header logo from image rendering to a slim static terminal glyph and tightened responsive logo/status spacing.
- Changed transcript rendering to show only real messages and style user/assistant markers with the selected theme colors.
- Set the crate version to `0.0.1`.
- Upgraded `ratatui` to `0.30`.

### Removed

- Removed PNG terminal image rendering and the `image`/`ratatui-image` dependencies.
- Removed obsolete ASCII/PNG source logo assets that are no longer used by the TUI.

### Fixed

- Fixed the broken `include_str!("../assets/artui.txt")` logo path by moving away from compile-time ASCII logo rendering.
- Fixed oversized/noisy logo rendering by replacing it with a constrained static glyph.
- Fixed the prompt arrow spacing, input cursor placement, and footer placement regressions from the layout iterations.

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

- Updated the default Ollama model to `gemma4:e2b`.
- Included Ollama HTTP status and response body details in provider errors.
