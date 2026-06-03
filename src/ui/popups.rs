//! Modal overlay dispatch — delegates to `components::{selectors, approvals}`.

use ratatui::Frame;

use crate::app::App;

use super::components::{approvals, selectors};

pub fn draw(frame: &mut Frame<'_>, app: &App) {
    if app.theme_picker_open
        || app.model_picker_open
        || app.login_picker_open
        || app.agent_picker_open
        || app.pending_approval.is_some()
    {
        selectors::draw_modal_backdrop(frame, app);
    }
    if app.theme_picker_open {
        selectors::draw_theme_picker(frame, app);
    }
    if app.model_picker_open {
        selectors::draw_model_picker(frame, app);
    }
    if app.login_picker_open {
        selectors::draw_login_picker(frame, app);
    }
    if app.agent_picker_open {
        selectors::draw_agent_picker(frame, app);
    }
    if app.statusline_open {
        selectors::draw_statusline_picker(frame, app);
    }
    if app.pending_approval.is_some() {
        approvals::draw(frame, app);
    }
}
