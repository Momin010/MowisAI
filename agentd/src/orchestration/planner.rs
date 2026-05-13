//! Layer 1: Fast Planner — Shell scan + single Gemini call
//!
//! Replaces old Context Gatherer (128 rounds) + Architect + Sandbox Owner
//! with ONE LLM call that produces both task graph AND sandbox topology

use super::provider_client::{generate_text, LlmConfig};
use super::verification::extract_json;
use agentd_protocol::{SandboxTopology, TaskGraph, TaskId};
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
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
    llm_config: &LlmConfig,
) -> Result<PlannerOutput> {
    log::info!("Starting fast planner:");
    log::info!("  Prompt: {}...", &prompt.chars().take(100).collect::<String>());
    log::info!("  Project root: {:?}", project_root);
    log::info!("  Provider: {}", llm_config.provider);

    if prompt.is_empty() {
        return Err(anyhow!("Planner: Prompt cannot be empty"));
    }
    std::fs::create_dir_all(project_root)
        .with_context(|| format!("Planner: Failed to create project root: {:?}", project_root))?;

    log::info!("  Scanning directory tree...");
    let dir_tree = scan_directory_tree(project_root)
        .context("Failed to scan directory tree")?;
    log::info!("  Directory scan complete: {} bytes", dir_tree.len());

    log::info!("  Calling LLM planner...");
    let llm_response = call_llm_planner(prompt, &dir_tree, llm_config)
        .await
        .context("LLM planner call failed")?;
    log::info!("  LLM call complete: {} bytes", llm_response.len());

    log::info!("  Parsing planner response...");
    let output = parse_planner_response(&llm_response)
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
    llm_config: &LlmConfig,
    dir_tree: &str,
) -> Result<PlannerOutput> {
    log::info!("Standard planner (constrained):");
    log::info!("  Prompt: {}...", &prompt.chars().take(100).collect::<String>());

    if prompt.is_empty() {
        return Err(anyhow!("Planner: Prompt cannot be empty"));
    }
    std::fs::create_dir_all(project_root)
        .with_context(|| format!("Planner: Failed to create project root: {:?}", project_root))?;

    let llm_response = call_llm_planner_standard(prompt, dir_tree, llm_config)
        .await
        .context("LLM standard planner call failed")?;

    let output = parse_planner_response(&llm_response)
        .context("Failed to parse standard planner response")?;

    log::info!(
        "  → Standard plan: {} tasks in {} sandbox(es)",
        output.task_graph.tasks.len(),
        output.sandbox_topology.sandboxes.len()
    );

    Ok(output)
}

/// Constrained LLM call for Standard mode: 1 sandbox, ≤ 3 agents, no cross-service.
async fn call_llm_planner_standard(
    prompt: &str,
    dir_tree: &str,
    llm_config: &LlmConfig,
) -> Result<String> {
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

    let text = generate_text(llm_config, system_prompt, &user_message, true, 0.1)
        .await
        .context("LLM standard planner call failed")?;

    if text.is_empty() {
        return Err(anyhow!("LLM returned empty response (standard planner)"));
    }

    Ok(text)
}

/// Expose directory-tree scan so the orchestrator can reuse it for the
/// complexity classifier without scanning twice.
pub fn scan_directory_tree_pub(root: &Path) -> Result<String> {
    scan_directory_tree(root)
}

/// Scan directory tree using shell command (fast, no LLM)
fn scan_directory_tree(root: &Path) -> Result<String> {
    // Create root path if it doesn't exist
    std::fs::create_dir_all(root)
        .with_context(|| format!("Failed to create project root directory: {:?}", root))?;

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

/// Call LLM planner with prompt + directory tree (full mode).
async fn call_llm_planner(
    prompt: &str,
    dir_tree: &str,
    llm_config: &LlmConfig,
) -> Result<String> {
    if prompt.is_empty() {
        return Err(anyhow!("Empty prompt provided to planner"));
    }
    if dir_tree.is_empty() {
        return Err(anyhow!("Empty directory tree from scan"));
    }

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

    let text = generate_text(llm_config, system_prompt, &user_message, true, 0.1)
        .await
        .context("LLM planner call failed")?;

    if text.is_empty() {
        return Err(anyhow!("LLM returned empty response (planner)"));
    }

    Ok(text)
}

/// Try to complete truncated JSON by adding missing closing brackets
fn complete_truncated_json(json_str: &str) -> String {
    let mut result = json_str.to_string();
    
    // Count unmatched braces and brackets
    let mut brace_count = 0;
    let mut bracket_count = 0;
    let mut in_string = false;
    let mut escape_next = false;
    
    for ch in result.chars() {
        if escape_next {
            escape_next = false;
            continue;
        }
        match ch {
            '\\' if in_string => escape_next = true,
            '"' => in_string = !in_string,
            '{' if !in_string => brace_count += 1,
            '}' if !in_string => brace_count -= 1,
            '[' if !in_string => bracket_count += 1,
            ']' if !in_string => bracket_count -= 1,
            _ => {}
        }
    }
    
    // If we have unmatched braces/brackets, try to complete them
    if brace_count > 0 || bracket_count > 0 {
        // Remove trailing incomplete string if present
        if let Some(last_brace) = result.rfind('}') {
            // Check if there's an incomplete string after the last brace
            let after_brace = &result[last_brace + 1..];
            if after_brace.contains('"') && !after_brace.contains('}') {
                // There's an incomplete string, truncate at the last brace
                result = result[..=last_brace].to_string();
                // Recalculate counts
                brace_count = 0;
                bracket_count = 0;
                in_string = false;
                escape_next = false;
                for ch in result.chars() {
                    if escape_next {
                        escape_next = false;
                        continue;
                    }
                    match ch {
                        '\\' if in_string => escape_next = true,
                        '"' => in_string = !in_string,
                        '{' if !in_string => brace_count += 1,
                        '}' if !in_string => brace_count -= 1,
                        '[' if !in_string => bracket_count += 1,
                        ']' if !in_string => bracket_count -= 1,
                        _ => {}
                    }
                }
            }
        }
        
        // Add missing closing brackets and braces
        let mut completion = String::new();
        for _ in 0..bracket_count {
            completion.push(']');
        }
        for _ in 0..brace_count {
            completion.push('}');
        }
        
        if !completion.is_empty() {
            result.push_str(&completion);
            log::info!("Planner: Completed truncated JSON with: {}", completion);
        }
    }
    
    result
}

/// Fix common JSON issues from LLM responses
fn fix_common_json_issues(json_str: &str) -> String {
    // First try to complete truncated JSON
    let mut result = complete_truncated_json(json_str);
    
    // Try to parse as Value first to see if it's already valid
    if serde_json::from_str::<Value>(&result).is_ok() {
        return result;
    }
    
    // Fix 1: If a field that should be an array is a map, convert it
    // This is a heuristic - we look for patterns like "tasks": {} and convert to "tasks": []
    // We'll do this by parsing as Value and fixing structure
    if let Ok(mut value) = serde_json::from_str::<Value>(&result) {
        // Fix task_graph.tasks if it's a map
        if let Some(task_graph) = value.get_mut("task_graph") {
            if let Some(tasks) = task_graph.get_mut("tasks") {
                if tasks.is_object() {
                    // Convert object to array of its values
                    if let Some(obj) = tasks.as_object() {
                        let arr: Vec<Value> = obj.values().cloned().collect();
                        *tasks = Value::Array(arr);
                    }
                }
            }
        }
        
        // Fix sandbox_topology.sandboxes if it's a map
        if let Some(topology) = value.get_mut("sandbox_topology") {
            if let Some(sandboxes) = topology.get_mut("sandboxes") {
                if sandboxes.is_object() {
                    if let Some(obj) = sandboxes.as_object() {
                        let arr: Vec<Value> = obj.values().cloned().collect();
                        *sandboxes = Value::Array(arr);
                    }
                }
            }
        }
        
        // Fix task deps if it's a string instead of array
        if let Some(task_graph) = value.get_mut("task_graph") {
            if let Some(tasks) = task_graph.get_mut("tasks") {
                if let Some(tasks_arr) = tasks.as_array_mut() {
                    for task in tasks_arr.iter_mut() {
                        if let Some(deps) = task.get_mut("deps") {
                            if deps.is_string() {
                                // Convert string to array with single element
                                if let Some(s) = deps.as_str() {
                                    *deps = Value::Array(vec![Value::String(s.to_string())]);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // Fix sandbox tools if it's a string instead of array
        if let Some(topology) = value.get_mut("sandbox_topology") {
            if let Some(sandboxes) = topology.get_mut("sandboxes") {
                if let Some(sandboxes_arr) = sandboxes.as_array_mut() {
                    for sandbox in sandboxes_arr.iter_mut() {
                        if let Some(tools) = sandbox.get_mut("tools") {
                            if tools.is_string() {
                                if let Some(s) = tools.as_str() {
                                    *tools = Value::Array(vec![Value::String(s.to_string())]);
                                }
                            }
                        }
                    }
                }
            }
        }
        
        // Serialize back to string
        if let Ok(fixed) = serde_json::to_string(&value) {
            if fixed != json_str {
                log::info!("Planner: Applied JSON fixes for common LLM issues");
            }
            result = fixed;
        }
    }
    
    result
}

/// Parse planner response into structured output
fn parse_planner_response(response: &str) -> Result<PlannerOutput> {
    if response.is_empty() {
        return Err(anyhow!("Empty response from planner"));
    }

    // Extract JSON from response using robust extraction
    let json_str = extract_json(response);
    
    if json_str.is_empty() {
        return Err(anyhow!("No JSON content found in planner response"));
    }

    // Fix common JSON issues from LLM
    let fixed_json = fix_common_json_issues(&json_str);
    
    #[derive(Deserialize)]
    struct PlannerJson {
        task_graph: TaskGraph,
        sandbox_topology: SandboxTopology,
    }

    let parsed: PlannerJson =
        serde_json::from_str(&fixed_json).context(format!("Failed to parse planner JSON from: {}", fixed_json.chars().take(500).collect::<String>()))?;

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

    #[test]
    fn test_fix_common_json_issues_tasks_map() {
        // Simulate LLM returning tasks as map instead of array
        let json = r#"{"task_graph":{"tasks":{"t1":{"id":"t1","description":"task 1","deps":[],"hint":"backend"}}},"sandbox_topology":{"sandboxes":[{"name":"backend","scope":"src/","tools":["read_file"],"max_agents":50}]}}"#;
        let fixed = fix_common_json_issues(json);
        let parsed: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        // tasks should now be an array
        assert!(parsed["task_graph"]["tasks"].is_array());
        assert_eq!(parsed["task_graph"]["tasks"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_fix_common_json_issues_sandboxes_map() {
        // Simulate LLM returning sandboxes as map instead of array
        let json = r#"{"task_graph":{"tasks":[{"id":"t1","description":"task 1","deps":[],"hint":"backend"}]},"sandbox_topology":{"sandboxes":{"backend":{"name":"backend","scope":"src/","tools":["read_file"],"max_agents":50}}}}"#;
        let fixed = fix_common_json_issues(json);
        let parsed: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        // sandboxes should now be an array
        assert!(parsed["sandbox_topology"]["sandboxes"].is_array());
        assert_eq!(parsed["sandbox_topology"]["sandboxes"].as_array().unwrap().len(), 1);
    }

    #[test]
    fn test_fix_common_json_issues_deps_string() {
        // Simulate LLM returning deps as string instead of array
        let json = r#"{"task_graph":{"tasks":[{"id":"t1","description":"task 1","deps":"t2","hint":"backend"}]},"sandbox_topology":{"sandboxes":[{"name":"backend","scope":"src/","tools":["read_file"],"max_agents":50}]}}"#;
        let fixed = fix_common_json_issues(json);
        let parsed: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        // deps should now be an array
        assert!(parsed["task_graph"]["tasks"][0]["deps"].is_array());
        assert_eq!(parsed["task_graph"]["tasks"][0]["deps"].as_array().unwrap().len(), 1);
        assert_eq!(parsed["task_graph"]["tasks"][0]["deps"][0].as_str().unwrap(), "t2");
    }

    #[test]
    fn test_parse_planner_response_malformed_json() {
        // Simulate the error the user encountered - JSON with map instead of array
        let response = r#"{"task_graph":{"tasks":{"t1":{"id":"t1","description":"Create index.html","deps":[],"hint":"main"}}},"sandbox_topology":{"sandboxes":{"main":{"name":"main","scope":".","tools":["read_file","write_file"],"max_agents":3}}}}"#;
        let result = parse_planner_response(response);
        assert!(result.is_ok());
        let output = result.unwrap();
        assert_eq!(output.task_graph.tasks.len(), 1);
        assert_eq!(output.sandbox_topology.sandboxes.len(), 1);
    }

    #[test]
    fn test_complete_truncated_json() {
        // Simulate truncated JSON (missing closing brackets)
        let truncated = r#"{"task_graph":{"tasks":[{"id":"t1","description":"task 1"#;
        let completed = complete_truncated_json(truncated);
        // Should be valid JSON after completion
        let parsed: serde_json::Value = serde_json::from_str(&completed).unwrap();
        assert!(parsed.is_object());
    }

    #[test]
    fn test_fix_common_json_issues_truncated() {
        // Simulate the exact error from user: truncated JSON
        let json = r#"{"task_graph":{"tasks":[{"id":"t1","description":"Create index.html with full flower shop website including embedded CSS and JavaScript, Google Fonts, all sections (nav, hero, featured flowers, about,"#;
        let fixed = fix_common_json_issues(json);
        // Should be parseable after fixes
        let parsed: serde_json::Value = serde_json::from_str(&fixed).unwrap();
        assert!(parsed.is_object());
    }

    #[cfg(not(target_os = "linux"))]
    #[test]
    fn test_scan_directory_tree_windows() {
        let temp_dir = std::env::temp_dir();
        let result = scan_directory_tree(&temp_dir);
        assert!(result.is_ok());
    }
}
