pub mod app;
pub mod ui;
pub mod event;
pub mod widgets;

use crate::orchestration::new_orchestrator::OrchestratorEvent;
use anyhow::Result;
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io;
use std::sync::mpsc::Receiver;
use std::time::Duration;

use self::app::App;
use self::event::{spawn_event_thread, TuiEvent};

/// Run the TUI event loop. Blocks until the user quits.
/// The `event_rx` channel receives orchestrator progress events.
pub fn run(event_rx: Receiver<OrchestratorEvent>) -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    // Restore terminal on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        original_hook(info);
    }));

    let result = run_loop(&mut terminal, event_rx);

    // Cleanup
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    result
}

fn run_loop<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    event_rx: Receiver<OrchestratorEvent>,
) -> Result<()> {
    let mut app = App::new();

    // Spawn keyboard/tick event thread
    let (ui_tx, ui_rx) = std::sync::mpsc::channel::<TuiEvent>();
    let _event_thread = spawn_event_thread(ui_tx, Duration::from_millis(100));

    loop {
        // Drain orchestrator events (non-blocking)
        loop {
            match event_rx.try_recv() {
                Ok(ev) => {
                    let done = matches!(ev, OrchestratorEvent::Done);
                    app.handle_orchestrator_event(ev);
                    if done {
                        break;
                    }
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    app.orchestrator_done = true;
                    break;
                }
            }
        }

        // Draw
        terminal.draw(|f| ui::draw(f, &app))?;

        // Handle UI events (keyboard + tick)
        match ui_rx.try_recv() {
            Ok(TuiEvent::Key(key)) => app.handle_key(key),
            Ok(TuiEvent::Tick) => app.on_tick(),
            _ => {}
        }

        if app.should_quit {
            break;
        }
    }

    Ok(())
}
