/// Claude API integration for task breaking and reasoning
///
/// Provides methods to:
/// - Break down complex tasks into subtasks using Claude
/// - Generate workerpersonality prompts
/// - Analyze task complexity

use serde::{Deserialize, Serialize};

/// Errors for Claude integration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClaudeError {
    ApiKeyMissing,
    RequestFailed(String),
    ResponseInvalid(String),
    RateLimited,
}

pub type ClaudeResult<T> = Result<T, ClaudeError>;

/// Configuration for Claude API
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClaudeConfig {
    pub api_key: String,
    pub api_endpoint: String, // "https://api.anthropic.com/v1/messages"
    pub model: String,        // "claude-3-haiku-20240307", "claude-3-opus-20240229"
}

/// Claude API message types
#[derive(Debug, Serialize, Deserialize)]
struct ClaudeMessage {
    role: String,
    content: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct ClaudeRequest {
    model: String,
    max_tokens: u32,
    messages: Vec<ClaudeMessage>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ClaudeResponse {
    content: Vec<ResponseBlock>,
}

#[derive(Debug, Serialize, Deserialize)]
struct ResponseBlock {
    #[serde(rename = "type")]
    block_type: String,
    text: Option<String>,
}

/// Claude client for task analysis
pub struct ClaudeClient {
    config: ClaudeConfig,
}

impl ClaudeClient {
    /// Create a new Claude client
    pub fn new(config: ClaudeConfig) -> Self {
        ClaudeClient { config }
    }

    /// Break down a task description into N subtasks using Claude
    pub fn break_down_task(&self, description: &str, num_subtasks: usize) -> ClaudeResult<Vec<String>> {
        if self.config.api_key.is_empty() {
            // If API key not set, fall back to simple splitting
            return Ok(self.simple_break_down(description, num_subtasks));
        }

        // In a real implementation, this would make an HTTP request to Claude API:
        let prompt = format!(
            "Break down the following task into exactly {} independent subtasks. \
             Format your response as a numbered list.\n\n\
             Task: {}",
            num_subtasks, description
        );

        // TODO: Implement actual Claude API HTTP request here
        // This would call self.config.api_endpoint with the prompt
        // and parse the JSON response

        // For now, return the fallback
        Ok(self.simple_break_down(description, num_subtasks))
    }

    /// Generate a personality-based system prompt for a worker
    pub fn generate_worker_prompt(&self, worker_name: &str) -> ClaudeResult<String> {
        if self.config.api_key.is_empty() {
            // Fallback to template
            return Ok(format!(
                "You are {}, a skilled software developer. Complete your assigned tasks with high quality and attention to detail.",
                worker_name
            ));
        }

        // In a real implementation, Claude could generate personalized prompts
        let prompt = format!(
            "Generate a short system prompt for a worker named '{}' who is a software specialist. \
             The prompt should define their role, responsibilities, and working style. \
             Keep it to 1-2 sentences.",
            worker_name
        );

        // TODO: Implement actual Claude API HTTP request here

        // For now, return template
        Ok(format!(
            "You are {}, a software development specialist. Complete your assigned work \
             with high quality. Use available tools to read files, execute code, and test your work. \
             Report all results clearly.",
            worker_name
        ))
    }

    /// Analyze task complexity to determine resource requirements
    pub fn analyze_complexity(&self, description: &str) -> ClaudeResult<TaskComplexityAnalysis> {
        if self.config.api_key.is_empty() {
            return Ok(TaskComplexityAnalysis {
                estimated_duration_secs: 300,
                required_skills: vec!["coding".to_string(), "testing".to_string()],
                recommended_workers: 3,
                risk_level: "medium".to_string(),
            });
        }

        // TODO: Implement actual Claude API HTTP request for complexity analysis

        Ok(TaskComplexityAnalysis {
            estimated_duration_secs: 300,
            required_skills: vec!["coding".to_string(), "testing".to_string()],
            recommended_workers: 3,
            risk_level: "medium".to_string(),
        })
    }

    /// Simple fallback task breakdown (when Claude API not available)
    fn simple_break_down(&self, description: &str, n: usize) -> Vec<String> {
        if n <= 1 {
            return vec![description.to_string()];
        }

        // Split by sentences or lines
        let parts: Vec<&str> = description.split(". ").collect();
        let mut subtasks = Vec::new();
        let items_per_task = (parts.len() / n).max(1);

        for i in 0..n {
            let start = i * items_per_task;
            let end = if i == n - 1 {
                parts.len()
            } else {
                (i + 1) * items_per_task
            };

            if start < parts.len() {
                let subtask = parts[start..end].join(". ");
                subtasks.push(subtask);
            }
        }

        if subtasks.is_empty() {
            vec![description.to_string()]
        } else {
            subtasks
        }
    }
}

/// Analysis results from Claude
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskComplexityAnalysis {
    pub estimated_duration_secs: u64,
    pub required_skills: Vec<String>,
    pub recommended_workers: usize,
    pub risk_level: String, // "low", "medium", "high"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_breakdown() {
        let config = ClaudeConfig {
            api_key: String::new(),
            api_endpoint: "https://api.anthropic.com/v1/messages".to_string(),
            model: "claude-3-haiku-20240307".to_string(),
        };
        let client = ClaudeClient::new(config);

        let description = "First part. Second part. Third part.";
        let result = client
            .break_down_task(description, 3)
            .expect("should break down");

        assert_eq!(result.len(), 3);
    }

    #[test]
    fn test_worker_prompt_generation() {
        let config = ClaudeConfig {
            api_key: String::new(),
            api_endpoint: "https://api.anthropic.com/v1/messages".to_string(),
            model: "claude-3-haiku-20240307".to_string(),
        };
        let client = ClaudeClient::new(config);

        let prompt = client
            .generate_worker_prompt("Jake")
            .expect("should generate prompt");

        assert!(prompt.contains("Jake"));
        assert!(prompt.contains("developer"));
    }

    #[test]
    fn test_complexity_analysis() {
        let config = ClaudeConfig {
            api_key: String::new(),
            api_endpoint: "https://api.anthropic.com/v1/messages".to_string(),
            model: "claude-3-haiku-20240307".to_string(),
        };
        let client = ClaudeClient::new(config);

        let analysis = client
            .analyze_complexity("Build a web server")
            .expect("should analyze");

        assert!(analysis.recommended_workers > 0);
    }
}
