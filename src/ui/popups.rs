use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Margin, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph},
    Frame,
};

use crate::{
    app::{App, ThemeId},
    ui::layout::theme,
};

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    if !app.theme_picker_open {
        return;
    }

    let area = centered(frame.area(), 58, 16);
    frame.render_widget(Clear, area);

    let palette = theme::palette(app.theme);
    let block = Block::default()
        .borders(Borders::ALL)
        .title(Line::from(vec![
            Span::styled(" /theme ", Style::default().fg(palette.accent)),
            Span::styled("choose color palette ", Style::default().fg(palette.muted)),
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
        .constraints([Constraint::Min(6), Constraint::Length(3)])
        .split(inner);

    let items = ThemeId::ALL
        .iter()
        .map(|theme_id| {
            let p = theme::palette(*theme_id);
            ListItem::new(Line::from(vec![
                Span::styled(
                    format!("{:<19}", theme_id.name()),
                    Style::default().fg(p.accent),
                ),
                Span::styled(theme_id.description(), Style::default().fg(p.muted)),
            ]))
        })
        .collect::<Vec<_>>();
    let mut state = ListState::default().with_selected(Some(app.theme_cursor));
    let list = List::new(items).highlight_symbol("› ").highlight_style(
        Style::default()
            .fg(palette.text)
            .bg(palette.rule)
            .add_modifier(Modifier::BOLD),
    );
    frame.render_stateful_widget(list, rows[0], &mut state);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("↑/↓ or j/k", Style::default().fg(palette.accent)),
            Span::styled(" move   ", Style::default().fg(palette.subtle)),
            Span::styled("enter", Style::default().fg(palette.accent)),
            Span::styled(" apply   ", Style::default().fg(palette.subtle)),
            Span::styled("esc", Style::default().fg(palette.accent)),
            Span::styled(" close", Style::default().fg(palette.subtle)),
        ]))
        .alignment(Alignment::Center)
        .style(Style::default().bg(palette.bg)),
        rows[1],
    );
}

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
