mod cells;
mod chat;
mod components;
mod composer;
mod geometry;
mod layout;
mod popups;
mod statusline;

#[cfg(test)]
mod render_tests;

pub use chat::TranscriptRenderCache;
#[allow(dead_code)]
mod tools;

use ratatui::Frame;

use crate::app::App;

pub fn draw(
    frame: &mut Frame<'_>,
    app: &mut App,
    transcript_cache: &mut chat::TranscriptRenderCache,
) {
    if app.theme_picker_open
        || app.model_picker_open
        || app.login_picker_open
        || app.agent_picker_open
    {
        popups::draw(frame, app);
        return;
    }

    layout::draw(frame, app, transcript_cache);
    popups::draw(frame, app);
}
