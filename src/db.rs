//! SQLite persistence for draft/version metadata. The HTML bytes themselves
//! live on disk (see storage.rs); each version row records the blob's
//! object key.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::ids::{new_draft_id, new_internal_id};
use crate::storage::BlobStore;
use crate::types::{DraftSummary, UploadMetadata, VersionInfo};

pub fn now() -> String {
    Utc::now().to_rfc3339_opts(SecondsFormat::Millis, true)
}

pub fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating database directory {}", parent.display()))?;
    }
    let conn =
        Connection::open(path).with_context(|| format!("opening database {}", path.display()))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    init(&conn)?;
    Ok(conn)
}

fn init(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS drafts (
            id TEXT PRIMARY KEY,
            title TEXT NOT NULL,
            description TEXT,
            current_version_id TEXT,
            repo_org TEXT,
            repo_name TEXT,
            repo_host TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL,
            deleted_at TEXT,
            disabled_at TEXT,
            disabled_reason TEXT
        );

        CREATE TABLE IF NOT EXISTS draft_versions (
            id TEXT PRIMARY KEY,
            draft_id TEXT NOT NULL REFERENCES drafts(id),
            version_number INTEGER NOT NULL,
            object_key TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            file_size INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            source_ip TEXT,
            user_agent TEXT,
            cli_version TEXT,
            git_branch TEXT,
            git_commit_sha TEXT,
            git_commit_subject TEXT,
            git_dirty INTEGER,
            original_filename TEXT,
            has_inline_script INTEGER NOT NULL DEFAULT 0,
            external_image_hosts TEXT NOT NULL DEFAULT '[]',
            UNIQUE (draft_id, version_number)
        );

        CREATE INDEX IF NOT EXISTS draft_versions_draft_id_idx ON draft_versions(draft_id);
        CREATE INDEX IF NOT EXISTS drafts_updated_at_idx ON drafts(updated_at);
        "#,
    )?;
    Ok(())
}

pub struct NewUpload<'a> {
    pub html: &'a str,
    pub filename: Option<String>,
    pub draft_id: Option<String>,
    pub description: Option<String>,
    pub title_from_html: Option<String>,
    pub metadata: &'a UploadMetadata,
    pub source_ip: Option<String>,
    pub user_agent: Option<String>,
    pub has_inline_script: bool,
    pub external_image_hosts: &'a [String],
}

pub struct UploadOutcome {
    pub draft_id: String,
    pub version_id: String,
    pub version_number: i64,
    pub title: String,
    pub created: bool,
}

#[derive(Debug)]
pub enum UploadError {
    DraftNotFound,
    Other(anyhow::Error),
}

impl std::fmt::Display for UploadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            UploadError::DraftNotFound => write!(f, "Draft not found."),
            UploadError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for UploadError {}

impl From<rusqlite::Error> for UploadError {
    fn from(e: rusqlite::Error) -> Self {
        UploadError::Other(e.into())
    }
}

pub fn record_upload(
    conn: &mut Connection,
    store: &BlobStore,
    upload: NewUpload,
) -> Result<UploadOutcome, UploadError> {
    let tx = conn.transaction()?;
    let timestamp = now();

    let existing: Option<(String, String)> = match &upload.draft_id {
        Some(id) => tx
            .query_row(
                "SELECT id, title FROM drafts WHERE id = ?1 AND deleted_at IS NULL",
                params![id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?,
        None => None,
    };

    if upload.draft_id.is_some() && existing.is_none() {
        return Err(UploadError::DraftNotFound);
    }

    let (draft_id, created) = match &existing {
        Some((id, _)) => (id.clone(), false),
        None => (new_draft_id(), true),
    };

    let version_number: i64 = if created {
        1
    } else {
        tx.query_row(
            "SELECT COALESCE(MAX(version_number), 0) + 1 FROM draft_versions WHERE draft_id = ?1",
            params![draft_id],
            |row| row.get(0),
        )?
    };

    let title = upload
        .title_from_html
        .clone()
        .or_else(|| existing.as_ref().map(|(_, t)| t.clone()))
        .or_else(|| upload.filename.clone())
        .unwrap_or_else(|| "Untitled Draft".to_string());

    let version_id = new_internal_id();
    let content_hash = crate::sha256_hex(upload.html);
    let file_size = upload.html.len() as i64;
    let image_hosts_json = serde_json::to_string(upload.external_image_hosts)
        .map_err(|e| UploadError::Other(e.into()))?;
    let m = upload.metadata;

    // Write the blob before the metadata commits: a failure here aborts the
    // transaction, and a crash after it leaves only an orphan file, never a
    // version row pointing at nothing.
    let object_key = BlobStore::object_key(&draft_id, &version_id);
    store
        .put(&object_key, upload.html)
        .map_err(UploadError::Other)?;

    if created {
        tx.execute(
            r#"
            INSERT INTO drafts (id, title, description, repo_org, repo_name, repo_host, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)
            "#,
            params![
                draft_id,
                title,
                upload.description,
                m.repo_org,
                m.repo_name,
                m.repo_host,
                timestamp
            ],
        )?;
    }

    tx.execute(
        r#"
        INSERT INTO draft_versions (
            id, draft_id, version_number, object_key, content_hash, file_size, created_at,
            source_ip, user_agent, cli_version, git_branch, git_commit_sha,
            git_commit_subject, git_dirty, original_filename, has_inline_script,
            external_image_hosts
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)
        "#,
        params![
            version_id,
            draft_id,
            version_number,
            object_key,
            content_hash,
            file_size,
            timestamp,
            upload.source_ip,
            upload.user_agent,
            m.cli_version,
            m.git_branch,
            m.git_commit_sha,
            m.git_commit_subject,
            m.git_dirty,
            upload.filename,
            upload.has_inline_script,
            image_hosts_json
        ],
    )?;

    tx.execute(
        r#"
        UPDATE drafts
        SET current_version_id = ?1,
            title = ?2,
            description = COALESCE(?3, description),
            repo_org = COALESCE(?4, repo_org),
            repo_name = COALESCE(?5, repo_name),
            repo_host = COALESCE(?6, repo_host),
            updated_at = ?7
        WHERE id = ?8
        "#,
        params![
            version_id,
            title,
            upload.description,
            m.repo_org,
            m.repo_name,
            m.repo_host,
            timestamp,
            draft_id
        ],
    )?;

    tx.commit()?;

    Ok(UploadOutcome {
        draft_id,
        version_id,
        version_number,
        title,
        created,
    })
}

pub struct ServedVersion {
    pub draft_id: String,
    pub version_number: i64,
    pub object_key: String,
}

/// Look up a publicly servable draft version: the draft must exist and be
/// neither deleted nor disabled. `version` of None means the current version.
pub fn find_public_version(
    conn: &Connection,
    draft_id: &str,
    version: Option<i64>,
) -> Result<Option<ServedVersion>> {
    let current: Option<(String, Option<String>)> = conn
        .query_row(
            r#"
            SELECT id, current_version_id
            FROM drafts
            WHERE id = ?1 AND deleted_at IS NULL AND disabled_at IS NULL
            "#,
            params![draft_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;

    let Some((draft_id, current_version_id)) = current else {
        return Ok(None);
    };

    let row = match version {
        Some(n) => conn
            .query_row(
                "SELECT version_number, object_key FROM draft_versions WHERE draft_id = ?1 AND version_number = ?2",
                params![draft_id, n],
                |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()?,
        None => match current_version_id {
            Some(version_id) => conn
                .query_row(
                    "SELECT version_number, object_key FROM draft_versions WHERE id = ?1",
                    params![version_id],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?)),
                )
                .optional()?,
            None => None,
        },
    };

    Ok(row.map(|(version_number, object_key)| ServedVersion {
        draft_id,
        version_number,
        object_key,
    }))
}

/// Every live draft, newest first, with the aggregates the dashboard, CLI, and
/// TUI need. `public_url`/`raw_url` are filled in by the server layer.
pub fn list_drafts(conn: &Connection) -> Result<Vec<DraftSummary>> {
    let mut statement = conn.prepare(
        r#"
        SELECT
            d.id, d.title, d.description, d.repo_org, d.repo_name, d.repo_host,
            d.created_at, d.updated_at, d.disabled_at,
            cv.version_number, cv.created_at,
            (SELECT COUNT(*) FROM draft_versions v WHERE v.draft_id = d.id)
        FROM drafts d
        LEFT JOIN draft_versions cv ON cv.id = d.current_version_id
        WHERE d.deleted_at IS NULL
        ORDER BY d.updated_at DESC
        "#,
    )?;

    let rows = statement.query_map([], |row| {
        Ok(DraftSummary {
            draft_id: row.get(0)?,
            title: row.get(1)?,
            description: row.get(2)?,
            repo_org: row.get(3)?,
            repo_name: row.get(4)?,
            repo_host: row.get(5)?,
            created_at: row.get(6)?,
            updated_at: row.get(7)?,
            disabled: row.get::<_, Option<String>>(8)?.is_some(),
            latest_version_number: row.get(9)?,
            latest_version_at: row.get(10)?,
            version_count: row.get(11)?,
            public_url: String::new(),
            raw_url: String::new(),
        })
    })?;

    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get_draft_summary(conn: &Connection, draft_id: &str) -> Result<Option<DraftSummary>> {
    Ok(list_drafts(conn)?
        .into_iter()
        .find(|d| d.draft_id == draft_id))
}

pub fn list_versions(conn: &Connection, draft_id: &str) -> Result<Vec<VersionInfo>> {
    let mut statement = conn.prepare(
        r#"
        SELECT id, version_number, created_at, git_branch, git_commit_sha,
               git_commit_subject, git_dirty, file_size, original_filename
        FROM draft_versions
        WHERE draft_id = ?1
        ORDER BY version_number DESC
        "#,
    )?;

    let rows = statement.query_map(params![draft_id], |row| {
        Ok(VersionInfo {
            id: row.get(0)?,
            version_number: row.get(1)?,
            created_at: row.get(2)?,
            git_branch: row.get(3)?,
            git_commit_sha: row.get(4)?,
            git_commit_subject: row.get(5)?,
            git_dirty: row.get(6)?,
            file_size: row.get(7)?,
            original_filename: row.get(8)?,
        })
    })?;

    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

/// Soft-delete: versions stay in the database but the draft stops serving
/// and disappears from listings.
pub fn soft_delete_draft(conn: &Connection, draft_id: &str) -> Result<bool> {
    let timestamp = now();
    let changed = conn.execute(
        "UPDATE drafts SET deleted_at = ?1, updated_at = ?1 WHERE id = ?2 AND deleted_at IS NULL",
        params![timestamp, draft_id],
    )?;
    Ok(changed > 0)
}

/// Hard delete: remove the draft and every version row, returning the object
/// keys of the removed versions so the caller can delete the blobs. Returns
/// None when the draft id doesn't exist at all. Soft-deleted drafts can be
/// purged — that is the point.
pub fn purge_draft(conn: &mut Connection, draft_id: &str) -> Result<Option<Vec<String>>> {
    let tx = conn.transaction()?;

    let exists: bool = tx
        .query_row(
            "SELECT 1 FROM drafts WHERE id = ?1",
            params![draft_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        return Ok(None);
    }

    let keys: Vec<String> = tx
        .prepare("SELECT object_key FROM draft_versions WHERE draft_id = ?1")?
        .query_map(params![draft_id], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    tx.execute(
        "DELETE FROM draft_versions WHERE draft_id = ?1",
        params![draft_id],
    )?;
    tx.execute("DELETE FROM drafts WHERE id = ?1", params![draft_id])?;
    tx.commit()?;

    Ok(Some(keys))
}

/// Housekeeping: hard-delete everything that was previously soft-deleted.
/// Returns the number of drafts removed and the object keys of their blobs.
pub fn purge_deleted_drafts(conn: &mut Connection) -> Result<(usize, Vec<String>)> {
    let tx = conn.transaction()?;

    let keys: Vec<String> = tx
        .prepare(
            r#"
            SELECT object_key FROM draft_versions
            WHERE draft_id IN (SELECT id FROM drafts WHERE deleted_at IS NOT NULL)
            "#,
        )?
        .query_map([], |row| row.get(0))?
        .collect::<Result<_, _>>()?;

    tx.execute(
        r#"
        DELETE FROM draft_versions
        WHERE draft_id IN (SELECT id FROM drafts WHERE deleted_at IS NOT NULL)
        "#,
        [],
    )?;
    let removed = tx.execute("DELETE FROM drafts WHERE deleted_at IS NOT NULL", [])?;
    tx.commit()?;

    Ok((removed, keys))
}

pub fn disable_draft(conn: &Connection, draft_id: &str, reason: &str) -> Result<bool> {
    let timestamp = now();
    let changed = conn.execute(
        r#"
        UPDATE drafts
        SET disabled_at = ?1, disabled_reason = ?2, updated_at = ?1
        WHERE id = ?3 AND deleted_at IS NULL
        "#,
        params![timestamp, reason, draft_id],
    )?;
    Ok(changed > 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_conn() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init(&conn).unwrap();
        conn
    }

    fn upload<'a>(
        html: &'a str,
        draft_id: Option<String>,
        meta: &'a UploadMetadata,
    ) -> NewUpload<'a> {
        NewUpload {
            html,
            filename: Some("plan.html".into()),
            draft_id,
            description: None,
            title_from_html: Some("Test".into()),
            metadata: meta,
            source_ip: None,
            user_agent: None,
            has_inline_script: false,
            external_image_hosts: &[],
        }
    }

    #[test]
    fn upload_versioning_and_delete_flow() {
        let mut conn = test_conn();
        let store = crate::storage::test_store();
        let meta = UploadMetadata::default();

        let first = record_upload(
            &mut conn,
            &store,
            upload("<title>Test</title>v1", None, &meta),
        )
        .unwrap();
        assert!(first.created);
        assert_eq!(first.version_number, 1);

        let second = record_upload(
            &mut conn,
            &store,
            upload("<title>Test</title>v2", Some(first.draft_id.clone()), &meta),
        )
        .unwrap();
        assert!(!second.created);
        assert_eq!(second.version_number, 2);

        let current = find_public_version(&conn, &first.draft_id, None)
            .unwrap()
            .unwrap();
        assert_eq!(current.version_number, 2);
        assert!(store.get(&current.object_key).unwrap().ends_with("v2"));

        let v1 = find_public_version(&conn, &first.draft_id, Some(1))
            .unwrap()
            .unwrap();
        assert!(store.get(&v1.object_key).unwrap().ends_with("v1"));

        let drafts = list_drafts(&conn).unwrap();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].version_count, 2);

        assert!(soft_delete_draft(&conn, &first.draft_id).unwrap());
        assert!(find_public_version(&conn, &first.draft_id, None)
            .unwrap()
            .is_none());
        assert!(list_drafts(&conn).unwrap().is_empty());

        std::fs::remove_dir_all(store.root()).ok();
    }

    #[test]
    fn purge_removes_rows_and_reports_blob_keys() {
        let mut conn = test_conn();
        let store = crate::storage::test_store();
        let meta = UploadMetadata::default();

        let first = record_upload(
            &mut conn,
            &store,
            upload("<title>Test</title>v1", None, &meta),
        )
        .unwrap();
        record_upload(
            &mut conn,
            &store,
            upload("<title>Test</title>v2", Some(first.draft_id.clone()), &meta),
        )
        .unwrap();

        assert!(purge_draft(&mut conn, "missing").unwrap().is_none());

        // Purge a live draft directly by id.
        let keys = purge_draft(&mut conn, &first.draft_id).unwrap().unwrap();
        assert_eq!(keys.len(), 2);
        for key in &keys {
            store.remove(key).unwrap();
        }
        assert!(list_drafts(&conn).unwrap().is_empty());
        assert!(find_public_version(&conn, &first.draft_id, None)
            .unwrap()
            .is_none());
        assert!(!store.root().join("drafts").join(&first.draft_id).exists());

        // Housekeeping purge collects soft-deleted drafts.
        let second = record_upload(
            &mut conn,
            &store,
            upload("<title>Test</title>x", None, &meta),
        )
        .unwrap();
        soft_delete_draft(&conn, &second.draft_id).unwrap();
        let (count, keys) = purge_deleted_drafts(&mut conn).unwrap();
        assert_eq!(count, 1);
        assert_eq!(keys.len(), 1);
        assert!(purge_draft(&mut conn, &second.draft_id).unwrap().is_none());

        std::fs::remove_dir_all(store.root()).ok();
    }

    #[test]
    fn unknown_target_draft_is_not_found() {
        let mut conn = test_conn();
        let store = crate::storage::test_store();
        let result = record_upload(
            &mut conn,
            &store,
            upload("<p>x</p>", Some("nope".into()), &UploadMetadata::default()),
        );
        assert!(matches!(result, Err(UploadError::DraftNotFound)));
    }
}
