use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::plan::{PlanId, TaskId};
use crate::critic::{Verdict, Issue};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    PlanDrafted {
        plan_id: PlanId,
        version: u32,
    },
    PlanRevised {
        plan_id: PlanId,
        version: u32,
    },
    PlanApproved {
        plan_id: PlanId,
    },

    CriticReviewing {
        plan_id: PlanId,
        version: u32,
    },
    CriticVerdict {
        plan_id: PlanId,
        version: u32,
        verdict: Verdict,
    },

    UserApproved {
        plan_id: PlanId,
    },
    UserOverride {
        plan_id: PlanId,
    },
    UserCancelled {
        plan_id: PlanId,
    },

    CaptainStarted {
        plan_id: PlanId,
        sandbox_id: String,
    },
    CrewStarted {
        plan_id: PlanId,
        task_id: TaskId,
        agent_id: String,
    },
    CrewProgress {
        plan_id: PlanId,
        agent_id: String,
        tool: String,
        round: u32,
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
    PlanCompleted {
        plan_id: PlanId,
    },
    PlanFailed {
        plan_id: PlanId,
        reason: String,
    },
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
