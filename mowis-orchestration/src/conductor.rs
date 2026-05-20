use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::OrchConfig;
use crate::critic::Verdict;
use crate::events::{Event, EventBus};
use crate::plan::{Plan, PlanId, PlanStatus, TaskGraph, TaskNode, ModelTier};

#[derive(Debug, Clone)]
pub struct Conductor {
    cfg: OrchConfig,
    bus: EventBus,
    conversation: Vec<String>,
    current_plan: Option<PlanId>,
}

impl Conductor {
    pub fn new(cfg: &OrchConfig, bus: EventBus) -> Result<Self> {
        Ok(Self {
            cfg: cfg.clone(),
            bus,
            conversation: Vec::new(),
            current_plan: None,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut rx = self.bus.subscribe();
        loop {
            tokio::select! {
                result = rx.recv() => {
                    match result {
                        Ok(Event::CriticVerdict { plan_id, version, verdict }) => {
                            self.handle_critic_verdict(plan_id, version, verdict).await?;
                        }
                        Ok(Event::UserApproved { plan_id }) => {
                            self.bus.emit(Event::PlanApproved { plan_id });
                        }
                        Ok(Event::UserOverride { plan_id }) => {
                            self.bus.emit(Event::PlanApproved { plan_id });
                        }
                        Ok(Event::UserCancelled { plan_id }) => {
                            tracing::info!(plan = %plan_id.0, "user cancelled plan");
                            self.current_plan = None;
                        }
                        Ok(_) => {}
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            tracing::warn!(n, "conductor: lagged on event bus");
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "conductor: event bus error");
                            break;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    pub async fn handle_user_message(&mut self, msg: String) -> Result<ConductorAction> {
        self.conversation.push(msg.clone());

        let llm_config = self.cfg.llm_for(&crate::plan::Tier::Conductor)?;

        let system_prompt = include_str!("prompts/conductor.md")
            .replace("{{repo_root}}", ".")
            .replace("{{conversation_id}}", "stdin-session");

        let history: Vec<serde_json::Value> = self
            .conversation
            .iter()
            .enumerate()
            .map(|(i, msg)| {
                if i % 2 == 0 {
                    serde_json::json!({"role": "user", "content": msg})
                } else {
                    serde_json::json!({"role": "assistant", "content": msg})
                }
            })
            .collect();

        let response =
            crate::providers::generate_chat(&llm_config, &system_prompt, &history, 0.7).await?;

        self.conversation.push(response.clone());

        if response.contains("<plan>") {
            let plan_id_str = format!(
                "{}-{:06x}",
                chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
                rand_hex()
            );
            let plan_id = PlanId(plan_id_str);
            let mut plan = Plan::new_draft(plan_id.clone(), &msg, "stdin-session");

            if let Some(toml_start) = response.find("<plan>") {
                if let Some(toml_end) = response.find("</plan>") {
                    let plan_toml = &response[toml_start + 6..toml_end];
                    if let Ok(task_graph) = toml::from_str::<TaskGraph>(plan_toml) {
                        plan.task_graph = task_graph;
                    }
                }
            }

            plan.overview = response.clone();
            plan.save()?;

            self.current_plan = Some(plan_id.clone());
            self.bus.emit(Event::PlanDrafted {
                plan_id: plan_id.clone(),
                version: 1,
            });

            Ok(ConductorAction::PlanDrafted { plan_id })
        } else {
            Ok(ConductorAction::Chat { reply: response })
        }
    }

    async fn handle_critic_verdict(
        &mut self,
        plan_id: PlanId,
        version: u32,
        verdict: Verdict,
    ) -> Result<()> {
        match verdict {
            Verdict::Approve => {
                tracing::info!(plan = %plan_id.0, version, "critic approved plan");
            }
            Verdict::Revise { issues } => {
                tracing::info!(plan = %plan_id.0, version, issues = issues.len(), "critic requests revision");

                let mut plan = Plan::load(&self.cfg.plans_dir, &plan_id)?;
                plan.snapshot_to_history()?;
                plan.current_version += 1;

                let fix_prompt = format!(
                    "The critic found these issues with the plan:\n{}\n\nPlease fix these issues and update the plan.",
                    issues
                        .iter()
                        .map(|i| format!("- [{}] {}: {}", i.severity, i.section, i.message))
                        .collect::<Vec<_>>()
                        .join("\n")
                );

                self.conversation.push(fix_prompt);

                let llm_config = self.cfg.llm_for(&crate::plan::Tier::Conductor)?;
                let system_prompt = include_str!("prompts/conductor.md")
                    .replace("{{repo_root}}", ".")
                    .replace("{{conversation_id}}", "stdin-session");

                let history: Vec<serde_json::Value> = self
                    .conversation
                    .iter()
                    .enumerate()
                    .map(|(i, msg)| {
                        if i % 2 == 0 {
                            serde_json::json!({"role": "user", "content": msg})
                        } else {
                            serde_json::json!({"role": "assistant", "content": msg})
                        }
                    })
                    .collect();

                let response = crate::providers::generate_chat(
                    &llm_config,
                    &system_prompt,
                    &history,
                    0.7,
                )
                .await?;

                self.conversation.push(response.clone());

                plan.overview = response;
                plan.save()?;

                self.bus.emit(Event::PlanRevised {
                    plan_id: plan_id.clone(),
                    version: plan.current_version,
                });
                self.bus.emit(Event::PlanDrafted {
                    plan_id,
                    version: plan.current_version,
                });
            }
            Verdict::Block { reason, .. } => {
                tracing::warn!(plan = %plan_id.0, reason = %reason, "critic blocked plan");
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConductorAction {
    Chat { reply: String },
    PlanDrafted { plan_id: PlanId },
    PlanRevised { plan_id: PlanId, new_version: u32 },
    AwaitingApproval { plan_id: PlanId },
}

fn rand_hex() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        & 0xFFFFFF) as u32
}
