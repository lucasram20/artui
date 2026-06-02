//! Platform defaults for TUI animation, glyphs, and effects.

/// Prefer ASCII-safe glyphs (Windows conhost / legacy PowerShell).
pub fn use_legacy_glyphs() -> bool {
    #[cfg(windows)]
    {
        !std::env::var_os("ARTUI_UNICODE").is_some_and(|value| !value.is_empty())
    }
    #[cfg(not(windows))]
    {
        false
    }
}

pub fn ui_effects_enabled() -> bool {
    #[cfg(windows)]
    {
        std::env::var_os("ARTUI_EFFECTS").is_some_and(|value| !value.is_empty())
    }
    #[cfg(not(windows))]
    {
        true
    }
}

pub fn animation_poll_ms() -> u64 {
    if use_legacy_glyphs() {
        83
    } else {
        25
    }
}

pub fn idle_poll_ms() -> u64 {
    200
}

pub fn context_bar_fill(filled: usize) -> String {
    if use_legacy_glyphs() {
        "#".repeat(filled)
    } else {
        "█".repeat(filled)
    }
}

pub fn context_bar_empty(empty: usize) -> String {
    if use_legacy_glyphs() {
        "-".repeat(empty)
    } else {
        "░".repeat(empty)
    }
}
