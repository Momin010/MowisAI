//! Layer 1: Fast Planner — Shell scan + single Gemini call
//!
//! Replaces old Context Gatherer (128 rounds) + Architect + Sandbox Owner
//! with ONE LLM call that produces both task graph AND sandbox topology

use agentd_protocol::{SandboxConfig, SandboxTopology, Task, TaskGraph, TaskId};
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
    // Step 1: Shell scan to get directory tree
    let dir_tree = scan_directory_tree(project_root)?;

    // Step 2: Single Gemini call with prompt + dir tree
    let gemini_response = call_gemini_planner(prompt, &dir_tree, project_id).await?;

    // Step 3: Parse response into task graph + topology
    parse_planner_response(&gemini_response)
}

/// Scan directory tree using shell command (fast, no LLM)
fn scan_directory_tree(root: &Path) -> Result<String> {
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
                return Ok(String::from_utf8_lossy(&output.stdout).to_string());
            }
        }

        // Fallback to find command
        let find_result = Command::new("find")
            .arg(root)
            .arg("-maxdepth")
            .arg("3")
            .arg("-type")
            .arg("d")
            .arg("-o")
            .arg("-type")
            .arg("f")
            .output()
            .context("Failed to run find command")?;

        if find_result.status.success() {
            Ok(String::from_utf8_lossy(&find_result.stdout).to_string())
        } else {
            Err(anyhow!("Directory scan failed"))
        }
    }

    #[cfg(not(target_os = "linux"))]
    {
        // Windows fallback - manual directory walk
        let mut result = String::new();
        walk_dir_recursive(root, 0, 3, &mut result)?;
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

    if !response.status().is_success() {
        let error_text = response.text().await.unwrap_or_default();
        return Err(anyhow!("Gemini API error: {}", error_text));
    }

    let response_json: serde_json::Value = response
        .json()
        .await
        .context("Failed to parse Gemini response")?;

    // Extract text from response
    let text = response_json
        .get("candidates")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("content"))
        .and_then(|c| c.get("parts"))
        .and_then(|p| p.get(0))
        .and_then(|p| p.get("text"))
        .and_then(|t| t.as_str())
        .ok_or_else(|| anyhow!("Invalid Gemini response structure"))?;

    Ok(text.to_string())
}

/// Parse planner response into structured output
fn parse_planner_response(response: &str) -> Result<PlannerOutput> {
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

    #[derive(Deserialize)]
    struct PlannerJson {
        task_graph: TaskGraph,
        sandbox_topology: SandboxTopology,
    }

    let parsed: PlannerJson =
        serde_json::from_str(json_str).context("Failed to parse planner JSON")?;

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
