// Example 1: Basic sandbox creation and tool invocation
use libagent::{tools::*, ResourceLimits, Sandbox};
use serde_json::json;

fn example_basic_sandbox() -> anyhow::Result<()> {
    let limits = ResourceLimits {
        ram_bytes: Some(512_000_000),
        cpu_millis: Some(1000),
    };

    let mut sandbox = Sandbox::new(limits)?;
    println!("Created sandbox {}", sandbox.id());

    sandbox.register_tool(Box::new(EchoTool));

    let input = json!({"message": "hello world"});
    let result = sandbox.invoke_tool("echo", input)?;
    println!("Echo result: {}", result);

    Ok(())
}

// Example 2: Agent loop
use libagent::AgentLoop;

fn example_agent_execution() -> anyhow::Result<()> {
    let mut agent = AgentLoop::new(1, 101, 100);

    let tools: Vec<Box<dyn Tool>> = vec![Box::new(EchoTool), Box::new(ReadFileTool)];

    let result = agent.run("Run command to list files", &tools)?;
    println!("Agent result: {}", result);

    Ok(())
}

fn main() {
    println!("MowisAI Agent Sandbox Examples");
    println!("==============================\n");
    println!("11 comprehensive examples included in library usage");
}
