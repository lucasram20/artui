use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use ratatui_image::Image;
use std::process::Command;

use crate::app::{App, UiMode};

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    frame.render_widget(
        Block::default().style(Style::default().bg(theme::BG)),
        frame.area(),
    );

    let content = frame.area().inner(Margin {
        vertical: 1,
        horizontal: 2,
    });
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(if content.width >= 118 { 12 } else { 7 }),
            Constraint::Length(1),
            Constraint::Min(10),
        ])
        .split(content);

    draw_header(frame, app, root[0]);
    draw_rule(frame, root[1]);
    draw_body(frame, app, root[2]);
}

fn draw_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if area.width < 76 || area.height < 8 {
        draw_compact_header(frame, app, area);
        return;
    }

    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(Line::from(vec![
                Span::styled(" artui ", Style::default().fg(theme::ACCENT)),
                Span::styled(
                    format!("v{} ", env!("CARGO_PKG_VERSION")),
                    Style::default().fg(theme::MUTED),
                ),
            ]))
            .border_style(Style::default().fg(theme::ACCENT))
            .style(Style::default().bg(theme::BG)),
        area,
    );

    let inner = area.inner(Margin {
        vertical: 1,
        horizontal: 2,
    });
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(35), Constraint::Percentage(65)])
        .split(inner);

    draw_brand(frame, app, columns[0]);
    draw_header_notes(frame, app, columns[1]);
}

fn draw_body(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let composer_height = input_height(app, area.width);
    let history_height = transcript_height(app, area.width, area.height, composer_height);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(history_height),
            Constraint::Length(composer_height),
            Constraint::Length(2),
            Constraint::Min(0),
        ])
        .split(area);

    super::chat::draw(frame, app, rows[0]);
    draw_input(frame, app, rows[1]);
    draw_footer(frame, app, rows[2]);
}

fn draw_compact_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let lines = vec![
        Line::from(vec![
            Span::styled(
                "artui",
                Style::default()
                    .fg(theme::ACCENT)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  v", Style::default().fg(theme::MUTED)),
            Span::styled(env!("CARGO_PKG_VERSION"), Style::default().fg(theme::TEXT)),
        ]),
        Line::from(vec![
            Span::styled(active_model(app), Style::default().fg(theme::TEXT)),
            Span::styled("  ", Style::default().fg(theme::SUBTLE)),
            Span::styled(compact_cwd(), Style::default().fg(theme::MUTED)),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::ACCENT)),
            )
            .style(Style::default().fg(theme::TEXT).bg(theme::BG)),
        area,
    );
}

fn draw_brand(frame: &mut Frame<'_>, app: &App, area: Rect) {
    frame.render_widget(
        Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(theme::BORDER))
            .style(Style::default().fg(theme::TEXT).bg(theme::BG)),
        area,
    );

    if area.width < 34 || area.height < 8 {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "artui",
                    Style::default()
                        .fg(theme::ACCENT)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "terminal coding agent",
                    Style::default().fg(theme::TEXT),
                )),
            ])
            .alignment(Alignment::Center)
            .style(Style::default().fg(theme::TEXT).bg(theme::BG)),
            area,
        );
        return;
    }

    let content = Rect {
        x: area.x,
        y: area.y,
        width: area.width.saturating_sub(1),
        height: area.height,
    };

    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            "Welcome back!",
            Style::default()
                .fg(theme::TEXT)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center)
        .style(Style::default().fg(theme::TEXT).bg(theme::BG)),
        Rect {
            x: content.x,
            y: content.y,
            width: content.width,
            height: 1,
        },
    );

    if let Some(logo) = &app.logo {
        let logo_area = centered_rect(content, 18, 6, 1);
        frame.render_widget(Image::new(logo).allow_clipping(true), logo_area);
    }

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(
                    "{} with {} mode • {}",
                    active_model(app),
                    mode_label(app),
                    app.config.default_provider
                ),
                Style::default().fg(theme::MUTED),
            )),
            Line::from(Span::styled(
                compact_cwd(),
                Style::default().fg(theme::MUTED),
            )),
        ])
        .alignment(Alignment::Center)
        .style(Style::default().fg(theme::TEXT).bg(theme::BG)),
        Rect {
            x: content.x,
            y: content.bottom().saturating_sub(2),
            width: content.width,
            height: 2,
        },
    );
}

fn draw_header_notes(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let quotes = dev_quotes(app);
    let lines = vec![
        Line::from(vec![Span::styled(
            "Tips for getting started",
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("Use ", Style::default().fg(theme::TEXT)),
            Span::styled(
                "/",
                Style::default()
                    .fg(theme::YELLOW)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " for commands or describe the change directly.",
                Style::default().fg(theme::TEXT),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Dev notes",
            Style::default()
                .fg(theme::ACCENT)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(quotes.0, Style::default().fg(theme::CYAN))),
        Line::from(Span::styled(quotes.1, Style::default().fg(theme::MUTED))),
        Line::from(Span::styled(
            format!("model {} • {}", active_model(app), compact_cwd()),
            Style::default().fg(theme::SUBTLE),
        )),
    ];

    frame.render_widget(
        Paragraph::new(lines).style(Style::default().fg(theme::TEXT).bg(theme::BG)),
        area.inner(Margin {
            vertical: 0,
            horizontal: 2,
        }),
    );
}

fn centered_rect(area: Rect, width: u16, height: u16, top_offset: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height.saturating_sub(top_offset));
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + top_offset,
        width,
        height,
    }
}

fn dev_quotes(app: &App) -> (&'static str, &'static str) {
    const QUOTES: [(&str, &str); 6] = [
        (
            "Make the change easy to verify.",
            "A good patch leaves fewer questions than it creates.",
        ),
        (
            "Small steps keep intent visible.",
            "The best abstractions are paid for by removed complexity.",
        ),
        (
            "Read first, then edit.",
            "A codebase usually tells you how it wants to change.",
        ),
        (
            "Prefer boring code with sharp edges removed.",
            "Clarity compounds faster than cleverness.",
        ),
        (
            "Ship the narrowest useful slice.",
            "Momentum comes from working software, not perfect guesses.",
        ),
        (
            "Tests are executable memory.",
            "Write the check where future you will look first.",
        ),
    ];
    let index = (app.transcript.len() + active_model(app).len()) % QUOTES.len();
    QUOTES[index]
}

fn draw_rule(frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(
        Paragraph::new(Line::from(""))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(theme::BORDER)),
            )
            .style(Style::default().bg(theme::BG)),
        area,
    );
}

fn draw_input(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let input_area = area.inner(Margin {
        vertical: 0,
        horizontal: 0,
    });
    let prompt = if app.mode == UiMode::Streaming {
        "…"
    } else {
        "›"
    };
    let prompt_width = prompt.chars().count() + 1;
    let text_width = input_area.width.saturating_sub(prompt_width as u16).max(1) as usize;
    let lines = if app.input.is_empty() {
        vec![Line::from(vec![
            Span::styled(format!("{prompt} "), Style::default().fg(theme::ACCENT)),
            Span::styled("Ask artui anything...", Style::default().fg(theme::SUBTLE)),
        ])]
    } else {
        wrapped_input_lines(prompt, app.input.as_str(), text_width)
    };

    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(theme::BG)),
        input_area,
    );

    let (cursor_row, cursor_col) = input_cursor(app.input.as_str(), text_width);
    let cursor_x = input_area.x + prompt_width as u16 + cursor_col;
    let cursor_y = input_area.y + cursor_row.min(input_area.height.saturating_sub(1));
    frame.set_cursor_position((cursor_x.min(input_area.right().saturating_sub(1)), cursor_y));
}

fn input_height(app: &App, width: u16) -> u16 {
    let prompt_width = 2usize;
    let text_width = width.saturating_sub(prompt_width as u16).max(1) as usize;
    let input_lines = if app.input.is_empty() {
        1
    } else {
        app.input
            .split('\n')
            .map(|line| {
                let len = line.chars().count();
                len.max(1).div_ceil(text_width)
            })
            .sum::<usize>()
    };
    input_lines.clamp(1, 6) as u16
}

fn transcript_height(app: &App, width: u16, available_height: u16, composer_height: u16) -> u16 {
    let usable_width = width.saturating_sub(2).max(1) as usize;
    let lines = app
        .transcript
        .iter()
        .map(|message| {
            let content_lines = if message.content.is_empty() {
                1
            } else {
                message
                    .content
                    .split('\n')
                    .map(|line| {
                        let len = line.chars().count() + 2;
                        len.max(1).div_ceil(usable_width)
                    })
                    .sum::<usize>()
            };
            content_lines + 1
        })
        .sum::<usize>();
    let max_history = available_height.saturating_sub(composer_height + 2);
    (lines as u16).min(max_history)
}

fn input_cursor(input: &str, text_width: usize) -> (u16, u16) {
    let mut row = 0usize;
    let mut col = 0usize;
    for (index, logical_line) in input.split('\n').enumerate() {
        let len = logical_line.chars().count();
        if index > 0 {
            row += 1;
        }
        row += len / text_width;
        col = len % text_width;
        if col == 0 && len > 0 && len % text_width == 0 {
            row = row.saturating_sub(1);
            col = text_width;
        }
    }
    (row as u16, col as u16)
}

fn wrapped_input_lines(prompt: &str, input: &str, text_width: usize) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (logical_index, logical_line) in input.split('\n').enumerate() {
        let chars = logical_line.chars().collect::<Vec<_>>();
        if chars.is_empty() {
            let prefix = if logical_index == 0 {
                Span::styled(format!("{prompt} "), Style::default().fg(theme::ACCENT))
            } else {
                Span::raw(" ".repeat(prompt.chars().count() + 1))
            };
            lines.push(Line::from(vec![prefix]));
            continue;
        }

        for (chunk_index, chunk) in chars.chunks(text_width).enumerate() {
            let prefix = if lines.is_empty() {
                Span::styled(format!("{prompt} "), Style::default().fg(theme::ACCENT))
            } else if chunk_index == 0 && logical_index == 0 {
                Span::raw(String::new())
            } else {
                Span::raw(" ".repeat(prompt.chars().count() + 1))
            };
            let content = chunk.iter().collect::<String>();
            lines.push(Line::from(vec![
                prefix,
                Span::styled(content, Style::default().fg(theme::TEXT)),
            ]));
        }
    }
    lines
}

fn draw_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let branch = git_branch().unwrap_or_else(|| "no-git".to_owned());
    let git_state = git_status_label().unwrap_or_else(|| "unknown".to_owned());
    let context_percent = context_percent(app);
    let mut spans = vec![
        Span::styled("● ", Style::default().fg(theme::GREEN)),
        Span::styled(active_model(app), Style::default().fg(theme::TEXT)),
        Span::styled("   ", Style::default().fg(theme::SUBTLE)),
        Span::styled(current_dir_label(), Style::default().fg(theme::MUTED)),
    ];
    if area.width >= 86 {
        spans.extend([
            Span::styled("   ", Style::default().fg(theme::SUBTLE)),
            Span::styled(branch, Style::default().fg(theme::PURPLE)),
            Span::styled("   ", Style::default().fg(theme::SUBTLE)),
            Span::styled("context ", Style::default().fg(theme::MUTED)),
            Span::styled(
                progress_bar(context_percent, 12, true),
                Style::default().fg(theme::ACCENT),
            ),
            Span::styled(
                progress_bar(context_percent, 12, false),
                Style::default().fg(theme::RULE),
            ),
            Span::styled(
                format!(" {context_percent}%   "),
                Style::default().fg(theme::TEXT),
            ),
            Span::styled(git_state, Style::default().fg(theme::PINK)),
            Span::styled("   ", Style::default().fg(theme::SUBTLE)),
            Span::styled("esc back", Style::default().fg(theme::SUBTLE)),
        ]);
    }

    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(theme::BORDER)),
            )
            .style(Style::default().fg(theme::TEXT).bg(theme::BG)),
        area,
    );
}

fn compact_cwd() -> String {
    std::env::current_dir()
        .ok()
        .map(|path| {
            let path = path.to_string_lossy();
            path.replace(&std::env::var("HOME").unwrap_or_default(), "~")
        })
        .unwrap_or_else(|| "~".to_owned())
}

fn current_dir_label() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "workspace".to_owned())
}

fn active_model(app: &App) -> &str {
    match app.config.default_provider.as_str() {
        "ollama" => app.config.providers.ollama.default_model.as_str(),
        "openai_compat" => app.config.providers.openai_compat.default_model.as_str(),
        _ => "default",
    }
}

fn git_branch() -> Option<String> {
    run_git(["branch", "--show-current"]).and_then(|branch| {
        if branch.is_empty() {
            run_git(["rev-parse", "--short", "HEAD"])
        } else {
            Some(branch)
        }
    })
}

fn git_status_label() -> Option<String> {
    run_git(["status", "--porcelain"]).map(|status| {
        if status.is_empty() {
            "working tree clean".to_owned()
        } else {
            format!("{} changed", status.lines().count())
        }
    })
}

fn run_git<const N: usize>(args: [&str; N]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

fn context_percent(app: &App) -> usize {
    let used = app
        .transcript
        .iter()
        .map(|message| message.content.chars().count())
        .sum::<usize>();
    let budget = app.config.agent.max_tool_output_chars.max(1);
    ((used.saturating_mul(100)) / budget).min(100)
}

fn progress_bar(percent: usize, width: usize, filled: bool) -> String {
    let filled_width = (width.saturating_mul(percent.min(100)) + 99) / 100;
    let len = if filled {
        filled_width
    } else {
        width.saturating_sub(filled_width)
    };
    if filled {
        "█".repeat(len)
    } else {
        "░".repeat(len)
    }
}

fn mode_label(app: &App) -> &'static str {
    match app.mode {
        UiMode::Input => "input",
        UiMode::Normal => "normal",
        UiMode::Streaming => "streaming",
    }
}

pub(crate) mod theme {
    use ratatui::style::Color;

    pub const BG: Color = Color::Rgb(39, 40, 34);
    pub const BORDER: Color = Color::Rgb(73, 72, 62);
    pub const RULE: Color = Color::Rgb(62, 61, 50);
    pub const TEXT: Color = Color::Rgb(248, 248, 242);
    pub const MUTED: Color = Color::Rgb(166, 159, 131);
    pub const SUBTLE: Color = Color::Rgb(117, 113, 94);
    pub const ACCENT: Color = Color::Rgb(253, 151, 31);
    pub const GREEN: Color = Color::Rgb(166, 226, 46);
    pub const PINK: Color = Color::Rgb(249, 38, 114);
    pub const CYAN: Color = Color::Rgb(102, 217, 239);
    pub const PURPLE: Color = Color::Rgb(174, 129, 255);
    pub const YELLOW: Color = Color::Rgb(230, 219, 116);
}
