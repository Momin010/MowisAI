use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::sync::{mpsc, oneshot};

use crate::config::OrchConfig;
use crate::critic::Verdict;
use crate::digest::SummaryDigestBuffer;
use crate::events::{CaptainStatus, ConductorReplyKind, Event, EventBus};
use crate::plan::{Plan, PlanId, PlanStatus, TaskId, TaskNode};

#[derive(Debug)]
pub struct Conductor {
    cfg: OrchConfig,
    bus: EventBus,
    cmd_tx: mpsc::Sender<ConductorCommand>,
    conversation: Vec<String>,
    current_plan: Option<PlanId>,
    digest: SummaryDigestBuffer,
}

#[derive(Debug)]
pub enum ConductorCommand {
    UserMessage {
        text: String,
        reply_tx: oneshot::Sender<ConductorReply>,
    },
    CriticVerdict {
        plan_id: PlanId,
        version: u32,
        verdict: Verdict,
    },
    EndConversation,
}

#[derive(Debug)]
pub enum ConductorReply {
    Chat {
        reply: String,
    },
    PlanDrafted {
        plan_id: PlanId,
        version: u32,
    },
    PlanRevised {
        plan_id: PlanId,
        version: u32,
    },
    HotPatched {
        task: TaskNode,
        target_plan: PlanId,
    },
    ScopeChanged {
        new_plan_id: PlanId,
    },
    Awaiting {
        plan_id: PlanId,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClassifierDecision {
    #[serde(rename = "informational")]
    Informational,
    #[serde(rename = "hot_patch")]
    HotPatch,
    #[serde(rename = "scope_change")]
    ScopeChange,
    #[serde(rename = "new_plan")]
    NewPlan,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifierOutput {
    pub decision: ClassifierDecision,
    pub reason: String,
    #[serde(default)]
    pub task: Option<ClassifierTask>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassifierTask {
    pub title: String,
    pub description: String,
    #[serde(default)]
    pub deps: Vec<String>,
}

impl Conductor {
    pub fn new(cfg: &OrchConfig, bus: EventBus) -> Result<(Self, mpsc::Sender<ConductorCommand>)> {
        let (cmd_tx, _cmd_rx) = mpsc::channel(64);
        let conductor = Self {
            cfg: cfg.clone(),
            bus: bus.clone(),
            cmd_tx: cmd_tx.clone(),
            conversation: Vec::new(),
            current_plan: None,
            digest: SummaryDigestBuffer::new(512),
        };
        Ok((conductor, cmd_tx))
    }

    pub async fn run(mut self) -> Result<()> {
        let (_cmd_tx, mut cmd_rx) = mpsc::channel::<ConductorCommand>(64);

        // Subscribe to bus for digest
        let bus_sub = self.bus.subscribe();
        let mut sub_handle = self.digest.spawn_subscriber(bus_sub);

        // Feed digest events from subscriber into buffer
        let digest_rx = &mut sub_handle.receiver;

        loop {
            tokio::select! {
                Some(cmd) = cmd_rx.recv() => {
                    match cmd {
                        ConductorCommand::UserMessage { text, reply_tx } => {
                            let reply = self.handle_user_message(text).await;
                            let reply = match reply {
                                Ok(r) => r,
                                Err(e) => ConductorReply::Error { message: e.to_string() },
                            };
                            let _ = reply_tx.send(reply);
                        }
                        ConductorCommand::CriticVerdict { plan_id, version, verdict } => {
                            if let Err(e) = self.handle_critic_verdict(plan_id, version, verdict).await {
                                tracing::error!(error = %e, "conductor: error handling critic verdict");
                            }
                        }
                        ConductorCommand::EndConversation => {
                            self.bus.emit(Event::ConversationEnded);
                            break;
                        }
                    }
                }
                Some(event) = digest_rx.recv() => {
                    self.digest.push(event);
                }
            }
        }

        sub_handle.shutdown().await;
        Ok(())
    }

    pub async fn handle_user_message(&mut self, msg: String) -> Result<ConductorReply> {
        self.bus.emit(Event::UserMessageReceived { text: msg.clone() });
        self.conversation.push(msg.clone());

        // Pull digest of new Crew summaries since last user message
        let digest_md = self.digest.drain_markdown();
        if !digest_md.is_empty() {
            self.conversation
                .push(format!("[System] {}", digest_md));
        }

        // If a plan is running, classify the message first
        if self.current_plan.is_some() {
            match self.classify_message(&msg).await {
                Ok(classifier) => match classifier.decision {
                    ClassifierDecision::Informational => {
                        // Just answer in chat
                        let reply = self.chat_reply().await?;
                        return Ok(ConductorReply::Chat { reply });
                    }
                    ClassifierDecision::HotPatch => {
                        if let Some(task_info) = classifier.task {
                            let task = TaskNode {
                                id: TaskId(format!("h{}", rand_hex())),
                                title: task_info.title,
                                description: task_info.description,
                                deps: task_info
                                    .deps
                                    .into_iter()
                                    .map(TaskId)
                                    .collect(),
                                model_tier: crate::plan::ModelTier::Fast,
                                tool_budget: 30,
                                files_hint: vec![],
                            };
                            let plan_id = self.current_plan.clone().unwrap();
                            return Ok(ConductorReply::HotPatched {
                                task,
                                target_plan: plan_id,
                            });
                        }
                    }
                    ClassifierDecision::ScopeChange => {
                        // Draft a new plan that supersedes the current one
                        let new_plan = self.draft_plan(&msg).await?;
                        let old_plan = self.current_plan.clone().unwrap();
                        self.bus.emit(Event::PlanSuperseded {
                            old_plan_id: old_plan,
                            new_plan_id: new_plan.clone(),
                        });
                        self.current_plan = Some(new_plan.clone());
                        return Ok(ConductorReply::ScopeChanged {
                            new_plan_id: new_plan,
                        });
                    }
                    ClassifierDecision::NewPlan => {
                        let plan_id = self.draft_plan(&msg).await?;
                        self.current_plan = Some(plan_id.clone());
                        return Ok(ConductorReply::PlanDrafted {
                            plan_id,
                            version: 1,
                        });
                    }
                },
                Err(e) => {
                    tracing::warn!(error = %e, "classifier failed, falling through to normal reply");
                }
            }
        }

        // No plan running or classifier failed — normal flow
        let reply = self.chat_reply().await?;

        // Check if the LLM wants to draft a plan (contains <plan> tag)
        if reply.contains("<plan>") {
            let plan_id = self.draft_plan_from_response(&reply, &msg).await?;
            self.current_plan = Some(plan_id.clone());
            return Ok(ConductorReply::PlanDrafted {
                plan_id,
                version: 1,
            });
        }

        // Check if the LLM output contains tool call JSON and strip it from the reply
        let clean_reply = self.strip_tool_calls(&reply);

        // If the reply mentions creating a plan or tasks, auto-draft a plan
        let lower = reply.to_lowercase();
        if (lower.contains("plan") && (lower.contains("creat") || lower.contains("draft") || lower.contains("put together")))
            || lower.contains("let me create")
            || lower.contains("i'll draft")
        {
            let plan_id = self.draft_plan(&msg).await?;
            self.current_plan = Some(plan_id.clone());
            return Ok(ConductorReply::PlanDrafted {
                plan_id,
                version: 1,
            });
        }

        Ok(ConductorReply::Chat { reply: clean_reply })
    }

    fn strip_tool_calls(&self, text: &str) -> String {
        // Remove JSON tool call blocks from the response
        let mut result = text.to_string();
        // Remove patterns like {"name": "...", "arguments": {...}}
        while let Some(start) = result.find("{\"name\":") {
            // Find the matching closing brace
            let mut depth = 0;
            let mut end = start;
            for (i, ch) in result[start..].char_indices() {
                if ch == '{' { depth += 1; }
                if ch == '}' { depth -= 1; }
                if depth == 0 {
                    end = start + i + 1;
                    break;
                }
            }
            if end > start {
                result = format!("{}{}", &result[..start], &result[end..]).trim().to_string();
            } else {
                break;
            }
        }
        // Remove ```json...``` blocks that contain tool calls
        while let Some(start) = result.find("```json") {
            if let Some(end) = result[start+7..].find("```") {
                let block_end = start + 7 + end + 3;
                let block = &result[start..block_end];
                if block.contains("\"name\"") && block.contains("\"arguments\"") {
                    result = format!("{}{}", &result[..start], &result[block_end..]).trim().to_string();
                } else {
                    break;
                }
            } else {
                break;
            }
        }
        result.trim().to_string()
    }

    async fn chat_reply(&mut self) -> Result<String> {
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
        Ok(response)
    }

    async fn classify_message(&self, msg: &str) -> Result<ClassifierOutput> {
        let llm_config = self.cfg.llm_for(&crate::plan::Tier::Crew)?; // cheap tier

        let plan_summary = if let Some(ref plan_id) = self.current_plan {
            match Plan::load(&self.cfg.plans_dir, plan_id) {
                Ok(p) => format!(
                    "Current plan: {}\nOverview: {}",
                    plan_id.0,
                    truncate_str(&p.overview, 200)
                ),
                Err(_) => "No plan loaded".into(),
            }
        } else {
            "No plan running".into()
        };

        let system_prompt = include_str!("prompts/conductor_classifier.md");
        let user_msg = format!(
            "User message: {}\n\n{}\n\nRespond with JSON only.",
            msg, plan_summary
        );

        let response = crate::providers::generate_text(
            &llm_config,
            system_prompt,
            &user_msg,
            true, // json_mode
            0.1,
        )
        .await?;

        let output: ClassifierOutput = serde_json::from_str(&response).unwrap_or(
            ClassifierOutput {
                decision: ClassifierDecision::Informational,
                reason: "failed to parse classifier output".into(),
                task: None,
            },
        );
        Ok(output)
    }

    async fn draft_plan(&mut self, user_msg: &str) -> Result<PlanId> {
        let plan_id_str = format!(
            "{}-{:06x}",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
            rand_hex()
        );
        let plan_id = PlanId(plan_id_str);
        let mut plan = Plan::new_draft(plan_id.clone(), user_msg, "stdin-session");

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

        if let Some(toml_start) = response.find("<plan>") {
            if let Some(toml_end) = response.find("</plan>") {
                let plan_toml = &response[toml_start + 6..toml_end];
                if let Ok(task_graph) = toml::from_str::<crate::plan::TaskGraph>(plan_toml) {
                    plan.task_graph = task_graph;
                }
            }
        }

        plan.overview = response;
        plan.save()?;

        self.bus.emit(Event::PlanDrafted {
            plan_id: plan_id.clone(),
            version: 1,
        });

        Ok(plan_id)
    }

    async fn draft_plan_from_response(&mut self, response: &str, user_msg: &str) -> Result<PlanId> {
        let plan_id_str = format!(
            "{}-{:06x}",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
            rand_hex()
        );
        let plan_id = PlanId(plan_id_str);
        let mut plan = Plan::new_draft(plan_id.clone(), user_msg, "stdin-session");

        if let Some(toml_start) = response.find("<plan>") {
            if let Some(toml_end) = response.find("</plan>") {
                let plan_toml = &response[toml_start + 6..toml_end];
                if let Ok(task_graph) = toml::from_str::<crate::plan::TaskGraph>(plan_toml) {
                    plan.task_graph = task_graph;
                }
            }
        }

        plan.overview = response.to_string();
        plan.save()?;

        self.bus.emit(Event::PlanDrafted {
            plan_id: plan_id.clone(),
            version: 1,
        });

        Ok(plan_id)
    }

    pub async fn handle_critic_verdict(
        &mut self,
        plan_id: PlanId,
        version: u32,
        verdict: Verdict,
    ) -> Result<()> {
        match verdict {
            Verdict::Approve => {
                tracing::info!(plan = %plan_id.0, version, "critic approved plan");
                self.bus.emit(Event::ConductorReply {
                    kind: ConductorReplyKind::Chat,
                    text: format!("Critic approved plan v{}", version),
                });
            }
            Verdict::Revise { issues } => {
                tracing::info!(
                    plan = %plan_id.0,
                    version,
                    issues = issues.len(),
                    "critic requests revision"
                );

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
                let response = self.chat_reply().await?;

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
                self.bus.emit(Event::ConductorReply {
                    kind: ConductorReplyKind::Chat,
                    text: format!("Critic blocked the plan: {}", reason),
                });
            }
        }
        Ok(())
    }

    pub fn current_plan(&self) -> Option<&PlanId> {
        self.current_plan.as_ref()
    }

    pub fn cmd_sender(&self) -> mpsc::Sender<ConductorCommand> {
        self.cmd_tx.clone()
    }
}

fn rand_hex() -> u32 {
    use std::time::{SystemTime, UNIX_EPOCH};
    (SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos()
        & 0xFFFFFF) as u32
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConductorAction {
    Chat { reply: String },
    PlanDrafted { plan_id: PlanId },
    PlanRevised { plan_id: PlanId, new_version: u32 },
    AwaitingApproval { plan_id: PlanId },
}
