//! Offline maintenance over any two backends: a verified copy between them
//! (`keryx storage migrate`) and orphan collection (`keryx storage gc`).
//! Both know nothing about providers; they only see `dyn BlobBackend`.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use anyhow::{bail, Result};
use futures_util::StreamExt;
use keryx_core::sha256_hex;

use crate::{BlobBackend, BlobEntry};

/// Every stored object lives under this prefix.
const DRAFTS_PREFIX: &str = "drafts/";

/// An upload writes its blob before its row commits, so for a moment an
/// object has no owner yet. gc never touches anything younger than this.
pub const GC_GRACE: Duration = Duration::from_secs(60 * 60);

/// One version's blob as the database records it.
#[derive(Debug, Clone)]
pub struct BlobRef {
    pub object_key: String,
    pub content_hash: String,
    pub file_size: u64,
}

#[derive(Debug, Clone, Copy)]
pub struct MigrateOptions {
    /// Report what would be copied without writing anything.
    pub dry_run: bool,
    /// Delete from the source, but only once every key has verified.
    pub remove_source: bool,
    pub concurrency: usize,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct MigrateReport {
    pub copied: usize,
    pub skipped: usize,
    /// `(object_key, reason)` for every key that did not verify.
    pub failed: Vec<(String, String)>,
    pub source_removed: bool,
}

enum Outcome {
    Copied,
    Skipped,
    Failed(String, String),
}

/// Copy every referenced blob from `from` to `to`, verifying each against the
/// content hash the database holds. Idempotent: a key already at the
/// destination with the recorded size is skipped, so an interrupted run
/// resumes by re-running.
pub async fn migrate(
    from: Arc<dyn BlobBackend>,
    to: Arc<dyn BlobBackend>,
    refs: Vec<BlobRef>,
    options: MigrateOptions,
) -> Result<MigrateReport> {
    let present: HashMap<String, u64> = to
        .list(DRAFTS_PREFIX)
        .await?
        .into_iter()
        .map(|entry| (entry.key, entry.size))
        .collect();
    let keys: Vec<String> = refs.iter().map(|blob| blob.object_key.clone()).collect();

    let outcomes: Vec<Outcome> = futures_util::stream::iter(refs)
        .map(|blob| {
            let (from, to) = (from.clone(), to.clone());
            let already_there = present.get(&blob.object_key) == Some(&blob.file_size);
            async move {
                if already_there {
                    return Outcome::Skipped;
                }
                if options.dry_run {
                    return Outcome::Copied;
                }
                match copy_verified(&*from, &*to, &blob).await {
                    Ok(()) => Outcome::Copied,
                    Err(error) => Outcome::Failed(blob.object_key, format!("{error:#}")),
                }
            }
        })
        .buffer_unordered(options.concurrency.max(1))
        .collect()
        .await;

    let mut report = MigrateReport::default();
    for outcome in outcomes {
        match outcome {
            Outcome::Copied => report.copied += 1,
            Outcome::Skipped => report.skipped += 1,
            Outcome::Failed(key, reason) => report.failed.push((key, reason)),
        }
    }
    report.failed.sort();

    if options.remove_source && !options.dry_run && report.failed.is_empty() {
        from.remove_many(&keys).await?;
        report.source_removed = true;
    }
    Ok(report)
}

/// Copy one blob, then read it back from the destination and check it against
/// the recorded hash. A mismatch is loud and the key does not count as migrated.
async fn copy_verified(from: &dyn BlobBackend, to: &dyn BlobBackend, blob: &BlobRef) -> Result<()> {
    let Some(html) = from.get(&blob.object_key).await? else {
        bail!("missing from the source {}", from.describe());
    };
    to.put(&blob.object_key, &html).await?;
    let Some(written) = to.get(&blob.object_key).await? else {
        bail!(
            "missing from the destination {} after the copy",
            to.describe()
        );
    };
    let actual = sha256_hex(&written);
    if actual != blob.content_hash {
        bail!(
            "sha256 mismatch at the destination: the database records {}, the copy hashes to {actual}",
            blob.content_hash
        );
    }
    Ok(())
}

#[derive(Debug, Default)]
pub struct GcReport {
    /// Objects with no owning row, old enough to act on.
    pub orphans: Vec<BlobEntry>,
    /// Unowned objects left alone because they are inside the grace window,
    /// or because the backend reported no modification time.
    pub too_young: usize,
    pub deleted: bool,
}

/// Find stored objects that no database row owns. Report-only unless `delete`.
pub async fn gc(
    store: &dyn BlobBackend,
    owned_keys: &HashSet<String>,
    delete: bool,
    now: SystemTime,
) -> Result<GcReport> {
    let mut report = GcReport::default();
    for entry in store.list(DRAFTS_PREFIX).await? {
        if owned_keys.contains(&entry.key) {
            continue;
        }
        let old_enough = entry
            .last_modified
            .and_then(|modified| now.duration_since(modified).ok())
            .is_some_and(|age| age >= GC_GRACE);
        if old_enough {
            report.orphans.push(entry);
        } else {
            report.too_young += 1;
        }
    }
    report.orphans.sort_by(|a, b| a.key.cmp(&b.key));

    if delete && !report.orphans.is_empty() {
        let keys: Vec<String> = report.orphans.iter().map(|e| e.key.clone()).collect();
        store.remove_many(&keys).await?;
        report.deleted = true;
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{create_backend, memory_backend, BackendConfig, DiskConfig};

    async fn disk(dir: &tempfile::TempDir) -> Arc<dyn BlobBackend> {
        create_backend(&BackendConfig::Disk(DiskConfig {
            data_dir: dir.path().to_path_buf(),
        }))
        .await
        .unwrap()
    }

    fn blob_ref(key: &str, html: &str) -> BlobRef {
        BlobRef {
            object_key: key.to_string(),
            content_hash: sha256_hex(html),
            file_size: html.len() as u64,
        }
    }

    const OPTIONS: MigrateOptions = MigrateOptions {
        dry_run: false,
        remove_source: false,
        concurrency: 4,
    };

    #[tokio::test]
    async fn migrate_copies_then_reruns_as_a_no_op_and_removes_the_source_last() {
        let (from_dir, to_dir) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
        let (from, to) = (disk(&from_dir).await, disk(&to_dir).await);
        let refs = vec![
            blob_ref("drafts/a/1.html", "<p>one</p>"),
            blob_ref("drafts/a/2.html", "<p>two</p>"),
            blob_ref("drafts/b/1.html", "<p>three</p>"),
        ];
        for (blob, html) in refs
            .iter()
            .zip(["<p>one</p>", "<p>two</p>", "<p>three</p>"])
        {
            from.put(&blob.object_key, html).await.unwrap();
        }

        let dry = MigrateOptions {
            dry_run: true,
            ..OPTIONS
        };
        let report = migrate(from.clone(), to.clone(), refs.clone(), dry)
            .await
            .unwrap();
        assert_eq!((report.copied, report.skipped), (3, 0));
        assert!(
            to.list("drafts/").await.unwrap().is_empty(),
            "dry run wrote"
        );

        let report = migrate(from.clone(), to.clone(), refs.clone(), OPTIONS)
            .await
            .unwrap();
        assert_eq!(
            (report.copied, report.skipped, report.failed.len()),
            (3, 0, 0)
        );
        assert_eq!(
            to.get("drafts/b/1.html").await.unwrap().as_deref(),
            Some("<p>three</p>")
        );

        let remove = MigrateOptions {
            remove_source: true,
            ..OPTIONS
        };
        let report = migrate(from.clone(), to.clone(), refs, remove)
            .await
            .unwrap();
        assert_eq!((report.copied, report.skipped), (0, 3));
        assert!(report.source_removed);
        assert!(from.list("drafts/").await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn migrate_fails_loudly_on_a_hash_mismatch_and_keeps_the_source() {
        let (from, to) = (memory_backend(), memory_backend());
        from.put("drafts/a/1.html", "<p>good</p>").await.unwrap();
        from.put("drafts/a/2.html", "<p>tampered</p>")
            .await
            .unwrap();
        let refs = vec![
            blob_ref("drafts/a/1.html", "<p>good</p>"),
            blob_ref("drafts/a/2.html", "<p>what the database recorded</p>"),
            blob_ref("drafts/a/3.html", "<p>never stored</p>"),
        ];

        let remove = MigrateOptions {
            remove_source: true,
            ..OPTIONS
        };
        let report = migrate(from.clone(), to, refs, remove).await.unwrap();
        assert_eq!(report.copied, 1);
        assert_eq!(report.failed.len(), 2);
        assert!(report.failed[0].1.contains("sha256 mismatch"));
        assert!(report.failed[1].1.contains("missing from the source"));
        assert!(
            !report.source_removed,
            "a failed run must never delete the source"
        );
        assert_eq!(from.list("drafts/").await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn gc_finds_old_orphans_and_leaves_owned_and_young_objects_alone() {
        let dir = tempfile::tempdir().unwrap();
        let store = disk(&dir).await;
        for key in ["drafts/a/owned.html", "drafts/a/orphan.html"] {
            store.put(key, "<p>x</p>").await.unwrap();
        }
        let owned = HashSet::from(["drafts/a/owned.html".to_string()]);

        // Just written and not yet recorded: inside the grace window.
        let report = gc(&*store, &owned, true, SystemTime::now()).await.unwrap();
        assert!(report.orphans.is_empty());
        assert_eq!(report.too_young, 1);
        assert_eq!(store.list("drafts/").await.unwrap().len(), 2);

        // The same object two hours later is an orphan. Report-only first.
        let later = SystemTime::now() + Duration::from_secs(2 * 60 * 60);
        let report = gc(&*store, &owned, false, later).await.unwrap();
        assert_eq!(report.orphans.len(), 1);
        assert_eq!(report.orphans[0].key, "drafts/a/orphan.html");
        assert!(!report.deleted);
        assert_eq!(store.list("drafts/").await.unwrap().len(), 2);

        let report = gc(&*store, &owned, true, later).await.unwrap();
        assert!(report.deleted);
        let left = store.list("drafts/").await.unwrap();
        assert_eq!(left.len(), 1);
        assert_eq!(left[0].key, "drafts/a/owned.html");
    }
}
