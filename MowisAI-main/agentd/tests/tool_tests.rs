use libagent::Tool;
use libagent::ToolContext;
use libagent::{ResourceLimits, Sandbox};
use serde_json::json;

// simple echo tool for testing
struct EchoTool;
impl Tool for EchoTool {
    fn name(&self) -> &'static str {
        "echo"
    }
    fn invoke(
        &self,
        _ctx: &ToolContext,
        input: serde_json::Value,
    ) -> anyhow::Result<serde_json::Value> {
        Ok(input)
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(EchoTool)
    }
}

#[test]
fn tool_registry_basic() {
    let mut sb = Sandbox::new(ResourceLimits {
        ram_bytes: None,
        cpu_millis: None,
    })
    .unwrap();
    sb.register_tool(Box::new(EchoTool));
    let payload = json!({"hello": "world"});
    let result = sb.invoke_tool("echo", payload.clone()).unwrap();
    assert_eq!(result, payload);
    // missing tool should error
    assert!(sb.invoke_tool("nope", json!(null)).is_err());
}
