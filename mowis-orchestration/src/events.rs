use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::critic::Verdict;
use crate::plan::{PlanId, PlanStatus, TaskId};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    // From Conductor
    PlanDrafted {
        plan_id: PlanId,
        version: u32,
    },
    PlanRevised {
        plan_id: PlanId,
        version: u32,
    },
    PlanSuperseded {
        old_plan_id: PlanId,
        new_plan_id: PlanId,
    },
    PlanApproved {
        plan_id: PlanId,
    },
    ConductorReply {
        kind: ConductorReplyKind,
        text: String,
    },

    // From Critic
    CriticReviewing {
        plan_id: PlanId,
        version: u32,
    },
    CriticVerdict {
        plan_id: PlanId,
        version: u32,
        verdict: Verdict,
    },

    // From user (via host)
    UserApproved {
        plan_id: PlanId,
    },
    UserOverride {
        plan_id: PlanId,
    },
    UserCancelled {
        plan_id: PlanId,
    },
    UserMessageReceived {
        text: String,
    },

    // From Captain
    CaptainStarted {
        plan_id: PlanId,
        sandbox_id: String,
    },
    CrewStarted {
        plan_id: PlanId,
        task_id: TaskId,
        agent_id: String,
    },
    CrewToolSummary {
        agent_id: String,
        text: String,
        tool_name: String,
        success: bool,
    },
    CrewDone {
        plan_id: PlanId,
        agent_id: String,
        summary: String,
    },
    CrewFailed {
        plan_id: PlanId,
        agent_id: String,
        reason: String,
    },
    MergeStarted {
        plan_id: PlanId,
        agent_id: String,
    },
    MergeCompleted {
        plan_id: PlanId,
        agent_id: String,
    },
    TaskInjected {
        plan_id: PlanId,
        task_id: TaskId,
        reason: String,
    },
    PlanCompleted {
        plan_id: PlanId,
    },
    PlanFailed {
        plan_id: PlanId,
        reason: String,
    },
    CaptainStatusUpdate {
        status: CaptainStatus,
    },

    // Lifecycle
    ConversationEnded,
    CaptainShutdown {
        sandbox_id: String,
        final_plan_status: PlanStatus,
    },

    // Streaming
    StreamToken {
        text: String,
    },
    StreamDone,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConductorReplyKind {
    Chat,
    PlanDrafted,
    PlanRevised,
    HotPatched,
    ScopeChanged,
    Awaiting,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptainStatus {
    pub plan_id: Option<PlanId>,
    pub sandbox_id: Option<String>,
    pub in_flight: Vec<(TaskId, String, u32)>,
    pub completed: Vec<TaskId>,
    pub failed: Vec<(TaskId, String)>,
}

#[derive(Debug, Clone)]
pub struct EventBus(broadcast::Sender<Event>);

impl EventBus {
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Self(tx)
    }

    pub fn sender(&self) -> broadcast::Sender<Event> {
        self.0.clone()
    }

    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.0.subscribe()
    }

    pub fn emit(&self, ev: Event) {
        tracing::debug!(?ev, "event emitted");
        let _ = self.0.send(ev);
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_bus_basic() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();

        bus.emit(Event::PlanDrafted {
            plan_id: PlanId("test".into()),
            version: 1,
        });

        let ev = rx.try_recv().unwrap();
        match ev {
            Event::PlanDrafted { plan_id, version } => {
                assert_eq!(plan_id.0, "test");
                assert_eq!(version, 1);
            }
            _ => panic!("wrong event"),
        }
    }

    #[test]
    fn test_event_bus_multiple_subscribers() {
        let bus = EventBus::new();
        let mut rx1 = bus.subscribe();
        let mut rx2 = bus.subscribe();

        bus.emit(Event::ConversationEnded);

        assert!(matches!(rx1.try_recv().unwrap(), Event::ConversationEnded));
        assert!(matches!(rx2.try_recv().unwrap(), Event::ConversationEnded));
    }

    #[test]
    fn test_event_bus_lagged() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe();

        for _ in 0..2048 {
            bus.emit(Event::ConversationEnded);
        }

        let mut got_lagged = false;
        loop {
            match rx.try_recv() {
                Err(broadcast::error::TryRecvError::Lagged(_)) => {
                    got_lagged = true;
                    break;
                }
                Err(_) => break,
                Ok(_) => continue,
            }
        }
        assert!(got_lagged);
    }

    #[test]
    fn test_captain_status_serializable() {
        let status = CaptainStatus {
            plan_id: Some(PlanId("p1".into())),
            sandbox_id: Some("sb-1".into()),
            in_flight: vec![(TaskId("t1".into()), "ag-1".into(), 5)],
            completed: vec![TaskId("t0".into())],
            failed: vec![],
        };
        let json = serde_json::to_string(&status).unwrap();
        assert!(json.contains("p1"));
        assert!(json.contains("ag-1"));
    }
}
