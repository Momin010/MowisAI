use crossterm::event::{Event as CEvent, KeyCode, KeyEvent, KeyModifiers};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum TuiEvent {
    Key(KeyEvent),
    Tick,
}

/// Spawn a background thread that polls crossterm events and sends them on `tx`.
/// Sends a `Tick` event every `tick_rate` if no key event arrives.
pub fn spawn_event_thread(tx: mpsc::Sender<TuiEvent>, tick_rate: Duration) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        let mut last_tick = Instant::now();
        loop {
            let timeout = tick_rate
                .checked_sub(last_tick.elapsed())
                .unwrap_or(Duration::from_millis(0));

            if crossterm::event::poll(timeout).unwrap_or(false) {
                match crossterm::event::read() {
                    Ok(CEvent::Key(key)) => {
                        if tx.send(TuiEvent::Key(key)).is_err() {
                            return;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => return,
                }
            }

            if last_tick.elapsed() >= tick_rate {
                if tx.send(TuiEvent::Tick).is_err() {
                    return;
                }
                last_tick = Instant::now();
            }
        }
    })
}
