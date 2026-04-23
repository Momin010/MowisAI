//! Layer 1: Fast Planner — Shell scan + single Gemini call
//!
//! Replaces old Context Gatherer (128 rounds) + Architect + Sandbox Owner
//! with ONE LLM call that produces both task graph AND sandbox topology

use agentd_protocol::{SandboxTopology, TaskGraph, TaskId};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::process::Command;

/// Fast planner output
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannerOutput {
    pub task_graph: TaskGraph,
    pub sandbox_topology: SandboxTopology,
    pub sandbox_hints: HashMap<TaskId, String>, // task_id -> sandbox_name
}

/// Plan a task using fast planner (shell scan + 1 LLM call)
pub async fn plan_task(
    prompt: &str,
    project_root: &Path,
    project_id: &str,
) -> Result<PlannerOutput> {
    log::info!("Starting fast planner:");
    log::info!("  Prompt: {}...", &prompt.chars().take(100).collect::<String>());
    log::info!("  Project root: {:?}", project_root);
    log::info!("  Project ID: {}", project_id);

    // Validate inputs
    if prompt.is_empty() {
        return Err(anyhow!("Planner: Prompt cannot be empty"));
    }
    if !project_root.exists() {
        return Err(anyhow!("Planner: Project root does not exist: {:?}", project_root));
    }
    if project_id.is_empty() {
        return Err(anyhow!("Planner: Project ID cannot be empty"));
    }

    // Step 1: Shell scan to get directory tree
    log::info!("  Scanning directory tree...");
    let dir_tree = scan_directory_tree(project_root)
        .context("Failed to scan directory tree")?;
    log::info!("  Directory scan complete: {} bytes", dir_tree.len());

    // Step 2: Single Gemini call with prompt + dir tree
    log::info!("  Calling Gemini planner...");
    let gemini_response = call_gemini_planner(prompt, &dir_tree, project_id)
        .await
        .context("Gemini planner call failed")?;
    log::info!("  Gemini call complete: {} bytes", gemini_response.len());

    // Step 3: Parse response into task graph + topology
    log::info!("  Parsing planner response...");
    let output = parse_planner_response(&gemini_response)
        .context("Failed to parse planner response")?;
    log::info!("  Planning complete!");

    Ok(output)
}

/// Plan a task using the *constrained* Standard-mode planner.
///
/// Uses a tighter system prompt that forces: 1 sandbox, ≤ 3 parallel agents,
/// and discourages cross-service work — appropriate for Mode 2 tasks.
pub async fn plan_task_standard(
    prompt: &str,
    project_root: &Path,
    project_id: &str,
    dir_tree: &str,
) -> Result<PlannerOutput> {
    log::info!("Standard planner (constrained):");
    log::info!("  Prompt: {}...", &prompt.chars().take(100).collect::<String>());

    if prompt.is_empty() {
        return Err(anyhow!("Planner: Prompt cannot be empty"));
    }
    if !project_root.exists() {
        return Err(anyhow!("Planner: Project root does not exist: {:?}", project_root));
    }
    if project_id.is_empty() {
        return Err(anyhow!("Planner: Project ID cannot be empty"));
    }

    let gemini_response = call_gemini_planner_standard(prompt, dir_tree, project_id)
        .await
        .context("Gemini standard planner call failed")?;

    let output = parse_planner_response(&gemini_response)
        .context("Failed to parse standard planner response")?;

    log::info!(
        "  → Standard plan: {} tasks in {} sandbox(es)",
        output.task_graph.tasks.len(),
        output.sandbox_topology.sandboxes.len()
    );

    Ok(output)
}

/// Constrained Gemini call for Standard mode: 1 sandbox, ≤ 3 agents, no cross-service.
async fn call_gemini_planner_standard(
    prompt: &str,
    dir_tree: &str,
    project_id: &str,
) -> Result<String> {
    let access_token = super::gcloud_access_token()?;
    let url = super::vertex_generate_url(project_id);

    let system_prompt = r#"You are a fast task planner for an AI agent orchestration system operating in STANDARD mode.

STANDARD MODE CONSTRAINTS (strictly enforced):
- Output EXACTLY 1 sandbox
- Output NO MORE than 3 tasks total
- All tasks belong to that 1 sandbox (same hint)
- Tasks may be parallel or sequential — use deps[] to express ordering
- Do NOT create cross-service or cross-domain tasks
- Keep scope tight — implement what is asked, nothing more

Your job: output a JSON object with a task graph and a sandbox topology.

Task graph format:
{
  "tasks": [
    {"id": "t1", "description": "implement feature X", "deps": [], "hint": "main"},
    {"id": "t2", "description": "write tests for X", "deps": ["t1"], "hint": "main"}
  ]
}

Sandbox topology format:
{
  "sandboxes": [
    {
      "name": "main",
      "scope": ".",
      "tools": ["read_file", "write_file", "run_command", "git_commit"],
      "max_agents": 3
    }
  ]
}

Output ONLY valid JSON in this exact format:
{
  "task_graph": { "tasks": [...] },
  "sandbox_topology": { "sandboxes": [...] }
}
"#;

    let user_message = format!(
        "User prompt: {}\n\nDirectory tree:\n{}",
        prompt, dir_tree
    );

    let request_body = serde_json::json!({
        "contents": [
            {
                "role": "user",
                "parts": [{"text": user_message}]
            }
        ],
        "systemInstruction": {
            "parts": [{"text": system_prompt}]
        },
        "generationConfig": super::vertex_generation_config_json(0.1)
    });

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS))
        .send()
        .await
        .context("Failed to send request to Gemini (standard planner)")?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "unknown error".to_string());
        return Err(anyhow!("Gemini API error ({}): {}", status, error_text));
    }

    let response_json: serde_json::Value = response
        .json()
        .await
        .context("Failed to parse Gemini response as JSON")?;

    let text = response_json
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("Invalid Gemini response structure"))?;

    if text.is_empty() {
        return Err(anyhow!("Gemini returned empty response (standard planner)"));
    }

    Ok(text.to_string())
}

/// Expose directory-tree scan so the orchestrator can reuse it for the
/// complexity classifier without scanning twice.
pub fn scan_directory_tree_pub(root: &Path) -> Result<String> {
    scan_directory_tree(root)
}

/// Scan directory tree using shell command (fast, no LLM)
fn scan_directory_tree(root: &Path) -> Result<String> {
    // Validate root path exists
    if !root.exists() {
        return Err(anyhow!("Project root does not exist: {:?}", root));
    }

    #[cfg(target_os = "linux")]
    {
        // Try tree command first (better output)
        let tree_result = Command::new("tree")
            .arg("-L")
            .arg("3") // 3 levels deep
            .arg("-I")
            .arg("node_modules|target|.git|dist|build") // Ignore common dirs
            .arg(root)
            .output();

        if let Ok(output) = tree_result {
            if output.status.success() {
                let output_str = String::from_utf8_lossy(&output.stdout).to_string();
                if !output_str.is_empty() {
                    return Ok(output_str);
                }
            }
        }

        // Fallback to find command with explicit parentheses for better clarity
        let find_result = Command::new("find")
            .arg(root)
            .arg("-maxdepth")
            .arg("3")
            .arg("(")
            .arg("-type")
            .arg("d")
            .arg("-o")
            .arg("-type")
            .arg("f")
            .arg(")")
            .output()
            .context("Failed to run find command")?;

        if !find_result.status.success() {
            let stderr = String::from_utf8_lossy(&find_result.stderr);
            return Err(anyhow!("find command failed: {}", stderr));
        }

        let output_str = String::from_utf8(find_result.stdout)
            .context("Failed to convert find output to UTF-8")?;

        if output_str.is_empty() {
            return Err(anyhow!("Directory scan returned empty result"));
        }

        Ok(output_str)
    }

    #[cfg(not(target_os = "linux"))]
    {
        // Windows/non-linux fallback - manual directory walk
        let mut result = String::new();
        walk_dir_recursive(root, 0, 3, &mut result)?;
        if result.is_empty() {
            return Err(anyhow!("Directory scan returned empty result"));
        }
        Ok(result)
    }
}

#[cfg(not(target_os = "linux"))]
fn walk_dir_recursive(path: &Path, depth: usize, max_depth: usize, output: &mut String) -> Result<()> {
    if depth > max_depth {
        return Ok(());
    }

    let indent = "  ".repeat(depth);

    if let Ok(entries) = std::fs::read_dir(path) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().unwrap_or_default().to_string_lossy();

            // Skip common ignore patterns
            if name.starts_with('.') || name == "node_modules" || name == "target" {
                continue;
            }

            if path.is_dir() {
                output.push_str(&format!("{}{}/\n", indent, name));
                walk_dir_recursive(&path, depth + 1, max_depth, output)?;
            } else {
                output.push_str(&format!("{}{}\n", indent, name));
            }
        }
    }

    Ok(())
}

/// Call Gemini planner with prompt + directory tree
async fn call_gemini_planner(
    prompt: &str,
    dir_tree: &str,
    project_id: &str,
) -> Result<String> {
    if prompt.is_empty() {
        return Err(anyhow!("Empty prompt provided to planner"));
    }
    if project_id.is_empty() {
        return Err(anyhow!("Empty project_id provided to planner"));
    }
    if dir_tree.is_empty() {
        return Err(anyhow!("Empty directory tree from scan"));
    }

    let access_token = super::gcloud_access_token()?;
    let url = super::vertex_generate_url(project_id);

    let system_prompt = r#"You are a fast task planner for an AI agent orchestration system.

Your job: analyze the user prompt and directory tree, then output a JSON object with:
1. A task graph: list of tasks with dependencies
2. A sandbox topology: which sandboxes to create and their configuration

Task graph format:
{
  "tasks": [
    {"id": "t1", "description": "implement auth module", "deps": [], "hint": "backend"},
    {"id": "t2", "description": "add API routes", "deps": ["t1"], "hint": "backend"},
    {"id": "t3", "description": "build login UI", "deps": [], "hint": "frontend"},
    {"id": "t4", "description": "integration test", "deps": ["t2", "t3"], "hint": "testing"}
  ]
}

Sandbox topology format:
{
  "sandboxes": [
    {
      "name": "backend",
      "scope": "src/backend/",
      "tools": ["read_file", "write_file", "run_command", "git_commit"],
      "max_agents": 100
    },
    {
      "name": "frontend",
      "scope": "src/frontend/",
      "tools": ["read_file", "write_file", "npm_install", "run_command"],
      "max_agents": 100
    }
  ]
}

Rules:
- Small project (< 5 files) → 1 sandbox, up to 50 agents
- Large project → multiple sandboxes by domain (frontend/backend/infra/testing)
- Each task gets an "id", "description", "deps" array, and "hint" (sandbox name)
- Break work into parallel tasks when possible
- Keep tasks focused and atomic

Output ONLY valid JSON in this exact format:
{
  "task_graph": { "tasks": [...] },
  "sandbox_topology": { "sandboxes": [...] }
}
"#;

    let user_message = format!(
        "User prompt: {}\n\nDirectory tree:\n{}",
        prompt, dir_tree
    );

    let request_body = serde_json::json!({
        "contents": [
            {
                "role": "user",
                "parts": [{"text": user_message}]
            }
        ],
        "systemInstruction": {
            "parts": [{"text": system_prompt}]
        },
        "generationConfig": super::vertex_generation_config_json(0.1)
    });

    let client = reqwest::Client::new();
    let response = client
        .post(&url)
        .header("Authorization", format!("Bearer {}", access_token))
        .header("Content-Type", "application/json")
        .json(&request_body)
        .timeout(std::time::Duration::from_secs(super::HTTP_TIMEOUT_SECS))
        .send()
        .await
        .context("Failed to send request to Gemini")?;

    let status = response.status();
    if !status.is_success() {
        let error_text = response.text().await.unwrap_or_else(|_| "unknown error".to_string());
        return Err(anyhow!("Gemini API error ({}): {}", status, error_text));
    }

    let response_json: serde_json::Value = response
        .json()
        .await
        .context("Failed to parse Gemini response as JSON")?;

    // Extract text from response
    let text = response_json
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("Invalid Gemini response structure: missing candidates/content/parts/text"))?;

    if text.is_empty() {
        return Err(anyhow!("Gemini returned empty response"));
    }

    Ok(text.to_string())
}

/// Parse planner response into structured output
fn parse_planner_response(response: &str) -> Result<PlannerOutput> {
    if response.is_empty() {
        return Err(anyhow!("Empty response from planner"));
    }

    // Extract JSON from response (may have markdown code blocks)
    let json_str = if response.contains("```json") {
        response
            .split("```json")
            .nth(1)
            .and_then(|s| s.split("```").next())
            .unwrap_or(response)
    } else if response.contains("```") {
        response
            .split("```")
            .nth(1)
            .and_then(|s| s.split("```").next())
            .unwrap_or(response)
    } else {
        response
    }
    .trim();

    if json_str.is_empty() {
        return Err(anyhow!("No JSON content found in planner response"));
    }

    #[derive(Deserialize)]
    struct PlannerJson {
        task_graph: TaskGraph,
        sandbox_topology: SandboxTopology,
    }

    let parsed: PlannerJson =
        serde_json::from_str(json_str).context(format!("Failed to parse planner JSON from: {}", json_str.chars().take(200).collect::<String>()))?;

    // Validate we have at least one task and one sandbox
    if parsed.task_graph.tasks.is_empty() {
        return Err(anyhow!("Planner returned empty task graph"));
    }
    if parsed.sandbox_topology.sandboxes.is_empty() {
        return Err(anyhow!("Planner returned empty sandbox topology"));
    }

    // Build sandbox hints map (task_id -> sandbox_name)
    let mut sandbox_hints = HashMap::new();
    for task in &parsed.task_graph.tasks {
        if let Some(hint) = &task.hint {
            sandbox_hints.insert(task.id.clone(), hint.clone());
        } else {
            // If no hint, assign to first sandbox
            if let Some(first_sandbox) = parsed.sandbox_topology.sandboxes.first() {
                sandbox_hints.insert(task.id.clone(), first_sandbox.name.clone());
            }
        }
    }

    log::info!(
        "Planner generated {} tasks across {} sandboxes",
        parsed.task_graph.tasks.len(),
        parsed.sandbox_topology.sandboxes.len()
    );

    Ok(PlannerOutput {
        task_graph: parsed.task_graph,
        sandbox_topology: parsed.sandbox_topology,
        sandbox_hints,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_planner_response() {
        let response = r#"
```json
{
  "task_graph": {
    "tasks": [
      {"id": "t1", "description": "task 1", "deps": [], "hint": "backend"}
    ]
  },
  "sandbox_topology": {
    "sandboxes": [
      {"name": "backend", "scope": "src/", "tools": ["read_file"], "max_agents": 50}
    ]
  }
}
```
        "#;

        let result = parse_planner_response(response);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.task_graph.tasks.len(), 1);
        assert_eq!(output.sandbox_topology.sandboxes.len(), 1);
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_scan_directory_tree_windows() {
        let temp_dir = std::env::temp_dir();
        let result = scan_directory_tree(&temp_dir);
        assert!(result.is_ok());
    }
}
