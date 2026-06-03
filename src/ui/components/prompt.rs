//! Composer / prompt input surface (multiline wrap, cursor, placeholder).

use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::{App, ThemeId, UiMode};

use super::super::{composer, layout::theme, statusline};

/// Reserved composer height for layout (border + up to 6 input lines).
pub fn height(app: &App, width: u16) -> u16 {
    let prompt_width = 2usize;
    let text_width = width.saturating_sub(prompt_width as u16).max(1) as usize;
    (composer::input_line_count(app.input.as_str(), text_width)
        .clamp(1, 6) as u16)
        .saturating_add(2)
}

pub fn draw(frame: &mut Frame<'_>, app: &App, theme: ThemeId, area: Rect) {
    let prompt = if app.mode == UiMode::Streaming {
        "…"
    } else {
        "›"
    };
    let prompt_width = prompt.chars().count() + 1;
    let text_width = area.width.saturating_sub(prompt_width as u16).max(1) as usize;
    let input_lines = composer::input_line_count(app.input.as_str(), text_width).clamp(1, 6) as u16;
    let input_area = Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width: area.width.saturating_sub(2),
        height: input_lines.min(area.height.saturating_sub(2)),
    };
    let palette = theme::palette(theme);
    let lines = if app.input.is_empty() {
        vec![Line::from(vec![
            Span::styled(format!("{prompt} "), Style::default().fg(palette.accent)),
            Span::styled("Ask artui anything...", Style::default().fg(palette.subtle)),
        ])]
    } else {
        composer::wrapped_input_lines(prompt, app.input.as_str(), text_width, theme)
    };

    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(statusline::reasoning_effort_color(
                palette,
                app.reasoning_effort,
            )))
            .style(Style::default().bg(palette.bg)),
        area,
    );
    statusline::draw_input_titles(frame, app, theme, area);
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(palette.bg)),
        input_area,
    );

    if input_area.height > 0
        && !app.theme_picker_open
        && !app.model_picker_open
        && !app.login_picker_open
        && !app.statusline_open
        && !app.agent_picker_open
    {
        let (cursor_row, cursor_col) = composer::input_cursor(app.input.as_str(), text_width);
        let cursor_x = input_area.x + prompt_width as u16 + cursor_col;
        let cursor_y = input_area.y + cursor_row.min(input_area.height.saturating_sub(1));
        frame.set_cursor_position((cursor_x.min(input_area.right().saturating_sub(1)), cursor_y));
    }
}