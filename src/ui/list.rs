//! Shared stateful `List` rendering for pickers and suggestions.

use ratatui::{
    style::{Modifier, Style},
    text::Line,
    widgets::{List, ListItem, ListState},
    Frame,
};

use super::layout::theme;

pub fn render_stateful_list(
    frame: &mut Frame<'_>,
    area: ratatui::layout::Rect,
    items: Vec<ListItem<'static>>,
    selected: usize,
    offset: usize,
    palette: theme::Palette,
) {
    if items.is_empty() || area.height == 0 {
        return;
    }
    let selected = selected.min(items.len().saturating_sub(1));
    let max_offset = items.len().saturating_sub(1);
    let offset = offset.min(max_offset);
    let mut state = ListState::default()
        .with_selected(Some(selected))
        .with_offset(offset);
    let list = List::new(items)
        .highlight_style(
            Style::default()
                .fg(palette.accent)
                .bg(palette.rule)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    frame.render_stateful_widget(list, area, &mut state);
}

/// Viewport offset so `selected` stays visible in `viewport_height` rows.
pub fn list_offset_for_selection(selected: usize, viewport_height: usize, len: usize) -> usize {
    if len == 0 || viewport_height == 0 {
        return 0;
    }
    if selected + 1 <= viewport_height {
        0
    } else {
        (selected + 1)
            .saturating_sub(viewport_height)
            .min(len.saturating_sub(viewport_height))
    }
}

pub fn empty_list_item(text: impl Into<String>, palette: theme::Palette) -> ListItem<'static> {
    ListItem::new(Line::from(ratatui::text::Span::styled(
        text.into(),
        Style::default().fg(palette.muted),
    )))
}
