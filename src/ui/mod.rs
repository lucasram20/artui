mod chat;
mod layout;
mod popups;
#[allow(dead_code)]
mod tools;

use ratatui::Frame;

use crate::app::App;

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    if app.theme_picker_open
        || app.model_picker_open
        || app.login_picker_open
        || app.agent_picker_open
    {
        popups::draw(frame, app);
        return;
    }

    layout::draw(frame, app);
    popups::draw(frame, app);
}
