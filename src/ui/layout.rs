use crate::app::{slash_command_matches, App, SlashCommand, StatusLineItem, UiMode};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let palette = theme::palette(app.theme);
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

    draw_header(frame, app, root[0]);
    draw_body(frame, app, root[1]);
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
                Span::styled(
                    " artui ",
                    Style::default().fg(theme::palette(app.theme).accent),
                ),
                Span::styled(
                    format!("v{} ", env!("CARGO_PKG_VERSION")),
                    Style::default().fg(theme::palette(app.theme).muted),
                ),
            ]))
            .border_style(Style::default().fg(theme::palette(app.theme).accent))
            .style(Style::default().bg(theme::palette(app.theme).bg)),
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
    let composer_height = input_height(app, area.width).saturating_add(1);
    let suggestions = visible_slash_commands(app);
    let suggestions_height = slash_commands_height(&suggestions);
    let footer_height = if suggestions.is_empty() { 2 } else { 0 };
    let reserved_height = composer_height + footer_height + suggestions_height;
    let max_history_height = area.height.saturating_sub(reserved_height);
    let history_height = conversation_anchor_height(app, area.width, max_history_height);
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(history_height),
            Constraint::Length(composer_height),
            Constraint::Length(suggestions_height),
            Constraint::Length(footer_height),
            Constraint::Min(0),
        ])
        .split(area);

    super::chat::draw(frame, app, rows[0]);
    draw_input(frame, app, rows[1]);
    if !suggestions.is_empty() {
        draw_slash_commands(frame, app, rows[2], &suggestions);
    }
    if suggestions.is_empty() {
        draw_footer(frame, app, rows[3]);
    }
}

fn draw_compact_header(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let lines = vec![
        Line::from(vec![
            Span::styled(
                "artui",
                Style::default()
                    .fg(theme::palette(app.theme).accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled("  v", Style::default().fg(theme::palette(app.theme).muted)),
            Span::styled(
                env!("CARGO_PKG_VERSION"),
                Style::default().fg(theme::palette(app.theme).text),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                active_model(app),
                Style::default().fg(theme::palette(app.theme).text),
            ),
            Span::styled("  ", Style::default().fg(theme::palette(app.theme).subtle)),
            Span::styled(
                compact_cwd(),
                Style::default().fg(theme::palette(app.theme).muted),
            ),
        ]),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::palette(app.theme).accent)),
            )
            .style(
                Style::default()
                    .fg(theme::palette(app.theme).text)
                    .bg(theme::palette(app.theme).bg),
            ),
        area,
    );
}

fn draw_brand(frame: &mut Frame<'_>, app: &App, area: Rect) {
    frame.render_widget(
        Block::default()
            .borders(Borders::RIGHT)
            .border_style(Style::default().fg(theme::palette(app.theme).border))
            .style(
                Style::default()
                    .fg(theme::palette(app.theme).text)
                    .bg(theme::palette(app.theme).bg),
            ),
        area,
    );

    if area.width < 34 || area.height < 6 {
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "artui",
                    Style::default()
                        .fg(theme::palette(app.theme).accent)
                        .add_modifier(Modifier::BOLD),
                )),
                Line::from(Span::styled(
                    "terminal coding agent",
                    Style::default().fg(theme::palette(app.theme).text),
                )),
            ])
            .alignment(Alignment::Center)
            .style(
                Style::default()
                    .fg(theme::palette(app.theme).text)
                    .bg(theme::palette(app.theme).bg),
            ),
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
                .fg(theme::palette(app.theme).text)
                .add_modifier(Modifier::BOLD),
        )))
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(theme::palette(app.theme).text)
                .bg(theme::palette(app.theme).bg),
        ),
        Rect {
            x: content.x,
            y: content.y,
            width: content.width,
            height: 1,
        },
    );

    let logo_width = 10.min(content.width);
    let logo_y = if content.height >= 8 {
        content.y + 2
    } else {
        content.y + 1
    };
    let logo_area = Rect {
        x: content.x + content.width.saturating_sub(logo_width) / 2,
        y: logo_y,
        width: logo_width,
        height: 3,
    };
    frame.render_widget(
        Paragraph::new(app.logo).style(
            Style::default()
                .fg(theme::palette(app.theme).accent)
                .bg(theme::palette(app.theme).bg),
        ),
        logo_area,
    );

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                format!(
                    "{} with {} mode • {}",
                    active_model(app),
                    mode_label(app),
                    app.config.default_provider
                ),
                Style::default().fg(theme::palette(app.theme).muted),
            )),
            Line::from(Span::styled(
                compact_cwd(),
                Style::default().fg(theme::palette(app.theme).muted),
            )),
        ])
        .alignment(Alignment::Center)
        .style(
            Style::default()
                .fg(theme::palette(app.theme).text)
                .bg(theme::palette(app.theme).bg),
        ),
        Rect {
            x: content.x,
            y: logo_area.bottom() + u16::from(content.height >= 8),
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
                .fg(theme::palette(app.theme).accent)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled("Use ", Style::default().fg(theme::palette(app.theme).text)),
            Span::styled(
                "/",
                Style::default()
                    .fg(theme::palette(app.theme).yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                " for commands or describe the change directly.",
                Style::default().fg(theme::palette(app.theme).text),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "Dev notes",
            Style::default()
                .fg(theme::palette(app.theme).accent)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            quotes.0,
            Style::default().fg(theme::palette(app.theme).cyan),
        )),
        Line::from(Span::styled(
            quotes.1,
            Style::default().fg(theme::palette(app.theme).muted),
        )),
        Line::from(Span::styled(
            format!("model {} • {}", active_model(app), compact_cwd()),
            Style::default().fg(theme::palette(app.theme).subtle),
        )),
    ];

    let vertical_margin = u16::from(area.height >= 10);
    frame.render_widget(
        Paragraph::new(lines).style(
            Style::default()
                .fg(theme::palette(app.theme).text)
                .bg(theme::palette(app.theme).bg),
        ),
        area.inner(Margin {
            vertical: vertical_margin,
            horizontal: 2,
        }),
    );
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

fn draw_input(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let prompt = if app.mode == UiMode::Streaming {
        "…"
    } else {
        "›"
    };
    let prompt_width = prompt.chars().count() + 1;
    let text_width = area.width.saturating_sub(prompt_width as u16).max(1) as usize;
    let input_lines = input_line_count(app.input.as_str(), text_width).clamp(1, 6) as u16;
    let input_area = Rect {
        x: area.x,
        y: area.y.saturating_add(1),
        width: area.width,
        height: input_lines.min(area.height.saturating_sub(1)),
    };
    let lines = if app.input.is_empty() {
        vec![Line::from(vec![
            Span::styled(
                format!("{prompt} "),
                Style::default().fg(theme::palette(app.theme).accent),
            ),
            Span::styled(
                "Ask artui anything...",
                Style::default().fg(theme::palette(app.theme).subtle),
            ),
        ])]
    } else {
        wrapped_input_lines(prompt, app.input.as_str(), text_width, app)
    };

    let palette = theme::palette(app.theme);
    frame.render_widget(
        Block::default()
            .borders(Borders::TOP)
            .border_style(Style::default().fg(palette.border))
            .style(Style::default().bg(palette.bg)),
        area,
    );
    frame.render_widget(
        Paragraph::new(lines).style(Style::default().bg(palette.bg)),
        input_area,
    );

    if input_area.height > 0
        && !app.theme_picker_open
        && !app.model_picker_open
        && !app.statusline_open
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

fn conversation_anchor_height(app: &App, width: u16, max_height: u16) -> u16 {
    let transcript_height = transcript_height(app, width);
    let empty_anchor = empty_conversation_anchor(max_height);
    empty_anchor
        .saturating_add(transcript_height)
        .min(max_height)
}

fn empty_conversation_anchor(max_height: u16) -> u16 {
    let anchor = max_height / EMPTY_CONVERSATION_ANCHOR_DIVISOR;
    anchor.clamp(MIN_EMPTY_CONVERSATION_ANCHOR, MAX_EMPTY_CONVERSATION_ANCHOR)
}

const EMPTY_CONVERSATION_ANCHOR_DIVISOR: u16 = 24;
const MIN_EMPTY_CONVERSATION_ANCHOR: u16 = 0;
const MAX_EMPTY_CONVERSATION_ANCHOR: u16 = 1;

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

fn wrapped_input_lines(
    prompt: &str,
    input: &str,
    text_width: usize,
    app: &App,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (logical_index, logical_line) in input.split('\n').enumerate() {
        let chars = logical_line.chars().collect::<Vec<_>>();
        if chars.is_empty() {
            let prefix = if logical_index == 0 {
                Span::styled(
                    format!("{prompt} "),
                    Style::default().fg(theme::palette(app.theme).accent),
                )
            } else {
                Span::raw(" ".repeat(prompt.chars().count() + 1))
            };
            lines.push(Line::from(vec![prefix]));
            continue;
        }

        for (chunk_index, chunk) in chars.chunks(text_width).enumerate() {
            let prefix = if lines.is_empty() {
                Span::styled(
                    format!("{prompt} "),
                    Style::default().fg(theme::palette(app.theme).accent),
                )
            } else if chunk_index == 0 && logical_index == 0 {
                Span::raw(String::new())
            } else {
                Span::raw(" ".repeat(prompt.chars().count() + 1))
            };
            let content = chunk.iter().collect::<String>();
            lines.push(Line::from(vec![
                prefix,
                Span::styled(content, Style::default().fg(theme::palette(app.theme).text)),
            ]));
        }
    }
    lines
}

fn visible_slash_commands(app: &App) -> Vec<&'static SlashCommand> {
    if app.mode == UiMode::Streaming
        || app.theme_picker_open
        || app.model_picker_open
        || app.statusline_open
    {
        return Vec::new();
    }

    slash_command_matches(app.input.as_str())
}

fn slash_commands_height(commands: &[&SlashCommand]) -> u16 {
    if commands.is_empty() {
        0
    } else {
        commands.len().min(6) as u16 + 1
    }
}

fn draw_slash_commands(frame: &mut Frame<'_>, app: &App, area: Rect, commands: &[&SlashCommand]) {
    let palette = theme::palette(app.theme);
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

fn draw_footer(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let spans = statusline_spans(app, area.width);

    frame.render_widget(
        Paragraph::new(Line::from(spans))
            .block(
                Block::default()
                    .borders(Borders::TOP)
                    .border_style(Style::default().fg(theme::palette(app.theme).border)),
            )
            .style(
                Style::default()
                    .fg(theme::palette(app.theme).text)
                    .bg(theme::palette(app.theme).bg),
            ),
        area,
    );
}

fn statusline_spans(app: &App, width: u16) -> Vec<Span<'static>> {
    let palette = theme::palette(app.theme);
    let mut spans = Vec::new();

    for item in StatusLineItem::ALL {
        if !app.statusline_enabled[item.index()] {
            continue;
        }

        if !spans.is_empty() {
            spans.push(Span::styled("   ", Style::default().fg(palette.subtle)));
        }

        match item {
            StatusLineItem::Model => {
                spans.push(Span::styled("● ", Style::default().fg(palette.green)));
                spans.push(Span::styled(
                    active_model(app).to_owned(),
                    Style::default().fg(palette.text),
                ));
            }
            StatusLineItem::CurrentDir => {
                spans.push(Span::styled(
                    compact_cwd(),
                    Style::default().fg(palette.muted),
                ));
            }
            StatusLineItem::ProjectName => {
                spans.push(Span::styled(
                    current_dir_label(),
                    Style::default().fg(palette.muted),
                ));
            }
            StatusLineItem::GitBranch => {
                spans.push(Span::styled(
                    app.git_branch_label.clone(),
                    Style::default().fg(palette.purple),
                ));
            }
            StatusLineItem::Context => {
                let context_percent = context_percent(app);
                let bar_width = if width >= 86 { 12 } else { 8 };
                spans.push(Span::styled("context ", Style::default().fg(palette.muted)));
                spans.push(Span::styled(
                    progress_bar(context_percent, bar_width, true),
                    Style::default().fg(palette.accent),
                ));
                spans.push(Span::styled(
                    progress_bar(context_percent, bar_width, false),
                    Style::default().fg(palette.rule),
                ));
                spans.push(Span::styled(
                    format!(" {context_percent}%"),
                    Style::default().fg(palette.text),
                ));
            }
            StatusLineItem::GitStatus => {
                spans.push(Span::styled(
                    app.git_status_label.clone(),
                    Style::default().fg(palette.pink),
                ));
            }
            StatusLineItem::EscHint => {
                spans.push(Span::styled(
                    "esc back",
                    Style::default().fg(palette.subtle),
                ));
            }
        }
    }

    if spans.is_empty() {
        spans.push(Span::styled(
            "statusline hidden",
            Style::default().fg(palette.subtle),
        ));
    }

    spans
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
    app.active_model()
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
    let filled_width = width.saturating_mul(percent.min(100)).div_ceil(100);
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
                accent: Color::Rgb(137, 180, 250),
                green: Color::Rgb(166, 227, 161),
                pink: Color::Rgb(243, 139, 168),
                cyan: Color::Rgb(148, 226, 213),
                purple: Color::Rgb(203, 166, 247),
                yellow: Color::Rgb(249, 226, 175),
            },
            ThemeId::Gruvbox => Palette {
                bg: Color::Rgb(40, 40, 40),
                border: Color::Rgb(102, 92, 84),
                rule: Color::Rgb(60, 56, 54),
                text: Color::Rgb(235, 219, 178),
                muted: Color::Rgb(168, 153, 132),
                subtle: Color::Rgb(124, 111, 100),
                accent: Color::Rgb(131, 165, 152),
                green: Color::Rgb(184, 187, 38),
                pink: Color::Rgb(251, 73, 52),
                cyan: Color::Rgb(142, 192, 124),
                purple: Color::Rgb(211, 134, 155),
                yellow: Color::Rgb(250, 189, 47),
            },
            ThemeId::Nord => Palette {
                bg: Color::Rgb(46, 52, 64),
                border: Color::Rgb(76, 86, 106),
                rule: Color::Rgb(59, 66, 82),
                text: Color::Rgb(216, 222, 233),
                muted: Color::Rgb(173, 184, 202),
                subtle: Color::Rgb(129, 161, 193),
                accent: Color::Rgb(136, 192, 208),
                green: Color::Rgb(163, 190, 140),
                pink: Color::Rgb(191, 97, 106),
                cyan: Color::Rgb(143, 188, 187),
                purple: Color::Rgb(180, 142, 173),
                yellow: Color::Rgb(235, 203, 139),
            },
            ThemeId::Dracula => Palette {
                bg: Color::Rgb(40, 42, 54),
                border: Color::Rgb(68, 71, 90),
                rule: Color::Rgb(52, 55, 70),
                text: Color::Rgb(248, 248, 242),
                muted: Color::Rgb(188, 190, 196),
                subtle: Color::Rgb(98, 114, 164),
                accent: Color::Rgb(139, 233, 253),
                green: Color::Rgb(80, 250, 123),
                pink: Color::Rgb(255, 121, 198),
                cyan: Color::Rgb(139, 233, 253),
                purple: Color::Rgb(189, 147, 249),
                yellow: Color::Rgb(241, 250, 140),
            },
        }
    }
}
