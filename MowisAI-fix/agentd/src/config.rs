use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use anyhow::{Result, Context};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum AiProvider {
    VertexAi,
    Grok,
}

impl Default for AiProvider {
    fn default() -> Self {
        AiProvider::VertexAi
    }
}

impl std::fmt::Display for AiProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AiProvider::VertexAi => write!(f, "Vertex AI (Google Cloud)"),
            AiProvider::Grok => write!(f, "Grok AI (xAI)"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MowisConfig {
    #[serde(default)]
    pub provider: AiProvider,

    // ── Vertex AI fields ────────────────────────────────────────────────────
    #[serde(default)]
    pub gcp_project_id: String,

    // ── Grok AI fields ──────────────────────────────────────────────────────
    /// AES-256-GCM encrypted xAI API key stored as "<nonce_b64>:<ciphertext_b64>".
    /// Decrypted at runtime via crypto::decrypt().
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grok_api_key_enc: Option<String>,

    /// Primary Grok model selected during setup (e.g. "grok-3").
    #[serde(default)]
    pub grok_model: String,

    // ── Shared fields ───────────────────────────────────────────────────────
    pub socket_path: String,
    /// Active model identifier (Gemini model for VertexAi, Grok model for Grok).
    pub model: String,
    pub max_agents: usize,
    pub overlay_root: String,
    pub checkpoint_root: String,
    pub merge_work_dir: String,
}

impl Default for MowisConfig {
    fn default() -> Self {
        Self {
            provider: AiProvider::default(),
            gcp_project_id: String::new(),
            grok_api_key_enc: None,
            grok_model: String::new(),
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
        // Restrict permissions to owner-only before writing (contains encrypted key).
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            let _ = std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&dir);
        }
        let contents = toml::to_string_pretty(self)
            .context("serializing config")?;
        let path = Self::config_path();
        std::fs::write(&path, contents)?;
        // Restrict config file to owner read/write only.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        Ok(())
    }

    /// Returns the decrypted Grok API key, or an error if not set / decryption fails.
    pub fn grok_api_key(&self) -> Result<String> {
        let enc = self.grok_api_key_enc.as_deref()
            .ok_or_else(|| anyhow::anyhow!("No Grok API key configured"))?;
        crate::crypto::decrypt(enc)
    }

    pub fn is_valid(&self) -> bool {
        match self.provider {
            AiProvider::VertexAi => !self.gcp_project_id.is_empty(),
            AiProvider::Grok => {
                self.grok_api_key_enc.is_some() && !self.grok_model.is_empty()
            }
        }
    }
}
