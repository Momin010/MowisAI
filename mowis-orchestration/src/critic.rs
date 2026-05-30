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
    // A critic that cannot be parsed must NOT silently approve. When the
    // response is unintelligible, fall back to requesting a revision so the
    // plan gets another look rather than sailing through.
    let parsed: serde_json::Value = match serde_json::from_str(response) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(error = %e, "critic: could not parse verdict JSON, defaulting to revise");
            return Ok(Verdict::Revise {
                issues: vec![Issue {
                    severity: "warn".into(),
                    section: "overview.md".into(),
                    message: "Critic response could not be parsed; requesting a revision so the plan is reviewed again.".into(),
                    suggested_fix: None,
                }],
            });
        }
    };

    let verdict_str = parsed
        .get("verdict")
        .and_then(|v| v.as_str())
        .unwrap_or("revise");

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
        // Unknown verdict string: treat as a revision request, never a free pass.
        _ => Ok(Verdict::Revise {
            issues: parse_issues(&parsed),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unparseable_response_requests_revision_not_approval() {
        // A critic whose output we can't parse must never be a free pass.
        let verdict = parse_verdict("the model rambled without JSON").unwrap();
        assert!(matches!(verdict, Verdict::Revise { .. }));
    }

    #[test]
    fn unknown_verdict_string_requests_revision() {
        let verdict = parse_verdict(r#"{"verdict": "looks-fine"}"#).unwrap();
        assert!(matches!(verdict, Verdict::Revise { .. }));
    }

    #[test]
    fn explicit_approve_still_approves() {
        let verdict = parse_verdict(r#"{"verdict": "approve"}"#).unwrap();
        assert!(matches!(verdict, Verdict::Approve));
    }

    #[test]
    fn revise_carries_issues() {
        let verdict = parse_verdict(
            r#"{"verdict":"revise","issues":[{"severity":"warn","section":"tasks.toml","message":"too broad"}]}"#,
        )
        .unwrap();
        match verdict {
            Verdict::Revise { issues } => assert_eq!(issues.len(), 1),
            _ => panic!("expected revise"),
        }
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
