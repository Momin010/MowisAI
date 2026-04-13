pub mod app;
pub mod ui;
pub mod event;
pub mod widgets;

use crate::config::MowisConfig;
use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;

use self::app::App;
use self::event::{spawn_event_thread, TuiEvent};

/// Main entry point: interactive Claude Code-style TUI
pub fn run_interactive(config: MowisConfig, socket_pid: Option<u32>) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    let result = run_loop(&mut terminal, config, socket_pid);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    config: MowisConfig,
    socket_pid: Option<u32>,
) -> Result<()> {
    let mut app = App::new(config, socket_pid);

    let (ui_tx, ui_rx) = std::sync::mpsc::channel::<TuiEvent>();
    app.event_tx = Some(ui_tx.clone());
    let _event_thread = spawn_event_thread(ui_tx.clone(), std::time::Duration::from_millis(50));

    // Log forwarding channel: log entries are sent here and forwarded as TuiEvent::LogEntry
    let (log_tx, log_rx) = std::sync::mpsc::channel::<(String, String, u64)>();
    crate::logging::set_tui_log_sender(log_tx);
    let ui_tx_for_logs = ui_tx.clone();
    std::thread::spawn(move || {
        loop {
            match log_rx.recv_timeout(std::time::Duration::from_millis(100)) {
                Ok((level, message, timestamp)) => {
                    if ui_tx_for_logs
                        .send(TuiEvent::LogEntry { level, message, timestamp })
                        .is_err()
                    {
                        break;
                    }
                }
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            }
        }
    });

    loop {
        terminal.draw(|f| ui::draw(f, &mut app))?;

        match ui_rx.try_recv() {
            Ok(TuiEvent::Key(key)) => app.handle_key(key),
            Ok(TuiEvent::Tick) => app.on_tick(),
            Ok(TuiEvent::GeminiChunk(text)) => app.on_gemini_chunk(text),
            Ok(TuiEvent::GeminiDone) => app.on_gemini_done(),
            Ok(TuiEvent::GeminiError(err)) => app.on_gemini_error(err),
            Ok(TuiEvent::OrchEvent(ev)) => app.on_orch_event(ev),
            Ok(TuiEvent::OrchDone) => app.on_orch_done(),
            Ok(TuiEvent::LogEntry { level, message, timestamp }) => {
                app.on_log_entry(level, message, timestamp);
            }
            _ => {}
        }

        if app.should_quit {
            break;
        }

        // Check for signal-based shutdown
        if crate::is_shutdown_requested() {
            log::info!("Shutdown signal received, exiting TUI...");
            break;
        }
    }

    // Print final message with socket server status
    if let Some(pid) = app.socket_pid {
        println!("✓ MowisAI closed. Socket server continues in background with PID: {}", pid);
        println!("To stop socket server: kill {} or /kill-socket next time", pid);
    }

    Ok(())
}
