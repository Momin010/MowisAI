use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use anyhow::{Result, Context};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MowisConfig {
    pub gcp_project_id: String,
    pub socket_path: String,
    pub model: String,
    pub max_agents: usize,
    pub overlay_root: String,
    pub checkpoint_root: String,
    pub merge_work_dir: String,
}

impl Default for MowisConfig {
    fn default() -> Self {
        Self {
            gcp_project_id: String::new(),
            socket_path: "/tmp/mowisai.sock".into(),
            model: "gemini-2.5-pro".into(),
            max_agents: 1000,
            overlay_root: "/tmp/mowis-overlay".into(),
            checkpoint_root: "/tmp/mowis-checkpoints".into(),
            merge_work_dir: "/tmp/mowis-merge".into(),
        }
    }
}

impl MowisConfig {
    pub fn config_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".mowisai")
    }

    pub fn config_path() -> PathBuf {
        Self::config_dir().join("config.toml")
    }

    pub fn load() -> Result<Option<Self>> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(None);
        }
        let contents = std::fs::read_to_string(&path)
            .context("reading config file")?;
        let config: Self = toml::from_str(&contents)
            .context("parsing config.toml")?;
        Ok(Some(config))
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir();
        std::fs::create_dir_all(&dir)?;
        let contents = toml::to_string_pretty(self)
            .context("serializing config")?;
        std::fs::write(Self::config_path(), contents)?;
        Ok(())
    }

    pub fn is_valid(&self) -> bool {
        !self.gcp_project_id.is_empty()
    }
}
