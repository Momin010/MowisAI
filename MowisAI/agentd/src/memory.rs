use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

// ============== SHORT-TERM MEMORY ==============

/// Short-Term Memory (STM) - volatile, session-based storage
/// Cleared when agent session ends. Stores current task context, intermediate results.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShortTermMemory {
    pub session_id: u64,
    pub context: HashMap<String, Value>,
    pub task_stack: Vec<TaskFrame>,
    pub recent_results: Vec<ExecutionResult>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TaskFrame {
    pub task_id: String,
    pub goal: String,
    pub state: TaskState,
    pub tools_used: Vec<String>,
    pub subtasks: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TaskState {
    Pending,
    Running,
    Completed,
    Failed,
    Blocked,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub tool: String,
    pub input: Value,
    pub output: Value,
    pub timestamp: u64,
    pub success: bool,
}

impl ShortTermMemory {
    pub fn new(session_id: u64) -> Self {
        ShortTermMemory {
            session_id,
            context: HashMap::new(),
            task_stack: Vec::new(),
            recent_results: Vec::new(),
        }
    }

    pub fn set_context(&mut self, key: String, value: Value) {
        self.context.insert(key, value);
    }

    pub fn get_context(&self, key: &str) -> Option<&Value> {
        self.context.get(key)
    }

    pub fn push_task(&mut self, task: TaskFrame) {
        self.task_stack.push(task);
    }

    pub fn pop_task(&mut self) -> Option<TaskFrame> {
        self.task_stack.pop()
    }

    pub fn current_task(&self) -> Option<&TaskFrame> {
        self.task_stack.last()
    }

    pub fn add_result(&mut self, result: ExecutionResult) {
        self.recent_results.push(result);
        if self.recent_results.len() > 50 {
            self.recent_results.remove(0);
        }
    }

    pub fn clear(&mut self) {
        self.context.clear();
        self.task_stack.clear();
        self.recent_results.clear();
    }
}

// ============== LONG-TERM MEMORY ==============

/// Long-Term Memory (LTM) - persistent, semantic storage
/// Persists across sessions. Stores learned patterns, embeddings, indexed knowledge.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LongTermMemory {
    pub agent_id: u64,
    pub knowledge_base: Vec<KnowledgeEntry>,
    pub pattern_index: HashMap<String, PatternInfo>,
    pub semantic_cache: Vec<SemanticEntry>,
    pub decision_log: Vec<DecisionLog>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub key: String,
    pub value: String,
    pub embedding: Vec<f32>,
    pub confidence: f32,
    pub source: String,
    pub created_at: u64,
    pub accessed_count: u64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PatternInfo {
    pub pattern: String,
    pub frequency: u64,
    pub success_rate: f32,
    pub optimal_tools: Vec<String>,
    pub context_clues: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SemanticEntry {
    pub query: String,
    pub embedding: Vec<f32>,
    pub results: Vec<Value>,
    pub relevance_scores: Vec<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DecisionLog {
    pub decision_id: String,
    pub options: Vec<String>,
    pub chosen: String,
    pub reasoning: String,
    pub outcome: String,
    pub timestamp: u64,
}

impl LongTermMemory {
    pub fn new(agent_id: u64) -> Self {
        LongTermMemory {
            agent_id,
            knowledge_base: Vec::new(),
            pattern_index: HashMap::new(),
            semantic_cache: Vec::new(),
            decision_log: Vec::new(),
        }
    }

    pub fn store_knowledge(&mut self, entry: KnowledgeEntry) {
        self.knowledge_base.push(entry);
    }

    pub fn retrieve_knowledge(&self, key: &str) -> Option<&KnowledgeEntry> {
        self.knowledge_base.iter().find(|e| e.key == key)
    }

    pub fn search_knowledge(&self, query: &str) -> Vec<&KnowledgeEntry> {
        self.knowledge_base
            .iter()
            .filter(|e| e.key.contains(query) || e.value.contains(query))
            .collect()
    }

    pub fn record_pattern(&mut self, pattern: String, tools: Vec<String>, success: bool) {
        let entry = self
            .pattern_index
            .entry(pattern.clone())
            .or_insert(PatternInfo {
                pattern: pattern.clone(),
                frequency: 0,
                success_rate: 0.0,
                optimal_tools: tools.clone(),
                context_clues: Vec::new(),
            });

        entry.frequency += 1;
        let old_rate = entry.success_rate;
        let n = entry.frequency as f32;
        entry.success_rate = (old_rate * (n - 1.0) + if success { 1.0 } else { 0.0 }) / n;
    }

    pub fn log_decision(&mut self, log: DecisionLog) {
        self.decision_log.push(log);
    }

    pub fn get_recent_decisions(&self, count: usize) -> Vec<&DecisionLog> {
        self.decision_log.iter().rev().take(count).collect()
    }
}

// ============== COMBINED AGENT MEMORY ==============

/// Combined memory system (STM + LTM) for an agent
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AgentMemory {
    pub short_term: ShortTermMemory,
    pub long_term: LongTermMemory,
}

impl AgentMemory {
    pub fn new(agent_id: u64, session_id: u64) -> Self {
        AgentMemory {
            short_term: ShortTermMemory::new(session_id),
            long_term: LongTermMemory::new(agent_id),
        }
    }

    pub fn serialize_to_json(&self) -> anyhow::Result<Value> {
        Ok(serde_json::to_value(self)?)
    }

    pub fn deserialize_from_json(json: &Value) -> anyhow::Result<Self> {
        Ok(serde_json::from_value(json.clone())?)
    }

    pub fn serialize_stm(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(&self.short_term)?)
    }

    pub fn deserialize_stm(json: &str) -> anyhow::Result<ShortTermMemory> {
        Ok(serde_json::from_str(json)?)
    }

    pub fn serialize_ltm(&self) -> anyhow::Result<String> {
        Ok(serde_json::to_string(&self.long_term)?)
    }

    pub fn deserialize_ltm(json: &str) -> anyhow::Result<LongTermMemory> {
        Ok(serde_json::from_str(json)?)
    }
}

// ============== SEMANTIC MATCHING ==============

/// Semantic search and similarity matching for memory retrieval
pub struct SemanticMatcher;

impl SemanticMatcher {
    /// Compute cosine similarity between two vectors
    pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
        if a.len() != b.len() || a.is_empty() {
            return 0.0;
        }
        let dot_product: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
        let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
        let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();

        if norm_a == 0.0 || norm_b == 0.0 {
            return 0.0;
        }

        dot_product / (norm_a * norm_b)
    }

    /// Find semantic matches in knowledge base
    pub fn find_similar_knowledge<'a>(
        ltm: &'a LongTermMemory,
        query_embedding: &[f32],
        threshold: f32,
    ) -> Vec<&'a KnowledgeEntry> {
        ltm.knowledge_base
            .iter()
            .filter(|entry| Self::cosine_similarity(&entry.embedding, query_embedding) >= threshold)
            .collect()
    }

    /// Suggest tools based on pattern history
    pub fn suggest_tools(ltm: &LongTermMemory, context: &str) -> Vec<String> {
        ltm.pattern_index
            .values()
            .filter(|p| context.contains(&p.pattern))
            .max_by(|a, b| a.success_rate.partial_cmp(&b.success_rate).unwrap())
            .map(|p| p.optimal_tools.clone())
            .unwrap_or_default()
    }
}

// ============== MEMORY PERSISTENCE ==============

/// Memory persistence layer - saves/loads memory to disk
pub struct MemoryPersistence;

impl MemoryPersistence {
    pub fn save_stm(memory: &AgentMemory, path: &std::path::Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(&memory.short_term)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load_stm(path: &std::path::Path) -> anyhow::Result<ShortTermMemory> {
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn save_ltm(memory: &AgentMemory, path: &std::path::Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(&memory.long_term)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load_ltm(path: &std::path::Path) -> anyhow::Result<LongTermMemory> {
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn save_full_memory(memory: &AgentMemory, path: &std::path::Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(memory)?;
        std::fs::write(path, json)?;
        Ok(())
    }

    pub fn load_full_memory(path: &std::path::Path) -> anyhow::Result<AgentMemory> {
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_memory_set_and_get() {
        let agent_id = 1u64;
        let session_id = 100u64;
        let mut memory = AgentMemory::new(agent_id, session_id);

        // Test set_context
        memory.short_term.set_context("mykey".into(), json!("myvalue"));

        // Test get_context
        let value = memory.short_term.get_context("mykey");
        assert_eq!(value, Some(&json!("myvalue")));
    }

    #[test]
    fn test_short_term_memory_context() {
        let mut stm = ShortTermMemory::new(42);
        assert_eq!(stm.session_id, 42);

        stm.set_context("key1".into(), json!("value1"));
        assert_eq!(stm.get_context("key1"), Some(&json!("value1")));

        stm.set_context("key2".into(), json!({"nested": "object"}));
        assert_eq!(stm.get_context("key2"), Some(&json!({"nested": "object"})));
    }

    #[test]
    fn test_memory_serialization() {
        let mut memory = AgentMemory::new(1, 1);
        memory.short_term.set_context("test".into(), json!("value"));

        let json = memory.serialize_to_json().unwrap();
        assert!(!json.is_null());

        let deserialized = AgentMemory::deserialize_from_json(&json).unwrap();
        assert_eq!(deserialized.short_term.get_context("test"), Some(&json!("value")));
    }
}
