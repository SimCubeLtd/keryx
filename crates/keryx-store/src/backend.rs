//! The blob storage seam. Keryx holds an `Arc<dyn BlobBackend>` and never
//! names a provider; [`OpenDalBackend`] covers disk and S3 through OpenDAL,
//! and anything OpenDAL lacks can implement the trait directly.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;

use anyhow::{Context, Result};
use async_trait::async_trait;
use opendal_core::{ErrorKind, Operator};
use opendal_service_fs::Fs;

/// Staging directory for atomic disk writes, under the data directory. It sits
/// outside `drafts/`, so temp files never show up in a listing.
const STAGING_DIR: &str = ".staging";

/// One stored object, as `list` reports it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobEntry {
    pub key: String,
    pub size: u64,
    /// `storage gc` uses this for its grace window. A backend that cannot
    /// report it answers `None`, and gc then leaves the object alone.
    pub last_modified: Option<SystemTime>,
}

/// Opaque byte objects addressed by key. Absence is not an error:
/// `get` answers None so callers take a clean miss path without
/// inspecting provider-specific error codes.
#[async_trait]
pub trait BlobBackend: Send + Sync {
    async fn put(&self, key: &str, html: &str) -> Result<()>;
    async fn get(&self, key: &str) -> Result<Option<String>>;
    /// Removing a key that does not exist succeeds, so purge stays idempotent.
    async fn remove_many(&self, keys: &[String]) -> Result<()>;
    /// Every object under `prefix`, recursively.
    async fn list(&self, prefix: &str) -> Result<Vec<BlobEntry>>;
    /// Startup check: write and delete one probe object, so a wrong bucket,
    /// missing credentials or a read-only credential fail at boot.
    async fn probe(&self) -> Result<()>;
    /// Human-readable location, such as `s3://bucket/prefix`.
    fn describe(&self) -> &str;
}

/// Keys are built from internally generated ids only, so they are always
/// safe relative paths. Backend-neutral: the same key works on every backend,
/// so switching backends never rewrites a database row.
pub fn object_key(draft_id: &str, version_id: &str) -> String {
    // OpenDAL 0.58.2 does not itself reject hostile keys, so nothing but an
    // internal id may ever reach this function.
    debug_assert!(
        [draft_id, version_id]
            .iter()
            .all(|id| !id.is_empty() && id.bytes().all(|b| b.is_ascii_alphanumeric())),
        "object keys are built from internal alphanumeric ids only"
    );
    format!("drafts/{draft_id}/{version_id}.html")
}

#[derive(Debug, Clone)]
pub struct DiskConfig {
    /// The Keryx data directory. Blobs live under `<data_dir>/drafts/`.
    pub data_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct S3Config {
    pub bucket: String,
    pub region: String,
    /// Custom endpoint for S3-compatible stores. Falls back to
    /// `AWS_ENDPOINT_URL_S3`, then AWS.
    pub endpoint: Option<String>,
    /// Key prefix inside the bucket, applied as the operator root. Never
    /// stored in the database.
    pub prefix: String,
    /// Named AWS profile for credential lookup.
    pub profile: Option<String>,
}

#[derive(Debug, Clone)]
pub enum BackendConfig {
    Disk(DiskConfig),
    S3(S3Config),
}

/// Build the backend named by `config`.
///
/// `Arc` rather than `Box`: `storage migrate` copies concurrently and needs an
/// owned `'static` handle per task.
pub async fn create_backend(config: &BackendConfig) -> Result<Arc<dyn BlobBackend>> {
    let backend = match config {
        BackendConfig::Disk(config) => OpenDalBackend {
            operator: create_disk_operator(&config.data_dir)?,
            description: format!("file://{}", config.data_dir.display()),
            tidies_directories: true,
        },
        #[cfg(feature = "s3")]
        BackendConfig::S3(config) => OpenDalBackend {
            operator: crate::s3::create_s3_operator(config)?,
            description: crate::s3::describe(config),
            tidies_directories: false,
        },
        #[cfg(not(feature = "s3"))]
        BackendConfig::S3(_) => {
            anyhow::bail!("this keryx binary was built without S3 support (the `s3` feature)")
        }
    };
    Ok(Arc::new(backend))
}

/// An in-memory backend for tests. Nothing to clean up afterwards.
#[cfg(any(test, feature = "test-support"))]
pub fn memory_backend() -> Arc<dyn BlobBackend> {
    let operator = Operator::new(opendal_core::services::Memory::default())
        .expect("building OpenDAL memory operator");
    Arc::new(OpenDalBackend {
        operator,
        description: "memory://".to_string(),
        tidies_directories: false,
    })
}

/// Disk and S3 both run through this one implementation.
pub struct OpenDalBackend {
    operator: Operator,
    description: String,
    /// Disk only: OpenDAL's fs delete leaves the emptied `drafts/<id>/`
    /// directory behind, so `remove_many` tidies it. Object stores have no
    /// directories, and the extra request would be pure cost.
    tidies_directories: bool,
}

#[async_trait]
impl BlobBackend for OpenDalBackend {
    async fn put(&self, key: &str, html: &str) -> Result<()> {
        self.operator
            .write(key, html.as_bytes().to_vec())
            .await
            .map(|_| ())
            .with_context(|| format!("writing blob {}/{key}", self.description))
    }

    async fn get(&self, key: &str) -> Result<Option<String>> {
        match self.operator.read(key).await {
            Ok(buffer) => String::from_utf8(buffer.to_vec())
                .map(Some)
                .with_context(|| format!("blob {}/{key} is not valid UTF-8", self.description)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => {
                Err(error).with_context(|| format!("reading blob {}/{key}", self.description))
            }
        }
    }

    async fn remove_many(&self, keys: &[String]) -> Result<()> {
        // OpenDAL batches this where the service can: S3 DeleteObjects takes
        // up to 1000 keys per request.
        self.operator
            .delete_iter(keys.iter().cloned())
            .await
            .with_context(|| format!("removing {} blobs from {}", keys.len(), self.description))?;

        if self.tidies_directories {
            let mut directories: Vec<&str> = keys
                .iter()
                .filter_map(|key| key.rfind('/').map(|end| &key[..=end]))
                .collect();
            directories.sort_unstable();
            directories.dedup();
            for directory in directories {
                // Best effort: this only succeeds once the directory is empty.
                let _ = self.operator.delete(directory).await;
            }
        }
        Ok(())
    }

    async fn list(&self, prefix: &str) -> Result<Vec<BlobEntry>> {
        let entries = match self.operator.list_with(prefix).recursive(true).await {
            Ok(entries) => entries,
            Err(error) if error.kind() == ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => {
                return Err(error).with_context(|| format!("listing {}/{prefix}", self.description))
            }
        };
        Ok(entries
            .into_iter()
            .filter(|entry| entry.metadata().is_file())
            .map(|entry| BlobEntry {
                key: entry.path().to_string(),
                size: entry.metadata().content_length(),
                last_modified: entry.metadata().last_modified().map(SystemTime::from),
            })
            .collect())
    }

    async fn probe(&self) -> Result<()> {
        let key = format!(".keryx-probe-{}", keryx_core::ids::new_internal_id());
        self.operator
            .write(&key, b"keryx startup probe".to_vec())
            .await
            .with_context(|| format!("blob store {} is not writable", self.description))?;
        self.operator
            .delete(&key)
            .await
            .with_context(|| format!("blob store {} does not allow deletes", self.description))
    }

    fn describe(&self) -> &str {
        &self.description
    }
}

/// The fs operator is rooted at the data directory, so files written by
/// earlier Keryx versions read in place. `atomic_write_dir` is mandatory:
/// with it OpenDAL writes a temp file, syncs, then renames; without it the
/// write happens in place and a crash can leave a truncated draft.
fn create_disk_operator(data_dir: &Path) -> Result<Operator> {
    let staging = data_dir.join(STAGING_DIR);
    // OpenDAL never cleans temp files left by a crash, so start empty.
    match std::fs::remove_dir_all(&staging) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(error)
                .with_context(|| format!("emptying staging directory {}", staging.display()))
        }
    }
    std::fs::create_dir_all(&staging)
        .with_context(|| format!("creating staging directory {}", staging.display()))?;
    verify_same_filesystem(data_dir, &staging)?;

    let root = data_dir
        .to_str()
        .context("data directory path is not valid UTF-8")?;
    let staging = staging
        .to_str()
        .context("staging directory path is not valid UTF-8")?;
    let builder = Fs::default().root(root).atomic_write_dir(staging);
    // No retry layer, deliberately: a failed write surfaces to the caller.
    Operator::new(builder).context("building OpenDAL filesystem operator")
}

/// Publishing renames from the staging directory onto the final path, and
/// `rename(2)` cannot cross a mount point, so a staging directory on another
/// filesystem would fail every write with `EXDEV`. A heuristic, not a proof:
/// being wrong only means the clearer error comes from the write instead.
#[cfg(unix)]
fn verify_same_filesystem(root: &Path, staging: &Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let device_of = |path: &Path| -> Option<u64> {
        let existing = path.ancestors().find(|candidate| candidate.exists())?;
        std::fs::metadata(existing).ok().map(|meta| meta.dev())
    };
    let (Some(root_device), Some(staging_device)) = (device_of(root), device_of(staging)) else {
        return Ok(());
    };
    if root_device != staging_device {
        anyhow::bail!(
            "staging directory {} is on a different filesystem than the data directory {}; \
             blob writes rename between them, which fails with EXDEV",
            staging.display(),
            root.display()
        );
    }
    Ok(())
}

#[cfg(not(unix))]
fn verify_same_filesystem(_root: &Path, _staging: &Path) -> Result<()> {
    // No portable device id without extra syscalls; the write's own error stands.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY: &str = "drafts/abc123def456/V1aB2cD3eF4gH5iJ6kL7.html";

    async fn disk_backend(data_dir: &Path) -> Arc<dyn BlobBackend> {
        create_backend(&BackendConfig::Disk(DiskConfig {
            data_dir: data_dir.to_path_buf(),
        }))
        .await
        .unwrap()
    }

    /// The contract every backend must meet. Runs against memory and disk.
    async fn conformance(backend: Arc<dyn BlobBackend>) {
        assert_eq!(backend.get(KEY).await.unwrap(), None);
        assert!(backend.list("drafts/").await.unwrap().is_empty());

        backend.put(KEY, "<p>héllo</p>").await.unwrap();
        assert_eq!(
            backend.get(KEY).await.unwrap().as_deref(),
            Some("<p>héllo</p>")
        );
        backend.put(KEY, "<p>updated</p>").await.unwrap();
        assert_eq!(
            backend.get(KEY).await.unwrap().as_deref(),
            Some("<p>updated</p>")
        );

        let listed = backend.list("drafts/").await.unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key, KEY);
        assert_eq!(listed[0].size, "<p>updated</p>".len() as u64);

        backend.probe().await.unwrap();
        assert_eq!(
            backend.list("").await.unwrap().len(),
            1,
            "probe left debris"
        );

        let keys = vec![KEY.to_string()];
        backend.remove_many(&keys).await.unwrap();
        backend.remove_many(&keys).await.unwrap();
        assert_eq!(backend.get(KEY).await.unwrap(), None);
    }

    #[tokio::test]
    async fn memory_backend_meets_the_contract() {
        conformance(memory_backend()).await;
    }

    #[tokio::test]
    async fn disk_backend_meets_the_contract() {
        let data_dir = tempfile::tempdir().unwrap();
        conformance(disk_backend(data_dir.path()).await).await;
    }

    #[test]
    fn object_key_shape_is_unchanged() {
        assert_eq!(object_key("abc123def456", "V1aB2cD3eF4gH5iJ6kL7"), KEY);
    }

    #[tokio::test]
    async fn disk_backend_uses_real_nested_paths_staging_and_modification_times() {
        let data_dir = tempfile::tempdir().unwrap();
        let backend = disk_backend(data_dir.path()).await;

        backend.put(KEY, "<p>on disk</p>").await.unwrap();
        assert_eq!(
            std::fs::read_to_string(data_dir.path().join(KEY)).unwrap(),
            "<p>on disk</p>"
        );
        assert!(data_dir.path().join(STAGING_DIR).is_dir());

        let listed = backend.list("drafts/").await.unwrap();
        let modified = listed[0]
            .last_modified
            .expect("disk reports modification times");
        assert!(modified.elapsed().unwrap() < std::time::Duration::from_secs(60));
    }

    #[tokio::test]
    async fn disk_backend_tidies_the_draft_directory_and_empties_staging_at_startup() {
        let data_dir = tempfile::tempdir().unwrap();
        let debris = data_dir.path().join(STAGING_DIR).join("crashed-write.tmp");
        std::fs::create_dir_all(debris.parent().unwrap()).unwrap();
        std::fs::write(&debris, "partial").unwrap();

        let backend = disk_backend(data_dir.path()).await;
        assert!(!debris.exists(), "startup must empty the staging directory");

        backend.put(KEY, "<p>soon gone</p>").await.unwrap();
        backend.remove_many(&[KEY.to_string()]).await.unwrap();
        assert!(!data_dir.path().join("drafts/abc123def456").exists());
    }

    /// A reader must never observe a torn object, which is what same-filesystem
    /// staging plus an atomic rename buys.
    #[tokio::test(flavor = "multi_thread")]
    async fn disk_concurrent_puts_of_one_key_never_tear() {
        let data_dir = tempfile::tempdir().unwrap();
        let backend = disk_backend(data_dir.path()).await;

        // Large enough that a non-atomic writer would be caught mid-write.
        const BODY: usize = 512 * 1024;
        let payload = "p".repeat(BODY);

        let writers: Vec<_> = (0..8)
            .map(|_| {
                let (backend, payload) = (backend.clone(), payload.clone());
                tokio::spawn(async move { backend.put(KEY, &payload).await })
            })
            .collect();
        let reader = {
            let backend = backend.clone();
            tokio::spawn(async move {
                let mut observed = Vec::new();
                for _ in 0..64 {
                    if let Some(html) = backend.get(KEY).await.unwrap() {
                        observed.push(html.len());
                    }
                    tokio::task::yield_now().await;
                }
                observed
            })
        };

        for writer in writers {
            writer
                .await
                .unwrap()
                .expect("every concurrent writer must succeed");
        }
        for len in reader.await.unwrap() {
            assert_eq!(len, BODY, "a reader observed a torn object ({len} bytes)");
        }
        assert_eq!(backend.get(KEY).await.unwrap().unwrap(), payload);

        let staged: Vec<_> = std::fs::read_dir(data_dir.path().join(STAGING_DIR))
            .unwrap()
            .flatten()
            .map(|entry| entry.path())
            .collect();
        assert!(staged.is_empty(), "staging left debris: {staged:?}");
    }
}
