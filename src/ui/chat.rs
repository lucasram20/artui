use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};

use crate::{
    app::{App, Role, StatusLineItem, ThemeId, UiMode},
    ui::layout::theme,
};

#[derive(Default)]
pub struct TranscriptRenderCache {
    entries: Vec<Option<CachedMessageLines>>,
}

#[derive(Clone)]
struct CachedMessageLines {
    content_hash: u64,
    lines: Vec<Line<'static>>,
}

fn hash_content(content: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    content.hash(&mut hasher);
    hasher.finish()
}

pub fn draw(
    frame: &mut Frame<'_>,
    app: &App,
    theme_id: ThemeId,
    area: Rect,
    cache: &mut TranscriptRenderCache,
) {
    let palette = theme::palette(theme_id);
    let mut lines = Vec::new();
    let streaming_last = app.mode == UiMode::Streaming
        && app
            .transcript
            .last()
            .is_some_and(|message| message.role == Role::Assistant);

    if cache.entries.len() != app.transcript.len() {
        cache.entries.resize(app.transcript.len(), None);
    }

    for (index, message) in app.transcript.iter().enumerate() {
        let (marker, color) = match message.role {
            Role::User => ("›", palette.accent),
            Role::Assistant => ("•", palette.green),
        };
        let content_hash = hash_content(&message.content);
        let must_rebuild = streaming_last && index + 1 == app.transcript.len()
            || cache.entries[index]
                .as_ref()
                .is_none_or(|entry| entry.content_hash != content_hash);

        if must_rebuild {
            let built = if message.content.is_empty() {
                vec![thinking_line(app, theme_id, marker, color)]
            } else {
                render_message_lines(message, theme_id, marker, color)
            };
            cache.entries[index] = Some(CachedMessageLines {
                content_hash,
                lines: built,
            });
        }

        if let Some(entry) = &cache.entries[index] {
            lines.extend(entry.lines.iter().cloned());
        }
        lines.push(Line::from(""));
    }

    // Active todo list (Phase N5) — render below the transcript so it
    // sits next to the spinner while the agent is working. Rendering
    // last keeps it visible without scroll math intervening.
    if !app.todos.is_empty() {
        lines.extend(render_todo_list(app, theme_id));
        lines.push(Line::from(""));
    }

    let scroll = transcript_scroll_offset(app, area) as usize;
    let viewport_lines = area.height.max(1) as usize;
    const OVERSCAN: usize = 4;
    let start = scroll.saturating_sub(OVERSCAN.min(scroll));
    let end = (scroll + viewport_lines + OVERSCAN).min(lines.len());
    let window = if start < end {
        &lines[start..end]
    } else {
        &lines[..]
    };
    let paragraph = Paragraph::new(window.to_vec())
        .style(Style::default().fg(palette.text).bg(palette.bg))
        .scroll(((scroll - start) as u16, 0))
        .wrap(Wrap { trim: false });
    frame.render_widget(paragraph, area);
}

fn render_message_lines(
    message: &crate::app::Message,
    theme_id: ThemeId,
    marker: &str,
    color: ratatui::style::Color,
) -> Vec<Line<'static>> {
    let segments = parse_markdown(&message.content);
    let mut lines = Vec::new();
    for (index, segment) in segments.iter().enumerate() {
        let prefix = if index == 0 {
            Span::styled(
                format!("{marker} "),
                Style::default().fg(color).add_modifier(Modifier::BOLD),
            )
        } else {
            Span::raw("  ")
        };

        let mut spans = vec![prefix];
        spans.extend(render_segment(segment, theme_id));
        lines.push(Line::from(spans));
    }
    lines
}

/// Render the active todo list as a checklist with header showing
/// "[done/total tasks]". Pending items render in muted color, the active
/// `in_progress` item gets the accent color so the user can spot
/// what's running, and completed items are dimmed and struck-through.
fn render_todo_list(app: &App, theme_id: ThemeId) -> Vec<Line<'static>> {
    use crate::tools::todo_write::TodoStatus;
    let palette = theme::palette(theme_id);
    let total = app.todos.len();
    let done = app
        .todos
        .iter()
        .filter(|t| t.status == TodoStatus::Completed)
        .count();
    let mut lines = Vec::with_capacity(total + 2);

    lines.push(Line::from(vec![
        Span::styled(
            "★ ",
            Style::default()
                .fg(palette.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!("Tasks ({done}/{total} done)"),
            Style::default()
                .fg(palette.text)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    for todo in &app.todos {
        let (color, modifier) = match todo.status {
            TodoStatus::Pending => (palette.muted, Modifier::empty()),
            TodoStatus::InProgress => (palette.accent, Modifier::BOLD),
            TodoStatus::Completed => (palette.muted, Modifier::DIM | Modifier::CROSSED_OUT),
        };
        lines.push(Line::from(vec![
            Span::raw("  "),
            Span::styled(
                format!("{} ", todo.status.glyph()),
                Style::default().fg(color).add_modifier(modifier),
            ),
            Span::styled(
                todo.subject.clone(),
                Style::default().fg(color).add_modifier(modifier),
            ),
        ]));
    }
    lines
}

fn thinking_line(
    app: &App,
    theme_id: ThemeId,
    marker: &str,
    marker_color: ratatui::style::Color,
) -> Line<'static> {
    let palette = theme::palette(theme_id);
    let elapsed = app
        .thinking_elapsed()
        .map(|duration| format_thinking_meta(app, duration))
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

/// Format the spinner header trailing meta block: elapsed wall-clock and
/// cumulative output tokens for the active turn. Mirrors Claude Code's
/// `(12m 50s · ↓ 26.3k tokens)` shape so the user can see at a glance how
/// long the agent has been working and how many tokens it's burning.
fn format_thinking_meta(app: &App, duration: std::time::Duration) -> String {
    let mut meta = String::from(" (");
    meta.push_str(&format_elapsed(duration));
    if app.turn_output_tokens > 0 {
        meta.push_str(" · ↓ ");
        meta.push_str(&format_token_count(app.turn_output_tokens));
        meta.push_str(" tokens");
    }
    if app.statusline_enabled[StatusLineItem::EscHint.index()] {
        meta.push_str(" · esc to interrupt)");
    } else {
        meta.push(')');
    }
    meta
}

/// Render an elapsed `Duration` as `42s` / `12m 50s` / `1h 5m`. Drops the
/// seconds entirely when over an hour to keep the header narrow on small
/// terminals.
fn format_elapsed(duration: std::time::Duration) -> String {
    let secs = duration.as_secs();
    if secs < 60 {
        format!("{secs}s")
    } else if secs < 3600 {
        let m = secs / 60;
        let s = secs % 60;
        format!("{m}m {s}s")
    } else {
        let h = secs / 3600;
        let m = (secs % 3600) / 60;
        format!("{h}h {m}m")
    }
}

/// Render a raw token count as `780` / `4.2k` / `1.2M`. Tracks Claude
/// Code's display style so users moving between tools see the same shape.
fn format_token_count(tokens: u32) -> String {
    if tokens < 1_000 {
        format!("{tokens}")
    } else if tokens < 1_000_000 {
        let k = tokens as f64 / 1_000.0;
        // Drop the trailing `.0` for clean integer thousands.
        if (k - k.round()).abs() < 0.05 {
            format!("{:.0}k", k)
        } else {
            format!("{:.1}k", k)
        }
    } else {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    }
}

#[cfg(test)]
mod chat_meta_tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn elapsed_under_minute() {
        assert_eq!(format_elapsed(Duration::from_secs(42)), "42s");
    }

    #[test]
    fn elapsed_minutes_and_seconds() {
        assert_eq!(format_elapsed(Duration::from_secs(12 * 60 + 50)), "12m 50s");
    }

    #[test]
    fn elapsed_hours_and_minutes() {
        assert_eq!(format_elapsed(Duration::from_secs(3600 + 5 * 60)), "1h 5m");
    }

    #[test]
    fn token_count_below_1k() {
        assert_eq!(format_token_count(780), "780");
    }

    #[test]
    fn token_count_thousands_with_decimal() {
        assert_eq!(format_token_count(4_200), "4.2k");
        assert_eq!(format_token_count(26_300), "26.3k");
    }

    #[test]
    fn token_count_round_thousands_drop_decimal() {
        assert_eq!(format_token_count(5_000), "5k");
        assert_eq!(format_token_count(10_000), "10k");
    }

    #[test]
    fn token_count_millions() {
        assert_eq!(format_token_count(1_200_000), "1.2M");
    }
}

pub(crate) fn transcript_scroll_offset(app: &App, area: Rect) -> u16 {
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
    let segments = parse_markdown(content);
    let content_height = if segments.is_empty() {
        1
    } else {
        segments
            .iter()
            .map(|segment| {
                let prefix_width = 2;
                let text_len = segment_text_len(segment);
                let line_width = text_len + prefix_width;
                line_width.max(1).div_ceil(width.max(1))
            })
            .sum::<usize>()
    };

    content_height + 1
}

// ── Markdown parsing ───────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq)]
enum SegmentKind {
    Body,
    Heading,
    Bullet,
    CodeBlock,
    BlankLine,
    DiffAdd,
    DiffRemove,
    DiffHeader,
}

enum InlineChunk {
    Plain(String),
    Bold(String),
    Code(String),
}

struct DisplaySegment {
    kind: SegmentKind,
    chunks: Vec<InlineChunk>,
}

fn segment_text_len(segment: &DisplaySegment) -> usize {
    segment
        .chunks
        .iter()
        .map(|chunk| match chunk {
            InlineChunk::Plain(t) | InlineChunk::Bold(t) | InlineChunk::Code(t) => {
                t.chars().count()
            }
        })
        .sum()
}

/// Parse markdown content into display segments with inline formatting.
fn parse_markdown(content: &str) -> Vec<DisplaySegment> {
    let normalized = content.replace("\r\n", "\n").replace('\r', "\n");
    let lines: Vec<&str> = normalized.lines().collect();
    let mut segments = Vec::new();
    let mut in_code_block = false;
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];

        // Code fence detection
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            i += 1;
            continue;
        }

        if in_code_block {
            segments.push(DisplaySegment {
                kind: SegmentKind::CodeBlock,
                chunks: vec![InlineChunk::Code(format!("  {line}"))],
            });
            i += 1;
            continue;
        }

        let trimmed = line.trim();

        // Blank lines → visual spacing
        if trimmed.is_empty() {
            segments.push(DisplaySegment {
                kind: SegmentKind::BlankLine,
                chunks: vec![InlineChunk::Plain(String::new())],
            });
            i += 1;
            continue;
        }

        // Headings
        if let Some(text) = trimmed.strip_prefix("### ") {
            segments.push(DisplaySegment {
                kind: SegmentKind::Heading,
                chunks: vec![InlineChunk::Bold(text.to_owned())],
            });
            i += 1;
            continue;
        }
        if let Some(text) = trimmed.strip_prefix("## ") {
            segments.push(DisplaySegment {
                kind: SegmentKind::Heading,
                chunks: vec![InlineChunk::Bold(text.to_owned())],
            });
            i += 1;
            continue;
        }
        if let Some(text) = trimmed.strip_prefix("# ") {
            segments.push(DisplaySegment {
                kind: SegmentKind::Heading,
                chunks: vec![InlineChunk::Bold(text.to_owned())],
            });
            i += 1;
            continue;
        }

        // Bullet lists (- or *)
        if let Some(text) = trimmed
            .strip_prefix("- ")
            .or_else(|| trimmed.strip_prefix("* "))
        {
            segments.push(DisplaySegment {
                kind: SegmentKind::Bullet,
                chunks: parse_inline(&format!("• {text}")),
            });
            i += 1;
            continue;
        }

        // Numbered lists (1. 2. etc.)
        if is_numbered_list(trimmed) {
            let text = trimmed.split(". ").nth(1).unwrap_or(trimmed);
            let num = trimmed.split('.').next().unwrap_or("1");
            segments.push(DisplaySegment {
                kind: SegmentKind::Bullet,
                chunks: parse_inline(&format!("{num}. {text}")),
            });
            i += 1;
            continue;
        }

        // Diff lines: detect unified diff format
        if is_diff_header(trimmed) {
            segments.push(DisplaySegment {
                kind: SegmentKind::DiffHeader,
                chunks: vec![InlineChunk::Plain(trimmed.to_owned())],
            });
            i += 1;
            continue;
        }
        if trimmed.starts_with('+') && !trimmed.starts_with("+++") {
            segments.push(DisplaySegment {
                kind: SegmentKind::DiffAdd,
                chunks: vec![InlineChunk::Plain(trimmed.to_owned())],
            });
            i += 1;
            continue;
        }
        if trimmed.starts_with('-') && !trimmed.starts_with("---") && !trimmed.starts_with("- ") {
            segments.push(DisplaySegment {
                kind: SegmentKind::DiffRemove,
                chunks: vec![InlineChunk::Plain(trimmed.to_owned())],
            });
            i += 1;
            continue;
        }

        // Regular body text
        segments.push(DisplaySegment {
            kind: SegmentKind::Body,
            chunks: parse_inline(trimmed),
        });
        i += 1;
    }

    segments
}

/// Check if a line is a diff header (---, +++, @@).
fn is_diff_header(line: &str) -> bool {
    line.starts_with("--- ") || line.starts_with("+++ ") || line.starts_with("@@ ")
}

/// Check if a line starts with a number followed by ". "
fn is_numbered_list(line: &str) -> bool {
    let mut chars = line.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_digit()) {
        return false;
    }
    for ch in chars.by_ref() {
        if ch == '.' {
            return chars.next() == Some(' ');
        }
        if !ch.is_ascii_digit() {
            return false;
        }
    }
    false
}

/// Parse inline markdown: **bold**, `code`, and plain text.
fn parse_inline(text: &str) -> Vec<InlineChunk> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        // Bold: **text**
        if i + 1 < chars.len() && chars[i] == '*' && chars[i + 1] == '*' {
            if !current.is_empty() {
                chunks.push(InlineChunk::Plain(std::mem::take(&mut current)));
            }
            i += 2;
            let mut bold_text = String::new();
            while i + 1 < chars.len() && !(chars[i] == '*' && chars[i + 1] == '*') {
                bold_text.push(chars[i]);
                i += 1;
            }
            if i + 1 < chars.len() {
                i += 2; // skip closing **
            }
            if !bold_text.is_empty() {
                chunks.push(InlineChunk::Bold(bold_text));
            }
            continue;
        }

        // Inline code: `text`
        if chars[i] == '`' {
            if !current.is_empty() {
                chunks.push(InlineChunk::Plain(std::mem::take(&mut current)));
            }
            i += 1;
            let mut code_text = String::new();
            while i < chars.len() && chars[i] != '`' {
                code_text.push(chars[i]);
                i += 1;
            }
            if i < chars.len() {
                i += 1; // skip closing `
            }
            if !code_text.is_empty() {
                chunks.push(InlineChunk::Code(code_text));
            }
            continue;
        }

        current.push(chars[i]);
        i += 1;
    }

    if !current.is_empty() {
        chunks.push(InlineChunk::Plain(current));
    }

    if chunks.is_empty() {
        chunks.push(InlineChunk::Plain(String::new()));
    }

    chunks
}

// ── Rendering ──────────────────────────────────────────────────────────

fn render_segment(segment: &DisplaySegment, theme_id: ThemeId) -> Vec<Span<'static>> {
    let palette = theme::palette(theme_id);

    match segment.kind {
        SegmentKind::BlankLine => vec![Span::raw("")],
        SegmentKind::Heading => segment
            .chunks
            .iter()
            .map(|chunk| {
                let text = chunk_text(chunk);
                Span::styled(
                    text,
                    Style::default()
                        .fg(palette.accent)
                        .add_modifier(Modifier::BOLD),
                )
            })
            .collect(),
        SegmentKind::CodeBlock => segment
            .chunks
            .iter()
            .map(|chunk| {
                let text = chunk_text(chunk);
                Span::styled(text, Style::default().fg(palette.muted))
            })
            .collect(),
        SegmentKind::DiffAdd => segment
            .chunks
            .iter()
            .map(|chunk| {
                let text = chunk_text(chunk);
                Span::styled(text, Style::default().fg(palette.green))
            })
            .collect(),
        SegmentKind::DiffRemove => segment
            .chunks
            .iter()
            .map(|chunk| {
                let text = chunk_text(chunk);
                Span::styled(text, Style::default().fg(palette.pink))
            })
            .collect(),
        SegmentKind::DiffHeader => segment
            .chunks
            .iter()
            .map(|chunk| {
                let text = chunk_text(chunk);
                Span::styled(text, Style::default().fg(palette.accent))
            })
            .collect(),
        SegmentKind::Body | SegmentKind::Bullet => segment
            .chunks
            .iter()
            .map(|chunk| match chunk {
                InlineChunk::Plain(t) => Span::styled(t.clone(), Style::default().fg(palette.text)),
                InlineChunk::Bold(t) => Span::styled(
                    t.clone(),
                    Style::default()
                        .fg(palette.text)
                        .add_modifier(Modifier::BOLD),
                ),
                InlineChunk::Code(t) => {
                    Span::styled(format!("`{t}`"), Style::default().fg(palette.muted))
                }
            })
            .collect(),
    }
}

fn chunk_text(chunk: &InlineChunk) -> String {
    match chunk {
        InlineChunk::Plain(t) | InlineChunk::Bold(t) | InlineChunk::Code(t) => t.clone(),
    }
}
