//! Ratatui rendering baseline tests (Phase 0 safety net).

use ratatui::{backend::TestBackend, buffer::Buffer, layout::Rect, Terminal};

use crate::{
    app::{App, Message, Role, StatusLineItem, ThemeId},
    config::AppConfig,
    permissions::ApprovalPrompt,
    providers::build_provider,
    ui::{
        self,
        chat::{self, TranscriptRenderCache},
        geometry, statusline,
    },
};

fn test_app() -> App {
    let config = AppConfig::default();
    let provider = build_provider(&config).expect("default provider");
    App::new(config, provider)
}

fn render_buffer(width: u16, height: u16, setup: impl FnOnce(&mut App)) -> Buffer {
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("terminal");
    let mut app = test_app();
    setup(&mut app);
    let mut cache = TranscriptRenderCache::default();
    terminal
        .draw(|frame| ui::draw(frame, &mut app, &mut cache))
        .expect("draw");
    terminal.backend().buffer().clone()
}

fn row_text(buffer: &Buffer, y: u16) -> String {
    let area = buffer.area;
    let mut row = String::new();
    for x in area.x..area.right() {
        row.push_str(buffer[(x, y)].symbol());
    }
    row.trim_end().to_owned()
}

fn buffer_contains(buffer: &Buffer, needle: &str) -> bool {
    let area = buffer.area;
    for y in area.y..area.bottom() {
        if row_text(buffer, y).contains(needle) {
            return true;
        }
    }
    false
}

#[test]
fn footer_renders_provider_label() {
    let buffer = render_buffer(100, 24, |_| {});
    assert!(
        buffer_contains(&buffer, "ollama") || buffer_contains(&buffer, "Provider"),
        "footer should show provider or status label"
    );
}

#[test]
fn footer_context_bar_when_enabled() {
    let buffer = render_buffer(100, 24, |app| {
        app.statusline_enabled[StatusLineItem::Context.index()] = true;
        app.turn_input_tokens = 1_000;
        app.turn_output_tokens = 500;
    });
    assert!(
        buffer_contains(&buffer, "ctx"),
        "context item should render ctx label when enabled"
    );
}

#[test]
fn context_bar_spans_short_budget_uses_percent_only() {
    let app = test_app();
    let palette = ui::layout::theme::palette(ThemeId::MonokaiBlue);
    assert!(statusline::context_bar_spans(&app, palette, 6).is_empty());
    let spans = statusline::context_bar_spans(&app, palette, 10);
    assert_eq!(spans.len(), 1);
    assert!(spans[0].content.contains("ctx"));
}

#[test]
fn theme_picker_renders_palette_names() {
    let buffer = render_buffer(100, 30, |app| {
        app.theme_picker_open = true;
    });
    assert!(buffer_contains(&buffer, "Monokai"));
    assert!(buffer_contains(&buffer, "Tokyo Night"));
}

#[test]
fn statusline_picker_lists_footer_items() {
    let buffer = render_buffer(100, 30, |app| {
        app.statusline_open = true;
    });
    assert!(buffer_contains(&buffer, "/statusline"));
    assert!(buffer_contains(&buffer, "Git branch"));
}

#[test]
fn approval_modal_shows_tool_name() {
    let buffer = render_buffer(100, 30, |app| {
        let (reply, _rx) = tokio::sync::oneshot::channel();
        app.pending_approval = Some(ApprovalPrompt {
            call_id: "call-1".to_owned(),
            tool_name: "shell".to_owned(),
            title: "Run command".to_owned(),
            body: "echo hello".to_owned(),
            reply,
        });
    });
    assert!(buffer_contains(&buffer, "Approve shell"));
    assert!(buffer_contains(&buffer, "echo hello"));
}

#[test]
fn transcript_scroll_offset_clamps_to_content() {
    let mut app = test_app();
    app.transcript.push(Message::new(Role::User, "line one"));
    app.transcript
        .push(Message::new(Role::Assistant, "line two\nwith wrap"));
    app.chat_scroll = 10_000;
    let area = Rect::new(0, 0, 40, 5);
    let offset = chat::transcript_scroll_offset(&app, area);
    let max = chat::transcript_visual_height(&app, area.width as usize) as u16;
    assert!(offset <= max.saturating_sub(area.height));
}

#[test]
fn multiline_prompt_increases_composer_height() {
    use super::composer;

    let empty_lines = composer::input_line_count("", 70);
    let multiline_lines = composer::input_line_count("first line\nsecond line", 70);
    assert_eq!(empty_lines, 1);
    assert_eq!(multiline_lines, 2);
}

#[test]
fn popup_geometry_centered_inside_terminal() {
    let term = Rect::new(0, 0, 80, 24);
    let popup = geometry::centered(term, 40, 12);
    assert!(popup.x >= term.x);
    assert!(popup.y >= term.y);
    assert!(popup.right() <= term.right());
    assert!(popup.bottom() <= term.bottom());
}

#[test]
fn cells_fit_spans_respects_footer_budget() {
    use ratatui::text::Span;

    use super::cells;

    let spans = vec![Span::raw("alpha"), Span::raw("beta"), Span::raw("gamma")];
    let fitted = cells::fit_spans_to_width(spans, 8);
    assert!(cells::spans_display_width(&fitted) <= 8);
}
