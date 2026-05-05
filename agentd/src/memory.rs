use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, VecDeque};

// ============== SHORT-TERM MEMORY ==============

/// Short-Term Memory (STM) - volatile, session-based storage
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ShortTermMemory {
    pub session_id: u64,
    pub context: HashMap<String, Value>,
    pub task_stack: Vec<TaskFrame>,
    pub recent_results: VecDeque<ExecutionResult>,
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

const MAX_RECENT_RESULTS: usize = 50;

impl ShortTermMemory {
    pub fn new(session_id: u64) -> Self {
        ShortTermMemory {
            session_id,
            context: HashMap::new(),
            task_stack: Vec::new(),
            recent_results: VecDeque::new(),
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

    /// Add result with bounded size (VecDeque::pop_front is O(1))
    pub fn add_result(&mut self, result: ExecutionResult) {
        self.recent_results.push_back(result);
        while self.recent_results.len() > MAX_RECENT_RESULTS {
            self.recent_results.pop_front();
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
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LongTermMemory {
    pub agent_id: u64,
    /// Index for O(1) key lookups
    pub knowledge_index: HashMap<String, usize>,
    pub knowledge_base: Vec<KnowledgeEntry>,
    pub pattern_index: HashMap<String, PatternInfo>,
    pub decision_log: Vec<DecisionLog>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct KnowledgeEntry {
    pub key: String,
    pub value: String,
    pub confidence: f32,
    pub source: String,
    pub created_at: u64,
    pub accessed_count: u64,
    /// TF-IDF-style keyword vector for lightweight similarity
    pub keywords: Vec<String>,
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
            knowledge_index: HashMap::new(),
            knowledge_base: Vec::new(),
            pattern_index: HashMap::new(),
            decision_log: Vec::new(),
        }
    }

    /// Store knowledge with keyword extraction for search
    pub fn store_knowledge(&mut self, mut entry: KnowledgeEntry) {
        // Extract keywords from key and value for search
        if entry.keywords.is_empty() {
            entry.keywords = extract_keywords(&format!("{} {}", entry.key, entry.value));
        }

        let idx = self.knowledge_base.len();
        self.knowledge_index.insert(entry.key.clone(), idx);
        self.knowledge_base.push(entry);
    }

    /// O(1) retrieval by key
    pub fn retrieve_knowledge(&self, key: &str) -> Option<&KnowledgeEntry> {
        self.knowledge_index
            .get(key)
            .and_then(|&idx| self.knowledge_base.get(idx))
    }

    /// Keyword-based search (much faster than full-text scan)
    pub fn search_knowledge(&self, query: &str) -> Vec<&KnowledgeEntry> {
        let query_keywords = extract_keywords(query);
        if query_keywords.is_empty() {
            return self.knowledge_base.iter().collect();
        }

        self.knowledge_base
            .iter()
            .filter(|e| {
                // Check keyword overlap
                query_keywords.iter().any(|qk| e.keywords.contains(qk))
                // Also check direct substring match for exact queries
                || e.key.contains(query)
                || e.value.contains(query)
            })
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

/// Extract keywords from text (simple tokenization + lowercase)
fn extract_keywords(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2) // Skip very short words
        .map(|w| w.to_string())
        .collect::<Vec<_>>()
        .into_iter()
        .collect::<std::collections::HashSet<_>>() // Deduplicate
        .into_iter()
        .collect()
}

// ============== COMBINED AGENT MEMORY ==============

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

    /// Jaccard similarity between keyword sets (fast, no embeddings needed)
    pub fn keyword_similarity(a: &[String], b: &[String]) -> f32 {
        if a.is_empty() && b.is_empty() {
            return 1.0;
        }
        if a.is_empty() || b.is_empty() {
            return 0.0;
        }

        let set_a: std::collections::HashSet<&String> = a.iter().collect();
        let set_b: std::collections::HashSet<&String> = b.iter().collect();
        let intersection = set_a.intersection(&set_b).count();
        let union = set_a.union(&set_b).count();

        if union == 0 {
            0.0
        } else {
            intersection as f32 / union as f32
        }
    }

    /// Find knowledge matches using keyword similarity
    pub fn find_similar_knowledge<'a>(
        ltm: &'a LongTermMemory,
        query_keywords: &[String],
        threshold: f32,
    ) -> Vec<&'a KnowledgeEntry> {
        ltm.knowledge_base
            .iter()
            .filter(|entry| Self::keyword_similarity(&entry.keywords, query_keywords) >= threshold)
            .collect()
    }

    /// Suggest tools based on pattern history with safe comparison
    pub fn suggest_tools(ltm: &LongTermMemory, context: &str) -> Vec<String> {
        let context_lower = context.to_lowercase();
        let candidates: Vec<&PatternInfo> = ltm
            .pattern_index
            .values()
            .filter(|p| {
                let pattern_lower = p.pattern.to_lowercase();
                context_lower.contains(&pattern_lower)
                    || pattern_lower
                        .split('_')
                        .any(|part| context_lower.contains(part))
            })
            .collect();

        candidates
            .iter()
            .max_by(|a, b| {
                a.success_rate
                    .partial_cmp(&b.success_rate)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .map(|p| p.optimal_tools.clone())
            .unwrap_or_default()
    }
}

// ============== MEMORY PERSISTENCE ==============

pub struct MemoryPersistence;

impl MemoryPersistence {
    pub fn save_stm(memory: &AgentMemory, path: &std::path::Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(&memory.short_term)?;
        crate::persistence::PersistenceManager::atomic_write(path, json.as_bytes())?;
        Ok(())
    }

    pub fn load_stm(path: &std::path::Path) -> anyhow::Result<ShortTermMemory> {
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn save_ltm(memory: &AgentMemory, path: &std::path::Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(&memory.long_term)?;
        crate::persistence::PersistenceManager::atomic_write(path, json.as_bytes())?;
        Ok(())
    }

    pub fn load_ltm(path: &std::path::Path) -> anyhow::Result<LongTermMemory> {
        let json = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&json)?)
    }

    pub fn save_full_memory(memory: &AgentMemory, path: &std::path::Path) -> anyhow::Result<()> {
        let json = serde_json::to_string_pretty(memory)?;
        crate::persistence::PersistenceManager::atomic_write(path, json.as_bytes())?;
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

        memory
            .short_term
            .set_context("mykey".into(), json!("myvalue"));
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
        assert_eq!(
            deserialized.short_term.get_context("test"),
            Some(&json!("value"))
        );
    }

    #[test]
    fn test_knowledge_search_with_keywords() {
        let mut ltm = LongTermMemory::new(1);

        ltm.store_knowledge(KnowledgeEntry {
            key: "rust-borrow-checker".to_string(),
            value: "The borrow checker ensures memory safety in Rust".to_string(),
            confidence: 0.9,
            source: "docs".to_string(),
            created_at: 0,
            accessed_count: 0,
            keywords: vec![],
        });

        ltm.store_knowledge(KnowledgeEntry {
            key: "python-gc".to_string(),
            value: "Python uses reference counting with cycle detection".to_string(),
            confidence: 0.8,
            source: "docs".to_string(),
            created_at: 0,
            accessed_count: 0,
            keywords: vec![],
        });

        // Search for "rust" should find the borrow checker entry
        let results = ltm.search_knowledge("rust borrow");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "rust-borrow-checker");

        // Search for "python" should find the GC entry
        let results = ltm.search_knowledge("python reference");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].key, "python-gc");
    }

    #[test]
    fn test_suggest_tools_safe_ordering() {
        let mut ltm = LongTermMemory::new(1);

        ltm.record_pattern(
            "read_file_write_file".to_string(),
            vec!["read_file".to_string(), "write_file".to_string()],
            true,
        );
        ltm.record_pattern(
            "read_file_write_file".to_string(),
            vec!["read_file".to_string(), "write_file".to_string()],
            true,
        );
        ltm.record_pattern(
            "run_command_run_command".to_string(),
            vec!["run_command".to_string()],
            false,
        );

        let tools = SemanticMatcher::suggest_tools(&ltm, "read_file");
        assert!(!tools.is_empty());
        assert!(tools.contains(&"read_file".to_string()));
    }

    #[test]
    fn test_keyword_similarity() {
        let a = vec![
            "rust".to_string(),
            "memory".to_string(),
            "safety".to_string(),
        ];
        let b = vec![
            "rust".to_string(),
            "memory".to_string(),
            "management".to_string(),
        ];
        let sim = SemanticMatcher::keyword_similarity(&a, &b);
        // 2 intersection (rust, memory) / 4 union (rust, memory, safety, management) = 0.5
        assert!((sim - 0.5).abs() < 0.01);
    }

    #[test]
    fn test_recent_results_bounded() {
        let mut stm = ShortTermMemory::new(1);
        for i in 0..100 {
            stm.add_result(ExecutionResult {
                tool: "test".to_string(),
                input: json!(null),
                output: json!(i),
                timestamp: i,
                success: true,
            });
        }
        assert_eq!(stm.recent_results.len(), MAX_RECENT_RESULTS);
        // Most recent should be 99
        assert_eq!(stm.recent_results.back().unwrap().output, json!(99));
    }
}
