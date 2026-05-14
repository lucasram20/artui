# Changelog

All notable changes to artui will be documented in this file.

## 2026-05-14

### Added

- Added `src/assets/artui.png`, `src/assets/artui-ascii.png`, and `src/assets/artui.txt` as source logo assets.
- Added PNG logo rendering through `ratatui-image`; the app now loads `src/assets/artui.png`, crops the mark, removes edge background pixels, tints it to the Monokai accent, and encodes it once as an `18x6` terminal image protocol.
- Added dynamic footer context usage display, git branch/status text, and compact model/project status in the main TUI.

### Changed

- Reworked the TUI into a minimal Claude Code-inspired layout with a bordered welcome header, centered logo area, tips/dev-notes panel, conversation-first body, Claude-style prompt row, and footer below the input.
- Changed the chat input so it sits directly below the conversation, expands with multi-line input/history growth, and avoids duplicated separator lines and cursor misalignment.
- Replaced static placeholder text with live provider/model/project/git/status values where available.
- Updated the visual theme to a Monokai palette for background, borders, text, accents, message markers, and status elements.
- Changed transcript rendering to show only real messages and style user/assistant markers with the theme accent colors.
- Set the crate version to `0.0.1`.
- Upgraded `ratatui` to `0.30` and added `image` plus `ratatui-image` with default `chafa` support disabled to avoid requiring `libchafa-dev`.

### Fixed

- Fixed the broken `include_str!("../assets/artui.txt")` logo path by moving away from compile-time ASCII logo rendering.
- Fixed oversized/noisy ASCII logo rendering by replacing it with a constrained PNG render path.
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
