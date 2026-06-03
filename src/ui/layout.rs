use crate::{
    app::{App, SlashCommand, ThemeId, UiMode},
    ui::{
        cells,
        chat::TranscriptRenderCache,
        components::{chrome, prompt},
        list, statusline,
    },
};
use ratatui::{
    layout::{Constraint, Direction, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{ListItem, Paragraph},
    Frame,
};

pub fn draw(frame: &mut Frame<'_>, app: &App, transcript_cache: &mut TranscriptRenderCache) {
    let theme = if app.theme_picker_open {
        ThemeId::ALL[app.theme_cursor]
    } else {
        app.theme
    };

    chrome::draw_app_background(frame, theme);
    let content = chrome::content_area(frame.area());
    let header_height = chrome::header_height(content.width);
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(header_height), Constraint::Min(10)])
        .split(content);

    chrome::draw_header(frame, app, theme, root[0]);
    draw_body(frame, app, theme, root[1], transcript_cache);
}

fn draw_body(
    frame: &mut Frame<'_>,
    app: &App,
    theme: ThemeId,
    area: Rect,
    transcript_cache: &mut TranscriptRenderCache,
) {
    let composer_height = prompt::height(app, area.width);
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

    super::chat::draw(frame, app, theme, rows[0], transcript_cache);
    prompt::draw(frame, app, theme, rows[1]);
    if !suggestions.is_empty() {
        draw_slash_commands(frame, app, theme, rows[2], &suggestions);
    } else if !file_mentions.is_empty() {
        draw_file_mentions(frame, app, theme, rows[2], &file_mentions);
    }
    if popup_height == 0 {
        statusline::draw_footer(frame, app, theme, rows[3]);
    }
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
    if commands.is_empty() {
        return;
    }
    let palette = theme::palette(theme);
    let divider = Line::from(Span::styled(
        "─".repeat(area.width as usize),
        Style::default().fg(palette.border),
    ));
    frame.render_widget(
        Paragraph::new(divider).style(Style::default().bg(palette.bg)),
        Rect {
            x: area.x,
            y: area.y,
            width: area.width,
            height: 1,
        },
    );
    let list_area = Rect {
        x: area.x,
        y: area.y.saturating_add(1),
        width: area.width,
        height: area.height.saturating_sub(1),
    };
    let items = commands
        .iter()
        .map(|command| {
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<12}", command.name),
                    Style::default().fg(palette.muted),
                ),
                Span::styled(
                    trim_to_width(command.description, area.width.saturating_sub(14) as usize),
                    Style::default().fg(palette.muted),
                ),
            ]))
        })
        .collect::<Vec<_>>();
    let selected = app.slash_cursor.min(commands.len().saturating_sub(1));
    let offset = list::list_offset_for_selection(selected, list_area.height as usize, items.len());
    list::render_stateful_list(frame, list_area, items, selected, offset, palette);
}

// ── File mention suggestions ───────────────────────────────────────────

const FILE_MENTION_MAX_VISIBLE: usize = 12;

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
    let cursor = app
        .file_mention_cursor
        .min(mentions.len().saturating_sub(1));

    // Scroll window so cursor is always visible
    let scroll_offset = if cursor >= max_visible {
        cursor - max_visible + 1
    } else {
        0
    };

    let rows = mentions
        .iter()
        .skip(scroll_offset)
        .take(max_visible)
        .enumerate()
        .map(|(index, path)| {
            let actual_index = index + scroll_offset;
            let selected = actual_index == cursor;
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
    cells::trim_to_width(value, width)
}

pub(crate) fn active_model(app: &App) -> &str {
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
        #[allow(dead_code)]
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
