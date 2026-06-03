//! Prompt input line wrapping and cursor geometry (no Ratatui draw calls).

use ratatui::{
    style::Style,
    text::{Line, Span},
};

use crate::app::ThemeId;

use super::layout::theme;

/// Visual line count for the composer at `text_width` (after the prompt prefix).
pub fn input_line_count(input: &str, text_width: usize) -> usize {
    if input.is_empty() {
        return 1;
    }

    input
        .split('\n')
        .map(|line| {
            let len = line.chars().count();
            len.max(1).div_ceil(text_width)
        })
        .sum::<usize>()
}

/// Cursor row/column within the wrapped composer (0-based).
pub fn input_cursor(input: &str, text_width: usize) -> (u16, u16) {
    let mut row = 0;
    let mut col = 0;
    for (i, line) in input.split('\n').enumerate() {
        if i > 0 {
            row += 1;
            col = 0;
        }
        let len = line.chars().count();
        if len == 0 {
            continue;
        }
        row += (len / text_width) as u16;
        col = (len % text_width) as u16;
    }
    (row, col)
}

/// Wrapped composer lines including the `›` / `…` prompt prefix on the first row.
pub fn wrapped_input_lines<'a>(
    prompt: &'a str,
    input: &'a str,
    text_width: usize,
    theme: ThemeId,
) -> Vec<Line<'a>> {
    let palette = theme::palette(theme);
    let mut lines = Vec::new();
    for (logical_index, line) in input.split('\n').enumerate() {
        let chars = line.chars().collect::<Vec<_>>();
        if chars.is_empty() {
            let prefix = if logical_index == 0 {
                Span::styled(format!("{prompt} "), Style::default().fg(palette.accent))
            } else {
                Span::raw(" ".repeat(prompt.chars().count() + 1))
            };
            lines.push(Line::from(vec![prefix]));
            continue;
        }

        for (chunk_index, chunk) in chars.chunks(text_width).enumerate() {
            let prefix = if lines.is_empty() {
                Span::styled(format!("{prompt} "), Style::default().fg(palette.accent))
            } else if chunk_index == 0 && logical_index == 0 {
                Span::raw(String::new())
            } else {
                Span::raw(" ".repeat(prompt.chars().count() + 1))
            };
            let content = chunk.iter().collect::<String>();
            lines.push(Line::from(vec![
                prefix,
                Span::styled(content, Style::default().fg(palette.text)),
            ]));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::ThemeId;

    #[test]
    fn empty_input_is_one_line() {
        assert_eq!(input_line_count("", 40), 1);
    }

    #[test]
    fn long_line_wraps_at_text_width() {
        let input = "a".repeat(85);
        assert_eq!(input_line_count(&input, 40), 3);
    }

    #[test]
    fn multiline_input_sums_logical_lines() {
        assert_eq!(input_line_count("short\nsecond line here", 10), 3);
    }

    #[test]
    fn cursor_at_end_of_wrapped_line() {
        let input = "a".repeat(41);
        let (row, col) = input_cursor(&input, 40);
        assert_eq!(row, 1);
        assert_eq!(col, 1);
    }

    #[test]
    fn wrapped_lines_include_prompt_on_first_row() {
        let lines = wrapped_input_lines("›", "hello", 40, ThemeId::MonokaiBlue);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans[0].content.contains('›'));
    }
}
