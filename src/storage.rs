//! On-disk blob storage for draft HTML. SQLite stays the metadata index;
//! the documents themselves live as plain files under the data directory,
//! keeping the database small and the bytes easy to inspect or back up.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

pub struct BlobStore {
    root: PathBuf,
}

impl BlobStore {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Keys are built from internally generated alphanumeric ids only, so
    /// they are always safe relative paths.
    pub fn object_key(draft_id: &str, version_id: &str) -> String {
        format!("drafts/{draft_id}/{version_id}.html")
    }

    fn path_for(&self, key: &str) -> PathBuf {
        self.root.join(key)
    }

    /// Write-then-rename so a crash mid-write never leaves a truncated draft
    /// behind the recorded object key.
    pub fn put(&self, key: &str, html: &str) -> Result<()> {
        let path = self.path_for(key);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("creating blob directory {}", parent.display()))?;
        }
        let tmp = path.with_extension("html.tmp");
        std::fs::write(&tmp, html).with_context(|| format!("writing blob {}", tmp.display()))?;
        std::fs::rename(&tmp, &path)
            .with_context(|| format!("finalizing blob {}", path.display()))?;
        Ok(())
    }

    pub fn get(&self, key: &str) -> Result<String> {
        let path = self.path_for(key);
        std::fs::read_to_string(&path).with_context(|| format!("reading blob {}", path.display()))
    }

    /// Remove a blob (missing files are fine — purge must be idempotent),
    /// then tidy the per-draft directory if it is now empty.
    pub fn remove(&self, key: &str) -> Result<()> {
        let path = self.path_for(key);
        match std::fs::remove_file(&path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e).with_context(|| format!("removing blob {}", path.display())),
        }
        if let Some(parent) = path.parent() {
            let _ = std::fs::remove_dir(parent); // only succeeds when empty
        }
        Ok(())
    }
}

#[cfg(test)]
pub fn test_store() -> BlobStore {
    let root = std::env::temp_dir()
        .join("keryx-tests")
        .join(crate::ids::new_internal_id());
    BlobStore::new(root)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_get_round_trip() {
        let store = test_store();
        let key = BlobStore::object_key("abc123def456", "V1aB2cD3eF4gH5iJ6kL7");
        store.put(&key, "<p>hello</p>").unwrap();
        assert_eq!(store.get(&key).unwrap(), "<p>hello</p>");
        std::fs::remove_dir_all(store.root()).ok();
    }
}
