use crate::tools::common::{Tool, ToolContext};
use fastrand;
use serde_json::{json, Value};

pub struct SpawnAgentTool;
impl Tool for SpawnAgentTool {
    fn name(&self) -> &'static str {
        "spawn_agent"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let _task = input.get("task").and_then(|v| v.as_str());
        let _tools = input.get("tools").and_then(|v| v.as_array());

        // Generate a random agent ID
        let agent_id = fastrand::u64(1..u64::MAX);

        Ok(json!({ "success": true, "agent_id": agent_id }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(SpawnAgentTool)
    }
}

// ============== ECHO TOOL (Legacy) ==============

pub struct EchoTool;
impl Tool for EchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        // Pass input fields through directly so callers get back what they sent
        // (e.g. {"message":"hello"} → {"message":"hello","echo":"..."}).
        // For non-object inputs wrap in {"message": <value>}.
        let echo_str = serde_json::to_string(&input).unwrap_or_default();
        let mut out = match input {
            Value::Object(map) => Value::Object(map),
            other => {
                let mut m = serde_json::Map::new();
                m.insert("message".to_string(), other);
                Value::Object(m)
            }
        };
        if let Value::Object(ref mut map) = out {
            map.entry("echo").or_insert(Value::String(echo_str));
        }
        Ok(out)
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(EchoTool)
    }
}
