//! Tool approval modal rendering.

use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
    Frame,
};

use crate::{app::App, ui::layout::theme};

use super::super::geometry;

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let Some(prompt) = app.pending_approval.as_ref() else {
        return;
    };
    let palette = theme::palette(app.theme);
    let area = frame.area();
    let dialog_w = (area.width.saturating_mul(80) / 100).clamp(50, 120);
    let dialog_h = (area.height.saturating_mul(70) / 100).clamp(10, 40);
    let dialog = geometry::selector_area(area, dialog_w, dialog_h);
    frame.render_widget(Clear, dialog);

    let layout = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(dialog);

    let header = Paragraph::new(Line::from(vec![
        Span::styled(
            format!("⚠ Approve {} ", prompt.tool_name),
            Style::default()
                .fg(palette.yellow)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("(call_id {})", prompt.call_id),
            Style::default().fg(palette.muted),
        ),
    ]))
    .alignment(Alignment::Left);

    let body_lines: Vec<Line> = prompt
        .body
        .lines()
        .take(dialog.height.saturating_sub(8) as usize)
        .map(|line| {
            let style = if line.starts_with('+') {
                Style::default().fg(palette.green)
            } else if line.starts_with('-') {
                Style::default().fg(palette.pink)
            } else if line.starts_with("@@") {
                Style::default().fg(palette.accent)
            } else {
                Style::default().fg(palette.text)
            };
            Line::from(Span::styled(line.to_owned(), style))
        })
        .collect();
    let body = Paragraph::new(body_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(palette.yellow))
                .title(prompt.title.clone()),
        )
        .style(Style::default().fg(palette.text).bg(palette.bg));

    let footer = Paragraph::new(Line::from(vec![
        Span::styled(
            "[a]",
            Style::default()
                .fg(palette.green)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" allow once  "),
        Span::styled(
            "[s]",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" allow session  "),
        Span::styled(
            "[d / Esc]",
            Style::default()
                .fg(palette.pink)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" deny"),
    ]))
    .alignment(Alignment::Center)
    .style(Style::default().fg(palette.muted));

    frame.render_widget(header, layout[0]);
    frame.render_widget(body, layout[1]);
    frame.render_widget(footer, layout[2]);
}
