use anyhow::Result;
use std::fs;
use std::path::PathBuf;

pub struct Cache {
    dir: PathBuf,
}

impl Cache {
    pub fn new() -> Result<Self> {
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("ai-assist");
        fs::create_dir_all(&cache_dir)?;
        Ok(Cache { dir: cache_dir })
    }

    pub fn get(&self, key: &str) -> Option<String> {
        let hash = blake3::hash(key.as_bytes()).to_hex().to_string();
        let path = self.dir.join(hash);
        if path.exists() {
            fs::read_to_string(path).ok()
        } else {
            None
        }
    }

    pub fn put(&self, key: &str, value: &str) -> Result<()> {
        let hash = blake3::hash(key.as_bytes()).to_hex().to_string();
        let path = self.dir.join(hash);
        fs::write(path, value)?;
        Ok(())
    }
}
