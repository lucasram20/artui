pub mod agent;
pub mod app;
pub mod auth;
pub mod config;
pub mod hooks;
pub mod index;
pub mod lsp;
pub mod mcp;
pub mod permissions;
pub mod providers;
pub mod sandbox;
pub mod session;
pub mod skills;
pub mod snapshots;
pub mod terminal_preset;
pub mod tools;
pub mod ui;
pub mod update;
pub mod util;

use std::{
    io, panic,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Result;
use app::{App, AppEvent, AppRequest, InputAction, UiMode};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tachyonfx::EffectManager;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing_subscriber::EnvFilter;

pub async fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(io::stderr)
        .init();

    install_panic_hook();

    let cli = parse_cli_args(std::env::args().skip(1));
    if cli.help_requested {
        print_help();
        return Ok(());
    }
    if cli.version_requested {
        println!("artui {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }

    let mut config = config::load_global_config()?;
    cli.apply_to_config(&mut config);
    if let Some(msg) =
        crate::sandbox::SandboxSettings::from_config(&config.sandbox).startup_message()
    {
        tracing::warn!("{msg}");
    }
    let provider = providers::build_provider(&config)?;
    let (event_tx, mut event_rx) = mpsc::channel(64);
    let mut app = App::new(config.clone(), provider);

    // Construct the LspManager (Phase N1). When `[lsp] enabled = false`,
    // the manager is None — the `lsp` tool isn't registered, the agent
    // never sees it, and there are zero language-server child processes.
    let workspace = std::env::current_dir().unwrap_or_default();
    if config.lsp.enabled {
        match lsp::ServerRegistry::load() {
            Ok(registry) => {
                let manager = Arc::new(lsp::LspManager::new(registry, event_tx.clone()));
                app.lsp_manager = Some(Arc::clone(&manager));
                // Replace the tool registry with one that includes the
                // lsp tool. Subsequent MCP merging (below) layers on top.
                let registry = tools::registry::ToolRegistry::new().with_lsp_tool();
                app.tool_registry = Arc::new(registry);
                if config.lsp.warmup_on_startup {
                    let manager_for_warmup = Arc::clone(&manager);
                    let warmup_cwd = workspace.clone();
                    tokio::spawn(async move {
                        let _ = manager_for_warmup.warmup(&warmup_cwd).await;
                    });
                }
            }
            Err(error) => {
                tracing::warn!(target: "lsp", "failed to load LSP registry: {error:#}");
            }
        }
    }

    // Background self-update check. Mirrors opencode's `autoupdate: notify`
    // and Codex's silent-on-launch poll. We only emit a banner when the
    // bump severity meets `[updates] notify_level` (default: major).
    if config.updates.auto_check {
        let updates = config.updates.clone();
        let current = env!("CARGO_PKG_VERSION").to_owned();
        let tx = event_tx.clone();
        tokio::spawn(async move {
            if let Some(info) = update::check_for_update(
                &updates.repo,
                &current,
                updates.notify_level,
                std::time::Duration::from_secs(updates.timeout_secs.max(1)),
            )
            .await
            {
                let _ = tx.send(AppEvent::UpdateAvailable(info)).await;
            }
        });
    }

    // Fetch Ollama context window asynchronously at startup
    if config.default_provider == "ollama" {
        let ollama_config = config.providers.ollama.clone();
        let model = config.providers.ollama.default_model.clone();
        let ctx_tx = event_tx.clone();
        tokio::spawn(async move {
            if let Some(ctx) =
                providers::ollama::fetch_ollama_context_window(&ollama_config, &model).await
            {
                let _ = ctx_tx.send(AppEvent::OllamaContextWindow(ctx)).await;
            }
        });
    }

    let _ = event_tx
        .send(AppEvent::Auth(app::AuthEvent::Status("Ready".to_owned())))
        .await;
    spawn_app_request(AppRequest::FetchQuote, event_tx.clone());

    // Best-effort freemodel model discovery on startup. Silent on failure so
    // offline users still launch instantly. Only runs when freemodel is the
    // active provider — a user who picked ollama doesn't need to hit the
    // network for it.
    if config.default_provider == "freemodel" {
        spawn_app_request(
            AppRequest::RefreshFreemodelModels {
                config: Box::new(config.providers.freemodel.clone()),
            },
            event_tx.clone(),
        );
    }

    let mut terminal = setup_terminal()?;
    let mut effects: EffectManager<&'static str> = EffectManager::default();
    let mut last_frame = Instant::now();
    let mut was_streaming = false;
    let mut transcript_cache = ui::TranscriptRenderCache::default();
    // Tracks the live terminal state so we issue Enable/Disable only on
    // transitions. Must match the actual mode set in `setup_terminal`,
    // which leaves mouse capture OFF for native drag-select + copy.
    let mut mouse_capture_active = false;

    // Paint immediately so the alternate screen is never blank while slow
    // startup work (index rebuild, MCP handshakes) runs.
    terminal.draw(|frame| {
        ui::draw(frame, &mut app, &mut transcript_cache);
    })?;
    let mut dirty = false;

    if let Some(index) = app.workspace_index.clone() {
        let workspace_for_index = workspace.clone();
        let max_size_mb = app.config.index.max_size_mb;
        tokio::spawn(async move {
            let _ = tokio::task::spawn_blocking(move || {
                index.rebuild(&workspace_for_index, max_size_mb)
            })
            .await;
        });
    }

    // MCP registration after first paint — failures are non-fatal.
    let mcp_cfg = mcp::load_mcp_config(&workspace);
    if !mcp_cfg.servers.is_empty() {
        let mut registry = tools::registry::ToolRegistry::new();
        if config.lsp.enabled && app.lsp_manager.is_some() {
            registry = registry.with_lsp_tool();
        }
        let statuses = mcp::register_servers(&mcp_cfg, &mut registry).await;
        app.tool_registry = Arc::new(registry);
        app.mcp_servers = statuses;
        dirty = true;
    }

    let result = async {
        loop {
            while let Ok(event) = event_rx.try_recv() {
                app.handle_event(event);
                dirty = true;
            }

            if app.should_quit {
                break;
            }

            let elapsed = last_frame.elapsed();
            let animation_due = app.needs_animation_tick();

            if dirty || animation_due {
                last_frame = Instant::now();
                app.advance_thinking_animation();
                let effects_enabled = terminal_preset::ui_effects_enabled();
                terminal.draw(|frame| {
                    ui::draw(frame, &mut app, &mut transcript_cache);
                    if effects_enabled {
                        let screen = frame.area();
                        effects.process_effects(
                            tachyonfx::Duration::from_millis(elapsed.as_millis() as u32),
                            frame.buffer_mut(),
                            screen,
                        );
                    }
                })?;
                dirty = false;
            }

            if terminal_preset::ui_effects_enabled() {
                // Trigger shimmer effect when streaming starts
                if app.mode == UiMode::Streaming && !was_streaming {
                    let shimmer = tachyonfx::fx::hsl_shift_fg(
                        [30.0, 0.0, 8.0],
                        (1200, tachyonfx::Interpolation::SineInOut),
                    );
                    effects.add_unique_effect("thinking", tachyonfx::fx::repeating(shimmer));
                }
                // Stop shimmer when streaming ends
                if app.mode != UiMode::Streaming && was_streaming {
                    effects.add_unique_effect("thinking", tachyonfx::fx::sleep(0));
                }
            }
            was_streaming = app.mode == UiMode::Streaming;

            // Reconcile mouse-capture mode with the user's `/mouse` toggle.
            if app.mouse_capture != mouse_capture_active {
                if app.mouse_capture {
                    let _ = execute!(terminal.backend_mut(), crossterm::event::EnableMouseCapture);
                } else {
                    let _ = execute!(
                        terminal.backend_mut(),
                        crossterm::event::DisableMouseCapture
                    );
                }
                mouse_capture_active = app.mouse_capture;
            }

            let poll_ms = if app.needs_animation_tick() {
                terminal_preset::animation_poll_ms()
            } else {
                terminal_preset::idle_poll_ms()
            };

            if event::poll(Duration::from_millis(poll_ms))? {
                match event::read()? {
                    Event::Key(key) => {
                        handle_key(key, &mut app, event_tx.clone());
                        dirty = true;
                    }
                    Event::Mouse(mouse) => {
                        handle_mouse(mouse, &mut app);
                        dirty = true;
                    }
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
                        dirty = true;
                    }
                    Event::Resize(_, _) => dirty = true,
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
    // NOTE: mouse capture is intentionally NOT enabled at startup so the
    // host terminal's native drag-select + clipboard copy works out of
    // the box (matches Claude Code, Codex CLI, and pi). Users can enable
    // in-app scroll-wheel handling via `/mouse`; the runtime then issues
    // EnableMouseCapture on demand.
    execute!(
        stdout,
        EnterAlternateScreen,
        crossterm::event::EnableBracketedPaste,
    )?;
    Terminal::new(CrosstermBackend::new(stdout)).map_err(Into::into)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::event::DisableMouseCapture,
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

fn handle_mouse(mouse: crossterm::event::MouseEvent, app: &mut App) {
    use crossterm::event::MouseEventKind;
    match mouse.kind {
        MouseEventKind::ScrollUp => app.scroll_chat_up(),
        MouseEventKind::ScrollDown => app.scroll_chat_down(),
        _ => {}
    }
}

fn handle_key(key: KeyEvent, app: &mut App, event_tx: mpsc::Sender<AppEvent>) {
    // Windows conhost/Terminal delivers both Press and Release for printable
    // keys; Release still carries KeyCode::Char, so handling it doubles input.
    if key.kind == KeyEventKind::Release {
        return;
    }

    // Approval modal intercepts keys first — block all other handling
    // until the user answers (a/s/d/Esc).
    if app.pending_approval.is_some() {
        match (key.modifiers, key.code) {
            (_, KeyCode::Char('a') | KeyCode::Char('A')) => {
                app.answer_approval(crate::permissions::ApprovalAnswer::Once);
            }
            (_, KeyCode::Char('s') | KeyCode::Char('S')) => {
                app.answer_approval(crate::permissions::ApprovalAnswer::Session);
            }
            (_, KeyCode::Char('d') | KeyCode::Char('D'))
            | (_, KeyCode::Esc)
            | (_, KeyCode::Char('q')) => {
                app.answer_approval(crate::permissions::ApprovalAnswer::Deny);
            }
            _ => {}
        }
        return;
    }

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
        let path = line.trim().strip_prefix("file://").unwrap_or(line.trim());
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

// ── CLI argument parsing ────────────────────────────────────────────────
//
// Hand-rolled to avoid pulling in clap. Supports a tiny set of flags that
// override config defaults at runtime.

/// Public client_id of Microsoft's VSCode GitHub OAuth App. Used for the
/// `--copilot-vscode-compat` escape hatch when artui's own client_id is
/// rate-limited or rejected.
const COPILOT_VSCODE_CLIENT_ID: &str = "Iv1.b507a08c87ecfe98";

#[derive(Debug, Default)]
struct CliArgs {
    help_requested: bool,
    version_requested: bool,
    copilot_vscode_compat: bool,
    copilot_client_id_override: Option<String>,
    /// Force every write tool through the Approval modal — Claude
    /// Code's default. Off by default in artui (pi-coding-agent style).
    strict_permissions: bool,
    /// Explicit acknowledgement that no-prompt mode is active. Already
    /// the default; flag exists for parity with Claude Code's
    /// `--dangerously-skip-permissions` / Codex's `--yolo`.
    yolo_acknowledged: bool,
}

impl CliArgs {
    fn apply_to_config(&self, config: &mut config::AppConfig) {
        if let Some(id) = &self.copilot_client_id_override {
            config.providers.copilot.github_oauth_client_id = id.clone();
        } else if self.copilot_vscode_compat {
            config.providers.copilot.github_oauth_client_id = COPILOT_VSCODE_CLIENT_ID.to_owned();
        }
        if self.strict_permissions {
            // Promote write tools to Ask. Plan mode still denies regardless.
            config
                .permissions
                .tools
                .insert("apply_patch".to_owned(), "ask".to_owned());
            config
                .permissions
                .tools
                .insert("shell".to_owned(), "ask".to_owned());
        }
    }
}

fn parse_cli_args<I: IntoIterator<Item = String>>(args: I) -> CliArgs {
    let mut out = CliArgs::default();
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        match arg.as_str() {
            "-h" | "--help" => out.help_requested = true,
            "-V" | "--version" => out.version_requested = true,
            "--copilot-vscode-compat" => out.copilot_vscode_compat = true,
            "--strict-permissions" | "--strict" => out.strict_permissions = true,
            "--yolo" | "--dangerously-skip-permissions" => out.yolo_acknowledged = true,
            "--copilot-client-id" => {
                if let Some(value) = iter.next() {
                    out.copilot_client_id_override = Some(value);
                }
            }
            other if other.starts_with("--copilot-client-id=") => {
                let value = other.trim_start_matches("--copilot-client-id=").to_owned();
                if !value.is_empty() {
                    out.copilot_client_id_override = Some(value);
                }
            }
            _ => {
                // Unknown args are ignored for forward-compat; the TUI will
                // start as usual.
            }
        }
    }
    out
}

fn print_help() {
    println!(
        "artui {} — TUI coding agent\n\
         \n\
         USAGE:\n    \
         artui [OPTIONS]\n\
         \n\
         OPTIONS:\n    \
         -h, --help                       Show this help and exit\n    \
         -V, --version                    Print version and exit\n    \
         --copilot-vscode-compat          Use VSCode's public GitHub OAuth client_id\n                                       \
         (Iv1.b507a08c87ecfe98) for /login copilot.\n                                       \
         Useful when artui's own client_id is rate-limited.\n    \
         --copilot-client-id <ID>         Override the Copilot GitHub OAuth client_id\n                                       \
         entirely (e.g. for GitHub Enterprise).\n    \
         --strict-permissions, --strict   Prompt before every write tool call\n                                       \
         (Claude Code default). Off by default — artui follows\n                                       \
         pi-coding-agent's no-prompt model.\n    \
         --yolo, --dangerously-skip-permissions\n                                       \
         No-op alias for the default behaviour. Records that you\n                                       \
         intend to run without prompts (parity with codex/Claude).\n",
        env!("CARGO_PKG_VERSION"),
    );
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod cli_tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn parses_no_args() {
        let cli = parse_cli_args(args(&[]));
        assert!(!cli.help_requested);
        assert!(!cli.copilot_vscode_compat);
        assert!(cli.copilot_client_id_override.is_none());
    }

    #[test]
    fn parses_help_flag() {
        assert!(parse_cli_args(args(&["--help"])).help_requested);
        assert!(parse_cli_args(args(&["-h"])).help_requested);
    }

    #[test]
    fn parses_version_flag() {
        assert!(parse_cli_args(args(&["--version"])).version_requested);
        assert!(parse_cli_args(args(&["-V"])).version_requested);
    }

    #[test]
    fn vscode_compat_flag_sets_vscode_client_id() {
        let cli = parse_cli_args(args(&["--copilot-vscode-compat"]));
        let mut config = config::AppConfig::default();
        cli.apply_to_config(&mut config);
        assert_eq!(
            config.providers.copilot.github_oauth_client_id,
            "Iv1.b507a08c87ecfe98"
        );
    }

    #[test]
    fn explicit_client_id_overrides_vscode_compat() {
        let cli = parse_cli_args(args(&[
            "--copilot-vscode-compat",
            "--copilot-client-id",
            "Ov23liCustomFleet",
        ]));
        let mut config = config::AppConfig::default();
        cli.apply_to_config(&mut config);
        assert_eq!(
            config.providers.copilot.github_oauth_client_id,
            "Ov23liCustomFleet"
        );
    }

    #[test]
    fn equals_form_for_client_id() {
        let cli = parse_cli_args(args(&["--copilot-client-id=Ov23liEquals"]));
        let mut config = config::AppConfig::default();
        cli.apply_to_config(&mut config);
        assert_eq!(
            config.providers.copilot.github_oauth_client_id,
            "Ov23liEquals"
        );
    }

    #[test]
    fn unknown_args_do_not_panic() {
        let cli = parse_cli_args(args(&["--mystery", "value", "extra"]));
        assert!(!cli.help_requested);
        assert!(!cli.copilot_vscode_compat);
    }

    #[test]
    fn no_flag_leaves_default_client_id() {
        let cli = parse_cli_args(args(&[]));
        let mut config = config::AppConfig::default();
        let before = config.providers.copilot.github_oauth_client_id.clone();
        cli.apply_to_config(&mut config);
        assert_eq!(config.providers.copilot.github_oauth_client_id, before);
    }
}

fn spawn_app_request(request: AppRequest, event_tx: mpsc::Sender<AppEvent>) {
    tokio::spawn(async move {
        match request {
            AppRequest::Provider(request) => {
                let cancel = CancellationToken::new();
                let registry = request.tool_registry;
                let config = agent::r#loop::AgentLoopConfig {
                    context_window: request.context_window,
                    compaction_auto: request.compaction_auto,
                    compaction_reserve_tokens: request.compaction_reserve_tokens,
                    compaction_keep_recent_tokens: request.compaction_keep_recent_tokens,
                    hooks: request.hooks,
                    permissions: Some(request.permissions),
                    lsp_manager: request.lsp_manager,
                    lsp_writethrough: request.lsp_writethrough,
                    lsp_diagnostics_timeout_ms: request.lsp_diagnostics_timeout_ms,
                    snapshots: request.snapshots,
                    snapshot_policy: request.snapshot_policy,
                    sandbox: request.sandbox,
                    workspace_index: request.workspace_index,
                    ..agent::r#loop::AgentLoopConfig::default()
                };
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
            AppRequest::OpenAiOAuthLogin { config, store } => {
                crate::auth::run_openai_oauth_login(config, store, event_tx).await;
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
            AppRequest::RefreshFreemodelModels { config } => {
                let models = crate::providers::freemodel::discover_models(&config)
                    .await
                    .unwrap_or_default();
                let _ = event_tx
                    .send(AppEvent::Auth(crate::app::AuthEvent::FreemodelModels(
                        models,
                    )))
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
