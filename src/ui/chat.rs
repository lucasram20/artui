use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

use crate::{
    app::{App, Role},
    ui::layout::theme,
};

pub fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut lines = Vec::new();

    for message in &app.transcript {
        let (marker, color) = match message.role {
            Role::User => ("›", theme::ACCENT),
            Role::Assistant => ("•", theme::GREEN),
        };

        let label = Span::styled(
            format!("{marker} "),
            Style::default().fg(color).add_modifier(Modifier::BOLD),
        );

        let content = if message.content.is_empty() {
            Span::styled("thinking...", Style::default().fg(theme::SUBTLE))
        } else {
            Span::styled(message.content.as_str(), Style::default().fg(theme::TEXT))
        };

        lines.push(Line::from(vec![label, content]).style(Style::default().fg(theme::TEXT)));
        lines.push(Line::from(""));
    }

    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(theme::TEXT).bg(theme::BG))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}
