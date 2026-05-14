use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::{
    app::{App, UiMode},
    ui::layout::theme,
};

pub fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let mode_color = match app.mode {
        UiMode::Streaming => theme::ACCENT,
        UiMode::Input => theme::GREEN,
        UiMode::Normal => theme::MUTED,
    };

    let lines = vec![
        section("session"),
        kv("provider", app.config.default_provider.as_str()),
        kv("mode", &format!("{:?}", app.mode)),
        Line::from(vec![
            Span::styled("status   ", Style::default().fg(theme::SUBTLE)),
            Span::styled(app.status.as_str(), Style::default().fg(mode_color)),
        ]),
        Line::from(""),
        section("agent limits"),
        kv("steps", &app.config.agent.max_steps_per_turn.to_string()),
        kv("patches", &app.config.agent.max_patch_retries.to_string()),
        kv("shell", &app.config.agent.max_shell_retries.to_string()),
        Line::from(""),
        section("keys"),
        key("enter", "send"),
        key("esc", "clear input"),
        key("ctrl+l", "clear chat"),
        key("ctrl+c", "quit"),
    ];

    let paragraph = Paragraph::new(lines).style(Style::default().fg(theme::TEXT).bg(theme::BG));
    frame.render_widget(paragraph, area);
}

fn section(label: &'static str) -> Line<'static> {
    Line::from(Span::styled(
        label,
        Style::default()
            .fg(theme::MUTED)
            .add_modifier(Modifier::BOLD),
    ))
}

fn kv(label: &'static str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<9}"), Style::default().fg(theme::SUBTLE)),
        Span::styled(value.to_owned(), Style::default().fg(theme::TEXT)),
    ])
}

fn key(key: &'static str, action: &'static str) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<9}"), Style::default().fg(theme::ACCENT)),
        Span::styled(action, Style::default().fg(theme::TEXT)),
    ])
}
