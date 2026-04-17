use crossterm::event::{Event as CEvent, KeyEvent};
use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub enum OrchActivityEvent {
    AgentStarted { agent_id: String, description: String },
    ToolCall { agent_id: String, tool_name: String },
    AgentCompleted { agent_id: String },
    AgentFailed { agent_id: String, error: String },
    LayerProgress { layer: u8, message: String },
    StatsUpdate { total: usize, completed: usize, failed: usize },
}

#[derive(Debug, Clone)]
pub enum TuiEvent {
    Key(KeyEvent),
    Tick,
    GeminiChunk(String),
    GeminiDone,
    GeminiError(String),
    OrchEvent(OrchActivityEvent),
    OrchDone,
    LogEntry { level: String, message: String, timestamp: u64 },
}

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
