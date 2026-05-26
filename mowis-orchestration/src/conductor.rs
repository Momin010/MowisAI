use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
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
    /// Session sandbox the crews build in. `save_to_host` copies from here.
    workspace: Option<PathBuf>,
    /// Where `save_to_host` writes to (the user's project dir, default cwd).
    save_dest: PathBuf,
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
            workspace: None,
            save_dest: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        };
        Ok((conductor, cmd_tx))
    }

    /// Point the Conductor at the session sandbox (`workspace`) and the
    /// destination its `save_to_host` tool should copy into (`save_dest`).
    pub fn set_workspace(&mut self, workspace: PathBuf, save_dest: PathBuf) {
        self.workspace = Some(workspace);
        self.save_dest = save_dest;
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
                        // Just answer in chat (with save_to_host available).
                        let (reply, build_dispatched) = self.conversational_turn().await?;
                        if build_dispatched {
                            return Ok(ConductorReply::Chat { reply: self.strip_tool_calls(&reply) });
                        }
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
        let (reply, build_dispatched) = self.conversational_turn().await?;

        // If start_build was called, the captain is already running — don't also
        // trigger draft_plan_from_response which would spawn a second captain.
        if build_dispatched {
            return Ok(ConductorReply::Chat { reply: self.strip_tool_calls(&reply) });
        }

        // Check if the LLM wants to draft a plan (contains <plan> tag)
        if reply.contains("<plan>") {
            let plan_id = self.draft_plan_from_response(&reply, &msg).await?;
            self.current_plan = Some(plan_id.clone());
            return Ok(ConductorReply::PlanDrafted {
                plan_id,
                version: 1,
            });
        }

        // Strip any tool call JSON the LLM might have output
        let clean_reply = self.strip_tool_calls(&reply);

        Ok(ConductorReply::Chat { reply: clean_reply })
    }

    fn strip_tool_calls(&self, text: &str) -> String {
        let mut result = text.to_string();
        // Remove JSON tool call blocks
        while let Some(start) = result.find("{\"name\":") {
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
        // Remove ```json...``` blocks containing tool calls
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

        // Use streaming — emit tokens as they arrive
        let (token_tx, mut token_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let bus = self.bus.clone();

        // Spawn a task to forward tokens to the event bus
        let forward_bus = bus.clone();
        let forward_handle = tokio::spawn(async move {
            while let Some(token) = token_rx.recv().await {
                forward_bus.emit(crate::events::Event::StreamToken { text: token });
            }
        });

        let (response, usage) = crate::providers::generate_chat_streaming(
            &llm_config,
            &system_prompt,
            &history,
            0.7,
            token_tx,
        )
        .await?;

        // Wait for forward task to finish
        let _ = forward_handle.await;

        // Emit stream done and token accounting
        bus.emit(crate::events::Event::StreamDone);
        bus.emit(crate::events::Event::TokensUsed {
            agent_id: "conductor".to_string(),
            role: "conductor".to_string(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            model: llm_config.model.clone(),
        });

        self.conversation.push(response.clone());
        Ok(response)
    }

    /// A conversational turn that can call Conductor tools (currently
    /// `save_to_host` and `start_build`). Returns the final text and a flag
    /// indicating whether `start_build` was successfully dispatched this turn.
    async fn conversational_turn(&mut self) -> Result<(String, bool)> {
        let llm_config = self.cfg.llm_for(&crate::plan::Tier::Conductor)?;
        let system_prompt = include_str!("prompts/conductor.md")
            .replace("{{repo_root}}", ".")
            .replace("{{conversation_id}}", "stdin-session");

        // Seed the agent conversation from history (even = user, odd = assistant).
        let mut convo = crate::providers::AgentConversation::new();
        for (i, msg) in self.conversation.iter().enumerate() {
            if i % 2 == 0 {
                convo.push_user(msg.clone());
            } else {
                convo.push_assistant(msg.clone());
            }
        }

        let tools = conductor_tool_schemas();
        let mut final_text = String::new();
        let mut build_dispatched = false;

        // Bounded tool loop so a misbehaving model can't spin forever.
        for _ in 0..5 {
            let (token_tx, mut token_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
            let fwd_bus = self.bus.clone();
            let fwd = tokio::spawn(async move {
                while let Some(t) = token_rx.recv().await {
                    fwd_bus.emit(Event::StreamToken { text: t });
                }
            });

            let round = crate::providers::call_agent_round_streaming(
                &llm_config,
                &convo,
                &tools,
                &system_prompt,
                token_tx,
            )
            .await?;
            let _ = fwd.await;
            self.bus.emit(Event::TokensUsed {
                agent_id: "conductor".to_string(),
                role: "conductor".to_string(),
                input_tokens: round.usage.input_tokens,
                output_tokens: round.usage.output_tokens,
                model: llm_config.model.clone(),
            });

            if round.tool_calls.is_empty() {
                final_text = round.text.unwrap_or_default();
                break;
            }

            convo.push_assistant_tool_calls(round.tool_calls.clone());
            let mut results = Vec::new();
            for tc in &round.tool_calls {
                let result = self.execute_conductor_tool(tc);
                if tc.name == "start_build" && result["started"].as_bool() == Some(true) {
                    build_dispatched = true;
                }
                results.push((tc.clone(), result));
            }
            convo.push_tool_results(results);

            // Captain is running — don't loop further and spawn duplicates
            if build_dispatched {
                final_text = round.text.unwrap_or_default();
                break;
            }
        }

        self.bus.emit(Event::StreamDone);
        self.conversation.push(final_text.clone());
        Ok((final_text, build_dispatched))
    }

    /// Dispatch a Conductor tool call.
    fn execute_conductor_tool(&self, tc: &crate::providers::ToolCall) -> serde_json::Value {
        match tc.name.as_str() {
            "start_build" => self.execute_start_build(),
            "save_to_host" => {
                let dest = tc.args.get("destination").and_then(|d| d.as_str());
                self.execute_save_to_host(dest)
            }
            other => serde_json::json!({ "error": format!("unknown tool: {}", other) }),
        }
    }

    /// Actually dispatch the Captain to execute the current plan. This is the
    /// ONLY way the Conductor starts a build — without calling it, no work
    /// happens (the model must never merely claim a build is running). The
    /// Captain runs in its own task and streams progress events to the bus,
    /// which the UI renders.
    fn execute_start_build(&self) -> serde_json::Value {
        let plan_id = match &self.current_plan {
            Some(p) => p.clone(),
            None => {
                return serde_json::json!({
                    "error": "no plan has been drafted yet — draft a plan first"
                })
            }
        };
        let workspace = self
            .workspace
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
        let cfg = self.cfg.clone();
        let bus = self.bus.clone();
        let plan_for_task = plan_id.clone();

        // Fire-and-forget: the Captain's events (CaptainStarted, CrewStarted,
        // CrewToolSummary, PlanCompleted/Failed) flow to the bus and the UI.
        tokio::spawn(async move {
            match crate::captain::SimpleCaptain::new_in(
                &cfg,
                plan_for_task.clone(),
                bus.clone(),
                workspace,
            ) {
                Ok(captain) => {
                    if let Err(e) = captain.run().await {
                        bus.emit(Event::PlanFailed {
                            plan_id: plan_for_task,
                            reason: e.to_string(),
                        });
                    }
                }
                Err(e) => {
                    bus.emit(Event::PlanFailed {
                        plan_id: plan_for_task,
                        reason: e.to_string(),
                    });
                }
            }
        });

        tracing::info!(plan = %plan_id.0, "conductor dispatched captain");
        serde_json::json!({
            "started": true,
            "plan_id": plan_id.0,
            "note": "Captain dispatched; build is now running. Progress will stream to the UI.",
            "warning": "DO NOT call start_build again — the captain is already running. Your NEXT response must be ONLY plain text confirming the build has started. No more tool calls."
        })
    }

    /// Copy the session sandbox to the user's machine. This is the only path by
    /// which sandbox output reaches the host — it runs only when the model calls
    /// the `save_to_host` tool, which the prompt restricts to explicit requests.
    fn execute_save_to_host(&self, dest_override: Option<&str>) -> serde_json::Value {
        let workspace = match &self.workspace {
            Some(w) => w,
            None => return serde_json::json!({ "error": "no session sandbox is configured" }),
        };
        let dest = match dest_override {
            Some(d) if !d.trim().is_empty() => self.save_dest.join(d.trim()),
            _ => self.save_dest.clone(),
        };
        match copy_tree(workspace, &dest) {
            Ok(count) => {
                self.bus.emit(Event::ConductorReply {
                    kind: ConductorReplyKind::Chat,
                    text: format!("Saved {} files to {}", count, dest.display()),
                });
                tracing::info!(files = count, dest = %dest.display(), "saved sandbox to host");
                serde_json::json!({
                    "saved": true,
                    "files": count,
                    "destination": dest.display().to_string()
                })
            }
            Err(e) => serde_json::json!({ "error": format!("save failed: {}", e) }),
        }
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
        // draft_plan is only reached once the decision to build is made (a
        // classifier NewPlan/ScopeChange). The base prompt is converse-first and
        // withholds plans until confirmation, so append an override authorizing
        // an immediate plan — otherwise the model may return questions and we'd
        // emit an empty plan.
        let system_prompt = format!(
            "{}\n\n---\n[Build authorized: the user wants this built now. Output the <plan>...</plan> block directly. Do NOT ask further questions or reply conversationally.]",
            include_str!("prompts/conductor.md")
                .replace("{{repo_root}}", ".")
                .replace("{{conversation_id}}", "stdin-session")
        );

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

        let (response, usage) =
            crate::providers::generate_chat(&llm_config, &system_prompt, &history, 0.7).await?;
        self.bus.emit(Event::TokensUsed {
            agent_id: "conductor".to_string(),
            role: "conductor".to_string(),
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            model: llm_config.model.clone(),
        });
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
            // Find the LAST </plan> to avoid matching partial tags in streaming
            if let Some(toml_end) = response.rfind("</plan>") {
                let plan_toml = &response[toml_start + 6..toml_end];
                tracing::debug!(toml = %plan_toml, "parsing plan TOML");
                match toml::from_str::<crate::plan::TaskGraph>(plan_toml) {
                    Ok(task_graph) => {
                        tracing::info!(tasks = task_graph.tasks.len(), "parsed plan tasks");
                        plan.task_graph = task_graph;
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, "failed to parse plan TOML, trying line-by-line extraction");
                        // Try to extract tasks line by line
                        let task_graph = extract_tasks_from_text(plan_toml);
                        if !task_graph.tasks.is_empty() {
                            tracing::info!(tasks = task_graph.tasks.len(), "extracted tasks from text");
                            plan.task_graph = task_graph;
                        }
                    }
                }
            }
        }

        plan.overview = response.to_string();
        plan.save()?;

        self.bus.emit(Event::PlanDrafted {
            plan_id: plan_id.clone(),
            version: 1,
        });

        tracing::info!(plan_id = %plan_id.0, tasks = plan.task_graph.tasks.len(), "plan drafted");
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

    /// After a build finishes, produce a message that tells the user what was
    /// built and offers to iterate further or save to their machine. The output
    /// lives only in the session sandbox until the user explicitly asks to save,
    /// so this must NOT claim anything has been persisted.
    pub async fn announce_completion(&mut self, built_summary: &str) -> Result<String> {
        self.conversation.push(format!(
            "[System] The build finished. The output is staged in the session sandbox and has NOT been saved to the user's machine yet. What was built:\n{}\n\nTell the user briefly what you built, then ask whether they want to change or add anything, or save it to their project. Do not claim it has been saved.",
            built_summary
        ));
        let reply = self.chat_reply().await?;
        Ok(self.strip_tool_calls(&reply))
    }

    /// Wrap a single task (e.g. a hot-patch from mid-conversation) into a
    /// throwaway one-task plan so the Captain can run it on the session
    /// workspace. Returns the new plan id and sets it as the current plan.
    pub async fn materialize_task_as_plan(&mut self, task: TaskNode) -> Result<PlanId> {
        let plan_id_str = format!(
            "{}-{:06x}",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ"),
            rand_hex()
        );
        let plan_id = PlanId(plan_id_str);
        let mut plan = Plan::new_draft(plan_id.clone(), &task.title, "stdin-session");
        plan.overview = format!("Follow-up task: {}", task.title);
        plan.task_graph = crate::plan::TaskGraph { tasks: vec![task] };
        plan.save()?;
        self.current_plan = Some(plan_id.clone());
        Ok(plan_id)
    }

    pub fn current_plan(&self) -> Option<&PlanId> {
        self.current_plan.as_ref()
    }

    pub fn cmd_sender(&self) -> mpsc::Sender<ConductorCommand> {
        self.cmd_tx.clone()
    }
}

fn conductor_tool_schemas() -> Vec<serde_json::Value> {
    vec![
    serde_json::json!({
        "name": "start_build",
        "description": "Dispatch the Captain to actually execute the most recently drafted plan (spawns the crews that write the code). Call this when the user approves the plan or tells you to build/start/go ahead — even if the Critic raised issues, because the user is the final approval gate. You MUST call this to begin work; never claim a build has started unless you have called it. Takes no arguments.",
        "parameters": { "type": "object", "properties": {}, "required": [] }
    }),
    serde_json::json!({
        "name": "save_to_host",
        "description": "Save everything built in the session sandbox to the user's machine. Call this ONLY when the user explicitly asks to save, export, download, or keep the project. Do not call it on your own initiative.",
        "parameters": {
            "type": "object",
            "properties": {
                "destination": {
                    "type": "string",
                    "description": "Optional subfolder name (under the user's project directory) to save into. Omit to save directly into the project directory."
                }
            },
            "required": []
        }
    })]
}

/// Recursively copy `src` into `dst`, skipping build/VCS artifacts. Returns the
/// number of files copied.
fn copy_tree(src: &Path, dst: &Path) -> Result<usize> {
    fn skip(name: &str) -> bool {
        matches!(name, "node_modules" | "target" | ".git" | "dist" | ".mowis")
    }
    fn recurse(src: &Path, dst: &Path, count: &mut usize) -> Result<()> {
        if !src.exists() {
            return Ok(());
        }
        std::fs::create_dir_all(dst)?;
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().to_string();
            if skip(&name) {
                continue;
            }
            let from = entry.path();
            let to = dst.join(&name);
            if entry.file_type()?.is_dir() {
                recurse(&from, &to, count)?;
            } else {
                if let Some(parent) = to.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::copy(&from, &to)?;
                *count += 1;
            }
        }
        Ok(())
    }
    let mut count = 0;
    recurse(src, dst, &mut count)?;
    Ok(count)
}

#[cfg(test)]
mod save_tests {
    use super::*;

    #[test]
    fn copy_tree_copies_files_and_skips_artifacts() {
        let base = std::env::temp_dir().join(format!("mowis-copytree-{}", rand_hex()));
        let src = base.join("src");
        let dst = base.join("dst");
        std::fs::create_dir_all(src.join("nested")).unwrap();
        std::fs::create_dir_all(src.join("node_modules")).unwrap();
        std::fs::write(src.join("index.html"), b"<html>").unwrap();
        std::fs::write(src.join("nested/app.js"), b"console.log(1)").unwrap();
        std::fs::write(src.join("node_modules/junk.js"), b"// skip me").unwrap();

        let count = copy_tree(&src, &dst).unwrap();

        assert_eq!(count, 2, "should copy 2 real files, skipping node_modules");
        assert!(dst.join("index.html").exists());
        assert!(dst.join("nested/app.js").exists());
        assert!(!dst.join("node_modules").exists(), "node_modules must be skipped");

        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn copy_tree_on_missing_source_is_a_noop() {
        let missing = std::env::temp_dir().join("mowis-does-not-exist-xyz");
        let dst = std::env::temp_dir().join(format!("mowis-dst-{}", rand_hex()));
        let count = copy_tree(&missing, &dst).unwrap();
        assert_eq!(count, 0);
        let _ = std::fs::remove_dir_all(&dst);
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

fn extract_tasks_from_text(text: &str) -> crate::plan::TaskGraph {
    use crate::plan::{TaskGraph, TaskNode, TaskId, ModelTier};

    let mut tasks = Vec::new();
    let mut current_id = String::new();
    let mut current_title = String::new();
    let mut current_desc = String::new();
    let mut current_deps: Vec<TaskId> = Vec::new();
    let mut current_tier = ModelTier::Fast;
    let mut current_budget: u32 = 20;
    let mut current_hint: Vec<String> = Vec::new();
    let mut in_task = false;

    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "[[task]]" {
            if in_task && !current_id.is_empty() {
                tasks.push(TaskNode {
                    id: TaskId(current_id.clone()),
                    title: current_title.clone(),
                    description: current_desc.clone(),
                    deps: current_deps.clone(),
                    model_tier: current_tier.clone(),
                    tool_budget: current_budget,
                    files_hint: current_hint.clone(),
                });
            }
            in_task = true;
            current_id.clear();
            current_title.clear();
            current_desc.clear();
            current_deps.clear();
            current_tier = ModelTier::Fast;
            current_budget = 20;
            current_hint.clear();
        } else if in_task {
            if let Some(val) = trimmed.strip_prefix("id = ") {
                current_id = val.trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("title = ") {
                current_title = val.trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("description = ") {
                current_desc = val.trim_matches('"').to_string();
            } else if let Some(val) = trimmed.strip_prefix("model_tier = ") {
                current_tier = match val.trim_matches('"') {
                    "mid" => ModelTier::Mid,
                    "flagship" => ModelTier::Flagship,
                    _ => ModelTier::Fast,
                };
            } else if let Some(val) = trimmed.strip_prefix("tool_budget = ") {
                current_budget = val.parse().unwrap_or(20);
            } else if let Some(val) = trimmed.strip_prefix("files_hint = ") {
                current_hint = val.trim_matches(|c| c == '[' || c == ']')
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
            } else if let Some(val) = trimmed.strip_prefix("deps = ") {
                current_deps = val.trim_matches(|c| c == '[' || c == ']')
                    .split(',')
                    .map(|s| s.trim().trim_matches('"').to_string())
                    .filter(|s| !s.is_empty())
                    .map(TaskId)
                    .collect();
            }
        }
    }

    // Push last task
    if in_task && !current_id.is_empty() {
        tasks.push(TaskNode {
            id: TaskId(current_id),
            title: current_title,
            description: current_desc,
            deps: current_deps,
            model_tier: current_tier,
            tool_budget: current_budget,
            files_hint: current_hint,
        });
    }

    TaskGraph { tasks }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConductorAction {
    Chat { reply: String },
    PlanDrafted { plan_id: PlanId },
    PlanRevised { plan_id: PlanId, new_version: u32 },
    AwaitingApproval { plan_id: PlanId },
}
