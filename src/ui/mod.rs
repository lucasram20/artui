mod chat;
mod layout;
mod popups;
mod tools;

use ratatui::Frame;

use crate::app::App;

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    layout::draw(frame, app);
}
