use crate::memory::{AgentMemory, DecisionLog, ExecutionResult, TaskFrame, TaskState};
use crate::tools::{Tool, ToolContext};
use dashmap::DashMap;
use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// The main agent execution loop - handles planning, tool selection, execution, and reflection
pub struct AgentLoop {
    pub agent_id: u64,
    pub memory: AgentMemory,
    pub max_iterations: usize,
    pub current_iteration: usize,
}

/// Agent state during execution
pub enum AgentState {
    Idle,
    Planning,
    Executing,
    Reflecting,
    Done,
}

/// Tool selection strategy
pub enum ToolSelectionStrategy {
    GreedyBest,      // pick highest scoring tool
    Exploration,     // sample tools with exploration
    PatternMatching, // use learned patterns
    RandomSelection, // random choice (for diversity)
}

/// Result of a single agent loop iteration
pub struct LoopIteration {
    pub iteration: usize,
    pub tool_used: String,
    pub input: Value,
    pub output: Value,
    pub success: bool,
    pub reasoning: String,
}

impl AgentLoop {
    pub fn new(agent_id: u64, session_id: u64, max_iterations: usize) -> Self {
        AgentLoop {
            agent_id,
            memory: AgentMemory::new(agent_id, session_id),
            max_iterations,
            current_iteration: 0,
        }
    }

    /// Main loop: execute until done or max iterations
    pub fn run(
        &mut self,
        prompt: &str,
        available_tools: &[Box<dyn Tool>],
    ) -> anyhow::Result<String> {
        // Initialize task from prompt
        self.memory
            .short_term
            .set_context("user_prompt".to_string(), Value::String(prompt.to_string()));

        let task = TaskFrame {
            task_id: format!("task_{}", self.agent_id),
            goal: prompt.to_string(),
            state: TaskState::Running,
            tools_used: Vec::new(),
            subtasks: Vec::new(),
        };

        self.memory.short_term.push_task(task);

        // Main agent loop
        let mut final_result = String::from("No result");
        loop {
            self.current_iteration += 1;
            if self.current_iteration > self.max_iterations {
                break;
            }

            // Planning phase: decide next action
            let next_action = self.plan(available_tools)?;
            if next_action.is_none() {
                break;
            }

            let (tool_name, tool_input) = next_action.unwrap();

            // Execution phase: run selected tool
            let result = self.execute_tool(&tool_name, tool_input.clone(), available_tools)?;

            // Store execution result in memory
            if let Some(_task) = self.memory.short_term.current_task() {
                let exec_result = ExecutionResult {
                    tool: tool_name.clone(),
                    input: tool_input.clone(),
                    output: result.clone(),
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    success: true,
                };
                self.memory.short_term.add_result(exec_result);
            }

            // Reflection phase: evaluate result and update memory
            self.reflect(&tool_name, &result)?;

            // Check if task is complete
            if let Some(task) = self.memory.short_term.pop_task() {
                if task.state == TaskState::Completed {
                    final_result = serde_json::to_string(&result.clone()).unwrap_or_default();
                    break;
                } else {
                    let mut updated_task = task;
                    updated_task.tools_used.push(tool_name);
                    self.memory.short_term.push_task(updated_task);
                }
            }
        }

        Ok(final_result)
    }

    /// Planning: analyze state and decide next tool
    fn plan(
        &mut self,
        available_tools: &[Box<dyn Tool>],
    ) -> anyhow::Result<Option<(String, Value)>> {
        // Get current context
        let prompt = self
            .memory
            .short_term
            .get_context("user_prompt")
            .cloned()
            .unwrap_or(Value::Null);

        // Simple heuristic: pick tool based on prompt keywords
        let tool_name = self.select_tool(available_tools, &prompt)?;

        if let Some(name) = tool_name {
            // Prepare input for the tool
            let input = json!({
                "prompt": prompt,
                "context": self.memory.short_term.context.clone(),
            });
            Ok(Some((name, input)))
        } else {
            Ok(None)
        }
    }

    /// Tool selection using pattern matching and heuristics
    fn select_tool(
        &self,
        available_tools: &[Box<dyn Tool>],
        context: &Value,
    ) -> anyhow::Result<Option<String>> {
        if available_tools.is_empty() {
            return Ok(None);
        }

        // Extract context string for matching
        let context_str = context.to_string().to_lowercase();

        // Keyword-based tool selection
        let selected = if context_str.contains("read") || context_str.contains("file") {
            "read_file"
        } else if context_str.contains("write") {
            "write_file"
        } else if context_str.contains("run") || context_str.contains("command") {
            "run_command"
        } else if context_str.contains("list") || context_str.contains("dir") {
            "list_files"
        } else if context_str.contains("http") || context_str.contains("fetch") {
            "http_get"
        } else if context_str.contains("json") {
            "json_parse"
        } else if context_str.contains("spawn") || context_str.contains("agent") {
            "spawn_subagent"
        } else {
            "echo" // default fallback
        };

        // Check if selected tool is available
        for tool in available_tools {
            if tool.name() == selected {
                return Ok(Some(selected.to_string()));
            }
        }

        // If not available, return first available tool
        Ok(available_tools.first().map(|t| t.name().to_string()))
    }

    /// Execute a tool and capture result
    fn execute_tool(
        &self,
        tool_name: &str,
        input: Value,
        available_tools: &[Box<dyn Tool>],
    ) -> anyhow::Result<Value> {
        for tool in available_tools {
            if tool.name() == tool_name {
                let ctx = ToolContext {
                    sandbox_id: self.agent_id,
                    root_path: None,
                };
                return tool.invoke(&ctx, input);
            }
        }

        Err(anyhow::anyhow!("Tool {} not found", tool_name))
    }

    /// Reflection: analyze result and update learning
    fn reflect(&mut self, tool_name: &str, result: &Value) -> anyhow::Result<()> {
        // Determine success
        let success = match result.get("success") {
            Some(v) => v.as_bool().unwrap_or(true),
            None => !result.is_null(),
        };

        // Record pattern in LTM
        if let Some(task) = self.memory.short_term.current_task() {
            let pattern = format!("{}_{}", task.goal.clone(), tool_name);
            self.memory
                .long_term
                .record_pattern(pattern, vec![tool_name.to_string()], success);
        }

        // Log decision
        let decision = DecisionLog {
            decision_id: format!("decision_{}", self.current_iteration),
            options: vec![tool_name.to_string()],
            chosen: tool_name.to_string(),
            reasoning: "keyword match".to_string(),
            outcome: if success { "success" } else { "failed" }.to_string(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };

        self.memory.long_term.log_decision(decision);

        Ok(())
    }

    /// Get agent status and memory snapshot
    pub fn status(&self) -> Value {
        json!({
            "agent_id": self.agent_id,
            "iteration": self.current_iteration,
            "max_iterations": self.max_iterations,
            "task_stack_depth": self.memory.short_term.task_stack.len(),
            "results_count": self.memory.short_term.recent_results.len(),
            "knowledge_base_size": self.memory.long_term.knowledge_base.len(),
            "patterns_learned": self.memory.long_term.pattern_index.len(),
            "decisions_logged": self.memory.long_term.decision_log.len(),
        })
    }

    /// Export memory to JSON for persistence
    pub fn export_memory(&self) -> anyhow::Result<Value> {
        self.memory.serialize_to_json()
    }

    /// Import memory from JSON
    pub fn import_memory(&mut self, json: &Value) -> anyhow::Result<()> {
        let imported = AgentMemory::deserialize_from_json(json)?;
        self.memory = imported;
        Ok(())
    }
}

/// Multi-agent coordinator for spawning and managing subagents
pub struct AgentCoordinator {
    pub agents: DashMap<u64, Mutex<AgentLoop>>,
    pub next_agent_id: AtomicU64,
}

impl AgentCoordinator {
    pub fn new() -> Self {
        AgentCoordinator {
            agents: DashMap::new(),
            next_agent_id: AtomicU64::new(1),
        }
    }

    pub fn spawn_agent(&self, max_iterations: usize) -> u64 {
        let agent_id = self.next_agent_id.fetch_add(1, Ordering::SeqCst);
        let session_id = agent_id;

        let agent = AgentLoop::new(agent_id, session_id, max_iterations);
        self.agents.insert(agent_id, Mutex::new(agent));
        agent_id
    }

    pub fn run_agent(&self, agent_id: u64, prompt: &str, available_tools: &[Box<dyn Tool>]) -> anyhow::Result<Option<String>> {
        match self.agents.get(&agent_id) {
            Some(agent) => {
                let mut locked = agent.lock().unwrap();
                let result = locked.run(prompt, available_tools)?;
                Ok(Some(result))
            }
            None => Ok(None),
        }
    }

    pub fn get_agent_status(&self, agent_id: u64) -> Option<Value> {
        self.agents.get(&agent_id).map(|agent| agent.lock().unwrap().status())
    }

    pub fn remove_agent(&self, agent_id: u64) -> Option<AgentLoop> {
        self.agents
            .remove(&agent_id)
            .map(|(_, agent)| agent.into_inner().unwrap())
    }

    pub fn get_all_statuses(&self) -> Value {
        let statuses: Vec<Value> = self
            .agents
            .iter()
            .map(|agent| agent.value().lock().unwrap().status())
            .collect();
        json!(statuses)
    }
}

/// Prompting strategies
pub struct PromptingStrategy;

impl PromptingStrategy {
    /// Chain-of-Thought: break down reasoning into steps
    pub fn chain_of_thought(prompt: &str, steps: &[&str]) -> String {
        let mut result = String::from(prompt);
        result.push_str("\n\nLet me break this down:\n");
        for (i, step) in steps.iter().enumerate() {
            result.push_str(&format!("{}. {}\n", i + 1, step));
        }
        result
    }

    /// Few-Shot: provide examples before main task
    pub fn few_shot(examples: &[(String, String)], task: &str) -> String {
        let mut result = String::from("Examples:\n");
        for (input, output) in examples {
            result.push_str(&format!("Input: {}\nOutput: {}\n\n", input, output));
        }
        result.push_str(&format!("Now solve this:\n{}", task));
        result
    }

    /// ReAct: Reasoning + Acting
    pub fn react(prompt: &str, thought: &str, action: &str, observation: &str) -> String {
        format!(
            "Prompt: {}\nThought: {}\nAction: {}\nObservation: {}",
            prompt, thought, action, observation
        )
    }
}

/// Error recovery strategies
pub struct ErrorRecovery;

impl ErrorRecovery {
    pub fn retry_with_backoff(max_retries: u32, initial_delay: u32) -> Vec<u32> {
        (0..max_retries)
            .map(|i| initial_delay * 2_u32.pow(i))
            .collect()
    }

    pub fn fallback_tools(primary: &str, fallbacks: &[&str]) -> Vec<String> {
        let mut tools = vec![primary.to_string()];
        tools.extend(fallbacks.iter().map(|s| s.to_string()));
        tools
    }
}
