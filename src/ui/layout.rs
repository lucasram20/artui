use ratatui::{
    layout::{Constraint, Direction, Layout},
    style::{Color, Style},
    text::Line,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::App;

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(3)])
        .split(frame.area());

    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(22),
            Constraint::Percentage(56),
            Constraint::Percentage(22),
        ])
        .split(root[0]);

    let left = Paragraph::new("File/Search\n\nRepo tools will appear here.")
        .block(Block::default().title("Workspace").borders(Borders::ALL));
    frame.render_widget(left, columns[0]);

    super::chat::draw(frame, app, columns[1]);
    super::tools::draw(frame, app, columns[2]);

    let input = Paragraph::new(Line::from(app.input.as_str()))
        .style(Style::default().fg(Color::White))
        .block(Block::default().title("Input").borders(Borders::ALL));
    frame.render_widget(input, root[1]);

    let cursor_x = root[1].x + app.input.chars().count() as u16 + 1;
    let cursor_y = root[1].y + 1;
    frame.set_cursor_position((cursor_x.min(root[1].right().saturating_sub(1)), cursor_y));
}
