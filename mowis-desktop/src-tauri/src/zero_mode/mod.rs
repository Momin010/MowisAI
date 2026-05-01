// zero_mode/mod.rs — Zero-Protection direct LLM orchestration
//
// No agentd, no daemon, no overlayfs.
// Creates a real folder on the user's disk, then runs a tool-calling loop
// directly against the configured LLM provider.  Events are emitted as the
// same BridgeEvent types the rest of the app uses, so the existing UI works
// unchanged.
//
// Tool-calling loop:
//   1. Send system prompt + user message + tool definitions to LLM
//   2. If LLM returns text chunks → stream them to the UI
//   3. If LLM returns tool calls → execute each, emit task events, feed results back
//   4. Repeat until LLM returns no tool calls (finish_reason = stop)
//   5. Emit OrchestrationComplete

pub mod llm;
pub mod tools;
pub mod workspace;
pub mod intent;

use intent::{classify_intent, UserIntent};
use llm::{LlmMessage, LlmResponse, MessageContent, Role, ToolCallRequest};
use tools::{execute_tool, tool_definitions};
use workspace::WorkspaceInfo;

use crate::{BridgeEvent, Config, Task, TaskStatus};
use std::path::Path;
use tokio::sync::mpsc;
use tokio::time::{sleep, Duration};

// ── Public entry point ────────────────────────────────────────────────────────

pub use workspace::WorkspaceInfo as ZeroWorkspaceInfo;

/// Run a full zero-mode session.  Emits BridgeEvents for the UI.
/// Never panics; errors are surfaced via OrchestrationFailed.
pub async fn run_zero_session(
    session_id: String,
    prompt: String,
    config: Config,
    workspace: WorkspaceInfo,
    event_tx: mpsc::Sender<BridgeEvent>,
) {
    let ws_path = Path::new(&workspace.path).to_path_buf();

    // Classify intent: Chat or Build
    let intent = classify_intent(&prompt);
    
    match intent {
        UserIntent::Chat => {
            run_chat_mode(session_id, prompt, config, workspace, event_tx).await;
        }
        UserIntent::Build => {
            run_build_mode(session_id, prompt, config, workspace, event_tx).await;
        }
    }
}

/// Chat mode — no tools, just conversation
async fn run_chat_mode(
    _session_id: String,
    prompt: String,
    config: Config,
    _workspace: WorkspaceInfo,
    event_tx: mpsc::Sender<BridgeEvent>,
) {
    // Tell the frontend we're in chat mode
    let _ = event_tx.send(BridgeEvent::PlanReady {
        sandboxes: vec!["chat".into()],
        task_count: 0,
        agent_count: 0,
        mode: "chat".into(),
    }).await;

    // NO intro text - just start responding

    // Simple chat system prompt
    let system_prompt = "You are MowisAI, a helpful AI coding assistant. \
        Answer questions, explain concepts, and provide guidance. \
        You're in chat mode - you cannot execute code or modify files right now. \
        If the user wants to build something, they should ask explicitly (e.g., 'build a login page').";

    let messages = vec![LlmMessage::user(prompt)];

    // Call LLM without tools
    let response = match llm::call_llm(&config, system_prompt, &messages, &[]).await {
        Ok(r) => r,
        Err(e) => {
            let _ = event_tx.send(BridgeEvent::OrchestrationFailed(
                format!("LLM error: {e}")
            )).await;
            return;
        }
    };

    // Stream the response (ONLY the actual LLM response)
    if !response.text.is_empty() {
        for chunk in chunk_text(&response.text, 60) {
            let _ = event_tx.send(BridgeEvent::AgentChunk(chunk)).await;
            sleep(Duration::from_millis(12)).await;
        }
    }

    let _ = event_tx.send(BridgeEvent::OrchestrationComplete).await;
}

/// Build mode — full tool-calling loop
async fn run_build_mode(
    _session_id: String,
    prompt: String,
    config: Config,
    workspace: WorkspaceInfo,
    event_tx: mpsc::Sender<BridgeEvent>,
) {
    let ws_path = Path::new(&workspace.path).to_path_buf();

    // Tell the frontend we're in zero mode (workspace path shown in UI box, not chat)
    let _ = event_tx.send(BridgeEvent::PlanReady {
        sandboxes: vec!["zero".into()],
        task_count: 0,
        agent_count: 1,
        mode: "zero".into(),
    }).await;

    // NO intro text - workspace path is in the UI already

    // Tool definitions (static; same for every provider).
    let tool_defs = tool_definitions();
    // Flatten if the definitions were returned as a nested array.
    let tool_defs: Vec<serde_json::Value> = tool_defs.into_iter().flat_map(|v| {
        if let serde_json::Value::Array(arr) = v { arr } else { vec![v] }
    }).collect();

    // Build initial conversation.
    let system_prompt = system_prompt_for(&workspace);
    let mut messages: Vec<LlmMessage> = vec![
        LlmMessage::user(prompt.clone()),
    ];

    let mut task_counter: usize = 0;
    let mut total_tool_calls: usize = 0;
    const MAX_ROUNDS: usize = 40; // safety cap

    for round in 0..MAX_ROUNDS {
        // Log context size for debugging
        let context_size: usize = messages.iter().map(|m| {
            m.parts.iter().map(|p| match p {
                MessageContent::Text(t) => t.len(),
                MessageContent::ToolResult { content, .. } => content.len(),
                _ => 50, // tool calls are small
            }).sum::<usize>()
        }).sum();
        log::info!("Round {}: {} messages, ~{} chars in context", round + 1, messages.len(), context_size);

        // Estimate tokens for this round (rough: 4 chars per token)
        let estimated_tokens = (context_size / 4) as u64 + 500; // +500 for system prompt & tool defs

        let response = match llm::call_llm(&config, &system_prompt, &messages, &tool_defs).await {
            Ok(r) => r,
            Err(e) => {
                let _ = event_tx.send(BridgeEvent::OrchestrationFailed(
                    format!("LLM error: {e}")
                )).await;
                return;
            }
        };

        // Stream any text the model produced (ONLY the actual LLM response, no metadata)
        if !response.text.is_empty() {
            for chunk in chunk_text(&response.text, 60) {
                let _ = event_tx.send(BridgeEvent::AgentChunk(chunk)).await;
                sleep(Duration::from_millis(12)).await;
            }
            // Record the assistant turn (text only, calls recorded below).
            if response.tool_calls.is_empty() {
                messages.push(LlmMessage::assistant_text(response.text.clone()));
            }
        }

        // If no tool calls, the model is done.
        if response.tool_calls.is_empty() {
            break;
        }

        // Record assistant turn that included tool calls.
        let all_parts: Vec<MessageContent> = {
            let mut p = Vec::new();
            if !response.text.is_empty() {
                p.push(MessageContent::Text(response.text.clone()));
            }
            for tc in &response.tool_calls {
                p.push(MessageContent::ToolCall(tc.clone()));
            }
            p
        };
        messages.push(LlmMessage {
            role: Role::Assistant,
            parts: all_parts,
        });

        // Execute tool calls sequentially, emit task events.
        let mut tool_results: Vec<(String, String)> = Vec::new();
        let mut file_changes: Vec<crate::FileChange> = Vec::new();

        for tc in &response.tool_calls {
            task_counter += 1;
            total_tool_calls += 1;
            let task_id = format!("z{task_counter:04}");

            // Emit task_added (pending → running → complete).
            let task_desc = tool_call_description(tc);
            let task = Task {
                id: task_id.clone(),
                description: task_desc.clone(),
                sandbox: Some("zero".into()),
                status: TaskStatus::Pending,
                started_at: None,
                completed_at: None,
                files: infer_files_from_call(tc),
                summary: None,
                views: Vec::new(),
            };
            let _ = event_tx.send(BridgeEvent::TaskAdded(task)).await;
            sleep(Duration::from_millis(30)).await;
            let _ = event_tx.send(BridgeEvent::TaskUpdated {
                id: task_id.clone(),
                status: TaskStatus::Running,
            }).await;

            // Execute on disk.
            let result = execute_tool(&ws_path, &tc.name, &tc.args);

            let task_ok = !result.starts_with("error");
            let _ = event_tx.send(BridgeEvent::TaskUpdated {
                id: task_id.clone(),
                status: if task_ok { TaskStatus::Complete } else { TaskStatus::Failed },
            }).await;

            // Emit stats tick for usage tracking (use estimated tokens from this round)
            let _ = event_tx.send(BridgeEvent::SimulationTick {
                tasks_done: task_counter,
                active_agents: 1,
                tokens_delta: estimated_tokens,
            }).await;

            // Track file changes for compact summary
            if task_ok {
                if let Some(path) = tc.args["path"].as_str() {
                    let action = match tc.name.as_str() {
                        tools::WRITE_FILE => crate::FileAction::Created,
                        tools::APPEND_FILE | tools::REPLACE_IN_FILE | tools::EDIT_FILE_LINES => crate::FileAction::Modified,
                        tools::DELETE_FILE => crate::FileAction::Deleted,
                        tools::READ_FILE | tools::READ_FILE_LINES => crate::FileAction::Read,
                        _ => continue,
                    };
                    file_changes.push(crate::FileChange {
                        path: path.to_string(),
                        action,
                    });
                } else if tc.name == tools::MOVE_FILE {
                    if let Some(to) = tc.args["to"].as_str() {
                        file_changes.push(crate::FileChange {
                            path: to.to_string(),
                            action: crate::FileAction::Moved,
                        });
                    }
                }
            }

            // NO chat echo for tool results - they're shown in the task list UI

            // CRITICAL: Truncate tool results to prevent token explosion
            // Full file contents can be 10KB+, but the LLM only needs a summary
            let truncated_result = if result.len() > 1000 {
                // For read operations, show first 500 chars + summary
                if tc.name == tools::READ_FILE || tc.name == tools::READ_FILE_LINES {
                    format!("{}...\n[truncated: {} total chars]", &result[..500], result.len())
                } else {
                    // For other operations, just show success/error
                    if task_ok {
                        format!("✓ {} completed successfully", tc.name)
                    } else {
                        result[..500].to_string()
                    }
                }
            } else {
                result
            };

            tool_results.push((tc.id.clone(), truncated_result));
        }

        // Emit compact file changes summary to show in chat
        if !file_changes.is_empty() {
            let _ = event_tx.send(BridgeEvent::FileChanges(file_changes)).await;
        }

        // Feed all tool results back as a single Tool message.
        messages.push(LlmMessage::tool_results(tool_results));

        // CRITICAL: Sliding window to prevent context explosion
        // Keep only: initial user message + last 6 messages (3 rounds of back-and-forth)
        // This prevents token usage from growing linearly with each tool call
        const MAX_CONTEXT_MESSAGES: usize = 7; // 1 user + 6 recent messages
        if messages.len() > MAX_CONTEXT_MESSAGES {
            let user_msg = messages[0].clone(); // Keep original prompt
            let recent: Vec<_> = messages.iter().rev().take(6).rev().cloned().collect();
            messages = vec![user_msg];
            messages.extend(recent);
        }

        // Brief yield so the UI can render.
        sleep(Duration::from_millis(40)).await;
    }

    // NO closing summary - just complete silently
    let _ = event_tx.send(BridgeEvent::OrchestrationComplete).await;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn system_prompt_for(ws: &WorkspaceInfo) -> String {
    format!(
        "You are an AI agent running in Zero-Protection mode on the user's computer.\n\
         Your workspace directory is: {path}\n\n\
         You have 13 tools available:\n\
         - read_file, read_file_lines — read entire files or specific line ranges\n\
         - write_file, append_file — create or modify files\n\
         - replace_in_file, edit_file_lines — find/replace text or edit specific lines\n\
         - list_directory, create_directory, delete_file, move_file — filesystem ops\n\
         - search_files, search_in_files — find files by name or search content (grep)\n\
         - run_command — execute shell commands (use sparingly)\n\n\
         All paths you supply must be workspace-relative (e.g. 'src/main.py', not '/home/…').\n\
         Do not reference files outside the workspace.\n\
         Work systematically: understand the request → plan → execute → verify.\n\
         Ask clarifying questions if the request is ambiguous.\n\
         When you are finished, summarize what was created or changed.",
        path = ws.path
    )
}

/// Split text into roughly equal-sized chunks for UI streaming effect.
fn chunk_text(text: &str, chunk_size: usize) -> Vec<String> {
    text.chars()
        .collect::<Vec<char>>()
        .chunks(chunk_size)
        .map(|c| c.iter().collect())
        .collect()
}

/// Human-readable description of what a tool call is doing.
fn tool_call_description(tc: &ToolCallRequest) -> String {
    match tc.name.as_str() {
        tools::READ_FILE        => format!("Read {}", tc.args["path"].as_str().unwrap_or("?")),
        tools::READ_FILE_LINES  => format!("Read {} (lines {}-{})", 
            tc.args["path"].as_str().unwrap_or("?"),
            tc.args["start_line"].as_i64().unwrap_or(0),
            tc.args["end_line"].as_i64().unwrap_or(0)),
        tools::WRITE_FILE       => format!("Write {}", tc.args["path"].as_str().unwrap_or("?")),
        tools::APPEND_FILE      => format!("Append {}", tc.args["path"].as_str().unwrap_or("?")),
        tools::REPLACE_IN_FILE  => format!("Replace in {}", tc.args["path"].as_str().unwrap_or("?")),
        tools::EDIT_FILE_LINES  => format!("Edit {} (lines {}-{})", 
            tc.args["path"].as_str().unwrap_or("?"),
            tc.args["start_line"].as_i64().unwrap_or(0),
            tc.args["end_line"].as_i64().unwrap_or(0)),
        tools::LIST_DIRECTORY   => format!("List {}", tc.args["path"].as_str().unwrap_or("workspace")),
        tools::CREATE_DIRECTORY => format!("mkdir {}", tc.args["path"].as_str().unwrap_or("?")),
        tools::DELETE_FILE      => format!("Delete {}", tc.args["path"].as_str().unwrap_or("?")),
        tools::MOVE_FILE        => format!("Move {} → {}", tc.args["from"].as_str().unwrap_or("?"), tc.args["to"].as_str().unwrap_or("?")),
        tools::SEARCH_FILES     => format!("Search '{}'", tc.args["pattern"].as_str().unwrap_or("?")),
        tools::SEARCH_IN_FILES  => format!("Grep '{}'", tc.args["pattern"].as_str().unwrap_or("?")),
        tools::RUN_COMMAND      => {
            let cmd = tc.args["command"].as_str().unwrap_or("?");
            let short = if cmd.len() > 40 { format!("{}…", &cmd[..40]) } else { cmd.to_owned() };
            format!("Run: {short}")
        }
        other => format!("{other}"),
    }
}

/// Infer which files are affected by a tool call (shown in the task detail panel).
fn infer_files_from_call(tc: &ToolCallRequest) -> Vec<String> {
    match tc.name.as_str() {
        tools::WRITE_FILE | tools::READ_FILE | tools::READ_FILE_LINES | 
        tools::APPEND_FILE | tools::DELETE_FILE | tools::REPLACE_IN_FILE | 
        tools::EDIT_FILE_LINES => {
            tc.args["path"].as_str().map(|p| vec![p.to_owned()]).unwrap_or_default()
        }
        tools::MOVE_FILE => {
            let from = tc.args["from"].as_str().map(ToOwned::to_owned);
            let to   = tc.args["to"].as_str().map(ToOwned::to_owned);
            [from, to].into_iter().flatten().collect()
        }
        _ => Vec::new(),
    }
}
