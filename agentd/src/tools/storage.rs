use crate::tools::common::{resolve_path, Tool, ToolContext, MEMORY_STORE, SECRET_STORE};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;

pub struct MemorySetTool;
impl Tool for MemorySetTool {
    fn name(&self) -> &'static str {
        "memory_set"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let key = input["key"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("memory_set: missing key"))?;
        let value = input["value"].clone();

        let mut store = MEMORY_STORE.lock().unwrap();
        store.insert(key.to_string(), value);

        Ok(json!({ "success": true, "key": key }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(MemorySetTool)
    }
}

pub struct MemoryGetTool;
impl Tool for MemoryGetTool {
    fn name(&self) -> &'static str {
        "memory_get"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let key = input["key"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("memory_get: missing key"))?;

        let store = MEMORY_STORE.lock().unwrap();
        let value = store.get(key).cloned().unwrap_or(Value::Null);

        Ok(json!({ "value": value, "found": value != Value::Null }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(MemoryGetTool)
    }
}

pub struct MemoryDeleteTool;
impl Tool for MemoryDeleteTool {
    fn name(&self) -> &'static str {
        "memory_delete"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let key = input["key"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("memory_delete: missing key"))?;

        let mut store = MEMORY_STORE.lock().unwrap();
        let existed = store.remove(key).is_some();

        Ok(json!({ "success": true, "existed": existed }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(MemoryDeleteTool)
    }
}

pub struct MemoryListTool;
impl Tool for MemoryListTool {
    fn name(&self) -> &'static str {
        "memory_list"
    }
    fn invoke(&self, _ctx: &ToolContext, _input: Value) -> anyhow::Result<Value> {
        let store = MEMORY_STORE.lock().unwrap();
        let keys: Vec<String> = store.keys().cloned().collect();

        Ok(json!({ "keys": keys, "count": keys.len() }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(MemoryListTool)
    }
}

pub struct MemorySaveTool;
impl Tool for MemorySaveTool {
    fn name(&self) -> &'static str {
        "memory_save"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("memory_save: missing path"))?;

        let json_str = {
            let store = MEMORY_STORE.lock().unwrap();
            serde_json::to_string_pretty(&*store)?
        };

        let path = resolve_path(ctx, path_str);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, json_str)?;

        let store = MEMORY_STORE.lock().unwrap();
        Ok(json!({ "success": true, "keys_saved": store.len() }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(MemorySaveTool)
    }
}

pub struct MemoryLoadTool;
impl Tool for MemoryLoadTool {
    fn name(&self) -> &'static str {
        "memory_load"
    }
    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let path_str = input["path"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("memory_load: missing path"))?;

        let path = resolve_path(ctx, path_str);
        let content = fs::read_to_string(&path)?;
        let data: HashMap<String, Value> = serde_json::from_str(&content)?;
        let keys_loaded = data.len();

        {
            let mut store = MEMORY_STORE.lock().unwrap();
            store.extend(data);
        }

        let store = MEMORY_STORE.lock().unwrap();
        Ok(json!({ "success": true, "keys_loaded": keys_loaded, "total_keys": store.len() }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(MemoryLoadTool)
    }
}

// ============== SECRETS TOOLS (2) ==============

pub struct SecretSetTool;
impl Tool for SecretSetTool {
    fn name(&self) -> &'static str {
        "secret_set"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let name = input["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("secret_set: missing name"))?;
        let value = input["value"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("secret_set: missing value"))?;

        let mut store = SECRET_STORE.lock().unwrap();
        store.insert(name.to_string(), value.to_string());

        Ok(json!({ "success": true }))
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(SecretSetTool)
    }
}

pub struct SecretGetTool;
impl Tool for SecretGetTool {
    fn name(&self) -> &'static str {
        "secret_get"
    }
    fn invoke(&self, _ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let name = input["name"]
            .as_str()
            .ok_or_else(|| anyhow::anyhow!("secret_get: missing name"))?;

        let store = SECRET_STORE.lock().unwrap();
        match store.get(name) {
            Some(value) => Ok(json!({ "value": value })),
            None => Err(anyhow::anyhow!("secret not found: {}", name)),
        }
    }
    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(SecretGetTool)
    }
}

// ============== PACKAGE TOOLS (3) ==============
