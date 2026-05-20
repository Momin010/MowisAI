use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::config::OrchConfig;
use crate::events::{Event, EventBus};
use crate::plan::{Plan, PlanId};

#[derive(Debug, Clone)]
pub struct Critic {
    cfg: OrchConfig,
    bus: EventBus,
}

impl Critic {
    pub fn new(cfg: &OrchConfig, bus: EventBus) -> Result<Self> {
        Ok(Self {
            cfg: cfg.clone(),
            bus,
        })
    }

    pub async fn run(&mut self) -> Result<()> {
        let mut rx = self.bus.subscribe();
        loop {
            match rx.recv().await {
                Ok(Event::PlanDrafted { plan_id, version }) => {
                    self.bus.emit(Event::CriticReviewing {
                        plan_id: plan_id.clone(),
                        version,
                    });

                    let plan = match Plan::load(&self.cfg.plans_dir, &plan_id) {
                        Ok(p) => p,
                        Err(e) => {
                            tracing::error!(error = %e, "critic: failed to load plan");
                            continue;
                        }
                    };

                    match self.review_once(&plan, version).await {
                        Ok(verdict) => {
                            self.bus.emit(Event::CriticVerdict {
                                plan_id,
                                version,
                                verdict,
                            });
                        }
                        Err(e) => {
                            tracing::error!(error = %e, "critic: review failed");
                        }
                    }
                }
                Ok(_) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(n, "critic: lagged on event bus");
                    continue;
                }
                Err(e) => {
                    tracing::error!(error = %e, "critic: event bus error");
                    break;
                }
            }
        }
        Ok(())
    }

    pub async fn review_once(&self, plan: &Plan, version: u32) -> Result<Verdict> {
        let llm_config = self.cfg.llm_for(&crate::plan::Tier::Critic)?;

        let plan_context = format!(
            "# Plan Overview\n\n{}\n\n# Tasks\n\n{}\n\n# Sandbox Config\n\n{}\n\n# Models Config\n\n{}",
            plan.overview,
            toml::to_string_pretty(&plan.task_graph).unwrap_or_default(),
            toml::to_string_pretty(&plan.sandbox_config).unwrap_or_default(),
            toml::to_string_pretty(&plan.models_config).unwrap_or_default(),
        );

        let system_prompt = include_str!("prompts/critic.md");

        let response = crate::providers::generate_text(
            &llm_config,
            system_prompt,
            &plan_context,
            true,
            0.3,
        )
        .await?;

        parse_verdict(&response)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Verdict {
    Approve,
    Revise { issues: Vec<Issue> },
    Block { reason: String, issues: Vec<Issue> },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Issue {
    pub severity: String,
    pub section: String,
    pub message: String,
    pub suggested_fix: Option<String>,
}

fn parse_verdict(response: &str) -> Result<Verdict> {
    let parsed: serde_json::Value = serde_json::from_str(response)
        .unwrap_or_else(|_| serde_json::json!({"verdict": "approve", "summary": response}));

    let verdict_str = parsed
        .get("verdict")
        .and_then(|v| v.as_str())
        .unwrap_or("approve");

    match verdict_str {
        "approve" => Ok(Verdict::Approve),
        "revise" => {
            let issues = parse_issues(&parsed);
            Ok(Verdict::Revise { issues })
        }
        "block" => {
            let reason = parsed
                .get("reason")
                .and_then(|r| r.as_str())
                .unwrap_or("plan blocked by critic")
                .to_string();
            let issues = parse_issues(&parsed);
            Ok(Verdict::Block { reason, issues })
        }
        _ => Ok(Verdict::Approve),
    }
}

fn parse_issues(parsed: &serde_json::Value) -> Vec<Issue> {
    parsed
        .get("issues")
        .and_then(|i| i.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|issue| {
                    Some(Issue {
                        severity: issue.get("severity")?.as_str()?.to_string(),
                        section: issue.get("section")?.as_str()?.to_string(),
                        message: issue.get("message")?.as_str()?.to_string(),
                        suggested_fix: issue
                            .get("suggested_fix")
                            .and_then(|s| s.as_str())
                            .map(|s| s.to_string()),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}
