use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};

/// A simple file-backed bucket store. Each key maps to a file under the base
/// directory. Designed for prototype use; not super performant.
pub struct BucketStore {
    base: PathBuf,
}

impl BucketStore {
    /// create a new bucket store rooted at the given directory. directory will
    /// be created if it does not exist.
    pub fn new<P: AsRef<Path>>(base: P) -> anyhow::Result<Self> {
        let base = base.as_ref().to_path_buf();
        fs::create_dir_all(&base)?;
        Ok(BucketStore { base })
    }

    fn path_for(&self, key: &str) -> anyhow::Result<PathBuf> {
        // SECURITY: Validate key doesn't escape base directory
        if key.contains('\0') || key.contains("..") || key.starts_with('/') {
            return Err(anyhow::anyhow!(
                "Invalid bucket key '{}': must not contain path traversal",
                key
            ));
        }

        let path = self.base.join(key);

        // Verify the resolved path is within the base directory
        let canonical_base = self
            .base
            .canonicalize()
            .unwrap_or_else(|_| self.base.clone());
        let canonical_path = path.canonicalize().unwrap_or_else(|_| {
            // For new files, check the parent
            let mut ancestor = path.as_path();
            while !ancestor.exists() {
                ancestor = match ancestor.parent() {
                    Some(p) => p,
                    None => return path.clone(),
                };
            }
            ancestor.canonicalize().unwrap_or_else(|_| path.clone())
        });

        if !canonical_path.starts_with(&canonical_base) {
            return Err(anyhow::anyhow!(
                "Bucket key '{}' resolves outside base directory",
                key
            ));
        }

        Ok(path)
    }

    /// write a value to the bucket. overwrites any existing content.
    pub fn put(&mut self, key: &str, value: &str) -> anyhow::Result<()> {
        let p = self.path_for(key)?;
        if let Some(parent) = p.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(p, value)?;
        Ok(())
    }

    /// read a value from the bucket, if it exists.
    pub fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let p = self.path_for(key)?;
        if !p.exists() {
            return Ok(None);
        }
        let mut s = String::new();
        fs::File::open(p)?.read_to_string(&mut s)?;
        Ok(Some(s))
    }

    /// delete a key from the bucket
    pub fn delete(&self, key: &str) -> anyhow::Result<bool> {
        let p = self.path_for(key)?;
        if p.exists() {
            fs::remove_file(p)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    /// list all keys in the bucket
    pub fn list_keys(&self) -> anyhow::Result<Vec<String>> {
        let mut keys = Vec::new();
        if self.base.exists() {
            for entry in fs::read_dir(&self.base)? {
                let entry = entry?;
                if entry.file_type()?.is_file() {
                    if let Some(name) = entry.file_name().to_str() {
                        keys.push(name.to_string());
                    }
                }
            }
        }
        Ok(keys)
    }
}
