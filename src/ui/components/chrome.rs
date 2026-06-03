//! App chrome: full/compact header and root background.

use ratatui::{
    layout::{Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

use crate::app::{App, ThemeId};

use super::super::layout::{active_model, theme};

pub fn draw_app_background(frame: &mut Frame<'_>, theme: ThemeId) {
    let palette = theme::palette(theme);
    frame.render_widget(
        Block::default().style(Style::default().bg(palette.bg)),
        frame.area(),
    );
}

pub fn content_area(frame_area: Rect) -> Rect {
    frame_area.inner(Margin {
        vertical: 1,
        horizontal: 2,
    })
}

pub fn header_height(content_width: u16) -> u16 {
    match content_width {
        118.. => 7,
        76.. => 6,
        _ => 4,
    }
}

pub fn draw_header(frame: &mut Frame<'_>, app: &App, theme: ThemeId, area: Rect) {
    if area.width < 76 || area.height < 6 {
        draw_compact_header(frame, app, theme, area);
        return;
    }

    let palette = theme::palette(theme);
    frame.render_widget(
        Block::default().style(Style::default().bg(palette.bg)),
        area,
    );

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(19), Constraint::Min(20)])
        .split(area);

    let logo_lines = render_logo_lines(palette.accent);
    frame.render_widget(
        Paragraph::new(logo_lines).style(Style::default().bg(palette.bg)),
        columns[0],
    );

    let mut info_lines = vec![
        Line::from(vec![
            Span::styled(
                "artui",
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!(" v{}", env!("CARGO_PKG_VERSION")),
                Style::default().fg(palette.muted),
            ),
        ]),
        Line::from(""),
    ];

    if let Some(quote) = &app.quote {
        info_lines.push(Line::from(Span::styled(
            format!("\"{}\"", quote.text),
            Style::default().fg(palette.text),
        )));
        info_lines.push(Line::from(Span::styled(
            format!("— {}", quote.author),
            Style::default().fg(palette.muted),
        )));
    } else {
        info_lines.push(Line::from(Span::styled(
            "\"Code is like humor. When you have to explain it, it's bad.\"",
            Style::default().fg(palette.text),
        )));
        info_lines.push(Line::from(Span::styled(
            "— Cory House",
            Style::default().fg(palette.muted),
        )));
    }

    frame.render_widget(
        Paragraph::new(info_lines)
            .wrap(Wrap { trim: false })
            .style(Style::default().fg(palette.text).bg(palette.bg)),
        Rect {
            x: columns[1].x,
            y: columns[1].y,
            width: columns[1].width,
            height: columns[1].height,
        },
    );
}

fn draw_compact_header(frame: &mut Frame<'_>, app: &App, theme: ThemeId, area: Rect) {
    let palette = theme::palette(theme);
    let lines = vec![
        Line::from(vec![
            Span::styled(
                "artui",
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  v", Style::default().fg(palette.muted)),
            Span::styled(env!("CARGO_PKG_VERSION"), Style::default().fg(palette.text)),
        ]),
        Line::from(vec![
            Span::styled(active_model(app), Style::default().fg(palette.text)),
            Span::styled(
                format!("  {}", app.active_agent_id()),
                Style::default().fg(palette.accent),
            ),
            Span::styled("  ", Style::default().fg(palette.subtle)),
            Span::styled(compact_cwd(), Style::default().fg(palette.muted)),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(palette.accent)),
            )
            .style(Style::default().fg(palette.text).bg(palette.bg)),
        area,
    );
}

/// Render "ART" logo using colored background spaces instead of block glyphs.
fn render_logo_lines(color: ratatui::style::Color) -> Vec<Line<'static>> {
    const LOGO_BITMAP: &[&[u8]] = &[
        b" ### ####  #####",
        b"#   # #  #   # ",
        b"##### ###    # ",
        b"#   # #  #   # ",
        b"#   # #  #   # ",
    ];

    let on = Style::default().bg(color);
    let off = Style::default();

    LOGO_BITMAP
        .iter()
        .map(|row| {
            let spans: Vec<Span<'static>> = row
                .iter()
                .map(|&cell| {
                    if cell == b'#' {
                        Span::styled(" ", on)
                    } else {
                        Span::styled(" ", off)
                    }
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

fn compact_cwd() -> String {
    std::env::current_dir()
        .ok()
        .map(|path| crate::util::paths::compact_display_path(&path))
        .unwrap_or_else(|| "~".to_owned())
}
