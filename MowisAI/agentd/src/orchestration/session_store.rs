//! Persist interactive orchestration state (chat, context, sandbox map, warm containers) to disk.

use std::collections::HashMap;
use std::fs;
use std::path::Path;

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use super::types::{ProjectContext, SandboxWarmState};

const SNAPSHOT_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveSessionSnapshot {
    pub schema_version: u32,
    pub project_id: String,
    pub socket_path: String,
    pub max_agents: usize,
    pub context: ProjectContext,
    pub transcript: Vec<String>,
    pub sandbox_by_team: HashMap<String, String>,
    pub warm_by_sandbox: HashMap<String, SandboxWarmState>,
    pub assistant_turns: Vec<String>,
}

impl InteractiveSessionSnapshot {
    pub fn new_v1(
        project_id: String,
        socket_path: String,
        max_agents: usize,
        context: ProjectContext,
        transcript: Vec<String>,
        sandbox_by_team: HashMap<String, String>,
        warm_by_sandbox: HashMap<String, SandboxWarmState>,
        assistant_turns: Vec<String>,
    ) -> Self {
        Self {
            schema_version: SNAPSHOT_VERSION,
            project_id,
            socket_path,
            max_agents,
            context,
            transcript,
            sandbox_by_team,
            warm_by_sandbox,
            assistant_turns,
        }
    }
}

/// Write snapshot to `path` (atomic replace).
pub fn write_snapshot(path: &Path, snap: &InteractiveSessionSnapshot) -> Result<()> {
    if snap.schema_version != SNAPSHOT_VERSION {
        anyhow::bail!("snapshot version mismatch");
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).with_context(|| format!("create_dir_all {:?}", parent))?;
    }
    let tmp = path.with_extension("json.tmp");
    let json = serde_json::to_string_pretty(snap).context("serialize session")?;
    fs::write(&tmp, &json).with_context(|| format!("write {:?}", tmp))?;
    fs::rename(&tmp, path).with_context(|| format!("rename {:?} -> {:?}", tmp, path))?;
    Ok(())
}

pub fn read_snapshot(path: &Path) -> Result<InteractiveSessionSnapshot> {
    let raw = fs::read_to_string(path).with_context(|| format!("read {:?}", path))?;
    let snap: InteractiveSessionSnapshot = serde_json::from_str(&raw).context("parse session json")?;
    if snap.schema_version != SNAPSHOT_VERSION {
        anyhow::bail!(
            "unsupported session schema {} (expected {})",
            snap.schema_version,
            SNAPSHOT_VERSION
        );
    }
    Ok(snap)
}
