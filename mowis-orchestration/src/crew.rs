use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

use crate::events::{Event, EventBus};
use crate::plan::{PlanId, TaskId};
use crate::providers::{AgentConversation, LlmConfig, ToolCall};
use crate::summaries::{summarize, ToolOutcome};
use crate::tools::ToolGateway;

/// Hard budget caps to prevent runaway agent loops.
const MAX_ROUNDS: u32 = 8;
const MAX_TOOL_CALLS: u32 = 12;
const MAX_INPUT_TOKENS: u64 = 100_000;

#[derive(Debug, Clone)]
pub struct Crew {
    plan_id: PlanId,
    sandbox_id: String,
    agent_id: String,
    task: CrewTask,
    llm_config: LlmConfig,
    tool_gateway: ToolGateway,
    bus: EventBus,
}

#[derive(Debug, Clone)]
pub struct CrewTask {
    pub task_id: TaskId,
    pub title: String,
    pub description: String,
    pub files_hint: Vec<String>,
    pub tool_budget: u32,
}

impl Crew {
    pub fn new(
        plan_id: PlanId,
        sandbox_id: String,
        agent_id: String,
        task: CrewTask,
        llm_config: LlmConfig,
        tool_gateway: ToolGateway,
        bus: EventBus,
    ) -> Self {
        Self {
            plan_id,
            sandbox_id,
            agent_id,
            task,
            llm_config,
            tool_gateway,
            bus,
        }
    }

    pub async fn run(self) -> Result<CrewOutcome> {
        let system_prompt = include_str!("prompts/crew.md")
            .replace("{{task_title}}", &self.task.title)
            .replace("{{task_description}}", &self.task.description)
            .replace("{{files_hint}}", &self.task.files_hint.join(", "));

        let mut conversation = AgentConversation::new();
        conversation.push_user(format!(
            "Complete this task: {}\n\nDescription: {}",
            self.task.title, self.task.description
        ));

        let tool_schemas = crew_tool_schemas();
        let mut tool_call_count: u32 = 0;
        let mut total_input_tokens: u64 = 0;

        // Track recent tool calls for duplicate detection.
        // Key: (tool_name, args_hash), Value: call index.
        let mut recent_calls: HashMap<(String, u64), u32> = HashMap::new();

        // Track files already written — reject second write to same path.
        let mut written_files: HashMap<String, u32> = HashMap::new();

        // Track all write/append/delete operations for dominance check.
        let mut has_written: bool = false;

        self.bus.emit(Event::CrewStarted {
            plan_id: self.plan_id.clone(),
            task_id: self.task.task_id.clone(),
            agent_id: self.agent_id.clone(),
        });

        loop {
            let rounds = conversation.round_count() as u32;

            // ── Budget checks ──────────────────────────────────────────
            if rounds >= MAX_ROUNDS {
                tracing::warn!(agent = %self.agent_id, rounds, "round budget hit — forcing finalization");
                conversation.push_user(
                    "[SYSTEM] You have used your iteration budget. Emit your final answer now. \
                     Do not call any more tools. Summarize what you accomplished.".to_string()
                );
                // One more LLM call to get the final answer, then break.
                let final_round = match crate::providers::call_agent_round(
                    &self.llm_config, &conversation, &[], &system_prompt,
                ).await {
                    Ok(r) => r,
                    Err(_) => break,
                };
                let summary = final_round.text.unwrap_or_else(|| "Task completed (budget)".into());
                self.bus.emit(Event::CrewDone {
                    plan_id: self.plan_id.clone(), agent_id: self.agent_id.clone(), summary: summary.clone(),
                });
                return Ok(CrewOutcome::Done { agent_id: self.agent_id.clone(), summary, tool_calls: tool_call_count });
            }
            if tool_call_count >= MAX_TOOL_CALLS {
                tracing::warn!(agent = %self.agent_id, tool_call_count, "tool call budget hit");
                return Ok(CrewOutcome::Failed {
                    agent_id: self.agent_id.clone(), reason: "tool call budget exceeded".into(), tool_calls: tool_call_count,
                });
            }
            if total_input_tokens >= MAX_INPUT_TOKENS {
                tracing::warn!(agent = %self.agent_id, total_input_tokens, "input token budget hit");
                return Ok(CrewOutcome::Failed {
                    agent_id: self.agent_id.clone(), reason: "input token budget exceeded".into(), tool_calls: tool_call_count,
                });
            }

            let round = match crate::providers::call_agent_round(
                &self.llm_config, &conversation, &tool_schemas, &system_prompt,
            ).await {
                Ok(r) => {
                    total_input_tokens += r.usage.input_tokens as u64;
                    self.bus.emit(Event::TokensUsed {
                        agent_id: self.agent_id.clone(), role: "crew".into(),
                        input_tokens: r.usage.input_tokens, output_tokens: r.usage.output_tokens,
                        model: self.llm_config.model.clone(),
                    });
                    r
                }
                Err(e) => {
                    tracing::error!(agent = %self.agent_id, error = %e, "LLM call failed");
                    return Ok(CrewOutcome::Failed {
                        agent_id: self.agent_id.clone(), reason: format!("LLM error: {}", e), tool_calls: tool_call_count,
                    });
                }
            };

            if round.is_final() {
                let summary = round.text.unwrap_or_else(|| "Task completed".into());
                self.bus.emit(Event::CrewDone {
                    plan_id: self.plan_id.clone(), agent_id: self.agent_id.clone(), summary: summary.clone(),
                });
                return Ok(CrewOutcome::Done { agent_id: self.agent_id.clone(), summary, tool_calls: tool_call_count });
            }

            let mut results = Vec::new();
            for tc in round.tool_calls {

                if !self.tool_gateway.allows(&tc.name) {
                    let outcome = ToolOutcome::Denied;
                    let summary_text = summarize(&tc, &outcome);
                    self.bus.emit(Event::CrewToolSummary {
                        agent_id: self.agent_id.clone(), text: summary_text, tool_name: tc.name.clone(), success: false,
                    });
                    results.push((tc, json!({"error": "tool forbidden"})));
                    continue;
                }

                // ── Per-file write dedup: reject second write to same path ──
                if tc.name == "write_file" {
                    if let Some(path) = tc.args.get("path").and_then(|v| v.as_str()) {
                        if let Some(&prev_idx) = written_files.get(path) {
                            let msg = format!(
                                "BLOCKED: You already wrote to {} (call #{}). \
                                 A crew agent must write each file EXACTLY ONCE. \
                                 You are wasting tokens. Move on to the next file or emit your summary.",
                                path, prev_idx
                            );
                            self.bus.emit(Event::CrewToolSummary {
                                agent_id: self.agent_id.clone(), text: msg.clone(),
                                tool_name: tc.name.clone(), success: false,
                            });
                            results.push((tc, json!({"error": msg})));
                            continue;
                        }
                        written_files.insert(path.to_string(), tool_call_count);
                        has_written = true;
                    }
                }

                // ── Block reads of files already written (model has content in context) ──
                if tc.name == "read_file" {
                    if let Some(path) = tc.args.get("path").and_then(|v| v.as_str()) {
                        if written_files.contains_key(path) {
                            let msg = format!(
                                "BLOCKED: You already wrote {}. You HAVE its content in context. \
                                 Reading it back is a waste of tokens. STOP verifying your own work.",
                                path
                            );
                            self.bus.emit(Event::CrewToolSummary {
                                agent_id: self.agent_id.clone(), text: msg.clone(),
                                tool_name: tc.name.clone(), success: false,
                            });
                            results.push((tc, json!({"error": msg})));
                            continue;
                        }
                    }
                }

                // ── Duplicate call detection ──────────────────────────────
                let args_hash = json_hash(&tc.args);
                let call_key = (tc.name.clone(), args_hash);
                if let Some(&prev_idx) = recent_calls.get(&call_key) {
                    // Same tool + same args — short-circuit with a reproach.
                    let msg = format!(
                        "BLOCKED: You already called {} with the same arguments (call #{}). \
                         Nothing changed. STOP and move on.",
                        tc.name, prev_idx
                    );
                    self.bus.emit(Event::CrewToolSummary {
                        agent_id: self.agent_id.clone(), text: msg.clone(),
                        tool_name: tc.name.clone(), success: false,
                    });
                    results.push((tc, json!({"error": msg})));
                    continue;
                }
                recent_calls.insert(call_key, tool_call_count);

                // Count this as a real tool call (not blocked).
                tool_call_count += 1;

                let result = self.tool_gateway.invoke(tc.clone()).await.unwrap_or_else(|e| {
                    json!({"error": e.to_string()})
                });

                if let Some(prompt_text) = result.get("interactive_prompt").and_then(|v| v.as_str()) {
                    if !prompt_text.is_empty() {
                        conversation.push_tool_results(vec![(tc.clone(), result.clone())]);
                        conversation.push_user(format!(
                            "The command is waiting for input. The prompt is:\n{}\nWhat should I type? Respond with ONLY the input text, nothing else.",
                            prompt_text
                        ));
                        let input_round = crate::providers::call_agent_round(
                            &self.llm_config, &conversation,
                            &[json!({"name": "send_input", "description": "Send input to interactive command", "parameters": {"type": "object", "properties": {"input": {"type": "string"}}, "required": ["input"]}})],
                            "You are helping an interactive command. Respond with the exact input to send.",
                        ).await;
                        if let Ok(input_result) = input_round {
                            let input_text = input_result.text.unwrap_or_else(|| "y".into());
                            let input_clean = input_text.trim().to_string();
                            let send_result = self.tool_gateway.invoke(crate::providers::ToolCall {
                                id: format!("{}-input", tc.id), name: "send_input".into(),
                                args: json!({"input": input_clean}),
                            }).await;
                            self.bus.emit(Event::CrewToolSummary {
                                agent_id: self.agent_id.clone(),
                                text: format!("Sent input to interactive command: {}", input_clean),
                                tool_name: "send_input".into(), success: send_result.is_ok(),
                            });
                        }
                        results.push((tc, result));
                        continue;
                    }
                }

                let outcome: ToolOutcome = result.clone().into();
                let summary_text = summarize(&tc, &outcome);
                self.bus.emit(Event::CrewToolSummary {
                    agent_id: self.agent_id.clone(), text: summary_text,
                    tool_name: tc.name.clone(), success: outcome.is_ok(),
                });
                results.push((tc, result));
            }
            conversation.push_tool_results(results);
        }

        // Loop exited without explicit Done — treat as completion with what we have.
        let summary: String = "Task completed (budget exhaustion)".into();
        self.bus.emit(Event::CrewDone {
            plan_id: self.plan_id.clone(), agent_id: self.agent_id.clone(), summary: summary.clone(),
        });
        Ok(CrewOutcome::Done { agent_id: self.agent_id.clone(), summary, tool_calls: tool_call_count })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CrewOutcome {
    Done {
        agent_id: String,
        summary: String,
        tool_calls: u32,
    },
    Failed {
        agent_id: String,
        reason: String,
        tool_calls: u32,
    },
}

/// Simple deterministic hash of a JSON value for duplicate call detection.
fn json_hash(v: &serde_json::Value) -> u64 {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let s = serde_json::to_string(v).unwrap_or_default();
    let mut hasher = DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

fn crew_tool_schemas() -> Vec<serde_json::Value> {
    vec![
        json!({
            "name": "read_file",
            "description": "Read a file from the sandbox.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path to read"}
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "write_file",
            "description": "Write content to a file.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path to write"},
                    "contents": {"type": "string", "description": "Content to write"}
                },
                "required": ["path", "contents"]
            }
        }),
        json!({
            "name": "list_files",
            "description": "List files in a directory.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory path"}
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "run_command",
            "description": "Run a shell command.",
            "parameters": {
                "type": "object",
                "properties": {
                    "cmd": {"type": "string", "description": "Command to run"},
                    "cwd": {"type": "string", "description": "Working directory"}
                },
                "required": ["cmd"]
            }
        }),
        json!({
            "name": "grep",
            "description": "Search files for a pattern.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Regex pattern"},
                    "path": {"type": "string", "description": "Directory to search"},
                    "include": {"type": "string", "description": "File glob filter"}
                },
                "required": ["pattern", "path"]
            }
        }),
        json!({
            "name": "find_files",
            "description": "Find files by name pattern.",
            "parameters": {
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Glob pattern"},
                    "path": {"type": "string", "description": "Directory to search"}
                },
                "required": ["pattern", "path"]
            }
        }),
        json!({
            "name": "git_status",
            "description": "Get git status.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Repository path"}
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "git_add",
            "description": "Stage files.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Repository path"},
                    "files": {"type": "array", "description": "Files to stage"}
                },
                "required": ["path", "files"]
            }
        }),
        json!({
            "name": "git_commit",
            "description": "Create a commit.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Repository path"},
                    "message": {"type": "string", "description": "Commit message"}
                },
                "required": ["path", "message"]
            }
        }),
        json!({
            "name": "git_diff",
            "description": "Get git diff.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Repository path"}
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "http_get",
            "description": "HTTP GET request.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "URL to fetch"}
                },
                "required": ["url"]
            }
        }),
        json!({
            "name": "http_post",
            "description": "HTTP POST request.",
            "parameters": {
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "URL"},
                    "body": {"type": "string", "description": "Request body"}
                },
                "required": ["url", "body"]
            }
        }),
        json!({
            "name": "append_file",
            "description": "Append to a file.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path"},
                    "contents": {"type": "string", "description": "Content to append"}
                },
                "required": ["path", "contents"]
            }
        }),
        json!({
            "name": "delete_file",
            "description": "Delete a file.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "File path"}
                },
                "required": ["path"]
            }
        }),
        json!({
            "name": "create_directory",
            "description": "Create a directory.",
            "parameters": {
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Directory path"}
                },
                "required": ["path"]
            }
        }),
    ]
}
