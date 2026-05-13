use ratatui::{
    layout::Rect,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::app::App;

pub fn draw(frame: &mut Frame<'_>, app: &App, area: Rect) {
    let body = format!(
        "{}\nMode: {:?}\n\nKeys\nEnter send\nEsc clear input\nCtrl+L clear chat\nq quit",
        app.status, app.mode
    );
    let paragraph =
        Paragraph::new(body).block(Block::default().title("Session Info").borders(Borders::ALL));
    frame.render_widget(paragraph, area);
}
