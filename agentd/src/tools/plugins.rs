//! Plugin System — Extensible tool registration via external scripts/commands
//!
//! Allows users to register custom tools that execute external commands.
//! Plugins are defined in ~/.mowisai/plugins/ as TOML files.

use crate::tools::common::{resolve_path, Tool, ToolContext};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Command;

/// Plugin definition loaded from TOML
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginDef {
    pub name: String,
    pub description: String,
    pub command: String,
    pub args: Vec<String>,
    pub timeout_secs: u64,
    pub input_schema: Option<Value>,
    pub output_schema: Option<Value>,
    pub tags: Vec<String>,
    pub author: Option<String>,
    pub version: Option<String>,
}

/// Load all plugins from the plugin directory
pub fn load_plugins() -> Vec<PluginDef> {
    let plugin_dir = plugin_directory();
    if !plugin_dir.exists() {
        return Vec::new();
    }

    let mut plugins = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&plugin_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "toml").unwrap_or(false) {
                if let Ok(content) = std::fs::read_to_string(&path) {
                    if let Ok(plugin) = toml::from_str::<PluginDef>(&content) {
                        plugins.push(plugin);
                    }
                }
            }
        }
    }

    plugins
}

fn plugin_directory() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".mowisai")
        .join("plugins")
}

/// Create a Tool implementation from a PluginDef
pub fn create_plugin_tool(def: PluginDef) -> Box<dyn Tool> {
    Box::new(PluginTool { def })
}

struct PluginTool {
    def: PluginDef,
}

impl Tool for PluginTool {
    fn name(&self) -> &'static str {
        // Leak the string to get a 'static str (acceptable for plugin tools)
        Box::leak(self.def.name.clone().into_boxed_str())
    }

    fn invoke(&self, ctx: &ToolContext, input: Value) -> anyhow::Result<Value> {
        let timeout = self.def.timeout_secs;
        let mut cmd = Command::new("timeout");
        cmd.arg(timeout.to_string());
        cmd.arg(&self.def.command);

        for arg in &self.def.args {
            cmd.arg(arg);
        }

        // Pass input as JSON via stdin
        let input_json = serde_json::to_string(&input)?;
        cmd.arg("--input").arg(&input_json);

        // Pass context
        cmd.arg("--sandbox-id").arg(ctx.sandbox_id.to_string());
        if let Some(ref root) = ctx.root_path {
            cmd.arg("--root").arg(root);
        }

        let output = cmd.output()?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        // Try to parse stdout as JSON
        if let Ok(result) = serde_json::from_str::<Value>(&stdout) {
            Ok(result)
        } else {
            Ok(json!({
                "success": output.status.success(),
                "output": stdout.to_string(),
                "stderr": stderr.to_string()
            }))
        }
    }

    fn clone_box(&self) -> Box<dyn Tool> {
        Box::new(PluginTool {
            def: self.def.clone(),
        })
    }
}

/// List all available plugins
pub fn list_plugins() -> Vec<HashMap<String, String>> {
    load_plugins()
        .into_iter()
        .map(|p| {
            let mut map = HashMap::new();
            map.insert("name".to_string(), p.name);
            map.insert("description".to_string(), p.description);
            map.insert("command".to_string(), p.command);
            map.insert("tags".to_string(), p.tags.join(", "));
            if let Some(author) = p.author {
                map.insert("author".to_string(), author);
            }
            if let Some(version) = p.version {
                map.insert("version".to_string(), version);
            }
            map
        })
        .collect()
}

/// Install a plugin from a TOML file
pub fn install_plugin(toml_path: &PathBuf) -> anyhow::Result<()> {
    let content = std::fs::read_to_string(toml_path)?;
    let _def: PluginDef = toml::from_str(&content)?;

    let plugin_dir = plugin_directory();
    std::fs::create_dir_all(&plugin_dir)?;

    let dest = plugin_dir.join(toml_path.file_name().unwrap_or_default());
    std::fs::copy(toml_path, &dest)?;

    Ok(())
}

/// Uninstall a plugin by name
pub fn uninstall_plugin(name: &str) -> anyhow::Result<bool> {
    let plugin_dir = plugin_directory();
    let path = plugin_dir.join(format!("{}.toml", name));
    if path.exists() {
        std::fs::remove_file(&path)?;
        Ok(true)
    } else {
        Ok(false)
    }
}
