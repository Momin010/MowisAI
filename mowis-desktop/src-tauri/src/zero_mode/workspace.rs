// zero_mode/workspace.rs — Native OS workspace folder
//
// Creates a persistent, human-visible folder on the user's real disk.
// Zero mode agents write directly here — no overlayfs, no temp dir cleanup.
//
// Paths:
//   macOS  → ~/Documents/MowisAI/workspaces/{session-slug}/
//   Windows→ %USERPROFILE%\Documents\MowisAI\workspaces\{session-slug}\
//   Linux  → ~/MowisAI/workspaces/{session-slug}/

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkspaceInfo {
    pub session_id: String,
    /// Human-readable slug used for the folder name.
    pub slug: String,
    /// Absolute path on the user's real disk.
    pub path: String,
}

// ── Workspace lifecycle ───────────────────────────────────────────────────────

/// Create (or reopen) the workspace directory for `session_id`.
/// Returns info including the absolute path that was created.
pub fn create_workspace(session_id: &str) -> Result<WorkspaceInfo> {
    let slug = session_slug(session_id);
    let base = workspace_base_dir();
    let path = base.join(&slug);

    fs::create_dir_all(&path)
        .with_context(|| format!("create workspace at {}", path.display()))?;

    // Write a README so the user knows what this folder is.
    let readme = path.join("README.md");
    if !readme.exists() {
        let content = format!(
            "# MowisAI Zero-Mode Workspace\n\n\
             Session: `{session_id}`\n\n\
             This folder was created by MowisAI's Zero-Protection mode.\n\
             AI agents write files here directly — everything is saved to your real disk.\n"
        );
        fs::write(&readme, content)
            .with_context(|| format!("write README at {}", readme.display()))?;
    }

    Ok(WorkspaceInfo {
        session_id: session_id.to_owned(),
        slug: slug.clone(),
        path: path.to_string_lossy().into_owned(),
    })
}

/// Base directory for all MowisAI zero-mode workspaces.
pub fn workspace_base_dir() -> PathBuf {
    #[cfg(target_os = "linux")]
    {
        dirs::home_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("MowisAI")
            .join("workspaces")
    }
    #[cfg(not(target_os = "linux"))]
    {
        dirs::document_dir()
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("MowisAI")
            .join("workspaces")
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Convert a UUID session id to a short, filesystem-safe folder name like
/// `mowis-20240501-a3f2b1`.
fn session_slug(session_id: &str) -> String {
    let short = session_id.split('-').next().unwrap_or(session_id);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| {
            let secs = d.as_secs();
            // Format as YYYYMMDD from epoch seconds (approximate)
            let days = secs / 86400;
            let y = 1970 + days / 365;
            let d_in_year = days % 365;
            let m = d_in_year / 30 + 1;
            let d = d_in_year % 30 + 1;
            format!("{y:04}{m:02}{d:02}")
        })
        .unwrap_or_else(|_| "000000".to_string());
    format!("mowis-{now}-{short}")
}
