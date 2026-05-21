pub mod agent;
pub mod app;
pub mod auth;
pub mod config;
pub mod permissions;
pub mod providers;
pub mod sandbox;
pub mod session;
pub mod tools;
pub mod ui;
pub mod util;

use std::{io, panic, sync::Arc, time::Duration};

use anyhow::Result;
use app::{App, AppEvent, AppRequest, InputAction, UiMode};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

pub async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(io::stderr)
        .init();

    install_panic_hook();

    let config = config::load_global_config()?;
    let provider = providers::build_provider(&config)?;
    let (event_tx, mut event_rx) = mpsc::channel(64);
    let mut app = App::new(config, provider);
    let _ = event_tx
        .send(AppEvent::Auth(app::AuthEvent::Status("Ready".to_owned())))
        .await;
    spawn_app_request(AppRequest::FetchQuote, event_tx.clone());

    let mut terminal = setup_terminal()?;
    let result = async {
        loop {
            app.advance_thinking_animation();
            terminal.draw(|frame| ui::draw(frame, &app))?;

            while let Ok(event) = event_rx.try_recv() {
                app.handle_event(event);
            }

            if app.should_quit {
                break;
            }

            if event::poll(Duration::from_millis(25))? {
                match event::read()? {
                    Event::Key(key) => handle_key(key, &mut app, event_tx.clone()),
                    Event::Paste(text) => {
                        // If pasted text is an image file path, read as image
                        let trimmed = text.trim();
                        if is_image_path(trimmed) && std::path::Path::new(trimmed).is_file() {
                            if let Ok(data) = std::fs::read(trimmed) {
                                app.paste_image(data);
                            } else {
                                app.paste_text(&text);
                            }
                        } else {
                            app.paste_text(&text);
                        }
                    }
                    _ => {}
                }
            }
        }

        Ok(())
    }
    .await;

    restore_terminal(&mut terminal)?;
    result
}

fn setup_terminal() -> Result<Terminal<CrosstermBackend<io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::event::EnableBracketedPaste
    )?;
    Terminal::new(CrosstermBackend::new(stdout)).map_err(Into::into)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::event::DisableBracketedPaste,
        LeaveAlternateScreen
    )?;
    terminal.show_cursor()?;
    Ok(())
}

fn install_panic_hook() {
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(panic_info);
    }));
}

fn handle_key(key: KeyEvent, app: &mut App, event_tx: mpsc::Sender<AppEvent>) {
    match (key.modifiers, key.code) {
        (KeyModifiers::CONTROL, KeyCode::Char('c')) => app.should_quit = true,
        (KeyModifiers::CONTROL, KeyCode::Char('l')) => app.clear_transcript(),
        (KeyModifiers::CONTROL, KeyCode::Char('v')) => {
            // Try to paste image from clipboard (Wayland: wl-paste, X11: xclip)
            if let Some(image_data) = try_clipboard_image() {
                app.paste_image(image_data);
            }
            // Text paste is handled by bracketed paste (Event::Paste)
        }
        (_, KeyCode::BackTab) => app.cycle_reasoning_effort(),
        (KeyModifiers::SHIFT, KeyCode::Tab) => app.cycle_reasoning_effort(),
        (_, KeyCode::Esc) => app.cancel_input(),
        (_, KeyCode::Up) if app.model_picker_open => app.previous_model(),
        (_, KeyCode::Down) if app.model_picker_open => app.next_model(),
        (_, KeyCode::Char('k')) if app.model_picker_open => app.previous_model(),
        (_, KeyCode::Char('j')) if app.model_picker_open => app.next_model(),
        (_, KeyCode::Enter) if app.model_picker_open => app.select_model(),
        (_, KeyCode::Char('l') | KeyCode::Char('L')) if app.model_picker_open => {
            if let Some(request) = app.login_current_model_provider() {
                spawn_app_request(request, event_tx);
            }
        }
        (_, KeyCode::Char('d') | KeyCode::Char('D')) if app.model_picker_open => {
            app.logout_current_model_provider();
        }
        (_, KeyCode::Up) if app.login_picker_open => app.previous_login_provider(),
        (_, KeyCode::Down) if app.login_picker_open => app.next_login_provider(),
        (_, KeyCode::Char('k')) if app.login_picker_open => app.previous_login_provider(),
        (_, KeyCode::Char('j')) if app.login_picker_open => app.next_login_provider(),
        (_, KeyCode::Enter) if app.login_picker_open => {
            if let Some(request) = app.select_login_provider() {
                spawn_app_request(request, event_tx);
            }
        }
        (_, KeyCode::Up) if app.statusline_open => app.previous_statusline_item(),
        (_, KeyCode::Down) if app.statusline_open => app.next_statusline_item(),
        (_, KeyCode::Char('k')) if app.statusline_open => app.previous_statusline_item(),
        (_, KeyCode::Char('j')) if app.statusline_open => app.next_statusline_item(),
        (_, KeyCode::Char(' ')) if app.statusline_open => app.toggle_statusline_item(),
        (_, KeyCode::Enter) if app.statusline_open => app.select_statusline(),
        (_, KeyCode::Up) if app.agent_picker_open => app.previous_agent(),
        (_, KeyCode::Down) if app.agent_picker_open => app.next_agent(),
        (_, KeyCode::Char('k')) if app.agent_picker_open => app.previous_agent(),
        (_, KeyCode::Char('j')) if app.agent_picker_open => app.next_agent(),
        (_, KeyCode::Enter) if app.agent_picker_open => app.select_agent(),
        (_, KeyCode::Up) if app.has_slash_command_matches() => app.previous_slash_command(),
        (_, KeyCode::Down) if app.has_slash_command_matches() => app.next_slash_command(),
        (_, KeyCode::Char('k')) if app.has_slash_command_matches() => app.previous_slash_command(),
        (_, KeyCode::Char('j')) if app.has_slash_command_matches() => app.next_slash_command(),
        (_, KeyCode::Up) if app.has_file_mention_matches() => app.previous_file_mention(),
        (_, KeyCode::Down) if app.has_file_mention_matches() => app.next_file_mention(),
        (_, KeyCode::Char('k')) if app.has_file_mention_matches() => app.previous_file_mention(),
        (_, KeyCode::Char('j')) if app.has_file_mention_matches() => app.next_file_mention(),
        (_, KeyCode::Up) if app.theme_picker_open => app.previous_theme(),
        (_, KeyCode::Down) if app.theme_picker_open => app.next_theme(),
        (_, KeyCode::Char('k')) if app.theme_picker_open => app.previous_theme(),
        (_, KeyCode::Char('j')) if app.theme_picker_open => app.next_theme(),
        (_, KeyCode::PageUp) => app.page_chat_up(),
        (_, KeyCode::PageDown) => app.page_chat_down(),
        (KeyModifiers::CONTROL, KeyCode::Char('u')) => app.page_chat_up(),
        (KeyModifiers::CONTROL, KeyCode::Char('d')) => app.page_chat_down(),
        (_, KeyCode::Up) => app.scroll_chat_up(),
        (_, KeyCode::Down) => app.scroll_chat_down(),
        (_, KeyCode::Char('q'))
            if app.mode == UiMode::Normal
                && !app.theme_picker_open
                && !app.model_picker_open
                && !app.login_picker_open
                && !app.statusline_open
                && !app.agent_picker_open =>
        {
            app.should_quit = true
        }
        (_, KeyCode::Enter) if app.theme_picker_open => app.select_theme(),
        (_, KeyCode::Tab) if app.has_slash_command_matches() => app.complete_slash_command(),
        (_, KeyCode::Tab) if app.has_file_mention_matches() => app.complete_file_mention(),
        (_, KeyCode::Tab) => app.cycle_agent(),
        (_, KeyCode::Enter) if app.has_slash_command_matches() => {
            if let Some(request) = app.submit_slash_command_selection() {
                spawn_app_request(request, event_tx);
            }
        }
        (_, KeyCode::Enter) if app.has_file_mention_matches() => app.complete_file_mention(),
        (_, KeyCode::Enter) => {
            if let Some(request) = app.submit_input() {
                spawn_app_request(request, event_tx);
            }
        }
        (_, KeyCode::Backspace) => app.edit_input(InputAction::Backspace),
        (_, KeyCode::Char(ch)) => app.edit_input(InputAction::Insert(ch)),
        _ => {}
    }
}

/// Try to read image data from the system clipboard.
/// Supports Wayland (wl-paste), X11 (xclip), and macOS (pbpaste).
/// Windows clipboard image requires win32 API — deferred.
fn try_clipboard_image() -> Option<Vec<u8>> {
    // Wayland — try raw PNG first
    if let Ok(output) = std::process::Command::new("wl-paste")
        .args(["--type", "image/png", "--no-newline"])
        .output()
    {
        if output.status.success() && !output.stdout.is_empty() {
            return Some(output.stdout);
        }
    }

    // Wayland — try file URI list (e.g. copied image file)
    if let Some(data) = try_clipboard_image_from_file_uri("wl-paste") {
        return Some(data);
    }

    // X11
    if let Ok(output) = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-target", "image/png", "-o"])
        .output()
    {
        if output.status.success() && !output.stdout.is_empty() {
            return Some(output.stdout);
        }
    }

    // X11 — try file URI
    if let Some(data) = try_clipboard_image_from_xclip_uri() {
        return Some(data);
    }

    // macOS — pbpaste doesn't support binary image; use osascript
    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("osascript")
            .args([
                "-e",
                "set png to (the clipboard as «class PNGf»)",
                "-e",
                "return png",
            ])
            .output()
        {
            if output.status.success() && !output.stdout.is_empty() {
                return Some(output.stdout);
            }
        }
    }

    None
}

/// Try reading an image file path from clipboard URI list (Wayland).
fn try_clipboard_image_from_file_uri(paste_cmd: &str) -> Option<Vec<u8>> {
    let output = std::process::Command::new(paste_cmd)
        .args(["--type", "text/uri-list", "--no-newline"])
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }
    read_image_from_uri_output(&output.stdout)
}

/// Try reading an image file path from clipboard URI list (X11).
fn try_clipboard_image_from_xclip_uri() -> Option<Vec<u8>> {
    let output = std::process::Command::new("xclip")
        .args(["-selection", "clipboard", "-target", "text/uri-list", "-o"])
        .output()
        .ok()?;
    if !output.status.success() || output.stdout.is_empty() {
        return None;
    }
    read_image_from_uri_output(&output.stdout)
}

/// Parse URI list output and read the first image file found.
fn read_image_from_uri_output(raw: &[u8]) -> Option<Vec<u8>> {
    let text = String::from_utf8_lossy(raw);
    for line in text.lines() {
        let path = line
            .trim()
            .strip_prefix("file://")
            .unwrap_or(line.trim());
        if path.is_empty() {
            continue;
        }
        if is_image_path(path) {
            return std::fs::read(path).ok();
        }
    }
    None
}

/// Check if a file path looks like a supported image format.
fn is_image_path(path: &str) -> bool {
    let lower = path.to_lowercase();
    lower.ends_with(".png")
        || lower.ends_with(".jpg")
        || lower.ends_with(".jpeg")
        || lower.ends_with(".gif")
        || lower.ends_with(".webp")
        || lower.ends_with(".bmp")
}

fn spawn_app_request(request: AppRequest, event_tx: mpsc::Sender<AppEvent>) {
    tokio::spawn(async move {
        match request {
            AppRequest::Provider(request) => {
                let cancel = CancellationToken::new();
                let registry = Arc::new(tools::registry::ToolRegistry::new());
                let config = agent::r#loop::AgentLoopConfig::default();
                agent::r#loop::run_turn(
                    request.provider,
                    registry,
                    request.request,
                    event_tx,
                    cancel,
                    &config,
                )
                .await;
            }
            AppRequest::GitHubDeviceLogin {
                config,
                copilot_config,
                store,
            } => {
                crate::auth::run_github_device_login(config, *copilot_config, store, event_tx)
                    .await;
            }
            AppRequest::RefreshCopilotModels { config, store } => {
                let result = crate::providers::copilot::fetch_copilot_models(&config, &store)
                    .await
                    .map_err(|error| error.to_string());
                if let Ok(models) = &result {
                    if let Ok(Some(mut record)) = store.record("copilot") {
                        if let Ok(serialized) = serde_json::to_string(models) {
                            record.metadata.insert("models".to_owned(), serialized);
                            let _ = store.upsert(record);
                        }
                    }
                }
                let _ = event_tx
                    .send(AppEvent::Auth(crate::app::AuthEvent::CopilotModels(result)))
                    .await;
            }
            AppRequest::FetchQuote => {
                let client = reqwest::Client::new();
                if let Ok(response) = client.get("https://zenquotes.io/api/random").send().await {
                    if let Ok(quotes) = response.json::<Vec<crate::app::Quote>>().await {
                        if let Some(quote) = quotes.into_iter().next() {
                            let _ = event_tx.send(AppEvent::Quote(quote)).await;
                        }
                    }
                }
            }
        }
    });
}
