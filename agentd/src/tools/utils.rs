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
        Ok(json!({ "echo": input.to_string() }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(EchoTool)
    }
}
