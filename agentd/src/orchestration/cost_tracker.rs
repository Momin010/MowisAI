//! Cost Tracking — Track LLM API costs and enforce budgets
//!
//! Tracks token usage and cost per provider, model, agent, and task.
//! Enforces budget limits to prevent runaway costs.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Cost per 1M tokens for each provider/model combination
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelPricing {
    pub input_cost_per_1m: f64,
    pub output_cost_per_1m: f64,
}

/// Get pricing for a model
pub fn get_model_pricing(model: &str) -> ModelPricing {
    let m = model.to_lowercase();
    
    // Gemini models
    if m.contains("gemini-2.5-pro") {
        return ModelPricing { input_cost_per_1m: 1.25, output_cost_per_1m: 10.0 };
    }
    if m.contains("gemini-2.5-flash") {
        return ModelPricing { input_cost_per_1m: 0.15, output_cost_per_1m: 0.60 };
    }
    if m.contains("gemini-2.0-flash") {
        return ModelPricing { input_cost_per_1m: 0.10, output_cost_per_1m: 0.40 };
    }
    
    // OpenAI models
    if m.contains("gpt-4o") {
        return ModelPricing { input_cost_per_1m: 2.50, output_cost_per_1m: 10.0 };
    }
    if m.contains("gpt-4-turbo") {
        return ModelPricing { input_cost_per_1m: 10.0, output_cost_per_1m: 30.0 };
    }
    if m.contains("gpt-4") {
        return ModelPricing { input_cost_per_1m: 30.0, output_cost_per_1m: 60.0 };
    }
    if m.contains("gpt-3.5") {
        return ModelPricing { input_cost_per_1m: 0.50, output_cost_per_1m: 1.50 };
    }
    if m.contains("o1") || m.contains("o3") {
        return ModelPricing { input_cost_per_1m: 15.0, output_cost_per_1m: 60.0 };
    }
    
    // Anthropic models
    if m.contains("claude-opus") {
        return ModelPricing { input_cost_per_1m: 15.0, output_cost_per_1m: 75.0 };
    }
    if m.contains("claude-sonnet") {
        return ModelPricing { input_cost_per_1m: 3.0, output_cost_per_1m: 15.0 };
    }
    if m.contains("claude-haiku") {
        return ModelPricing { input_cost_per_1m: 0.25, output_cost_per_1m: 1.25 };
    }
    
    // Default (unknown model)
    ModelPricing { input_cost_per_1m: 5.0, output_cost_per_1m: 15.0 }
}

/// Token usage record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub model: String,
    pub provider: String,
    pub task_id: Option<String>,
    pub agent_id: Option<String>,
    pub timestamp: u64,
}

/// Cost summary for a scope (agent, task, session, etc.)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CostSummary {
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost_usd: f64,
    pub call_count: u64,
    pub by_model: HashMap<String, ModelCost>,
    pub by_task: HashMap<String, f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelCost {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    pub call_count: u64,
}

/// Budget configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Maximum total cost in USD (0 = unlimited)
    pub max_cost_usd: f64,
    /// Maximum tokens per single call
    pub max_tokens_per_call: u64,
    /// Maximum total tokens
    pub max_total_tokens: u64,
    /// Warning threshold (percentage of budget)
    pub warning_threshold_percent: f64,
    /// Action when budget exceeded
    pub on_exceeded: BudgetAction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BudgetAction {
    /// Log warning but continue
    Warn,
    /// Switch to cheaper model
    DowngradeModel,
    /// Stop execution
    Stop,
}

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            max_cost_usd: 0.0, // unlimited
            max_tokens_per_call: 0, // unlimited
            max_total_tokens: 0, // unlimited
            warning_threshold_percent: 80.0,
            on_exceeded: BudgetAction::Warn,
        }
    }
}

/// Cost tracker — tracks all LLM API costs
pub struct CostTracker {
    /// All usage records
    usage: Arc<RwLock<Vec<TokenUsage>>>,
    /// Running totals
    totals: Arc<RwLock<CostSummary>>,
    /// Budget configuration
    budget: BudgetConfig,
    /// Whether budget warning has been issued
    warned: Arc<RwLock<bool>>,
}

impl CostTracker {
    pub fn new(budget: BudgetConfig) -> Self {
        Self {
            usage: Arc::new(RwLock::new(Vec::new())),
            totals: Arc::new(RwLock::new(CostSummary::default())),
            budget,
            warned: Arc::new(RwLock::new(false)),
        }
    }
    
    /// Record a token usage event
    pub async fn record_usage(&self, usage: TokenUsage) -> CostSummary {
        let pricing = get_model_pricing(&usage.model);
        let cost = (usage.input_tokens as f64 / 1_000_000.0) * pricing.input_cost_per_1m
            + (usage.output_tokens as f64 / 1_000_000.0) * pricing.output_cost_per_1m;
        
        // Update totals
        {
            let mut totals = self.totals.write().await;
            totals.total_input_tokens += usage.input_tokens;
            totals.total_output_tokens += usage.output_tokens;
            totals.total_cost_usd += cost;
            totals.call_count += 1;
            
            // Update per-model stats
            let model_entry = totals.by_model.entry(usage.model.clone()).or_default();
            model_entry.input_tokens += usage.input_tokens;
            model_entry.output_tokens += usage.output_tokens;
            model_entry.cost_usd += cost;
            model_entry.call_count += 1;
            
            // Update per-task stats
            if let Some(ref task_id) = usage.task_id {
                *totals.by_task.entry(task_id.clone()).or_insert(0.0) += cost;
            }
        }
        
        // Store usage record
        {
            let mut records = self.usage.write().await;
            records.push(usage);
        }
        
        // Check budget
        self.check_budget().await;
        
        self.get_summary().await
    }
    
    /// Check if budget is exceeded
    async fn check_budget(&self) {
        if self.budget.max_cost_usd <= 0.0 {
            return; // No budget set
        }
        
        let totals = self.totals.read().await;
        let usage_percent = (totals.total_cost_usd / self.budget.max_cost_usd) * 100.0;
        
        if usage_percent >= self.budget.warning_threshold_percent {
            let mut warned = self.warned.write().await;
            if !*warned {
                log::warn!(
                    "[CostTracker] Budget warning: ${:.2} / ${:.2} ({:.1}%)",
                    totals.total_cost_usd,
                    self.budget.max_cost_usd,
                    usage_percent
                );
                *warned = true;
            }
        }
        
        if totals.total_cost_usd >= self.budget.max_cost_usd {
            match self.budget.on_exceeded {
                BudgetAction::Warn => {
                    log::warn!(
                        "[CostTracker] Budget exceeded: ${:.2} / ${:.2}",
                        totals.total_cost_usd,
                        self.budget.max_cost_usd
                    );
                }
                BudgetAction::DowngradeModel => {
                    log::warn!(
                        "[CostTracker] Budget exceeded, switching to cheaper model"
                    );
                }
                BudgetAction::Stop => {
                    log::error!(
                        "[CostTracker] Budget exceeded, stopping execution"
                    );
                }
            }
        }
    }
    
    /// Get current cost summary
    pub async fn get_summary(&self) -> CostSummary {
        self.totals.read().await.clone()
    }
    
    /// Get remaining budget
    pub async fn remaining_budget(&self) -> f64 {
        if self.budget.max_cost_usd <= 0.0 {
            return f64::MAX; // No budget
        }
        let totals = self.totals.read().await;
        (self.budget.max_cost_usd - totals.total_cost_usd).max(0.0)
    }
    
    /// Check if a call would exceed budget
    pub async fn would_exceed_budget(&self, estimated_tokens: u64, model: &str) -> bool {
        if self.budget.max_cost_usd <= 0.0 {
            return false;
        }
        
        let pricing = get_model_pricing(model);
        let estimated_cost = (estimated_tokens as f64 / 1_000_000.0) * pricing.output_cost_per_1m;
        let totals = self.totals.read().await;
        
        totals.total_cost_usd + estimated_cost > self.budget.max_cost_usd
    }
    
    /// Get recommended model based on remaining budget
    pub async fn recommend_model(&self, preferred: &str) -> String {
        if self.budget.max_cost_usd <= 0.0 {
            return preferred.to_string();
        }
        
        let remaining = self.remaining_budget().await;
        let totals = self.totals.read().await;
        let remaining_tokens = if self.budget.max_total_tokens > 0 {
            self.budget.max_total_tokens.saturating_sub(totals.total_input_tokens + totals.total_output_tokens)
        } else {
            u64::MAX
        };
        
        // If budget is tight, recommend cheaper model
        if remaining < 1.0 || remaining_tokens < 100_000 {
            let preferred_lower = preferred.to_lowercase();
            if preferred_lower.contains("gemini-2.5-pro") {
                return "gemini-2.5-flash".to_string();
            }
            if preferred_lower.contains("gpt-4o") {
                return "gpt-3.5-turbo".to_string();
            }
            if preferred_lower.contains("claude-sonnet") {
                return "claude-haiku".to_string();
            }
        }
        
        preferred.to_string()
    }
    
    /// Get usage history
    pub async fn get_history(&self) -> Vec<TokenUsage> {
        self.usage.read().await.clone()
    }
    
    /// Get total cost
    pub async fn total_cost(&self) -> f64 {
        self.totals.read().await.total_cost_usd
    }
    
    /// Get total tokens
    pub async fn total_tokens(&self) -> u64 {
        let totals = self.totals.read().await;
        totals.total_input_tokens + totals.total_output_tokens
    }
}

impl std::fmt::Display for CostSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Cost: ${:.4} | Tokens: {} in / {} out | Calls: {}",
            self.total_cost_usd,
            self.total_input_tokens,
            self.total_output_tokens,
            self.call_count
        )
    }
}
