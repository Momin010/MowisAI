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

    // Tell the frontend we're in zero mode.
    let _ = event_tx.send(BridgeEvent::PlanReady {
        sandboxes: vec!["zero".into()],
        task_count: 0,
        agent_count: 1,
        mode: "zero".into(),
    }).await;

    // Announce workspace in the chat.
    let intro = format!(
        "**Zero-Protection mode** — writing directly to disk.\n\
         Workspace: `{}`\n\n\
         Connecting to {} ({})…\n",
        workspace.path,
        config.provider,
        if config.model.is_empty() { "default model" } else { config.model.as_str() }
    );
    for chunk in chunk_text(&intro, 40) {
        let _ = event_tx.send(BridgeEvent::AgentChunk(chunk)).await;
        sleep(Duration::from_millis(18)).await;
    }

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
        let response = match llm::call_llm(&config, &system_prompt, &messages, &tool_defs).await {
            Ok(r) => r,
            Err(e) => {
                let _ = event_tx.send(BridgeEvent::OrchestrationFailed(
                    format!("LLM error: {e}")
                )).await;
                return;
            }
        };

        // Stream any text the model produced.
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

            // Show tool result as a small chunk in the chat.
            let summary = if result.len() > 120 { format!("{}…", &result[..120]) } else { result.clone() };
            let echo = format!("\n`{}` → {}\n", tc.name, summary);
            let _ = event_tx.send(BridgeEvent::AgentChunk(echo)).await;

            tool_results.push((tc.id.clone(), result));
        }

        // Feed all tool results back as a single Tool message.
        messages.push(LlmMessage::tool_results(tool_results));

        // Brief yield so the UI can render.
        sleep(Duration::from_millis(40)).await;
    }

    // Closing summary.
    let closing = format!(
        "\n\n**Done.** {} tool call(s) executed.\n\
         Files are saved at: `{}`\n",
        total_tool_calls,
        workspace.path
    );
    for chunk in chunk_text(&closing, 60) {
        let _ = event_tx.send(BridgeEvent::AgentChunk(chunk)).await;
        sleep(Duration::from_millis(10)).await;
    }

    let _ = event_tx.send(BridgeEvent::OrchestrationComplete).await;
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn system_prompt_for(ws: &WorkspaceInfo) -> String {
    format!(
        "You are an AI agent running in Zero-Protection mode on the user's computer.\n\
         Your workspace directory is: {path}\n\n\
         You have nine tools:\n\
         - read_file, write_file, append_file — file I/O\n\
         - list_directory, create_directory, delete_file, move_file — filesystem ops\n\
         - search_files — find files by name\n\
         - run_command — execute a shell command in the workspace (use sparingly)\n\n\
         All paths you supply must be workspace-relative (e.g. 'src/main.py', not '/home/…').\n\
         Do not reference files outside the workspace.\n\
         Work systematically: plan → create directories → write files → verify.\n\
         When you are finished, summarise what was created.",
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
        tools::WRITE_FILE       => format!("Write {}", tc.args["path"].as_str().unwrap_or("?")),
        tools::READ_FILE        => format!("Read {}", tc.args["path"].as_str().unwrap_or("?")),
        tools::APPEND_FILE      => format!("Append {}", tc.args["path"].as_str().unwrap_or("?")),
        tools::LIST_DIRECTORY   => format!("List {}", tc.args["path"].as_str().unwrap_or("workspace")),
        tools::CREATE_DIRECTORY => format!("mkdir {}", tc.args["path"].as_str().unwrap_or("?")),
        tools::DELETE_FILE      => format!("Delete {}", tc.args["path"].as_str().unwrap_or("?")),
        tools::MOVE_FILE        => format!("Move {} → {}", tc.args["from"].as_str().unwrap_or("?"), tc.args["to"].as_str().unwrap_or("?")),
        tools::SEARCH_FILES     => format!("Search '{}'", tc.args["pattern"].as_str().unwrap_or("?")),
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
        tools::WRITE_FILE | tools::READ_FILE | tools::APPEND_FILE | tools::DELETE_FILE => {
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
