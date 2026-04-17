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

    fn path_for(&self, key: &str) -> PathBuf {
        self.base.join(key)
    }

    /// write a value to the bucket. overwrites any existing content.
    pub fn put(&mut self, key: &str, value: &str) -> anyhow::Result<()> {
        let p = self.path_for(key);
        fs::write(p, value)?;
        Ok(())
    }

    /// read a value from the bucket, if it exists.
    pub fn get(&self, key: &str) -> anyhow::Result<Option<String>> {
        let p = self.path_for(key);
        if !p.exists() {
            return Ok(None);
        }
        let mut s = String::new();
        fs::File::open(p)?.read_to_string(&mut s)?;
        Ok(Some(s))
    }
}
