use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::events::{Event, EventBus};
use crate::plan::{PlanId, TaskId};
use crate::providers::{AgentConversation, LlmConfig, ToolCall};
use crate::summaries::{summarize, ToolOutcome};
use crate::tools::ToolGateway;

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

        self.bus.emit(Event::CrewStarted {
            plan_id: self.plan_id.clone(),
            task_id: self.task.task_id.clone(),
            agent_id: self.agent_id.clone(),
        });

        loop {
            if conversation.round_count() as u32 >= self.task.tool_budget {
                tracing::warn!(
                    agent = %self.agent_id,
                    rounds = conversation.round_count(),
                    budget = self.task.tool_budget,
                    "tool budget exceeded"
                );
                return Ok(CrewOutcome::Failed {
                    agent_id: self.agent_id.clone(),
                    reason: "tool budget exceeded".into(),
                    tool_calls: tool_call_count,
                });
            }

            let round = match crate::providers::call_agent_round(
                &self.llm_config,
                &conversation,
                &tool_schemas,
                &system_prompt,
            )
            .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(agent = %self.agent_id, error = %e, "LLM call failed");
                    return Ok(CrewOutcome::Failed {
                        agent_id: self.agent_id.clone(),
                        reason: format!("LLM error: {}", e),
                        tool_calls: tool_call_count,
                    });
                }
            };

            if round.is_final() {
                let summary = round.text.unwrap_or_else(|| "Task completed".into());
                self.bus.emit(Event::CrewDone {
                    plan_id: self.plan_id.clone(),
                    agent_id: self.agent_id.clone(),
                    summary: summary.clone(),
                });
                return Ok(CrewOutcome::Done {
                    agent_id: self.agent_id.clone(),
                    summary,
                    tool_calls: tool_call_count,
                });
            }

            let mut results = Vec::new();
            for tc in round.tool_calls {
                tool_call_count += 1;

                // Check whitelist
                if !self.tool_gateway.allows(&tc.name) {
                    let outcome = ToolOutcome::Denied;
                    let summary_text = summarize(&tc, &outcome);

                    self.bus.emit(Event::CrewToolSummary {
                        agent_id: self.agent_id.clone(),
                        text: summary_text,
                        tool_name: tc.name.clone(),
                        success: false,
                    });

                    results.push((tc, json!({"error": "tool forbidden"})));
                    continue;
                }

                // Invoke tool through gateway
                let result = self.tool_gateway.invoke(tc.clone()).await.unwrap_or_else(|e| {
                    json!({"error": e.to_string()})
                });

                // Generate deterministic summary
                let outcome: ToolOutcome = result.clone().into();
                let summary_text = summarize(&tc, &outcome);

                self.bus.emit(Event::CrewToolSummary {
                    agent_id: self.agent_id.clone(),
                    text: summary_text,
                    tool_name: tc.name.clone(),
                    success: outcome.is_ok(),
                });

                results.push((tc, result));
            }
            conversation.push_tool_results(results);
        }
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
