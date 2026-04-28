use crate::tools::common::{Tool, ToolContext, CHANNELS};
use serde_json::{json, Value};

pub struct CreateChannelTool;
impl Tool for CreateChannelTool {
    fn name(&self) -> &'static str {
        "create_channel"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let name = input["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("create_channel: missing name"))?;

        let mut channels = CHANNELS.lock().unwrap();
        channels.insert(name.to_string(), vec![]);

        Ok(json!({ "success": true, "name": name }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(CreateChannelTool)
    }
}

pub struct SendMessageTool;
impl Tool for SendMessageTool {
    fn name(&self) -> &'static str {
        "send_message"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let channel = input["channel"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("send_message: missing channel"))?;
        let message = &input["message"];
        let sender = input
            .get("sender")
            .and_then(|v| v.as_str())
            .unwrap_or("system");

        let mut channels = CHANNELS.lock().unwrap();
        if let Some(msgs) = channels.get_mut(channel) {
            msgs.push(json!({ "sender": sender, "message": message }));
            Ok(json!({ "success": true }))
        } else {
            Err(anyhow::anyhow!("channel not found: {}", channel))
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(SendMessageTool)
    }
}

pub struct ReadMessagesTool;
impl Tool for ReadMessagesTool {
    fn name(&self) -> &'static str {
        "read_messages"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let channel = input["channel"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("read_messages: missing channel"))?;

        let channels = CHANNELS.lock().unwrap();
        let messages = channels
            .get(channel)
            .map(|msgs| msgs.clone())
            .unwrap_or_default();

        Ok(json!({ "messages": messages, "count": messages.len() }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(ReadMessagesTool)
    }
}

pub struct BroadcastTool;
impl Tool for BroadcastTool {
    fn name(&self) -> &'static str {
        "broadcast"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let message = &input["message"];
        let sender = input
            .get("sender")
            .and_then(|v| v.as_str())
            .unwrap_or("system");

        let mut channels = CHANNELS.lock().unwrap();
        let msg = json!({ "sender": sender, "message": message });
        let channels_count = channels.len();

        for msgs in channels.values_mut() {
            msgs.push(msg.clone());
        }

        Ok(json!({ "success": true, "channels_count": channels_count }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(BroadcastTool)
    }
}

pub struct WaitForTool;
impl Tool for WaitForTool {
    fn name(&self) -> &'static str {
        "wait_for"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let channel = input["channel"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("wait_for: missing channel"))?;
        let _timeout = input.get("timeout").and_then(|v| v.as_u64());

        let channels = CHANNELS.lock().unwrap();
        let has_messages = channels
            .get(channel)
            .map(|msgs| !msgs.is_empty())
            .unwrap_or(false);

        Ok(json!({ "success": has_messages }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(WaitForTool)
    }
}
