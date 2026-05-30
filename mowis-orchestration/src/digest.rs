use std::collections::VecDeque;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::events::Event;

const DEFAULT_CAPACITY: usize = 512;

pub struct SummaryDigestBuffer {
    events: VecDeque<DigestEvent>,
    capacity: usize,
    dropped_count: u64,
}

impl std::fmt::Debug for SummaryDigestBuffer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SummaryDigestBuffer")
            .field("len", &self.events.len())
            .field("capacity", &self.capacity)
            .field("dropped_count", &self.dropped_count)
            .finish()
    }
}

#[derive(Debug, Clone)]
struct DigestEvent {
    event: Event,
    timestamp: chrono::DateTime<chrono::Utc>,
}

impl SummaryDigestBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            events: VecDeque::with_capacity(capacity),
            capacity,
            dropped_count: 0,
        }
    }

    pub fn push(&mut self, event: Event) {
        if self.events.len() >= self.capacity {
            self.events.pop_front();
            self.dropped_count += 1;
        }
        self.events.push_back(DigestEvent {
            event,
            timestamp: chrono::Utc::now(),
        });
    }

    pub fn drain_markdown(&mut self) -> String {
        if self.events.is_empty() && self.dropped_count == 0 {
            return String::new();
        }

        let mut lines: Vec<String> = Vec::new();
        let mut tool_calls = 0u64;
        let mut crews_done = 0u64;
        let mut crews_failed = 0u64;
        let mut crews_running: std::collections::HashSet<String> = std::collections::HashSet::new();
        let mut last_files: Vec<String> = Vec::new();
        let mut merges_completed = 0u64;
        let mut tasks_injected = 0u64;

        for de in &self.events {
            match &de.event {
                Event::CrewToolSummary {
                    agent_id: _,
                    text,
                    tool_name: _,
                    success: _,
                } => {
                    tool_calls += 1;
                    last_files.push(text.clone());
                }
                Event::CrewDone {
                    agent_id, summary, ..
                } => {
                    crews_done += 1;
                    crews_running.remove(agent_id);
                    lines.push(format!("- Crew finished: {}", summary));
                }
                Event::CrewFailed {
                    agent_id, reason, ..
                } => {
                    crews_failed += 1;
                    crews_running.remove(agent_id);
                    lines.push(format!("- Crew failed: {}", reason));
                }
                Event::CrewStarted {
                    agent_id, task_id, ..
                } => {
                    crews_running.insert(agent_id.clone());
                    lines.push(format!("- Crew started task {}", task_id.0));
                }
                Event::MergeCompleted { agent_id, .. } => {
                    merges_completed += 1;
                    lines.push(format!("- Merged overlay for {}", agent_id));
                }
                Event::TaskInjected { task_id, reason, .. } => {
                    tasks_injected += 1;
                    lines.push(format!("- Task {} injected: {}", task_id.0, reason));
                }
                Event::PlanCompleted { plan_id, .. } => {
                    lines.push(format!("- Plan {} completed!", plan_id.0));
                }
                Event::PlanFailed { plan_id, reason, .. } => {
                    lines.push(format!("- Plan {} failed: {}", plan_id.0, reason));
                }
                _ => {}
            }
        }

        let mut result = String::new();
        result.push_str("While you were away:\n\n");

        if tool_calls > 0 {
            result.push_str(&format!(
                "- {} tool calls across {} crews\n",
                tool_calls,
                crews_done + crews_running.len() as u64
            ));
        }
        if crews_done > 0 {
            result.push_str(&format!("- {} crews finished\n", crews_done));
        }
        if !crews_running.is_empty() {
            result.push_str(&format!(
                "- {} crews still running\n",
                crews_running.len()
            ));
        }
        if crews_failed > 0 {
            result.push_str(&format!("- {} crews failed\n", crews_failed));
        }
        if merges_completed > 0 {
            result.push_str(&format!("- {} merges completed\n", merges_completed));
        }
        if tasks_injected > 0 {
            result.push_str(&format!("- {} tasks injected mid-run\n", tasks_injected));
        }

        if !last_files.is_empty() {
            let recent: Vec<&str> = last_files.iter().rev().take(5).map(|s| s.as_str()).collect();
            result.push_str(&format!(
                "- Recent activity: {}\n",
                recent.join("; ")
            ));
        }

        if self.dropped_count > 0 {
            result.push_str(&format!(
                "- {} dropped events (digest buffer overflow)\n",
                self.dropped_count
            ));
        }

        for line in &lines {
            result.push_str(line);
            result.push('\n');
        }

        self.dropped_count = 0;
        result
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }

    pub fn spawn_subscriber(&self, mut rx: broadcast::Receiver<Event>) -> SubscriberHandle {
        // We need a channel to send events to the buffer
        let (tx, mut internal_rx) = tokio::sync::mpsc::unbounded_channel::<Event>();

        // Spawn the bus reader
        let reader_handle = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(event) => {
                        let is_relevant = matches!(
                            event,
                            Event::CrewToolSummary { .. }
                                | Event::CrewDone { .. }
                                | Event::CrewFailed { .. }
                                | Event::CrewStarted { .. }
                                | Event::MergeCompleted { .. }
                                | Event::TaskInjected { .. }
                                | Event::PlanCompleted { .. }
                                | Event::PlanFailed { .. }
                        );
                        if is_relevant {
                            let _ = tx.send(event);
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });

        SubscriberHandle {
            _reader: reader_handle,
            receiver: internal_rx,
        }
    }
}

pub struct SubscriberHandle {
    _reader: JoinHandle<()>,
    pub receiver: tokio::sync::mpsc::UnboundedReceiver<Event>,
}

impl SubscriberHandle {
    pub async fn shutdown(self) {
        self._reader.abort();
    }
}

impl Default for SummaryDigestBuffer {
    fn default() -> Self {
        Self::new(DEFAULT_CAPACITY)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::events::EventBus;
    use crate::plan::{PlanId, TaskId};

    #[test]
    fn test_digest_empty() {
        let mut buf = SummaryDigestBuffer::new(512);
        let md = buf.drain_markdown();
        assert!(md.is_empty());
    }

    #[test]
    fn test_digest_with_events() {
        let mut buf = SummaryDigestBuffer::new(512);

        buf.push(Event::CrewStarted {
            plan_id: PlanId("p1".into()),
            task_id: TaskId("t1".into()),
            agent_id: "ag-1".into(),
        });
        buf.push(Event::CrewToolSummary {
            agent_id: "ag-1".into(),
            text: "Agent read src/main.rs (4.2 KB)".into(),
            tool_name: "read_file".into(),
            success: true,
        });
        buf.push(Event::CrewDone {
            plan_id: PlanId("p1".into()),
            agent_id: "ag-1".into(),
            summary: "Task completed".into(),
        });

        let md = buf.drain_markdown();
        assert!(md.contains("While you were away"));
        assert!(md.contains("1 tool calls"));
        assert!(md.contains("1 crews finished"));
    }

    #[test]
    fn test_digest_overflow() {
        let mut buf = SummaryDigestBuffer::new(5);

        for i in 0..10 {
            buf.push(Event::CrewToolSummary {
                agent_id: format!("ag-{}", i),
                text: format!("tool call {}", i),
                tool_name: "read_file".into(),
                success: true,
            });
        }

        assert_eq!(buf.len(), 5);
        let md = buf.drain_markdown();
        assert!(md.contains("5 dropped events"));
    }

    #[test]
    fn test_digest_1000_events_capacity_512() {
        let mut buf = SummaryDigestBuffer::new(512);

        for i in 0..1000 {
            buf.push(Event::CrewToolSummary {
                agent_id: "ag-1".into(),
                text: format!("tool call {}", i),
                tool_name: "read_file".into(),
                success: true,
            });
        }

        assert_eq!(buf.len(), 512);
        let md = buf.drain_markdown();
        assert!(md.contains("488 dropped events"));
        assert!(md.contains("512 tool calls"));
    }

    #[tokio::test]
    async fn test_digest_subscriber() {
        let bus = EventBus::new();
        let mut buf = SummaryDigestBuffer::new(512);
        let mut handle = buf.spawn_subscriber(bus.subscribe());

        bus.emit(Event::CrewStarted {
            plan_id: PlanId("p1".into()),
            task_id: TaskId("t1".into()),
            agent_id: "ag-1".into(),
        });

        // Give the subscriber a moment
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        // Drain events from the subscriber
        while let Ok(event) = handle.receiver.try_recv() {
            buf.push(event);
        }

        assert!(!buf.is_empty());
        handle.shutdown().await;
    }
}
