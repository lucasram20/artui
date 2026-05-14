use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

use crate::{
    app::{App, Role, UiMode},
    ui::layout::theme,
};

pub fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let palette = theme::palette(app.theme);
    let mut lines = Vec::new();

    for message in &app.transcript {
        let (marker, color) = match message.role {
            Role::User => ("›", palette.accent),
            Role::Assistant => ("•", palette.green),
        };

        if message.content.is_empty() {
            lines.push(thinking_line(app, marker, color));
        } else {
            for (index, segment) in display_segments(message.content.as_str())
                .into_iter()
                .enumerate()
            {
                let prefix = if index == 0 {
                    Span::styled(
                        format!("{marker} "),
                        Style::default().fg(color).add_modifier(Modifier::BOLD),
                    )
                } else {
                    Span::raw("  ")
                };
                let text_style = segment_style(segment.kind, app);
                lines.push(Line::from(vec![
                    prefix,
                    Span::styled(segment.text, text_style),
                ]));
            }
        }
        lines.push(Line::from(""));
    }

    let scroll = transcript_scroll_offset(app, area);
    let paragraph = Paragraph::new(lines)
        .style(Style::default().fg(palette.text).bg(palette.bg))
        .scroll((scroll, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn thinking_line(app: &App, marker: &str, marker_color: ratatui::style::Color) -> Line<'static> {
    let palette = theme::palette(app.theme);
    let elapsed = app
        .thinking_elapsed()
        .map(|duration| format!(" ({}s • esc to interrupt)", duration.as_secs()))
        .unwrap_or_default();
    let phrase = if app.mode == UiMode::Streaming {
        format!("{}{}", app.thinking_phrase(), elapsed)
    } else {
        "waiting".to_owned()
    };

    Line::from(vec![
        Span::styled(
            format!("{marker} "),
            Style::default()
                .fg(marker_color)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("{} ", app.thinking_frame()),
            Style::default().fg(palette.accent),
        ),
        Span::styled(
            phrase,
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        ),
    ])
}

fn transcript_scroll_offset(app: &App, area: Rect) -> u16 {
    let content_height = transcript_visual_height(app, area.width.max(1) as usize);
    let bottom_scroll = content_height.saturating_sub(area.height as usize) as u16;
    bottom_scroll.saturating_sub(app.chat_scroll.min(bottom_scroll))
}

pub fn transcript_visual_height(app: &App, width: usize) -> usize {
    app.transcript
        .iter()
        .map(|message| message_visual_height(message.content.as_str(), width.max(1)))
        .sum::<usize>()
}

pub fn message_visual_height(content: &str, width: usize) -> usize {
    let segments = display_segments(content);
    let content_height = if segments.is_empty() {
        1
    } else {
        segments
            .iter()
            .enumerate()
            .map(|(index, segment)| {
                let prefix_width = if index == 0 { 2 } else { 2 };
                let line_width = segment.text.chars().count() + prefix_width;
                line_width.max(1).div_ceil(width.max(1))
            })
            .sum::<usize>()
    };

    content_height + 1
}

#[derive(Clone, Copy)]
enum SegmentKind {
    Body,
    Heading,
    Bullet,
}

struct DisplaySegment {
    kind: SegmentKind,
    text: String,
}

fn display_segments(content: &str) -> Vec<DisplaySegment> {
    normalize_markdown_flow(content)
        .lines()
        .flat_map(segment_from_line)
        .collect()
}

fn segment_from_line(line: &str) -> Vec<DisplaySegment> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    if let Some(text) = trimmed.strip_prefix("###") {
        return vec![DisplaySegment {
            kind: SegmentKind::Heading,
            text: strip_inline_markdown(text.trim()).to_owned(),
        }];
    }

    if let Some(text) = trimmed.strip_prefix("##") {
        return vec![DisplaySegment {
            kind: SegmentKind::Heading,
            text: strip_inline_markdown(text.trim()).to_owned(),
        }];
    }

    if let Some(text) = trimmed.strip_prefix("#") {
        return vec![DisplaySegment {
            kind: SegmentKind::Heading,
            text: strip_inline_markdown(text.trim()).to_owned(),
        }];
    }

    if let Some(text) = trimmed.strip_prefix("- ") {
        return vec![DisplaySegment {
            kind: SegmentKind::Bullet,
            text: format!("• {}", strip_inline_markdown(text.trim())),
        }];
    }

    vec![DisplaySegment {
        kind: SegmentKind::Body,
        text: strip_inline_markdown(trimmed).to_owned(),
    }]
}

fn normalize_markdown_flow(content: &str) -> String {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut out = String::with_capacity(normalized.len() + 16);
    let chars: Vec<char> = normalized.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        if starts_with(&chars, i, "###") || starts_with(&chars, i, "## ") {
            push_break_if_needed(&mut out);
        }

        if starts_with(&chars, i, "- ") && !line_is_empty(&out) {
            push_break_if_needed(&mut out);
        }

        out.push(chars[i]);
        i += 1;
    }

    out
}

fn starts_with(chars: &[char], start: usize, pattern: &str) -> bool {
    pattern
        .chars()
        .enumerate()
        .all(|(offset, expected)| chars.get(start + offset) == Some(&expected))
}

fn push_break_if_needed(out: &mut String) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
}

fn line_is_empty(out: &str) -> bool {
    out.rsplit('\n')
        .next()
        .unwrap_or_default()
        .trim()
        .is_empty()
}

fn strip_inline_markdown(text: &str) -> String {
    text.replace("**", "")
        .replace("__", "")
        .replace('`', "")
        .trim()
        .to_owned()
}

fn segment_style(kind: SegmentKind, app: &App) -> Style {
    let palette = theme::palette(app.theme);
    match kind {
        SegmentKind::Body => Style::default().fg(palette.text),
        SegmentKind::Heading => Style::default()
            .fg(palette.accent)
            .add_modifier(Modifier::BOLD),
        SegmentKind::Bullet => Style::default().fg(palette.text),
    }
}
