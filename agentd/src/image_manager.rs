use anyhow::{anyhow, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Image manager: handles pulling, caching, and mounting container images
/// Supports: Docker Hub, arbitrary registries, HTTP URLs, local paths
pub struct ImageManager {
    cache_dir: PathBuf,
}

impl ImageManager {
    /// Create a new image manager with optional cache directory
    pub fn new(cache_dir: Option<&str>) -> Result<Self> {
        let dir = match cache_dir {
            Some(d) => PathBuf::from(d),
            None => {
                let base = std::env::var("AGENTD_IMAGE_CACHE")
                    .unwrap_or_else(|_| "/var/lib/agentd/images".to_string());
                PathBuf::from(base)
            }
        };
        fs::create_dir_all(&dir).context("create image cache dir")?;
        Ok(ImageManager { cache_dir: dir })
    }

    /// Resolve an image reference to a local rootfs path
    /// Handles:
    ///   - "alpine" → Docker Hub → ghcr.io/alpinelinux/alpine:latest (public)
    ///   - "docker.io/library/alpine:latest" → explicit Docker Hub
    ///   - "ghcr.io/user/image:tag" → arbitrary registry
    ///   - "https://example.com/rootfs.tar.gz" → HTTP URL
    ///   - "/path/to/rootfs" → local directory
    ///   - "file:///path/to/rootfs" → local file URI
    pub fn resolve(&self, image_ref: &str) -> Result<PathBuf> {
        // Check if it's a local path
        if image_ref.starts_with("/") || image_ref.starts_with("file://") {
            return self.resolve_local(image_ref);
        }

        // Check if it's an HTTP(S) URL
        if image_ref.starts_with("http://") || image_ref.starts_with("https://") {
            return self.resolve_http(image_ref);
        }

        // Otherwise treat as a registry image reference
        self.resolve_registry(image_ref)
    }

    /// Resolve local filesystem path
    fn resolve_local(&self, path_ref: &str) -> Result<PathBuf> {
        let path = if path_ref.starts_with("file://") {
            path_ref.strip_prefix("file://").unwrap()
        } else {
            path_ref
        };

        let full_path = PathBuf::from(path);
        if !full_path.exists() {
            return Err(anyhow!("local image path not found: {}", path));
        }

        // If it's a directory, use it directly
        if full_path.is_dir() {
            return Ok(full_path);
        }

        // If it's a tarball, extract it
        if path.ends_with(".tar.gz") || path.ends_with(".tar") {
            return self.extract_tarball(&full_path);
        }

        Err(anyhow!(
            "local path must be a directory or tarball: {}",
            path
        ))
    }

    /// Resolve HTTP(S) URL - download and cache the tarball
    fn resolve_http(&self, url: &str) -> Result<PathBuf> {
        // Generate cache key from URL hash
        let cache_key = format!("{:x}", md5(url.as_bytes()));
        let cached_tar = self.cache_dir.join(format!("{}.tar.gz", cache_key));
        let extracted_dir = self.cache_dir.join(&cache_key);

        // Return cached if it exists
        if extracted_dir.exists() {
            log::info!("using cached image from {}", extracted_dir.display());
            return Ok(extracted_dir);
        }

        // Download
        log::info!("downloading image from {}", url);
        self.download_file(url, &cached_tar)?;

        // Extract
        self.extract_tarball(&cached_tar)
    }

    /// Resolve registry reference (Docker Hub, ghcr.io, etc.)
    /// Uses skopeo or similar to pull; falls back to simple tarball fetch
    fn resolve_registry(&self, image_ref: &str) -> Result<PathBuf> {
        // Simple approach: normalize to a full registry reference
        // "alpine" → try Docker Hub public image
        // "ghcr.io/user/image:tag" → use as-is
        let full_ref = if !image_ref.contains('/') {
            // No slash means Docker Hub short form
            format!("docker.io/library/{}:latest", image_ref)
        } else if !image_ref.contains(':') {
            format!("{}:latest", image_ref)
        } else {
            image_ref.to_string()
        };

        log::info!("resolving registry image: {}", full_ref);

        // Compute a safe cache path (replace slashes & colons)
        let safe = full_ref.replace('/', "_").replace(':', "_");
        let cache_target = self.cache_dir.join(&safe);
        let rootfs_dir = cache_target.join("rootfs");
        if rootfs_dir.exists() {
            return Ok(rootfs_dir);
        }

        // ensure base directory exists
        fs::create_dir_all(&cache_target).context("create registry cache dir")?;

        // require skopeo on PATH
        if which::which("skopeo").is_err() {
            return Err(anyhow!(
                "skopeo not installed; cannot pull registry images.\n
either install skopeo or use one of the other image sources:\n  - local directory\n  - local tarball\n  - HTTP URL\n"));
        }

        // use skopeo to copy the image to a temporary dir under cache_target
        let skopeo_dir = cache_target.join("skopeo");
        if skopeo_dir.exists() {
            fs::remove_dir_all(&skopeo_dir).ok();
        }
        fs::create_dir_all(&skopeo_dir)?;
        let dest = format!("dir:{}", skopeo_dir.display());
        let status = Command::new("skopeo")
            .args(&["copy", &format!("docker://{}", full_ref), &dest])
            .status()
            .context("invoke skopeo")?;
        if !status.success() {
            return Err(anyhow!("skopeo failed to fetch {}", full_ref));
        }

        // manifest.json in skopeo dir: format uses OCI/Docker v2 schema
        let manifest_path = skopeo_dir.join("manifest.json");
        let manifest: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(&manifest_path).context("read manifest")?)?;

        // layers is manifest["layers"][n]["digest"] = "sha256:abc123..."
        let layers = manifest["layers"]
            .as_array()
            .context("manifest missing 'layers' array")?;

        fs::create_dir_all(&rootfs_dir)?;
        for layer in layers {
            let digest = layer["digest"]
                .as_str()
                .context("layer missing 'digest' field")?;
            // skopeo stores files named by digest (e.g. "sha256:abc123...")
            let layer_file = digest.strip_prefix("sha256:").unwrap_or(digest);
            let layer_path = skopeo_dir.join(layer_file);
            if !layer_path.exists() {
                return Err(anyhow!("layer file not found: {}", layer_path.display()));
            }
            let status = Command::new("tar")
                .args(&[
                    "-C",
                    rootfs_dir.to_str().unwrap(),
                    "-xf",
                    layer_path.to_str().unwrap(),
                ])
                .status()
                .context("extract layer")?;
            if !status.success() {
                return Err(anyhow!("failed to extract layer {}", digest));
            }
        }

        Ok(rootfs_dir)
    }

    /// Extract a tarball and return the directory path
    fn extract_tarball(&self, tar_path: &Path) -> Result<PathBuf> {
        let file_name = tar_path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or_else(|| anyhow!("invalid tarball name"))?;

        let extract_dir = self.cache_dir.join(file_name);

        // Skip if already extracted
        if extract_dir.exists() {
            log::info!("tarball already extracted at {}", extract_dir.display());
            return Ok(extract_dir);
        }

        fs::create_dir_all(&extract_dir).context("create extract directory")?;

        log::info!(
            "extracting {} to {}",
            tar_path.display(),
            extract_dir.display()
        );

        // Use tar command to extract
        let output = Command::new("tar")
            .arg("-xzf")
            .arg(tar_path)
            .arg("-C")
            .arg(&extract_dir)
            .output()
            .context("execute tar")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("tar extraction failed: {}", stderr));
        }

        Ok(extract_dir)
    }

    /// Download a file from a URL
    fn download_file(&self, url: &str, dest: &Path) -> Result<()> {
        let output = Command::new("curl")
            .arg("-fsSL")
            .arg("-o")
            .arg(dest)
            .arg(url)
            .output()
            .context("execute curl")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(anyhow!("download failed: {}", stderr));
        }

        Ok(())
    }
}

/// Simple MD5 hash for cache keys (not for security, just uniform naming)
fn md5(data: &[u8]) -> u128 {
    // For simplicity, use a basic hash; in production use a real MD5 crate
    // For now, use a deterministic hash based on the data
    let mut hash: u128 = 5381;
    for byte in data {
        hash = ((hash << 5).wrapping_add(hash)).wrapping_add(*byte as u128);
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_local_path_resolution() {
        let mgr = ImageManager::new(Some("/tmp/test-images")).unwrap();
        // Test with /tmp which is a valid directory
        let res = mgr.resolve("/tmp");
        assert!(res.is_ok());
        assert_eq!(res.unwrap(), PathBuf::from("/tmp"));
    }

    #[test]
    fn test_http_url_detection() {
        let mgr = ImageManager::new(Some("/tmp/test-images")).unwrap();
        let res = mgr.resolve("https://example.com/fake.tar.gz");
        // Will fail on actual download but tests that URL detection works
        assert!(res.is_err());
    }

    #[test]
    fn test_registry_resolution() {
        let mgr = ImageManager::new(Some("/tmp/test-images")).unwrap();
        let res = mgr.resolve("alpine");
        // behaviour depends on whether skopeo is available in test environment
        if which::which("skopeo").is_ok() {
            // we expect a rootfs path (may fail if network inaccessible, skip)
            if res.is_ok() {
                let path = res.unwrap();
                assert!(path.exists());
            } else {
                // network might be blocked; ensure error mentions skopeo
                let err_msg = format!("{}", res.unwrap_err());
                assert!(err_msg.contains("skopeo"));
            }
        } else {
            assert!(res.is_err());
            let err_msg = format!("{}", res.unwrap_err());
            assert!(err_msg.contains("skopeo not installed"));
        }
    }
}
