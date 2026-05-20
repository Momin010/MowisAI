use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use crate::plan::{PlanId, SandboxConfig, TaskGraph, TaskId, TaskNode, ModelTier, Tier};
use crate::providers::Provider;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchConfig {
    pub providers: HashMap<Provider, ProviderCreds>,
    pub tiers: HashMap<Tier, ModelRef>,
    pub sandbox: SandboxConfig,
    pub plans_dir: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCreds {
    pub api_key_enc: Option<String>,
    pub project_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelRef {
    pub provider: Provider,
    pub model: String,
}

impl OrchConfig {
    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if !path.exists() {
            return Ok(Self::default());
        }
        let contents = std::fs::read_to_string(&path)?;
        let config: Self = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn save(&self) -> Result<()> {
        let dir = Self::config_dir();
        std::fs::create_dir_all(&dir)?;
        let contents = toml::to_string_pretty(self)?;
        let path = Self::config_path();
        std::fs::write(&path, contents)?;
        Ok(())
    }

    pub fn config_dir() -> PathBuf {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join(".mowisai")
    }

    fn config_path() -> PathBuf {
        Self::config_dir().join("mowis.toml")
    }

    pub fn llm_for(&self, tier: &Tier) -> Result<crate::providers::LlmConfig> {
        let model_ref = self
            .tiers
            .get(tier)
            .ok_or_else(|| anyhow::anyhow!("no model configured for tier {:?}", tier))?;
        let creds = self.providers.get(&model_ref.provider);
        let api_key = creds
            .and_then(|c| c.api_key_enc.as_deref())
            .map(crate::crypto::decrypt)
            .transpose()?;
        let project_id = creds
            .and_then(|c| c.project_id.clone());

        Ok(crate::providers::LlmConfig {
            provider: model_ref.provider.clone(),
            model: model_ref.model.clone(),
            vertex_project_id: project_id,
            api_key,
        })
    }

    pub fn llm_for_task(
        &self,
        plan: &crate::plan::Plan,
        task: &TaskNode,
    ) -> Result<crate::providers::LlmConfig> {
        let tier = match task.model_tier {
            ModelTier::Fast => Tier::Crew,
            ModelTier::Mid => Tier::Captain,
            ModelTier::Flagship => Tier::Conductor,
        };
        self.llm_for(&tier)
    }
}

impl Default for OrchConfig {
    fn default() -> Self {
        let mut tiers = HashMap::new();
        tiers.insert(
            Tier::Conductor,
            ModelRef {
                provider: Provider::Anthropic,
                model: "claude-opus-4-7".into(),
            },
        );
        tiers.insert(
            Tier::Critic,
            ModelRef {
                provider: Provider::Anthropic,
                model: "claude-opus-4-7".into(),
            },
        );
        tiers.insert(
            Tier::Captain,
            ModelRef {
                provider: Provider::Anthropic,
                model: "claude-sonnet-4-6".into(),
            },
        );
        tiers.insert(
            Tier::Crew,
            ModelRef {
                provider: Provider::Anthropic,
                model: "claude-haiku-4-5-20251001".into(),
            },
        );

        Self {
            providers: HashMap::new(),
            tiers,
            sandbox: SandboxConfig::default(),
            plans_dir: PathBuf::from(".mowis/plans"),
        }
    }
}
