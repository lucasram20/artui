use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
    Frame,
};

use crate::{
    agent::PrimaryAgent,
    app::{App, StatusLineItem, ThemeId},
    providers::registry::{provider_display_name, AuthRequirement, LOGIN_PROVIDERS},
    ui::{geometry, layout::theme},
};

pub fn draw_modal_backdrop(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    frame.render_widget(
        Block::default().style(Style::default().bg(theme::palette(app.theme).bg)),
        area,
    );
}

pub fn draw_theme_picker(frame: &mut Frame<'_>, app: &App) {
    let item_count = ThemeId::ALL.len() as u16;
    let area = geometry::selector_area(frame.area(), 76, item_count.saturating_add(8));
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

pub fn draw_statusline_picker(frame: &mut Frame<'_>, app: &App) {
    let area = geometry::centered(frame.area(), 84, 18);
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

pub fn draw_model_picker(frame: &mut Frame<'_>, app: &App) {
    let item_count = app.model_options.len().max(1) as u16;
    let area = geometry::selector_area(frame.area(), 78, item_count.saturating_add(8));
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
                    provider_display_name(&app.config.default_provider),
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
            "No models found. Connected account providers may still be refreshing.",
            Style::default().fg(palette.muted),
        )))]
    } else {
        let visible_rows = rows[1].height as usize;
        let start = app
            .model_scroll
            .min(app.model_options.len().saturating_sub(visible_rows));
        let end = start
            .saturating_add(visible_rows)
            .min(app.model_options.len());
        app.model_options[start..end]
            .iter()
            .enumerate()
            .map(|(index, option)| {
                let index = start + index;
                let is_selected = index == app.model_cursor;
                let is_active = option.provider_id == app.config.default_provider
                    && option.model.as_deref() == Some(app.active_model());
                if option.model.is_none() {
                    return ListItem::new(Line::from(vec![
                        Span::styled("  ", Style::default().fg(palette.subtle)),
                        Span::styled(
                            option.provider_name.clone(),
                            Style::default()
                                .fg(palette.accent)
                                .add_modifier(Modifier::BOLD),
                        ),
                    ]));
                }
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
                let model = option.model.clone().unwrap_or_default();
                let hint = option.hint.clone().unwrap_or_default();
                ListItem::new(Line::from(vec![
                    Span::styled(selector_pointer(is_selected), item_style),
                    Span::styled(selected_mark(is_active), item_style),
                    Span::styled(format!("{model:<28}"), item_style),
                    Span::styled(hint, Style::default().fg(palette.subtle)),
                ]))
            })
            .collect::<Vec<_>>()
    };
    frame.render_widget(List::new(items), rows[1]);

    draw_model_selector_help(frame, app, rows[2]);
}

fn draw_model_selector_help(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let palette = theme::palette(app.theme);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("↑/↓", Style::default().fg(palette.accent)),
            Span::styled(" or ", Style::default().fg(palette.subtle)),
            Span::styled("j/k", Style::default().fg(palette.accent)),
            Span::styled(" move   ", Style::default().fg(palette.subtle)),
            Span::styled("enter", Style::default().fg(palette.accent)),
            Span::styled(" switch   ", Style::default().fg(palette.subtle)),
            Span::styled("L", Style::default().fg(palette.accent)),
            Span::styled(" login   ", Style::default().fg(palette.subtle)),
            Span::styled("D", Style::default().fg(palette.accent)),
            Span::styled(" disconnect   ", Style::default().fg(palette.subtle)),
            Span::styled("esc", Style::default().fg(palette.accent)),
            Span::styled(" close", Style::default().fg(palette.subtle)),
        ]))
        .alignment(Alignment::Center)
        .style(Style::default().bg(palette.bg)),
        area,
    );
}

pub fn draw_agent_picker(frame: &mut Frame<'_>, app: &App) {
    let item_count = PrimaryAgent::ALL.len() as u16;
    let area = geometry::selector_area(frame.area(), 76, item_count.saturating_add(8));
    render_popup_surface(frame, app, area);

    let palette = theme::palette(app.theme);
    let block = selector_block(app, "Select Agent", "/agent");
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
                "Choose the primary agent behavior for the next turn.",
                Style::default().fg(palette.text),
            )),
            Line::from(vec![
                Span::styled("Current ", Style::default().fg(palette.muted)),
                Span::styled(app.active_agent.name(), Style::default().fg(palette.accent)),
            ]),
        ])
        .style(Style::default().bg(palette.bg)),
        rows[0],
    );

    let items = PrimaryAgent::ALL
        .iter()
        .enumerate()
        .map(|(index, agent)| {
            let is_selected = index == app.agent_cursor;
            let is_active = *agent == app.active_agent;
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
            let description_style = if is_selected {
                Style::default().fg(palette.text).bg(palette.rule)
            } else {
                Style::default().fg(palette.muted)
            };
            ListItem::new(Line::from(vec![
                Span::styled(selector_pointer(is_selected), item_style),
                Span::styled(selected_mark(is_active), item_style),
                Span::styled(format!("{:<10}", agent.name()), item_style),
                Span::styled(agent.description(), description_style),
            ]))
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items), rows[1]);

    draw_selector_help(frame, app, rows[2], "select");
}

pub fn draw_login_picker(frame: &mut Frame<'_>, app: &App) {
    let item_count = LOGIN_PROVIDERS.len() as u16;
    let area = geometry::selector_area(frame.area(), 82, item_count.saturating_add(8));
    render_popup_surface(frame, app, area);

    let palette = theme::palette(app.theme);
    let block = selector_block(app, "Provider Login", "/login");
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

    let auth_path = app
        .auth_store
        .as_ref()
        .map(|store| store.path().display().to_string())
        .unwrap_or_else(|| "unavailable".to_owned());
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "Choose a provider to connect or inspect.",
                Style::default().fg(palette.text),
            )),
            Line::from(vec![
                Span::styled("Auth store ", Style::default().fg(palette.muted)),
                Span::styled(auth_path, Style::default().fg(palette.subtle)),
            ]),
        ])
        .style(Style::default().bg(palette.bg)),
        rows[0],
    );

    let items = LOGIN_PROVIDERS
        .iter()
        .enumerate()
        .map(|(index, provider)| {
            let is_selected = index == app.login_cursor;
            let is_connected = matches!(
                provider.auth_requirement,
                AuthRequirement::None | AuthRequirement::ApiKey
            ) || app
                .provider_status_label(provider.id)
                .starts_with("connected");
            let item_style = if is_selected {
                Style::default()
                    .fg(palette.accent)
                    .bg(palette.rule)
                    .add_modifier(Modifier::BOLD)
            } else if is_connected {
                Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(palette.muted)
            };
            let description_style = if is_selected {
                Style::default().fg(palette.text).bg(palette.rule)
            } else {
                Style::default().fg(palette.muted)
            };
            let short_name = truncate_provider_name(provider.display_name);
            ListItem::new(Line::from(vec![
                Span::styled(selector_pointer(is_selected), item_style),
                Span::styled(selected_mark(is_connected), item_style),
                Span::styled(format!("{:<18}", short_name), item_style),
                Span::styled(
                    login_status_short(app, provider.id, is_connected),
                    description_style,
                ),
            ]))
        })
        .collect::<Vec<_>>();
    frame.render_widget(List::new(items), rows[1]);

    draw_selector_help(frame, app, rows[2], "connect");
}

fn login_status_short(app: &App, provider_id: &str, is_connected: bool) -> String {
    if is_connected {
        "ready".to_owned()
    } else if provider_id == "ollama" {
        "local".to_owned()
    } else {
        let status = app.provider_status_label(provider_id);
        if status.contains("not connected") {
            "sign in needed".to_owned()
        } else if status.contains("missing") {
            "key needed".to_owned()
        } else if status.contains("expired") {
            "expired".to_owned()
        } else {
            status
        }
    }
}

fn truncate_provider_name(name: &str) -> &str {
    match name {
        "OpenAI-compatible API" => "OpenAI-compat",
        "OpenAI Codex" => "OpenAI Codex",
        other => other,
    }
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

fn render_popup_surface(frame: &mut Frame<'_>, app: &App, area: Rect) {
    if let Some(shadow) = geometry::shadow_area(area, frame.area()) {
        frame.render_widget(
            Block::default().style(Style::default().bg(theme::palette(app.theme).rule)),
            shadow,
        );
    }
    frame.render_widget(Clear, area);
}
