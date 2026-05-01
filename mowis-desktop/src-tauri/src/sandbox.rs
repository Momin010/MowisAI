// sandbox.rs — Cross-platform soft sandbox (no overlayfs / kernel isolation)
//
// Creates a tmpfs-style workspace for each session:
//   lower_dir = original project path (read-only by convention)
//   upper_dir = temp directory where agents write
//
// Works on Windows, macOS, and Linux without elevated privileges.
// Not kernel-isolated; the goal is to protect the original codebase from
// accidental writes while agents run, not to enforce security boundaries.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use uuid::Uuid;

// ── Public types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxInfo {
    pub id: String,
    /// Original project root — agents read from here for files not yet in upper_dir.
    pub lower_dir: String,
    /// Writable temp workspace — agents write here (copy-on-write intent).
    pub upper_dir: String,
}

// ── Sandbox lifecycle ─────────────────────────────────────────────────────────

/// Create a new soft sandbox for `project_path`.
///
/// Copies project files (excluding .git / target / node_modules) into a fresh
/// temp directory that becomes `upper_dir`.  `lower_dir` is simply the original
/// project path stored for reference.
pub fn create_sandbox(project_path: &Path) -> Result<SandboxInfo> {
    let id = Uuid::new_v4().to_string();
    let sandbox_root = std::env::temp_dir()
        .join("mowis-sandbox")
        .join(&id);
    let upper_dir = sandbox_root.join("upper");

    fs::create_dir_all(&upper_dir)
        .with_context(|| format!("create sandbox upper dir {}", upper_dir.display()))?;

    copy_dir_recursive(project_path, &upper_dir)
        .with_context(|| format!("copy project into sandbox ({})", project_path.display()))?;

    Ok(SandboxInfo {
        id,
        lower_dir: project_path.to_string_lossy().into_owned(),
        upper_dir: upper_dir.to_string_lossy().into_owned(),
    })
}

/// Remove the sandbox directory entirely.  Safe to call on a non-existent id.
pub fn destroy_sandbox(sandbox_id: &str) -> Result<()> {
    let sandbox_root = std::env::temp_dir()
        .join("mowis-sandbox")
        .join(sandbox_id);
    if sandbox_root.exists() {
        fs::remove_dir_all(&sandbox_root)
            .with_context(|| format!("remove sandbox dir {}", sandbox_root.display()))?;
    }
    Ok(())
}

/// Return the disk size (bytes) of the upper_dir.  Best-effort; returns 0 on error.
pub fn upper_dir_size(info: &SandboxInfo) -> u64 {
    dir_size(Path::new(&info.upper_dir)).unwrap_or(0)
}

// ── Internals ─────────────────────────────────────────────────────────────────

/// Directories skipped during the initial copy to keep sandbox creation fast.
const SKIP_DIRS: &[&str] = &[
    ".git",
    "target",
    "node_modules",
    ".next",
    "dist",
    "build",
    "__pycache__",
    ".venv",
    "venv",
];

fn copy_dir_recursive(src: &Path, dst: &Path) -> Result<()> {
    if !src.exists() {
        return Ok(());
    }
    fs::create_dir_all(dst)?;

    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let file_name = entry.file_name();
        let name = file_name.to_string_lossy();

        if SKIP_DIRS.contains(&name.as_ref()) {
            continue;
        }

        let src_path = entry.path();
        let dst_path = dst.join(&file_name);

        if src_path.is_symlink() {
            // Copy symlinks as regular files on platforms that support it;
            // otherwise just skip — agents can recreate them if needed.
            if let Ok(target) = fs::read_link(&src_path) {
                #[cfg(unix)]
                {
                    let _ = std::os::unix::fs::symlink(&target, &dst_path);
                }
                #[cfg(windows)]
                {
                    // Windows symlinks require elevated rights; skip silently.
                    let _ = target;
                }
            }
        } else if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            fs::copy(&src_path, &dst_path)
                .with_context(|| format!("copy {} → {}", src_path.display(), dst_path.display()))?;
        }
    }
    Ok(())
}

fn dir_size(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let meta = entry.metadata()?;
        if meta.is_dir() {
            total += dir_size(&entry.path()).unwrap_or(0);
        } else {
            total += meta.len();
        }
    }
    Ok(total)
}
