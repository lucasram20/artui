mod app;
mod config;
mod providers;
mod ui;
mod util;

use std::{io, panic, time::Duration};

use anyhow::Result;
use app::{App, AppEvent, InputAction, UiMode};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use tokio::sync::mpsc;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .with_writer(io::stderr)
        .init();

    install_panic_hook();

    let config = config::load_global_config()?;
    let provider = providers::build_provider(&config)?;
    let (event_tx, mut event_rx) = mpsc::channel(64);
    let mut app = App::new(config, provider);

    let mut terminal = setup_terminal()?;
    let result = async {
        loop {
            terminal.draw(|frame| ui::draw(frame, &app))?;

            while let Ok(event) = event_rx.try_recv() {
                app.handle_event(event);
            }

            if app.should_quit {
                break;
            }

            if event::poll(Duration::from_millis(25))? {
                if let Event::Key(key) = event::read()? {
                    handle_key(key, &mut app, event_tx.clone());
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
    execute!(stdout, EnterAlternateScreen)?;
    Terminal::new(CrosstermBackend::new(stdout)).map_err(Into::into)
}

fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
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
        (_, KeyCode::Esc) => app.cancel_input(),
        (_, KeyCode::Up) => app.previous_theme(),
        (_, KeyCode::Down) => app.next_theme(),
        (_, KeyCode::Char('k')) if app.theme_picker_open => app.previous_theme(),
        (_, KeyCode::Char('j')) if app.theme_picker_open => app.next_theme(),
        (_, KeyCode::Char('q')) if app.mode == UiMode::Normal && !app.theme_picker_open => {
            app.should_quit = true
        }
        (_, KeyCode::Enter) if app.theme_picker_open => app.select_theme(),
        (_, KeyCode::Enter) => {
            if let Some(request) = app.submit_input() {
                tokio::spawn(async move {
                    request
                        .provider
                        .stream_turn(request.request, event_tx)
                        .await;
                });
            }
        }
        (_, KeyCode::Backspace) => app.edit_input(InputAction::Backspace),
        (_, KeyCode::Char(ch)) => app.edit_input(InputAction::Insert(ch)),
        _ => {}
    }
}
