//! OCI image pulling via `skopeo` + rootfs extraction.
//!
//! MVP shells out to `skopeo` and `tar` because they're widely available and
//! the alternative (pure-Rust OCI distribution client + layer reassembly) is
//! a separate effort. The host crate exposes the surface so a Rust-native
//! implementation can replace this transparently later.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use tokio::process::Command;

/// Pull an OCI image (e.g. `docker://alpine:3.19`) and extract its rootfs.
///
/// Returns the path to the extracted rootfs directory, suitable for use as an
/// overlayfs lower layer or for packing into an initrd / disk image.
pub async fn pull_rootfs(image_ref: &str, cache_dir: &Path) -> Result<PathBuf> {
    which::which("skopeo").context("`skopeo` not found on PATH (install it to pull OCI images)")?;
    which::which("tar").context("`tar` not found on PATH")?;

    std::fs::create_dir_all(cache_dir).context("create image cache dir")?;

    let safe_name = image_ref.replace(['/', ':', '@'], "_");
    let image_dir = cache_dir.join(format!("oci-{safe_name}"));
    let rootfs_dir = cache_dir.join(format!("rootfs-{safe_name}"));

    if rootfs_dir.exists() {
        tracing::info!(image = image_ref, "rootfs already cached");
        return Ok(rootfs_dir);
    }

    let _ = std::fs::remove_dir_all(&image_dir);
    let src = if image_ref.contains("://") {
        image_ref.to_string()
    } else {
        format!("docker://{image_ref}")
    };

    tracing::info!(image = %src, dest = %image_dir.display(), "skopeo copy");
    let status = Command::new("skopeo")
        .args([
            "copy",
            "--override-os",
            "linux",
            &src,
            &format!("oci:{}:latest", image_dir.display()),
        ])
        .status()
        .await
        .context("spawn skopeo")?;
    if !status.success() {
        anyhow::bail!("skopeo copy failed: exit {}", status);
    }

    std::fs::create_dir_all(&rootfs_dir)?;
    extract_oci_layers(&image_dir, &rootfs_dir).await?;
    Ok(rootfs_dir)
}

/// Unpack each layer tarball from an OCI image directory into `dst`.
/// We read the index → manifest → layers chain rather than using `umoci` so
/// the surface stays small and dependency-free.
async fn extract_oci_layers(image_dir: &Path, dst: &Path) -> Result<()> {
    let index_path = image_dir.join("index.json");
    let index: serde_json::Value = serde_json::from_slice(
        &std::fs::read(&index_path)
            .with_context(|| format!("read {}", index_path.display()))?,
    )?;
    let manifest_digest = index["manifests"][0]["digest"]
        .as_str()
        .context("index.json: no manifests[0].digest")?;
    let manifest_path = blob_path(image_dir, manifest_digest)?;
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&manifest_path)?)?;
    let layers = manifest["layers"]
        .as_array()
        .context("manifest: no layers array")?;

    for layer in layers {
        let digest = layer["digest"]
            .as_str()
            .context("layer: missing digest")?;
        let path = blob_path(image_dir, digest)?;
        tracing::info!(layer = digest, "extracting");
        let status = Command::new("tar")
            .arg("-xf")
            .arg(&path)
            .arg("-C")
            .arg(dst)
            .status()
            .await
            .context("spawn tar")?;
        if !status.success() {
            anyhow::bail!("tar -xf {} failed: exit {}", path.display(), status);
        }
    }
    Ok(())
}

fn blob_path(image_dir: &Path, digest: &str) -> Result<PathBuf> {
    let (algo, hex) = digest
        .split_once(':')
        .with_context(|| format!("invalid digest `{digest}`"))?;
    Ok(image_dir.join("blobs").join(algo).join(hex))
}
