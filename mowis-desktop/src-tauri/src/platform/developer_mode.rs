// platform/developer_mode.rs — Custom QEMU configuration for advanced users
//
// For users with custom QEMU setups (custom ISO, persistent disks, locked-down systems).
// Asks configuration questions and generates a tailored launch script.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeveloperConfig {
    /// Path to qemu-system-x86_64 binary
    pub qemu_path: PathBuf,
    
    /// Path to ISO file (e.g., alpine-virt-3.19.1-x86_64.iso)
    pub iso_path: PathBuf,
    
    /// Path to persistent disk (e.g., momin_disk.qcow2)
    pub disk_path: PathBuf,
    
    /// Mount point inside VM (e.g., /mnt/mowisai)
    pub mount_point: String,
    
    /// Device name for disk (e.g., /dev/vda)
    pub disk_device: String,
    
    /// RAM in MB
    pub ram_mb: u32,
    
    /// TCP port for agentd communication
    pub agent_port: u16,
    
    /// Auto-sync strategy: "always", "interval", "manual"
    pub sync_strategy: String,
    
    /// Sync interval in seconds (if strategy is "interval")
    pub sync_interval_secs: u32,
    
    /// Path to agentd binary inside VM
    pub agentd_path: String,
    
    /// Additional QEMU args (comma-separated)
    pub extra_qemu_args: Vec<String>,
}

impl Default for DeveloperConfig {
    fn default() -> Self {
        Self {
            qemu_path: PathBuf::from("qemu-system-x86_64"),
            iso_path: PathBuf::from("alpine-virt-3.19.1-x86_64.iso"),
            disk_path: PathBuf::from("../momin_disk.qcow2"),
            mount_point: "/mnt/mowisai".into(),
            disk_device: "/dev/vda".into(),
            ram_mb: 512,
            agent_port: 9722,
            sync_strategy: "interval".into(),
            sync_interval_secs: 30,
            agentd_path: "/mnt/mowisai/agentd".into(),
            extra_qemu_args: vec![],
        }
    }
}

impl DeveloperConfig {
    /// Load from file or create default
    pub fn load_or_default() -> Self {
        let config_path = Self::config_file_path();
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(config) = serde_json::from_str(&content) {
                return config;
            }
        }
        Self::default()
    }
    
    /// Save to file
    pub fn save(&self) -> Result<()> {
        let config_path = Self::config_file_path();
        if let Some(parent) = config_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&config_path, json)?;
        Ok(())
    }
    
    fn config_file_path() -> PathBuf {
        dirs::config_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("MowisAI")
            .join("developer_config.json")
    }
    
    /// Build QEMU command line
    pub fn build_qemu_command(&self) -> Vec<String> {
        let mut args = vec![
            "-cdrom".to_string(), self.iso_path.display().to_string(),
            "-hda".to_string(), self.disk_path.display().to_string(),
            "-m".to_string(), self.ram_mb.to_string(),
            "-netdev".to_string(), format!("user,id=net0,hostfwd=tcp::{}-:22,hostfwd=tcp::{}-:{}", 
                self.agent_port, self.agent_port, self.agent_port),
            "-device".to_string(), "virtio-net,netdev=net0".to_string(),
            "-nographic".to_string(),
        ];
        
        // Add extra args
        args.extend(self.extra_qemu_args.clone());
        
        args
    }
    
    /// Generate startup script that runs inside VM
    pub fn generate_startup_script(&self) -> String {
        format!(
            r#"#!/bin/sh
# MowisAI Developer Mode Startup Script
# Auto-generated based on your configuration

echo "🚀 MowisAI Developer Mode Starting..."

# Mount persistent disk
echo "📁 Mounting {disk_device} to {mount_point}..."
mkdir -p {mount_point}
mount {disk_device} {mount_point}

if [ $? -ne 0 ]; then
    echo "❌ Failed to mount disk!"
    exit 1
fi

echo "✓ Disk mounted successfully"

# Copy agentd if needed
if [ ! -f {agentd_path} ]; then
    echo "📦 Copying agentd to persistent storage..."
    cp /tmp/agentd {agentd_path}
    chmod +x {agentd_path}
fi

# Start agentd
echo "🤖 Starting agentd..."
{agentd_path} socket --path /tmp/agentd.sock &
AGENTD_PID=$!

# Auto-sync daemon (if enabled)
{sync_daemon}

echo "✅ MowisAI Developer Mode Ready!"
echo "   Disk: {mount_point}"
echo "   Agent: {agentd_path}"
echo "   Port: {agent_port}"

# Keep running
wait $AGENTD_PID
"#,
            disk_device = self.disk_device,
            mount_point = self.mount_point,
            agentd_path = self.agentd_path,
            agent_port = self.agent_port,
            sync_daemon = self.generate_sync_daemon()
        )
    }
    
    fn generate_sync_daemon(&self) -> String {
        match self.sync_strategy.as_str() {
            "always" => {
                // Sync after every write (using inotify)
                format!(
                    r#"
# Auto-sync on file changes
echo "🔄 Auto-sync: ALWAYS (after every write)"
while true; do
    inotifywait -r -e modify,create,delete {} 2>/dev/null && sync
done &
"#,
                    self.mount_point
                )
            }
            "interval" => {
                // Sync every N seconds
                format!(
                    r#"
# Auto-sync every {} seconds
echo "🔄 Auto-sync: INTERVAL (every {}s)"
while true; do
    sleep {}
    sync
    echo "💾 Synced at $(date)"
done &
"#,
                    self.sync_interval_secs, self.sync_interval_secs, self.sync_interval_secs
                )
            }
            "manual" => {
                // No auto-sync
                r#"
# Manual sync only
echo "🔄 Auto-sync: MANUAL (run 'sync' command yourself)"
"#
                .to_string()
            }
            _ => String::new(),
        }
    }
    
    /// Validate configuration
    pub fn validate(&self) -> Result<Vec<String>> {
        let mut warnings = Vec::new();
        
        // Check if QEMU exists
        if !self.qemu_path.exists() {
            warnings.push(format!("QEMU binary not found: {}", self.qemu_path.display()));
        }
        
        // Check if ISO exists
        if !self.iso_path.exists() {
            warnings.push(format!("ISO file not found: {}", self.iso_path.display()));
        }
        
        // Check if disk exists
        if !self.disk_path.exists() {
            warnings.push(format!("Disk file not found: {}", self.disk_path.display()));
        }
        
        // Check port range
        if self.agent_port < 1024 || self.agent_port > 65535 {
            warnings.push(format!("Invalid port: {} (must be 1024-65535)", self.agent_port));
        }
        
        Ok(warnings)
    }
}

/// Interactive configuration wizard
pub struct DeveloperWizard {
    config: DeveloperConfig,
}

impl DeveloperWizard {
    pub fn new() -> Self {
        Self {
            config: DeveloperConfig::load_or_default(),
        }
    }
    
    /// Get current config
    pub fn config(&self) -> &DeveloperConfig {
        &self.config
    }
    
    /// Update a field
    pub fn set_field(&mut self, field: &str, value: String) -> Result<()> {
        match field {
            "qemu_path" => self.config.qemu_path = PathBuf::from(value),
            "iso_path" => self.config.iso_path = PathBuf::from(value),
            "disk_path" => self.config.disk_path = PathBuf::from(value),
            "mount_point" => self.config.mount_point = value,
            "disk_device" => self.config.disk_device = value,
            "ram_mb" => self.config.ram_mb = value.parse().context("invalid RAM value")?,
            "agent_port" => self.config.agent_port = value.parse().context("invalid port")?,
            "sync_strategy" => self.config.sync_strategy = value,
            "sync_interval_secs" => self.config.sync_interval_secs = value.parse().context("invalid interval")?,
            "agentd_path" => self.config.agentd_path = value,
            _ => anyhow::bail!("Unknown field: {}", field),
        }
        Ok(())
    }
    
    /// Save and return final config
    pub fn finish(self) -> Result<DeveloperConfig> {
        self.config.save()?;
        Ok(self.config)
    }
}
