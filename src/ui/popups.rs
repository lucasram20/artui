use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::{
    app::{App, StatusLineItem, ThemeId},
    ui::layout::theme,
};

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    if app.theme_picker_open || app.model_picker_open {
        draw_modal_backdrop(frame, app);
    }
    if app.theme_picker_open {
        draw_theme_picker(frame, app);
    }
    if app.model_picker_open {
        draw_model_picker(frame, app);
    }
    if app.statusline_open {
        draw_statusline_picker(frame, app);
    }
}

fn draw_modal_backdrop(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme::palette(app.theme).bg)),
        area,
    );
}

fn draw_theme_picker(frame: &mut Frame<'_>, app: &App) {
    let item_count = ThemeId::ALL.len() as u16;
    let area = selector_area(frame.area(), 76, item_count.saturating_add(8));
    render_popup_surface(frame, app, area);

    let palette = theme::palette(app.theme);
    let block = selector_block(app, "Select Theme", "/theme");
    let inner = block.inner(area).inner(Margin {
        vertical: 1,
        horizontal: 2,
    });
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(item_count),
            Constraint::Length(2),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Choose the palette for this terminal session.",
                Style::default().fg(palette.text),
            )),
            Line::from(vec![
                Span::styled("Current ", Style::default().fg(palette.muted)),
                Span::styled(app.theme.name(), Style::default().fg(palette.accent)),
            ]),
        ])
        .style(Style::default().bg(palette.bg)),
        rows[0],
    );

    let items = ThemeId::ALL
        .iter()
        .enumerate()
        .map(|(index, theme_id)| {
            let is_selected = index == app.theme_cursor;
            let is_active = *theme_id == app.theme;
            let theme_palette = theme::palette(*theme_id);
            let name_style = if is_selected {
                Style::default()
                    .fg(palette.accent)
                    .bg(palette.rule)
                    .add_modifier(Modifier::BOLD)
            } else if is_active {
                Style::default()
                    .fg(theme_palette.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme_palette.accent)
            };
            let description_style = if is_selected {
                Style::default().fg(palette.text).bg(palette.rule)
            } else {
                Style::default().fg(palette.muted)
            };
            ListItem::new(Line::from(vec![
                Span::styled(selector_pointer(is_selected), name_style),
                Span::styled(selected_mark(is_active), name_style),
                Span::styled(format!("{:<19}", theme_id.name()), name_style),
                Span::styled(theme_id.description(), description_style),
            ]))
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items), rows[1]);

    draw_selector_help(frame, app, rows[2], "apply");
}

fn draw_statusline_picker(frame: &mut Frame<'_>, app: &App) {
    let area = centered(frame.area(), 84, 18);
    frame.render_widget(Clear, area);

    let palette = theme::palette(app.theme);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Line::from(vec![
            Span::styled(" /statusline ", Style::default().fg(palette.accent)),
            Span::styled(
                "configure footer items ",
                Style::default().fg(palette.muted),
            ),
        ]))
        .border_style(Style::default().fg(palette.accent))
        .style(Style::default().bg(palette.bg));
    let inner = block.inner(area).inner(Margin {
        vertical: 1,
        horizontal: 2,
    });
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(StatusLineItem::ALL.len() as u16),
            Constraint::Length(3),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Select which items to display in the statusline.",
                Style::default().fg(palette.text),
            )),
            Line::from(Span::styled(
                "Space toggles an item. Enter confirms. Esc cancels.",
                Style::default().fg(palette.muted),
            )),
        ])
        .style(Style::default().bg(palette.bg)),
        rows[0],
    );

    let items = StatusLineItem::ALL
        .iter()
        .map(|item| {
            let enabled = app.statusline_enabled[item.index()];
            let selected = item.index() == app.statusline_cursor;
            let item_style = if selected {
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette.text)
            };
            let description_style = if selected {
                Style::default().fg(palette.text)
            } else {
                Style::default().fg(palette.muted)
            };
            ListItem::new(Line::from(vec![
                Span::styled(if selected { "› " } else { "  " }, item_style),
                Span::styled(if enabled { "[x] " } else { "[ ] " }, item_style),
                Span::styled(format!("{:<14}", item.label()), item_style),
                Span::styled(item.description(), description_style),
            ]))
        })
        .collect::<Vec<_>>();
    let list = List::new(items);
    frame.render_widget(list, rows[1]);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("↑/↓ or j/k", Style::default().fg(palette.accent)),
            Span::styled(" move   ", Style::default().fg(palette.subtle)),
            Span::styled("space", Style::default().fg(palette.accent)),
            Span::styled(" toggle   ", Style::default().fg(palette.subtle)),
            Span::styled("enter", Style::default().fg(palette.accent)),
            Span::styled(" confirm   ", Style::default().fg(palette.subtle)),
            Span::styled("esc", Style::default().fg(palette.accent)),
            Span::styled(" close", Style::default().fg(palette.subtle)),
        ]))
        .alignment(Alignment::Center)
        .style(Style::default().bg(palette.bg)),
        rows[2],
    );
}

fn draw_model_picker(frame: &mut Frame<'_>, app: &App) {
    let item_count = app.model_options.len().max(1) as u16;
    let area = selector_area(frame.area(), 78, item_count.saturating_add(8));
    render_popup_surface(frame, app, area);

    let palette = theme::palette(app.theme);
    let block = selector_block(app, "Select Model", "/model");
    let inner = block.inner(area).inner(Margin {
        vertical: 1,
        horizontal: 2,
    });
    frame.render_widget(block, area);

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(item_count),
            Constraint::Length(2),
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Choose the model used for the next turn.",
                Style::default().fg(palette.text),
            )),
            Line::from(vec![
                Span::styled("Provider ", Style::default().fg(palette.muted)),
                Span::styled(
                    &app.config.default_provider,
                    Style::default().fg(palette.accent),
                ),
                Span::styled("  Current ", Style::default().fg(palette.muted)),
                Span::styled(app.active_model(), Style::default().fg(palette.text)),
            ]),
        ])
        .style(Style::default().bg(palette.bg)),
        rows[0],
    );

    let items = if app.model_options.is_empty() {
        vec![ListItem::new(Line::from(Span::styled(
            "No models found. Use /model <name> to set one manually.",
            Style::default().fg(palette.muted),
        )))]
    } else {
        app.model_options
            .iter()
            .enumerate()
            .map(|(index, model)| {
                let is_selected = index == app.model_cursor;
                let is_active = model == app.active_model();
                let item_style = if is_selected {
                    Style::default()
                        .fg(palette.accent)
                        .bg(palette.rule)
                        .add_modifier(Modifier::BOLD)
                } else if is_active {
                    Style::default()
                        .fg(palette.text)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(palette.muted)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(selector_pointer(is_selected), item_style),
                    Span::styled(selected_mark(is_active), item_style),
                    Span::styled(model.clone(), item_style),
                ]))
            })
            .collect::<Vec<_>>()
    };
    frame.render_widget(List::new(items), rows[1]);

    draw_selector_help(frame, app, rows[2], "switch");
}

fn selector_block<'a>(app: &App, title: &'a str, command: &'a str) -> Block<'a> {
    let palette = theme::palette(app.theme);
    Block::default()
        .borders(Borders::ALL)
        .title(Line::from(vec![
            Span::styled(" ", Style::default().fg(palette.accent)),
            Span::styled(
                title,
                Style::default()
                    .fg(palette.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ", Style::default().fg(palette.accent)),
            Span::styled(command, Style::default().fg(palette.muted)),
            Span::styled(" ", Style::default().fg(palette.accent)),
        ]))
        .border_style(Style::default().fg(palette.accent))
        .style(Style::default().bg(palette.bg))
}

fn draw_selector_help(frame: &mut Frame<'_>, app: &App, area: Rect, confirm_label: &str) {
    let palette = theme::palette(app.theme);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("↑/↓", Style::default().fg(palette.accent)),
            Span::styled(" or ", Style::default().fg(palette.subtle)),
            Span::styled("j/k", Style::default().fg(palette.accent)),
            Span::styled(" move   ", Style::default().fg(palette.subtle)),
            Span::styled("enter", Style::default().fg(palette.accent)),
            Span::styled(
                format!(" {confirm_label}   "),
                Style::default().fg(palette.subtle),
            ),
            Span::styled("esc", Style::default().fg(palette.accent)),
            Span::styled(" close", Style::default().fg(palette.subtle)),
        ]))
        .alignment(Alignment::Center)
        .style(Style::default().bg(palette.bg)),
        area,
    );
}

fn selector_pointer(is_selected: bool) -> &'static str {
    if is_selected {
        "› "
    } else {
        "  "
    }
}

fn selected_mark(is_active: bool) -> &'static str {
    if is_active {
        "● "
    } else {
        "  "
    }
}

fn selector_area(area: Rect, width: u16, height: u16) -> Rect {
    centered(
        area,
        width.min(MAX_SELECTOR_WIDTH),
        height.clamp(MIN_SELECTOR_HEIGHT, MAX_SELECTOR_HEIGHT),
    )
}

fn render_popup_surface(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if let Some(shadow) = shadow_area(area, frame.area()) {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::palette(app.theme).rule)),
            shadow,
        );
    }
    frame.render_widget(Clear, area);
}

fn shadow_area(area: Rect, bounds: Rect) -> Option<Rect> {
    let x = area.x.checked_add(POPUP_SHADOW_OFFSET)?;
    let y = area.y.checked_add(POPUP_SHADOW_OFFSET)?;
    let width = area.width.min(bounds.right().saturating_sub(x));
    let height = area.height.min(bounds.bottom().saturating_sub(y));
    (width > 0 && height > 0).then_some(Rect {
        x,
        y,
        width,
        height,
    })
}

const MAX_SELECTOR_WIDTH: u16 = 82;
const MIN_SELECTOR_HEIGHT: u16 = 10;
const MAX_SELECTOR_HEIGHT: u16 = 22;
const POPUP_SHADOW_OFFSET: u16 = 1;

fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width.saturating_sub(2));
    let height = height.min(area.height.saturating_sub(2));
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}
