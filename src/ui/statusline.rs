//! Footer statusline and composer title strip composition.

use ratatui::{
    layout::{Alignment, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

use crate::{
    app::{App, ReasoningEffort, StatusLineItem, ThemeId},
    terminal_preset,
    ui::{cells, layout::theme},
};

pub fn draw_footer(frame: &mut Frame<'_>, app: &App, theme: ThemeId, area: Rect) {
    let palette = theme::palette(theme);
    let budget = area.width as usize;
    let mut spans = Vec::new();

    if statusline_enabled(app, StatusLineItem::CurrentDir) {
        spans.push(Span::styled(
            compact_cwd(),
            Style::default().fg(palette.muted),
        ));
    }

    if statusline_enabled(app, StatusLineItem::ProjectName) {
        if let Some(name) = project_dir_name() {
            push_separator(&mut spans, palette);
            spans.push(Span::styled(name, Style::default().fg(palette.muted)));
        }
    }

    if statusline_enabled(app, StatusLineItem::GitBranch) && app.git_branch_label != "no-git" {
        push_separator(&mut spans, palette);
        spans.push(Span::styled("(", Style::default().fg(palette.muted)));
        spans.push(Span::styled(
            app.git_branch_label.clone(),
            Style::default().fg(palette.muted),
        ));
        if statusline_enabled(app, StatusLineItem::GitStatus)
            && app.git_status_label != "clean"
            && app.git_status_label != "unknown"
        {
            spans.push(Span::styled(" ±", Style::default().fg(palette.muted)));
            spans.push(Span::styled(
                app.git_status_label.clone(),
                Style::default().fg(palette.pink),
            ));
        }
        spans.push(Span::styled(")", Style::default().fg(palette.muted)));
    } else if statusline_enabled(app, StatusLineItem::GitStatus)
        && app.git_status_label != "clean"
        && app.git_status_label != "unknown"
    {
        push_separator(&mut spans, palette);
        spans.push(Span::styled(
            app.git_status_label.clone(),
            Style::default().fg(palette.pink),
        ));
    }

    if statusline_enabled(app, StatusLineItem::Context) {
        push_separator(&mut spans, palette);
        let used = cells::spans_display_width(&spans);
        let remaining = budget.saturating_sub(used);
        spans.extend(context_bar_spans(app, palette, remaining));
    }

    let fitted = cells::fit_spans_to_width(spans, budget);

    frame.render_widget(
        Paragraph::new(Line::from(fitted))
            .alignment(Alignment::Left)
            .style(Style::default().fg(palette.text).bg(palette.bg)),
        area,
    );
}

pub fn draw_input_titles(frame: &mut Frame<'_>, app: &App, theme: ThemeId, area: Rect) {
    if area.width < 8 {
        return;
    }
    let palette = theme::palette(theme);
    let dot = " · ";

    if statusline_enabled(app, StatusLineItem::Agent) {
        let agent_name = app.active_agent_name();
        let eye = app.eye_glyph();
        let left_text = format!("{eye}{dot}{agent_name}");
        let left_width = cells::display_width(&left_text).saturating_add(2) as u16;
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(" ", Style::default().fg(palette.border).bg(palette.bg)),
                Span::styled(eye, Style::default().fg(palette.accent).bg(palette.bg)),
                Span::styled(dot, Style::default().fg(palette.subtle).bg(palette.bg)),
                Span::styled(
                    agent_name,
                    Style::default()
                        .fg(palette.accent)
                        .bg(palette.bg)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(" ", Style::default().fg(palette.border).bg(palette.bg)),
            ])),
            Rect {
                x: area.x.saturating_add(2),
                y: area.y,
                width: left_width.min(area.width / 2),
                height: 1,
            },
        );
    }

    let mut spans = vec![Span::styled(
        " ",
        Style::default().fg(palette.border).bg(palette.bg),
    )];
    let mut width_used = 2usize;
    let budget = area.width.saturating_sub(10) as usize;
    let mut first = true;

    if statusline_enabled(app, StatusLineItem::ProviderUsage) {
        let provider = app.provider_usage_label().to_lowercase();
        width_used +=
            cells::display_width(&provider) + if first { 0 } else { cells::display_width(dot) };
        if width_used <= budget {
            if !first {
                spans.push(Span::styled(
                    dot,
                    Style::default().fg(palette.subtle).bg(palette.bg),
                ));
            }
            spans.push(Span::styled(
                provider,
                Style::default().fg(palette.muted).bg(palette.bg),
            ));
            first = false;
        }
    }
    if statusline_enabled(app, StatusLineItem::Model) && width_used < budget {
        let model = app.active_model().to_owned();
        let extra =
            cells::display_width(&model) + if first { 0 } else { cells::display_width(dot) };
        if width_used + extra <= budget {
            if !first {
                spans.push(Span::styled(
                    dot,
                    Style::default().fg(palette.subtle).bg(palette.bg),
                ));
            }
            spans.push(Span::styled(
                model,
                Style::default().fg(palette.text).bg(palette.bg),
            ));
            width_used += extra;
            first = false;
        }
    }
    if statusline_enabled(app, StatusLineItem::Reasoning) && width_used < budget {
        let reasoning = app.reasoning_effort.label().to_owned();
        let extra =
            cells::display_width(&reasoning) + if first { 0 } else { cells::display_width(dot) };
        if width_used + extra <= budget {
            if !first {
                spans.push(Span::styled(
                    dot,
                    Style::default().fg(palette.subtle).bg(palette.bg),
                ));
            }
            spans.push(Span::styled(
                reasoning,
                reasoning_effort_style(palette, app.reasoning_effort),
            ));
        }
    }

    if spans.len() <= 1 {
        return;
    }

    spans.push(Span::styled(
        " ",
        Style::default().fg(palette.border).bg(palette.bg),
    ));
    let right_area_width = cells::spans_display_width(&spans).saturating_add(1) as u16;

    frame.render_widget(
        Paragraph::new(Line::from(spans)),
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

/// Render context usage — full bar when there is room, otherwise a short percent label.
pub(crate) fn context_bar_spans(
    app: &App,
    palette: theme::Palette,
    budget: usize,
) -> Vec<Span<'static>> {
    use ratatui::style::Color;

    let percent = app.context_usage_percent() as usize;
    const BAR_PREFIX: usize = 4; // "ctx "
    const BAR_WIDTH: usize = 10;

    if budget < BAR_PREFIX + 3 {
        return Vec::new();
    }

    if budget < BAR_PREFIX + BAR_WIDTH {
        return vec![Span::styled(
            format!("ctx {percent}%"),
            Style::default().fg(palette.muted),
        )];
    }

    let filled = (percent * BAR_WIDTH) / 100;
    let bar_color = match percent {
        0..=50 => palette.green,
        51..=75 => Color::Yellow,
        _ => palette.pink,
    };

    vec![
        Span::styled("ctx ", Style::default().fg(palette.muted)),
        Span::styled(
            terminal_preset::context_bar_fill(filled),
            Style::default().fg(bar_color),
        ),
        Span::styled(
            terminal_preset::context_bar_empty(BAR_WIDTH - filled),
            Style::default().fg(palette.subtle),
        ),
    ]
}

fn statusline_enabled(app: &App, item: StatusLineItem) -> bool {
    app.statusline_enabled[item.index()]
}

fn push_separator(spans: &mut Vec<Span<'static>>, palette: theme::Palette) {
    if spans.is_empty() {
        return;
    }
    let sep = if terminal_preset::use_legacy_glyphs() {
        " | "
    } else {
        " │ "
    };
    spans.push(Span::styled(sep, Style::default().fg(palette.subtle)));
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

pub(crate) fn reasoning_effort_color(
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
        .map(|path| crate::util::paths::compact_display_path(&path))
        .unwrap_or_else(|| "~".to_owned())
}

fn project_dir_name() -> Option<String> {
    std::env::current_dir().ok().and_then(|path| {
        path.file_name()
            .map(|name| name.to_string_lossy().into_owned())
    })
}
