use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::{
    app::{App, UiMode},
    ui::layout::theme::{self, Palette},
};

pub fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let palette = theme::palette(app.theme);
    let mode_color = match app.mode {
        UiMode::Streaming => palette.accent,
        UiMode::Input => palette.green,
        UiMode::Normal => palette.muted,
    };

    let lines = vec![
        section("session", palette),
        kv("provider", app.config.default_provider.as_str(), palette),
        kv("mode", &format!("{:?}", app.mode), palette),
        Line::from(vec![
            Span::styled("status   ", Style::default().fg(palette.subtle)),
            Span::styled(app.status.as_str(), Style::default().fg(mode_color)),
        ]),
        Line::from(""),
        section("agent limits", palette),
        kv(
            "steps",
            &app.config.agent.max_steps_per_turn.to_string(),
            palette,
        ),
        kv(
            "patches",
            &app.config.agent.max_patch_retries.to_string(),
            palette,
        ),
        kv(
            "shell",
            &app.config.agent.max_shell_retries.to_string(),
            palette,
        ),
        Line::from(""),
        section("keys", palette),
        key("enter", "send", palette),
        key("esc", "clear input", palette),
        key("ctrl+l", "clear chat", palette),
        key("ctrl+c", "quit", palette),
    ];

    let paragraph = Paragraph::new(lines).style(Style::default().fg(palette.text).bg(palette.bg));
    frame.render_widget(paragraph, area);
}

fn section(label: &'static str, palette: Palette) -> Line<'static> {
    Line::from(Span::styled(
        label,
        Style::default()
            .fg(palette.muted)
            .add_modifier(Modifier::BOLD),
    ))
}

fn kv(label: &'static str, value: &str, palette: Palette) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<9}"), Style::default().fg(palette.subtle)),
        Span::styled(value.to_owned(), Style::default().fg(palette.text)),
    ])
}

fn key(key: &'static str, action: &'static str, palette: Palette) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{key:<9}"), Style::default().fg(palette.accent)),
        Span::styled(action, Style::default().fg(palette.text)),
    ])
}
