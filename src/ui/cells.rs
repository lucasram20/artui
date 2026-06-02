//! Terminal display-cell width helpers (Unicode-aware).

use ratatui::text::Span;
use unicode_width::UnicodeWidthChar;
use unicode_width::UnicodeWidthStr;

pub fn display_width(value: &str) -> usize {
    UnicodeWidthStr::width(value)
}

pub fn trim_to_width(value: &str, width: usize) -> String {
    if display_width(value) <= width {
        return value.to_owned();
    }
    if width <= 1 {
        return "…".to_owned();
    }

    let mut output = String::new();
    let mut used = 0usize;
    let budget = width.saturating_sub(1);
    for ch in value.chars() {
        let ch_width = ch.width().unwrap_or(0);
        if used + ch_width > budget {
            break;
        }
        output.push(ch);
        used += ch_width;
    }
    output.push('…');
    output
}

pub fn spans_display_width(spans: &[Span<'_>]) -> usize {
    spans
        .iter()
        .map(|span| display_width(span.content.as_ref()))
        .sum()
}

/// Drop trailing spans until the line fits `budget` display cells.
pub fn fit_spans_to_width(mut spans: Vec<Span<'static>>, budget: usize) -> Vec<Span<'static>> {
    while spans_display_width(&spans) > budget && !spans.is_empty() {
        spans.pop();
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_respects_wide_glyphs() {
        let trimmed = trim_to_width("█░ctx", 3);
        assert!(display_width(&trimmed) <= 3);
    }
}
