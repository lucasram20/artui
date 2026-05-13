use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, Role};

pub fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mut lines = Vec::new();

    for message in &app.transcript {
        let label = match message.role {
            Role::User => Span::styled(
                "User: ",
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
            Role::Assistant => Span::styled(
                "Assistant: ",
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD),
            ),
        };

        lines.push(Line::from(vec![label, Span::raw(message.content.as_str())]));
        lines.push(Line::from(""));
    }

    if lines.is_empty() {
        lines.push(Line::from("Type a prompt and press Enter."));
    }

    let paragraph = Paragraph::new(lines)
        .block(
            Block::default()
                .title("Chat + Tool Timeline")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}
