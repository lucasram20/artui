use crate::app::{App, ReasoningEffort, SlashCommand, ThemeId, UiMode};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let theme = if app.theme_picker_open {
        ThemeId::ALL[app.theme_cursor]
    } else {
        app.theme
    };

    let palette = theme::palette(theme);
    frame.render_widget(
        Block::default().style(Style::default().bg(palette.bg)),
        frame.area(),
    );

    let content = frame.area().inner(Margin {
        vertical: 1,
        horizontal: 2,
    });
    let header_height = match content.width {
        118.. => 10,
        76.. => 8,
        _ => 4,
    };
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_height), Constraint::Min(10)])
        .split(content);

    draw_header(frame, app, theme, root[0]);
    draw_body(frame, app, theme, root[1]);
}

fn draw_header(frame: &mut Frame<'_>, app: &App, theme: ThemeId, area: Rect) {
    if area.width < 76 || area.height < 8 {
        draw_compact_header(frame, app, theme, area);
        return;
    }

    let palette = theme::palette(theme);
    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .title(Line::from(vec![
                Span::styled(" artui ", Style::default().fg(palette.accent)),
                Span::styled(
                    format!("v{} ", env!("CARGO_PKG_VERSION")),
                    Style::default().fg(palette.muted),
                ),
            ]))
            .border_style(Style::default().fg(palette.accent))
            .style(Style::default().bg(palette.bg)),
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

    draw_brand(frame, app, theme, columns[0]);
    draw_header_notes(frame, app, theme, columns[1]);
}

fn draw_body(frame: &mut Frame<'_>, app: &App, theme: ThemeId, area: Rect) {
    let composer_height = input_height(app, area.width).saturating_add(2);
    let suggestions = visible_slash_commands(app);
    let suggestions_height = slash_commands_height(&suggestions);
    let file_mentions = visible_file_mentions(app);
    let file_mentions_height = file_mentions_popup_height(&file_mentions);
    let popup_height = suggestions_height + file_mentions_height;
    let footer_height = if popup_height == 0 { 1 } else { 0 };
    let reserved_height = composer_height + footer_height + popup_height;
    let max_history_height = area.height.saturating_sub(reserved_height);
    let history_height = conversation_anchor_height(app, area.width, max_history_height);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(history_height),
            Constraint::Length(composer_height),
            Constraint::Length(popup_height),
            Constraint::Length(footer_height),
            Constraint::Min(0),
        ])
        .split(area);

    super::chat::draw(frame, app, theme, rows[0]);
    draw_input(frame, app, theme, rows[1]);
    if !suggestions.is_empty() {
        draw_slash_commands(frame, app, theme, rows[2], &suggestions);
    } else if !file_mentions.is_empty() {
        draw_file_mentions(frame, app, theme, rows[2], &file_mentions);
    }
    if popup_height == 0 {
        draw_footer(frame, app, theme, rows[3]);
    }
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

fn draw_brand(frame: &mut Frame<'_>, app: &App, theme: ThemeId, area: Rect) {
    let palette = theme::palette(theme);
    frame.render_widget(
        Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(palette.border))
            .style(Style::default().fg(palette.text).bg(palette.bg)),
        area,
    );

    let content = Rect {
        x: area.x,
        y: area.y,
        width: area.width.saturating_sub(1),
        height: area.height,
    };

    let logo_width = 10.min(content.width);
    let logo_y = if content.height >= 8 {
        content.y + 2
    } else {
        content.y + 1
    };

    if content.height >= 8 {
        frame.render_widget(
            Paragraph::new("Welcome back!")
                .alignment(Alignment::Center)
                .style(
                    Style::default()
                        .fg(palette.text)
                        .add_modifier(Modifier::BOLD),
                ),
            Rect {
                x: content.x,
                y: content.y + 1,
                width: content.width,
                height: 1,
            },
        );
    }

    let logo_area = Rect {
        x: content.x + content.width.saturating_sub(logo_width) / 2,
        y: logo_y,
        width: logo_width,
        height: 3,
    };
    frame.render_widget(
        Paragraph::new(app.logo).style(Style::default().fg(palette.accent).bg(palette.bg)),
        logo_area,
    );

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(vec![
                Span::styled("Agent: ", Style::default().fg(palette.muted)),
                Span::styled(
                    app.active_agent_name(),
                    Style::default()
                        .fg(palette.accent)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    format!(" ({})", app.active_agent_id()),
                    Style::default().fg(palette.muted),
                ),
            ]),
            Line::from(Span::styled(
                trim_to_width(app.active_agent_description(), content.width as usize),
                Style::default().fg(palette.muted),
            )),
        ])
        .alignment(Alignment::Center)
        .style(Style::default().fg(palette.text).bg(palette.bg)),
        Rect {
            x: content.x,
            y: logo_area.bottom() + u16::from(content.height >= 8),
            width: content.width,
            height: 2,
        },
    );
}

fn draw_header_notes(frame: &mut Frame<'_>, app: &App, theme: ThemeId, area: Rect) {
    let palette = theme::palette(theme);
    let mut text = vec![
        Line::from(Span::styled(
            "Tips for getting started",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Use / for commands or describe the change directly.",
            Style::default().fg(palette.text),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Quotes of the day",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        )),
    ];

    if let Some(quote) = &app.quote {
        text.push(Line::from(Span::styled(
            format!("\"{}\"", quote.text),
            Style::default().fg(palette.cyan),
        )));
        text.push(Line::from(Span::styled(
            format!("— {}", quote.author),
            Style::default().fg(palette.muted),
        )));
    } else {
        text.push(Line::from(Span::styled(
            "Fetching wisdom...",
            Style::default().fg(palette.muted),
        )));
    }

    if app.mode == UiMode::Streaming {
        text.push(Line::from(""));
        text.push(Line::from(Span::styled(
            "Streaming response...",
            Style::default().fg(palette.cyan),
        )));
    }

    let vertical_margin = u16::from(area.height >= 10);
    frame.render_widget(
        Paragraph::new(text)
            .style(Style::default().bg(palette.bg))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: true }),
        area.inner(Margin {
            vertical: vertical_margin,
            horizontal: 2,
        }),
    );
}

fn draw_input(frame: &mut Frame<'_>, app: &App, theme: ThemeId, area: Rect) {
    let prompt = if app.mode == UiMode::Streaming {
        "…"
    } else {
        "›"
    };
    let prompt_width = prompt.chars().count() + 1;
    let text_width = area.width.saturating_sub(prompt_width as u16).max(1) as usize;
    let input_lines = input_line_count(app.input.as_str(), text_width).clamp(1, 6) as u16;
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
        wrapped_input_lines(prompt, app.input.as_str(), text_width, theme)
    };

    frame.render_widget(
        Block::default()
            .borders(Borders::ALL)
            .border_style(
                Style::default().fg(reasoning_effort_color(palette, app.reasoning_effort)),
            )
            .style(Style::default().bg(palette.bg)),
        area,
    );
    draw_input_titles(frame, app, theme, area);
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
        let (cursor_row, cursor_col) = input_cursor(app.input.as_str(), text_width);
        let cursor_x = input_area.x + prompt_width as u16 + cursor_col;
        let cursor_y = input_area.y + cursor_row.min(input_area.height.saturating_sub(1));
        frame.set_cursor_position((cursor_x.min(input_area.right().saturating_sub(1)), cursor_y));
    }
}

fn input_height(app: &App, width: u16) -> u16 {
    let prompt_width = 2usize;
    let text_width = width.saturating_sub(prompt_width as u16).max(1) as usize;
    input_line_count(app.input.as_str(), text_width).clamp(1, 6) as u16
}

fn input_line_count(input: &str, text_width: usize) -> usize {
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

fn conversation_anchor_height(app: &App, width: u16, max_height: u16) -> u16 {
    let transcript_height = transcript_height(app, width);
    let empty_anchor = empty_conversation_anchor(max_height);
    empty_anchor
        .saturating_add(transcript_height)
        .min(max_height)
}

fn transcript_height(app: &App, width: u16) -> u16 {
    let usable_width = width.max(1) as usize;
    app.transcript
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
        .sum::<usize>()
        .min(u16::MAX as usize) as u16
}

fn empty_conversation_anchor(_max_height: u16) -> u16 {
    1
}

fn wrapped_input_lines<'a>(
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

fn input_cursor(input: &str, text_width: usize) -> (u16, u16) {
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

fn visible_slash_commands(app: &App) -> Vec<&'static SlashCommand> {
    if app.mode == UiMode::Streaming
        || app.theme_picker_open
        || app.model_picker_open
        || app.login_picker_open
        || app.statusline_open
        || app.agent_picker_open
    {
        return Vec::new();
    }

    crate::app::slash_command_matches(app.input.as_str())
}

fn slash_commands_height(commands: &[&SlashCommand]) -> u16 {
    if commands.is_empty() {
        0
    } else {
        commands.len().min(6) as u16 + 1
    }
}

fn draw_slash_commands(
    frame: &mut Frame<'_>,
    app: &App,
    theme: ThemeId,
    area: Rect,
    commands: &[&SlashCommand],
) {
    let palette = theme::palette(theme);
    let rows = commands
        .iter()
        .take(area.height.saturating_sub(1) as usize)
        .enumerate()
        .map(|(index, command)| {
            let selected = index == app.slash_cursor.min(commands.len().saturating_sub(1));
            let command_style = if selected {
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette.muted)
            };
            let description_style = if selected {
                Style::default().fg(palette.text)
            } else {
                Style::default().fg(palette.muted)
            };
            Line::from(vec![
                Span::styled(format!("{:<12}", command.name), command_style),
                Span::styled(
                    trim_to_width(command.description, area.width.saturating_sub(14) as usize),
                    description_style,
                ),
            ])
        })
        .collect::<Vec<_>>();

    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(Line::from(Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(palette.border),
    )));
    lines.extend(rows);

    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(palette.bg)),
        area,
    );
}

// ── File mention suggestions ───────────────────────────────────────────

const FILE_MENTION_MAX_VISIBLE: usize = 8;

fn visible_file_mentions(app: &App) -> Vec<String> {
    if app.mode == UiMode::Streaming
        || app.theme_picker_open
        || app.model_picker_open
        || app.login_picker_open
        || app.statusline_open
        || app.agent_picker_open
    {
        return Vec::new();
    }

    app.file_mention_matches()
}

fn file_mentions_popup_height(mentions: &[String]) -> u16 {
    if mentions.is_empty() {
        0
    } else {
        mentions.len().min(FILE_MENTION_MAX_VISIBLE) as u16 + 1
    }
}

fn draw_file_mentions(
    frame: &mut Frame<'_>,
    app: &App,
    theme: ThemeId,
    area: Rect,
    mentions: &[String],
) {
    let palette = theme::palette(theme);
    let max_visible = area.height.saturating_sub(1) as usize;
    let rows = mentions
        .iter()
        .take(max_visible)
        .enumerate()
        .map(|(index, path)| {
            let selected = index == app.file_mention_cursor.min(mentions.len().saturating_sub(1));
            let is_dir = path.ends_with('/');
            let icon = if is_dir { "\u{1F4C1} " } else { "\u{1F4C4} " };
            let style = if selected {
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette.muted)
            };
            Line::from(vec![
                Span::styled(icon, style),
                Span::styled(
                    trim_to_width(path, area.width.saturating_sub(4) as usize),
                    style,
                ),
            ])
        })
        .collect::<Vec<_>>();

    let mut lines = Vec::with_capacity(rows.len() + 1);
    lines.push(Line::from(Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(palette.border),
    )));
    lines.extend(rows);

    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(palette.bg)),
        area,
    );
}

fn trim_to_width(value: &str, width: usize) -> String {
    if value.chars().count() <= width {
        return value.to_owned();
    }
    if width <= 1 {
        return "…".to_owned();
    }

    let mut output = value.chars().take(width - 1).collect::<String>();
    output.push('…');
    output
}

fn draw_footer(frame: &mut Frame<'_>, app: &App, theme: ThemeId, area: Rect) {
    let palette = theme::palette(theme);
    let mut spans = vec![Span::styled(
        compact_cwd(),
        Style::default().fg(palette.muted),
    )];

    if app.git_branch_label != "no-git" {
        spans.push(Span::raw(" "));
        spans.push(Span::styled("(", Style::default().fg(palette.muted)));
        spans.push(Span::styled(
            &app.git_branch_label,
            Style::default().fg(palette.muted),
        ));

        if app.git_status_label != "clean" && app.git_status_label != "unknown" {
            spans.push(Span::styled(" ±", Style::default().fg(palette.muted)));
            spans.push(Span::styled(
                app.git_status_label.clone(),
                Style::default().fg(palette.pink),
            ));
        }

        spans.push(Span::styled(")", Style::default().fg(palette.muted)));
    }

    spans.push(Span::styled(" | ", Style::default().fg(palette.subtle)));
    spans.push(Span::styled(
        app.context_usage_label(),
        Style::default().fg(palette.muted),
    ));

    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .alignment(Alignment::Left)
            .style(Style::default().fg(palette.text).bg(palette.bg)),
        area,
    );
}

fn draw_input_titles(frame: &mut Frame<'_>, app: &App, theme: ThemeId, area: Rect) {
    if area.width < 8 {
        return;
    }
    let palette = theme::palette(theme);

    // Left side: Eye animation
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default().fg(palette.border).bg(palette.bg)),
            Span::styled(
                app.eye_frame(),
                Style::default().fg(palette.accent).bg(palette.bg),
            ),
            Span::styled(" ", Style::default().fg(palette.border).bg(palette.bg)),
        ])),
        Rect {
            x: area.x.saturating_add(2),
            y: area.y,
            width: 4,
            height: 1,
        },
    );

    let provider = app.provider_usage_label().to_lowercase();
    let model = active_model(app);
    let reasoning = app.reasoning_effort.label();

    let full_text = format!("{} · {} · {}", provider, model, reasoning);
    let trimmed = trim_to_width(&full_text, area.width.saturating_sub(10) as usize);
    let right_area_width = (trimmed.chars().count() + 2) as u16;

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(" ", Style::default().fg(palette.border).bg(palette.bg)),
            Span::styled(provider, Style::default().fg(palette.muted).bg(palette.bg)),
            Span::styled(" · ", Style::default().fg(palette.subtle).bg(palette.bg)),
            Span::styled(
                model.to_owned(),
                Style::default().fg(palette.text).bg(palette.bg),
            ),
            Span::styled(" · ", Style::default().fg(palette.subtle).bg(palette.bg)),
            Span::styled(
                reasoning.to_owned(),
                reasoning_effort_style(palette, app.reasoning_effort),
            ),
            Span::styled(" ", Style::default().fg(palette.border).bg(palette.bg)),
        ])),
        Rect {
            x: area
                .right()
                .saturating_sub(right_area_width.saturating_add(1)),
            y: area.y,
            width: right_area_width,
            height: 1,
        },
    );
}

fn reasoning_effort_style(palette: theme::Palette, effort: ReasoningEffort) -> Style {
    let style = Style::default()
        .fg(reasoning_effort_color(palette, effort))
        .bg(palette.bg);
    if matches!(effort, ReasoningEffort::High | ReasoningEffort::XHigh) {
        style.add_modifier(Modifier::BOLD)
    } else {
        style
    }
}

fn reasoning_effort_color(
    palette: theme::Palette,
    effort: ReasoningEffort,
) -> ratatui::style::Color {
    match effort {
        ReasoningEffort::Auto => palette.border,
        ReasoningEffort::Low => palette.green,
        ReasoningEffort::Medium => palette.yellow,
        ReasoningEffort::High => palette.pink,
        ReasoningEffort::XHigh => palette.purple,
    }
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

fn active_model(app: &App) -> &str {
    app.active_model()
}

pub(crate) mod theme {
    use ratatui::style::Color;

    use crate::app::ThemeId;

    #[derive(Debug, Clone, Copy)]
    pub struct Palette {
        pub bg: Color,
        pub border: Color,
        pub rule: Color,
        pub text: Color,
        pub muted: Color,
        pub subtle: Color,
        pub accent: Color,
        pub green: Color,
        pub pink: Color,
        pub cyan: Color,
        #[allow(dead_code)]
        pub purple: Color,
        pub yellow: Color,
    }

    pub fn palette(theme: ThemeId) -> Palette {
        match theme {
            ThemeId::MonokaiBlue => Palette {
                bg: Color::Rgb(32, 33, 29),
                border: Color::Rgb(61, 74, 83),
                rule: Color::Rgb(50, 58, 61),
                text: Color::Rgb(235, 232, 220),
                muted: Color::Rgb(153, 164, 165),
                subtle: Color::Rgb(101, 116, 120),
                accent: Color::Rgb(89, 178, 255),
                green: Color::Rgb(170, 190, 150),
                pink: Color::Rgb(190, 143, 170),
                cyan: Color::Rgb(132, 206, 226),
                purple: Color::Rgb(142, 168, 226),
                yellow: Color::Rgb(210, 197, 136),
            },
            ThemeId::TokyoNight => Palette {
                bg: Color::Rgb(26, 27, 38),
                border: Color::Rgb(65, 72, 104),
                rule: Color::Rgb(45, 50, 70),
                text: Color::Rgb(192, 202, 245),
                muted: Color::Rgb(154, 165, 206),
                subtle: Color::Rgb(86, 95, 137),
                accent: Color::Rgb(122, 162, 247),
                green: Color::Rgb(158, 206, 106),
                pink: Color::Rgb(247, 118, 142),
                cyan: Color::Rgb(125, 207, 255),
                purple: Color::Rgb(187, 154, 247),
                yellow: Color::Rgb(224, 175, 104),
            },
            ThemeId::CatppuccinMocha => Palette {
                bg: Color::Rgb(30, 30, 46),
                border: Color::Rgb(88, 91, 112),
                rule: Color::Rgb(49, 50, 68),
                text: Color::Rgb(205, 214, 244),
                muted: Color::Rgb(166, 173, 200),
                subtle: Color::Rgb(108, 112, 134),
                accent: Color::Rgb(180, 190, 254),
                green: Color::Rgb(166, 227, 161),
                pink: Color::Rgb(245, 194, 231),
                cyan: Color::Rgb(137, 220, 235),
                purple: Color::Rgb(203, 166, 247),
                yellow: Color::Rgb(249, 226, 175),
            },
            ThemeId::Gruvbox => Palette {
                bg: Color::Rgb(40, 40, 40),
                border: Color::Rgb(102, 92, 84),
                rule: Color::Rgb(60, 56, 54),
                text: Color::Rgb(235, 219, 178),
                muted: Color::Rgb(168, 153, 132),
                subtle: Color::Rgb(146, 131, 116),
                accent: Color::Rgb(250, 189, 47),
                green: Color::Rgb(184, 187, 38),
                pink: Color::Rgb(211, 134, 155),
                cyan: Color::Rgb(142, 192, 124),
                purple: Color::Rgb(211, 134, 155),
                yellow: Color::Rgb(250, 189, 47),
            },
            ThemeId::Nord => Palette {
                bg: Color::Rgb(46, 52, 64),
                border: Color::Rgb(76, 86, 106),
                rule: Color::Rgb(59, 66, 82),
                text: Color::Rgb(216, 222, 233),
                muted: Color::Rgb(143, 188, 187),
                subtle: Color::Rgb(136, 192, 208),
                accent: Color::Rgb(129, 161, 193),
                green: Color::Rgb(163, 190, 140),
                pink: Color::Rgb(191, 97, 106),
                cyan: Color::Rgb(136, 192, 208),
                purple: Color::Rgb(180, 142, 173),
                yellow: Color::Rgb(235, 203, 139),
            },
            ThemeId::Dracula => Palette {
                bg: Color::Rgb(40, 42, 54),
                border: Color::Rgb(68, 71, 90),
                rule: Color::Rgb(56, 58, 89),
                text: Color::Rgb(248, 248, 242),
                muted: Color::Rgb(139, 233, 253),
                subtle: Color::Rgb(98, 114, 164),
                accent: Color::Rgb(139, 233, 253),
                green: Color::Rgb(80, 250, 123),
                pink: Color::Rgb(255, 121, 198),
                cyan: Color::Rgb(139, 233, 253),
                purple: Color::Rgb(187, 154, 247),
                yellow: Color::Rgb(224, 175, 104),
            },
            ThemeId::Aura => Palette {
                bg: Color::Rgb(21, 19, 26),
                border: Color::Rgb(61, 55, 74),
                rule: Color::Rgb(45, 41, 54),
                text: Color::Rgb(237, 235, 242),
                muted: Color::Rgb(141, 134, 161),
                subtle: Color::Rgb(109, 103, 128),
                accent: Color::Rgb(162, 117, 255),
                green: Color::Rgb(97, 255, 169),
                pink: Color::Rgb(255, 103, 194),
                cyan: Color::Rgb(130, 230, 255),
                purple: Color::Rgb(162, 117, 255),
                yellow: Color::Rgb(255, 202, 117),
            },
            ThemeId::SolarizedDark => Palette {
                bg: Color::Rgb(0, 43, 54),
                border: Color::Rgb(7, 54, 66),
                rule: Color::Rgb(10, 60, 72),
                text: Color::Rgb(131, 148, 150),
                muted: Color::Rgb(101, 123, 131),
                subtle: Color::Rgb(88, 110, 117),
                accent: Color::Rgb(38, 139, 210),
                green: Color::Rgb(133, 153, 0),
                pink: Color::Rgb(211, 54, 130),
                cyan: Color::Rgb(42, 161, 152),
                purple: Color::Rgb(108, 113, 196),
                yellow: Color::Rgb(181, 137, 0),
            },
            ThemeId::OceanicNext => Palette {
                bg: Color::Rgb(27, 43, 52),
                border: Color::Rgb(52, 61, 70),
                rule: Color::Rgb(40, 50, 60),
                text: Color::Rgb(216, 222, 233),
                muted: Color::Rgb(167, 173, 186),
                subtle: Color::Rgb(101, 115, 126),
                accent: Color::Rgb(102, 153, 204),
                green: Color::Rgb(153, 199, 148),
                pink: Color::Rgb(236, 95, 103),
                cyan: Color::Rgb(102, 197, 180),
                purple: Color::Rgb(197, 148, 197),
                yellow: Color::Rgb(250, 200, 99),
            },
            ThemeId::RosePine => Palette {
                bg: Color::Rgb(25, 23, 36),
                border: Color::Rgb(64, 61, 82),
                rule: Color::Rgb(38, 35, 58),
                text: Color::Rgb(224, 222, 244),
                muted: Color::Rgb(144, 140, 170),
                subtle: Color::Rgb(110, 106, 134),
                accent: Color::Rgb(235, 188, 186),
                green: Color::Rgb(156, 207, 216),
                pink: Color::Rgb(235, 111, 146),
                cyan: Color::Rgb(156, 207, 216),
                purple: Color::Rgb(196, 167, 231),
                yellow: Color::Rgb(246, 193, 119),
            },
            ThemeId::Everforest => Palette {
                bg: Color::Rgb(43, 51, 49),
                border: Color::Rgb(74, 86, 82),
                rule: Color::Rgb(55, 63, 61),
                text: Color::Rgb(211, 198, 170),
                muted: Color::Rgb(133, 153, 144),
                subtle: Color::Rgb(157, 171, 162),
                accent: Color::Rgb(167, 192, 128),
                green: Color::Rgb(167, 192, 128),
                pink: Color::Rgb(230, 126, 128),
                cyan: Color::Rgb(127, 187, 179),
                purple: Color::Rgb(214, 153, 182),
                yellow: Color::Rgb(219, 171, 121),
            },
            ThemeId::Kanagawa => Palette {
                bg: Color::Rgb(31, 31, 40),
                border: Color::Rgb(54, 54, 70),
                rule: Color::Rgb(42, 42, 52),
                text: Color::Rgb(211, 191, 155),
                muted: Color::Rgb(114, 123, 126),
                subtle: Color::Rgb(152, 147, 165),
                accent: Color::Rgb(122, 146, 173),
                green: Color::Rgb(118, 135, 101),
                pink: Color::Rgb(195, 122, 141),
                cyan: Color::Rgb(122, 157, 150),
                purple: Color::Rgb(149, 123, 171),
                yellow: Color::Rgb(255, 159, 28),
            },
            ThemeId::AyuMirage => Palette {
                bg: Color::Rgb(23, 27, 33),
                border: Color::Rgb(45, 52, 64),
                rule: Color::Rgb(31, 36, 43),
                text: Color::Rgb(204, 204, 204),
                muted: Color::Rgb(114, 127, 140),
                subtle: Color::Rgb(92, 103, 115),
                accent: Color::Rgb(255, 145, 112),
                green: Color::Rgb(152, 195, 121),
                pink: Color::Rgb(237, 110, 131),
                cyan: Color::Rgb(149, 230, 203),
                purple: Color::Rgb(212, 191, 255),
                yellow: Color::Rgb(255, 195, 112),
            },
            ThemeId::NightOwl => Palette {
                bg: Color::Rgb(1, 22, 39),
                border: Color::Rgb(28, 45, 65),
                rule: Color::Rgb(10, 31, 51),
                text: Color::Rgb(214, 222, 235),
                muted: Color::Rgb(99, 119, 119),
                subtle: Color::Rgb(127, 141, 141),
                accent: Color::Rgb(130, 170, 255),
                green: Color::Rgb(173, 219, 103),
                pink: Color::Rgb(199, 146, 234),
                cyan: Color::Rgb(127, 219, 202),
                purple: Color::Rgb(199, 146, 234),
                yellow: Color::Rgb(236, 173, 103),
            },
        }
    }
}
