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

// ── Skill loading ─────────────────────────────────────────────────────────────

/// Check if the prompt is asking to build a website/frontend
fn is_website_project(prompt: &str) -> bool {
    let lower = prompt.to_lowercase();
    let keywords = [
        "website", "web page", "landing page", "frontend", "ui", "interface",
        "dashboard", "web app", "html", "css", "react", "vue", "component",
        "portfolio", "blog", "site", "webpage"
    ];
    keywords.iter().any(|kw| lower.contains(kw))
}

/// Load frontend skill files if they exist
fn load_frontend_skills() -> Option<String> {
    let skill_files = [
        "SKILL_FRONTNED (1).md",
        "SKILL_FRONTNED (2).md",
        "SKILL_FRONTEND (1).md",
        "SKILL_FRONTEND (2).md",
    ];
    let mut skills = Vec::new();
    
    for file in &skill_files {
        if let Ok(content) = std::fs::read_to_string(file) {
            skills.push(content);
        }
    }
    
    if skills.is_empty() {
        None
    } else {
        Some(skills.join("\n\n---\n\n"))
    }
}

// ── Public entry point ────────────────────────────────────────────────────────

pub use workspace::WorkspaceInfo as ZeroWorkspaceInfo;

// ── Session State Management ──────────────────────────────────────────────────

use std::sync::Mutex;
use std::collections::HashMap;

lazy_static::lazy_static! {
    static ref SESSION_HISTORY: Mutex<HashMap<String, Vec<LlmMessage>>> = Mutex::new(HashMap::new());
}

fn get_session_history(session_id: &str) -> Vec<LlmMessage> {
    SESSION_HISTORY
        .lock()
        .unwrap()
        .get(session_id)
        .cloned()
        .unwrap_or_default()
}

fn append_to_session(session_id: &str, messages: Vec<LlmMessage>) {
    let mut history = SESSION_HISTORY.lock().unwrap();
    history.entry(session_id.to_string())
        .or_insert_with(Vec::new)
        .extend(messages);
}

fn set_session_history(session_id: &str, messages: Vec<LlmMessage>) {
    let mut history = SESSION_HISTORY.lock().unwrap();
    history.insert(session_id.to_string(), messages);
}

fn clear_session(session_id: &str) {
    SESSION_HISTORY.lock().unwrap().remove(session_id);
}

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
    let original_history = get_session_history(&session_id);

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
    session_id: String,
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

    // Load previous conversation history
    let mut messages = get_session_history(&session_id);
    
    // Append new user message
    messages.push(LlmMessage::user(prompt.clone()));

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
    
    // Save assistant response to history
    let response_text = response.text.clone();
    messages.push(LlmMessage::assistant_text(response.text));
    append_to_session(&session_id, vec![
        LlmMessage::user(prompt),
        LlmMessage::assistant_text(response_text),
    ]);

    // NO OrchestrationComplete in chat mode - session stays active for follow-up messages
}

/// Build mode — full tool-calling loop
async fn run_build_mode(
    session_id: String,
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

    // Check if this is a website project and load skills
    let is_website = is_website_project(&prompt);
    let frontend_skills = if is_website {
        load_frontend_skills()
    } else {
        None
    };

    // Build initial conversation - load previous history
    let system_prompt = system_prompt_for(&workspace, frontend_skills.as_deref());
    let mut messages: Vec<LlmMessage> = get_session_history(&session_id);
    
    // Append new user message
    messages.push(LlmMessage::user(prompt.clone()));

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

        let response = match call_llm_with_retry(&config, &system_prompt, &messages, &tool_defs, &event_tx).await {
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

            // Capture "before" content for diff view (best-effort).
            let before_snapshot: Option<String> = tc
                .args
                .get("path")
                .and_then(|v| v.as_str())
                .and_then(|rel| {
                    let full = ws_path.join(rel);
                    if full.is_file() {
                        std::fs::read_to_string(full).ok()
                    } else {
                        None
                    }
                });

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
                    let full_path = ws_path.join(path);
                    let existed_before = before_snapshot.is_some();
                    let action = match tc.name.as_str() {
                        tools::WRITE_FILE => {
                            if existed_before { crate::FileAction::Modified } else { crate::FileAction::Created }
                        }
                        tools::APPEND_FILE | tools::REPLACE_IN_FILE | tools::EDIT_FILE_LINES => crate::FileAction::Modified,
                        tools::DELETE_FILE => crate::FileAction::Deleted,
                        tools::READ_FILE | tools::READ_FILE_LINES => crate::FileAction::Read,
                        _ => continue,
                    };
                    
                    // Count lines and read content for diff view
                    let (lines_added, lines_deleted, content) = if action != crate::FileAction::Read && action != crate::FileAction::Deleted {
                        if let Ok(file_content) = std::fs::read_to_string(&full_path) {
                            let line_count = file_content.lines().count();
                            let (added, deleted) = match action {
                                crate::FileAction::Created => (line_count, 0),
                                crate::FileAction::Modified => {
                                    // For modifications, estimate based on tool args
                                    if tc.name == tools::APPEND_FILE {
                                        if let Some(text) = tc.args["text"].as_str() {
                                            (text.lines().count(), 0)
                                        } else {
                                            (0, 0)
                                        }
                                    } else if tc.name == tools::REPLACE_IN_FILE {
                                        // Estimate: assume similar line count
                                        (5, 5)
                                    } else {
                                        (line_count, 0)
                                    }
                                },
                                _ => (0, 0),
                            };
                            (added, deleted, Some(file_content))
                        } else {
                            (0, 0, None)
                        }
                    } else if action == crate::FileAction::Deleted {
                        // For deleted files, we can't read content anymore
                        (0, 0, None)
                    } else {
                        (0, 0, None)
                    };
                    
                    file_changes.push(crate::FileChange {
                        path: path.to_string(),
                        action,
                        lines_added,
                        lines_deleted,
                        before_content: before_snapshot,
                        content,
                    });
                } else if tc.name == tools::MOVE_FILE {
                    if let Some(to) = tc.args["to"].as_str() {
                        file_changes.push(crate::FileChange {
                            path: to.to_string(),
                            action: crate::FileAction::Moved,
                            lines_added: 0,
                            lines_deleted: 0,
                            before_content: None,
                            content: None,
                        });
                    }
                }
            }

            // NO chat echo for tool results - they're shown in the task list UI

            // CRITICAL: Truncate tool results to prevent token explosion
            // Full file contents can be 10KB+, but the LLM only needs a summary
            let truncated_result = compact_tool_result(&tc.name, result, task_ok);

            tool_results.push((tc.id.clone(), truncated_result));
        }

        // Emit compact file changes summary to show in chat
        if !file_changes.is_empty() {
            let _ = event_tx.send(BridgeEvent::FileChanges(file_changes)).await;
        }

        // Feed all tool results back as a single Tool message.
        messages.push(LlmMessage::tool_results(tool_results));

        shrink_context_for_budget(&mut messages);

        // Brief yield so the UI can render.
        sleep(Duration::from_millis(40)).await;
    }

    // ═══════════════════════════════════════════════════════════════════════════════
    // VALIDATION LOOP - Force the model to do better work
    // ═══════════════════════════════════════════════════════════════════════════════
    
    // Check if this was a build task (not just chat)
    if total_tool_calls > 0 {
        // Force validation: tell the model to review and improve its work
        let validation_prompt = "Perform one focused quality pass on your latest changes. \
            Check correctness, missing edge cases, obvious UX issues, and broken imports. \
            If issues exist, fix them now with tool calls. \
            Keep edits high-signal and avoid rewriting files unless necessary.";
        
        messages.push(LlmMessage::user(validation_prompt.to_string()));
        
        // Run ONE more round of tool-calling for improvements
        let response = match call_llm_with_retry(&config, &system_prompt, &messages, &tool_defs, &event_tx).await {
            Ok(r) => r,
            Err(e) => {
                log::warn!("Validation round failed: {e}");
                return; // Don't fail the whole session
            }
        };
        
        // Stream validation response
        if !response.text.is_empty() {
            for chunk in chunk_text(&response.text, 60) {
                let _ = event_tx.send(BridgeEvent::AgentChunk(chunk)).await;
                sleep(Duration::from_millis(12)).await;
            }
        }
        
        // Execute any improvement tool calls
        if !response.tool_calls.is_empty() {
            messages.push(LlmMessage {
                role: Role::Assistant,
                parts: response.tool_calls.iter().map(|tc| MessageContent::ToolCall(tc.clone())).collect(),
            });
            
            let mut improvement_results = Vec::new();
            let mut improvement_changes = Vec::new();
            
            for tc in &response.tool_calls {
                task_counter += 1;
                let task_id = format!("z{task_counter:04}");
                
                let task_desc = tool_call_description(tc);
                let task = Task {
                    id: task_id.clone(),
                    description: format!("✨ Improve: {}", task_desc),
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
                
                let result = execute_tool(&ws_path, &tc.name, &tc.args);
                let task_ok = !result.starts_with("error");
                
                let _ = event_tx.send(BridgeEvent::TaskUpdated {
                    id: task_id.clone(),
                    status: if task_ok { TaskStatus::Complete } else { TaskStatus::Failed },
                }).await;
                
                // Track improvements
                if task_ok {
                    if let Some(path) = tc.args["path"].as_str() {
                        let action = match tc.name.as_str() {
                            tools::WRITE_FILE => crate::FileAction::Created,
                            tools::APPEND_FILE | tools::REPLACE_IN_FILE | tools::EDIT_FILE_LINES => crate::FileAction::Modified,
                            _ => continue,
                        };
                        
                        let full_path = ws_path.join(path);
                        let (lines_added, lines_deleted, content) = if let Ok(file_content) = std::fs::read_to_string(&full_path) {
                            let line_count = file_content.lines().count();
                            let (added, deleted) = match action {
                                crate::FileAction::Created => (line_count, 0),
                                crate::FileAction::Modified => {
                                    if tc.name == tools::APPEND_FILE {
                                        if let Some(text) = tc.args["text"].as_str() {
                                            (text.lines().count(), 0)
                                        } else {
                                            (0, 0)
                                        }
                                    } else {
                                        (10, 5) // Estimate for edits
                                    }
                                },
                                _ => (0, 0),
                            };
                            (added, deleted, Some(file_content))
                        } else {
                            (0, 0, None)
                        };
                        
                    improvement_changes.push(crate::FileChange {
                            path: path.to_string(),
                            action,
                            lines_added,
                            lines_deleted,
                        before_content: None,
                            content,
                        });
                    }
                }
                
                let truncated_result = compact_tool_result(&tc.name, result, task_ok);
                improvement_results.push((tc.id.clone(), truncated_result));
            }
            
            if !improvement_changes.is_empty() {
                let _ = event_tx.send(BridgeEvent::FileChanges(improvement_changes)).await;
            }
            
            messages.push(LlmMessage::tool_results(improvement_results));
        }
    }
    
    // Persist compacted history to prevent unbounded growth across turns.
    set_session_history(&session_id, messages);

    // NO OrchestrationComplete in zero mode - session stays active for follow-up messages
    // The session will naturally pause after inactivity timeout (handled by frontend)
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn system_prompt_for(ws: &WorkspaceInfo, frontend_skills: Option<&str>) -> String {
    let base_prompt = format!(
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
         Do not reference files outside the workspace.\n\n\
         Efficiency and quality rules:\n\
         - Prefer complete writes over many tiny append operations.\n\
         - Keep tool calls purposeful; avoid repeated reads/writes of the same file.\n\
         - For broad changes, create a short plan, then execute in coherent batches.\n\
         - Use run_command for validation/build checks when useful, then fix failures.\n\
         - If a command or tool fails, analyze the error and recover before continuing.\n\n\
         Work systematically: understand the request → plan → execute → verify.\n\
         Ask clarifying questions if the request is ambiguous.\n\
         When you are finished, summarize what was created or changed.",
        path = ws.path
    );

    if let Some(skills) = frontend_skills {
        format!(
            "{}\n\n\
             ═══════════════════════════════════════════════════════════════════════════════\n\
             FRONTEND DESIGN SKILLS LOADED\n\
             ═══════════════════════════════════════════════════════════════════════════════\n\n\
             You are building a website/frontend interface. The following design guidelines \n\
             MUST be followed to create distinctive, production-grade interfaces:\n\n\
             {}\n\n\
             ═══════════════════════════════════════════════════════════════════════════════\n\
             Remember:\n\
             - NO generic fonts (Inter, Roboto, Arial) - choose distinctive typography\n\
             - NO emojis as icons - use SVG icons (Heroicons, Lucide, etc.)\n\
             - NO purple gradients or cookie-cutter SaaS layouts\n\
             - Commit to a BOLD aesthetic direction and execute it fully\n\
             - Write complete, production-ready code in single file operations\n\
             ═══════════════════════════════════════════════════════════════════════════════",
            base_prompt, skills
        )
    } else {
        base_prompt
    }
}

/// Split text into roughly equal-sized chunks for UI streaming effect.
fn chunk_text(text: &str, chunk_size: usize) -> Vec<String> {
    text.chars()
        .collect::<Vec<char>>()
        .chunks(chunk_size)
        .map(|c| c.iter().collect())
        .collect()
}

fn compact_tool_result(tool_name: &str, result: String, task_ok: bool) -> String {
    let max_len = match tool_name {
        tools::READ_FILE | tools::READ_FILE_LINES => 2_500,
        tools::RUN_COMMAND => 3_000,
        _ => 1_200,
    };

    if result.len() <= max_len {
        return result;
    }

    if task_ok && tool_name != tools::READ_FILE && tool_name != tools::READ_FILE_LINES && tool_name != tools::RUN_COMMAND {
        return format!("ok: {tool_name} completed (result truncated, {} chars)", result.len());
    }

    let head = max_len / 2;
    let tail = max_len.saturating_sub(head);
    let start = &result[..head.min(result.len())];
    let end = &result[result.len().saturating_sub(tail)..];
    format!("{start}\n...[truncated {} chars]...\n{end}", result.len().saturating_sub(max_len))
}

fn message_size_estimate(msg: &LlmMessage) -> usize {
    msg.parts.iter().map(|p| match p {
        MessageContent::Text(t) => t.len(),
        MessageContent::ToolResult { content, .. } => content.len(),
        MessageContent::ToolCall(_) => 160,
    }).sum()
}

fn shrink_context_for_budget(messages: &mut Vec<LlmMessage>) {
    const TARGET_CHARS: usize = 36_000;
    if messages.is_empty() {
        return;
    }

    let total: usize = messages.iter().map(message_size_estimate).sum();
    if total <= TARGET_CHARS {
        return;
    }

    let first = messages[0].clone();
    let mut keep: Vec<LlmMessage> = Vec::new();
    let mut kept_total = message_size_estimate(&first);

    for msg in messages.iter().rev() {
        let size = message_size_estimate(msg);
        if kept_total + size > TARGET_CHARS {
            continue;
        }
        keep.push(msg.clone());
        kept_total += size;
    }
    keep.reverse();

    if !matches!(keep.first().map(|m| &m.role), Some(Role::User)) {
        keep.insert(0, first);
    }

    log::info!("Compacted context to ~{} chars ({} messages)", kept_total, keep.len());
    *messages = keep;
}

fn is_retryable_llm_error(err: &str) -> bool {
    let e = err.to_lowercase();
    e.contains("rate limit")
        || e.contains("429")
        || e.contains("resource exhausted")
        || e.contains("too many requests")
        || e.contains("quota")
}

async fn call_llm_with_retry(
    config: &Config,
    system_prompt: &str,
    messages: &[LlmMessage],
    tool_defs: &[serde_json::Value],
    event_tx: &mpsc::Sender<BridgeEvent>,
) -> anyhow::Result<LlmResponse> {
    const MAX_ATTEMPTS: usize = 4;
    let mut backoff_secs = 2u64;

    for attempt in 1..=MAX_ATTEMPTS {
        match llm::call_llm(config, system_prompt, messages, tool_defs).await {
            Ok(r) => return Ok(r),
            Err(e) => {
                let text = e.to_string();
                if attempt == MAX_ATTEMPTS || !is_retryable_llm_error(&text) {
                    return Err(e);
                }

                let _ = event_tx.send(BridgeEvent::AgentChunk(
                    format!("Rate limit detected. Retrying in {backoff_secs}s (attempt {attempt}/{MAX_ATTEMPTS})...\n")
                )).await;
                sleep(Duration::from_secs(backoff_secs)).await;
                backoff_secs = (backoff_secs * 2).min(16);
            }
        }
    }

    Err(anyhow::anyhow!("LLM call failed after retries"))
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
