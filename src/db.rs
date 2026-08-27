//! SQLite persistence for draft/version metadata. The HTML bytes themselves
//! live on disk (see storage.rs); each version row records the blob's
//! object key.

use std::path::Path;

use anyhow::{Context, Result};
use chrono::{DateTime, SecondsFormat, Utc};
use rusqlite::{params, Connection, OptionalExtension};

use crate::ids::{new_draft_id, new_internal_id};
use crate::storage::BlobStore;
use crate::types::{AvailabilityUpdate, DraftSummary, UploadMetadata, VersionInfo};

pub fn now() -> String {
    format_timestamp(Utc::now())
}

/// Every stored timestamp uses this one shape, so string comparison in SQL
/// orders correctly and equal instants compare equal.
pub fn format_timestamp(value: DateTime<Utc>) -> String {
    value.to_rfc3339_opts(SecondsFormat::Millis, true)
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
            disabled_reason TEXT,
            snoozed_until TEXT
        );

        CREATE TABLE IF NOT EXISTS draft_versions (
            id TEXT PRIMARY KEY,
            draft_id TEXT NOT NULL REFERENCES drafts(id),
            version_number INTEGER NOT NULL,
            object_key TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            file_size INTEGER NOT NULL,
            created_at TEXT NOT NULL,
            repo_org TEXT,
            repo_name TEXT,
            repo_host TEXT,
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

    // Schema version 1 moves repository provenance onto immutable versions.
    // The transaction makes the ALTER/backfill marker atomic across restarts.
    let schema_version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if schema_version < 1 {
        let tx = conn.unchecked_transaction()?;
        let version_columns = table_columns(&tx, "draft_versions")?;
        for (column, definition) in [
            ("repo_org", "repo_org TEXT"),
            ("repo_name", "repo_name TEXT"),
            ("repo_host", "repo_host TEXT"),
        ] {
            if !version_columns.iter().any(|existing| existing == column) {
                tx.execute(
                    &format!("ALTER TABLE draft_versions ADD COLUMN {definition}"),
                    [],
                )?;
            }
        }

        // Older databases kept repository provenance only on the draft row.
        // Preserve it on the version that was current at migration time.
        tx.execute_batch(
            r#"
            UPDATE draft_versions
            SET repo_org = (
                    SELECT d.repo_org FROM drafts d
                    WHERE d.current_version_id = draft_versions.id
                ),
                repo_name = (
                    SELECT d.repo_name FROM drafts d
                    WHERE d.current_version_id = draft_versions.id
                ),
                repo_host = (
                    SELECT d.repo_host FROM drafts d
                    WHERE d.current_version_id = draft_versions.id
                )
            WHERE id IN (
                SELECT current_version_id FROM drafts
                WHERE current_version_id IS NOT NULL
            );
            "#,
        )?;
        tx.pragma_update(None, "user_version", 1)?;
        tx.commit()?;
    }

    // Schema version 2 adds snooze: a nullable wake time on the draft row.
    if schema_version < 2 {
        let tx = conn.unchecked_transaction()?;
        if !table_columns(&tx, "drafts")?
            .iter()
            .any(|column| column == "snoozed_until")
        {
            tx.execute("ALTER TABLE drafts ADD COLUMN snoozed_until TEXT", [])?;
        }
        tx.pragma_update(None, "user_version", 2)?;
        tx.commit()?;
    }
    Ok(())
}

fn table_columns(conn: &Connection, table: &str) -> Result<Vec<String>> {
    let mut statement = conn.prepare(&format!("PRAGMA table_info({table})"))?;
    let columns = statement.query_map([], |row| row.get(1))?;
    Ok(columns.collect::<Result<Vec<_>, _>>()?)
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
            repo_org, repo_name, repo_host, source_ip, user_agent, cli_version,
            git_branch, git_commit_sha, git_commit_subject, git_dirty,
            original_filename, has_inline_script, external_image_hosts
        )
        VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)
        "#,
        params![
            version_id,
            draft_id,
            version_number,
            object_key,
            content_hash,
            file_size,
            timestamp,
            m.repo_org,
            m.repo_name,
            m.repo_host,
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
            repo_org = ?4,
            repo_name = ?5,
            repo_host = ?6,
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
    pub created_at: String,
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
                "SELECT version_number, object_key, created_at FROM draft_versions WHERE draft_id = ?1 AND version_number = ?2",
                params![draft_id, n],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()?,
        None => match current_version_id {
            Some(version_id) => conn
                .query_row(
                    "SELECT version_number, object_key, created_at FROM draft_versions WHERE id = ?1",
                    params![version_id],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .optional()?,
            None => None,
        },
    };

    Ok(
        row.map(|(version_number, object_key, created_at)| ServedVersion {
            draft_id,
            version_number,
            object_key,
            created_at,
        }),
    )
}

const SUMMARY_SELECT: &str = r#"
    SELECT
        d.id, d.title, d.description,
        cv.repo_org,
        cv.repo_name,
        cv.repo_host,
        d.created_at, d.updated_at, d.disabled_at,
        cv.version_number, cv.created_at, cv.git_branch, cv.git_commit_sha,
        cv.git_commit_subject, cv.git_dirty,
        (SELECT COUNT(*) FROM draft_versions v WHERE v.draft_id = d.id),
        d.snoozed_until
    FROM drafts d
    LEFT JOIN draft_versions cv ON cv.id = d.current_version_id
    WHERE d.deleted_at IS NULL
"#;

fn read_summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<DraftSummary> {
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
        latest_git_branch: row.get(11)?,
        latest_git_commit_sha: row.get(12)?,
        latest_git_commit_subject: row.get(13)?,
        latest_git_dirty: row.get(14)?,
        version_count: row.get(15)?,
        snoozed_until: row.get(16)?,
        public_url: String::new(),
        raw_url: String::new(),
    })
}

/// Every live draft, newest first, with the aggregates the dashboard, CLI, and
/// TUI need. Snoozed drafts are included; callers derive the display state.
/// `public_url`/`raw_url` are filled in by the server layer.
pub fn list_drafts(conn: &Connection) -> Result<Vec<DraftSummary>> {
    let mut statement = conn.prepare(&format!("{SUMMARY_SELECT} ORDER BY d.updated_at DESC"))?;
    let rows = statement.query_map([], read_summary)?;
    Ok(rows.collect::<Result<Vec<_>, _>>()?)
}

pub fn get_draft_summary(conn: &Connection, draft_id: &str) -> Result<Option<DraftSummary>> {
    Ok(conn
        .query_row(
            &format!("{SUMMARY_SELECT} AND d.id = ?1"),
            params![draft_id],
            read_summary,
        )
        .optional()?)
}

pub fn list_versions(conn: &Connection, draft_id: &str) -> Result<Vec<VersionInfo>> {
    let mut statement = conn.prepare(
        r#"
        SELECT id, version_number, created_at, repo_org, repo_name, repo_host,
               git_branch, git_commit_sha, git_commit_subject, git_dirty,
               file_size, original_filename
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
            repo_org: row.get(3)?,
            repo_name: row.get(4)?,
            repo_host: row.get(5)?,
            git_branch: row.get(6)?,
            git_commit_sha: row.get(7)?,
            git_commit_subject: row.get(8)?,
            git_dirty: row.get(9)?,
            file_size: row.get(10)?,
            original_filename: row.get(11)?,
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

#[derive(Debug)]
pub enum AvailabilityError {
    DraftNotFound,
    InvalidWakeTime(String),
    Other(anyhow::Error),
}

impl std::fmt::Display for AvailabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AvailabilityError::DraftNotFound => write!(f, "Draft not found."),
            AvailabilityError::InvalidWakeTime(message) => write!(f, "{message}"),
            AvailabilityError::Other(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for AvailabilityError {}

impl From<rusqlite::Error> for AvailabilityError {
    fn from(e: rusqlite::Error) -> Self {
        AvailabilityError::Other(e.into())
    }
}

impl From<anyhow::Error> for AvailabilityError {
    fn from(e: anyhow::Error) -> Self {
        AvailabilityError::Other(e)
    }
}

/// Accept any RFC 3339 wake time, store it as UTC with milliseconds, and
/// reject anything that is not strictly in the future.
pub fn normalize_wake_time(value: &str, now: DateTime<Utc>) -> Result<String, AvailabilityError> {
    let until = DateTime::parse_from_rfc3339(value.trim())
        .map_err(|_| {
            AvailabilityError::InvalidWakeTime(
                "Wake time must be an RFC 3339 timestamp, e.g. 2026-08-28T08:00:00Z.".into(),
            )
        })?
        .with_timezone(&Utc);
    if until <= now {
        return Err(AvailabilityError::InvalidWakeTime(
            "Wake time must be in the future.".into(),
        ));
    }
    Ok(format_timestamp(until))
}

pub const DEFAULT_DISABLE_REASON: &str = "Disabled by owner.";

/// The one mutation that changes availability. Each state clears the fields
/// of the others, so a row is never both snoozed and disabled, and every
/// manual transition bumps `updated_at`. Returns the updated summary.
pub fn set_availability(
    conn: &mut Connection,
    draft_id: &str,
    update: &AvailabilityUpdate,
) -> Result<DraftSummary, AvailabilityError> {
    let tx = conn.transaction()?;
    let timestamp = now();

    let exists = tx
        .query_row(
            "SELECT 1 FROM drafts WHERE id = ?1 AND deleted_at IS NULL",
            params![draft_id],
            |_| Ok(()),
        )
        .optional()?
        .is_some();
    if !exists {
        return Err(AvailabilityError::DraftNotFound);
    }

    match update {
        AvailabilityUpdate::Active => {
            tx.execute(
                r#"
                UPDATE drafts
                SET disabled_at = NULL, disabled_reason = NULL, snoozed_until = NULL, updated_at = ?1
                WHERE id = ?2
                "#,
                params![timestamp, draft_id],
            )?;
        }
        AvailabilityUpdate::Snoozed { until } => {
            let until = normalize_wake_time(until, Utc::now())?;
            tx.execute(
                r#"
                UPDATE drafts
                SET disabled_at = NULL, disabled_reason = NULL, snoozed_until = ?1, updated_at = ?2
                WHERE id = ?3
                "#,
                params![until, timestamp, draft_id],
            )?;
        }
        AvailabilityUpdate::Disabled { reason } => {
            let reason = reason.as_deref().unwrap_or(DEFAULT_DISABLE_REASON);
            tx.execute(
                r#"
                UPDATE drafts
                SET disabled_at = ?1, disabled_reason = ?2, snoozed_until = NULL, updated_at = ?1
                WHERE id = ?3
                "#,
                params![timestamp, reason, draft_id],
            )?;
        }
    }

    let summary = get_draft_summary(&tx, draft_id)?.ok_or(AvailabilityError::DraftNotFound)?;
    tx.commit()?;
    Ok(summary)
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
    fn repository_and_branch_provenance_are_versioned() {
        let mut conn = test_conn();
        let store = crate::storage::test_store();
        let first_meta = UploadMetadata {
            repo_org: Some("acme".into()),
            repo_name: Some("widgets".into()),
            repo_host: Some("github.com".into()),
            git_branch: Some("main".into()),
            ..UploadMetadata::default()
        };
        let second_meta = UploadMetadata {
            repo_org: Some("acme-labs".into()),
            repo_name: Some("widgets-next".into()),
            repo_host: Some("gitlab.com".into()),
            git_branch: Some("feature/dashboard".into()),
            ..UploadMetadata::default()
        };

        let first = record_upload(
            &mut conn,
            &store,
            upload("<title>Test</title>v1", None, &first_meta),
        )
        .unwrap();
        record_upload(
            &mut conn,
            &store,
            upload(
                "<title>Test</title>v2",
                Some(first.draft_id.clone()),
                &second_meta,
            ),
        )
        .unwrap();

        let summary = get_draft_summary(&conn, &first.draft_id).unwrap().unwrap();
        assert_eq!(summary.repo_host.as_deref(), Some("gitlab.com"));
        assert_eq!(summary.repo_org.as_deref(), Some("acme-labs"));
        assert_eq!(summary.repo_name.as_deref(), Some("widgets-next"));
        assert_eq!(
            summary.latest_git_branch.as_deref(),
            Some("feature/dashboard")
        );

        let versions = list_versions(&conn, &first.draft_id).unwrap();
        assert_eq!(versions[0].repo_host.as_deref(), Some("gitlab.com"));
        assert_eq!(versions[0].repo_org.as_deref(), Some("acme-labs"));
        assert_eq!(versions[0].git_branch.as_deref(), Some("feature/dashboard"));
        assert_eq!(versions[1].repo_host.as_deref(), Some("github.com"));
        assert_eq!(versions[1].repo_org.as_deref(), Some("acme"));
        assert_eq!(versions[1].git_branch.as_deref(), Some("main"));

        std::fs::remove_dir_all(store.root()).ok();
    }

    #[test]
    fn latest_summary_does_not_inherit_repository_from_an_older_version() {
        let mut conn = test_conn();
        let store = crate::storage::test_store();
        let recorded = UploadMetadata {
            repo_org: Some("acme".into()),
            repo_name: Some("widgets".into()),
            repo_host: Some("github.com".into()),
            git_branch: Some("main".into()),
            ..UploadMetadata::default()
        };

        let first = record_upload(
            &mut conn,
            &store,
            upload("<title>Test</title>v1", None, &recorded),
        )
        .unwrap();
        record_upload(
            &mut conn,
            &store,
            upload(
                "<title>Test</title>v2",
                Some(first.draft_id.clone()),
                &UploadMetadata::default(),
            ),
        )
        .unwrap();

        let summary = get_draft_summary(&conn, &first.draft_id).unwrap().unwrap();
        assert_eq!(summary.repo_org, None);
        assert_eq!(summary.repo_name, None);
        assert_eq!(summary.repo_host, None);
        assert_eq!(summary.latest_git_branch, None);

        std::fs::remove_dir_all(store.root()).ok();
    }

    #[test]
    fn init_adds_version_repository_columns_to_existing_databases() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE drafts (
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
            CREATE TABLE draft_versions (id TEXT PRIMARY KEY, draft_id TEXT NOT NULL);
            INSERT INTO drafts (
                id, title, current_version_id, repo_org, repo_name, repo_host,
                created_at, updated_at
            ) VALUES (
                'draft-1', 'Legacy', 'version-1', 'acme', 'widgets', 'github.com',
                '2026-01-01', '2026-01-01'
            );
            INSERT INTO draft_versions (id, draft_id) VALUES ('version-1', 'draft-1');
            "#,
        )
        .unwrap();

        init(&conn).unwrap();

        let columns = table_columns(&conn, "draft_versions").unwrap();
        assert!(columns.iter().any(|column| column == "repo_org"));
        assert!(columns.iter().any(|column| column == "repo_name"));
        assert!(columns.iter().any(|column| column == "repo_host"));
        let draft_columns = table_columns(&conn, "drafts").unwrap();
        assert!(draft_columns.iter().any(|column| column == "snoozed_until"));
        let legacy_title: String = conn
            .query_row("SELECT title FROM drafts WHERE id = 'draft-1'", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(legacy_title, "Legacy");

        let repository = conn
            .query_row(
                "SELECT repo_org, repo_name, repo_host FROM draft_versions WHERE id = 'version-1'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .unwrap();
        assert_eq!(
            repository,
            ("acme".into(), "widgets".into(), "github.com".into())
        );

        conn.execute(
            "UPDATE draft_versions SET repo_org = 'new-owner' WHERE id = 'version-1'",
            [],
        )
        .unwrap();
        init(&conn).unwrap();
        let owner: String = conn
            .query_row(
                "SELECT repo_org FROM draft_versions WHERE id = 'version-1'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let schema_version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(owner, "new-owner");
        assert_eq!(schema_version, 2);
    }

    #[test]
    fn availability_transitions_are_exclusive_and_validated() {
        let mut conn = test_conn();
        let store = crate::storage::test_store();
        let meta = UploadMetadata::default();
        let draft_id = record_upload(
            &mut conn,
            &store,
            upload("<title>Test</title>v1", None, &meta),
        )
        .unwrap()
        .draft_id;

        assert!(matches!(
            set_availability(&mut conn, "missing", &AvailabilityUpdate::Active),
            Err(AvailabilityError::DraftNotFound)
        ));

        let snoozed = set_availability(
            &mut conn,
            &draft_id,
            &AvailabilityUpdate::Snoozed {
                until: "2099-01-01T09:00:00+01:00".into(),
            },
        )
        .unwrap();
        assert_eq!(
            snoozed.snoozed_until.as_deref(),
            Some("2099-01-01T08:00:00.000Z")
        );
        assert!(!snoozed.disabled);
        assert!(find_public_version(&conn, &draft_id, None)
            .unwrap()
            .is_some());
        assert!(find_public_version(&conn, &draft_id, Some(1))
            .unwrap()
            .is_some());

        for bad in ["2000-01-01T00:00:00Z", "tomorrow", ""] {
            assert!(matches!(
                set_availability(
                    &mut conn,
                    &draft_id,
                    &AvailabilityUpdate::Snoozed { until: bad.into() }
                ),
                Err(AvailabilityError::InvalidWakeTime(_))
            ));
        }

        // A rejected transition leaves the previous state untouched, and a
        // new version never changes availability.
        record_upload(
            &mut conn,
            &store,
            upload("<title>Test</title>v2", Some(draft_id.clone()), &meta),
        )
        .unwrap();
        let unchanged = get_draft_summary(&conn, &draft_id).unwrap().unwrap();
        assert_eq!(
            unchanged.snoozed_until.as_deref(),
            Some("2099-01-01T08:00:00.000Z")
        );

        let disabled = set_availability(
            &mut conn,
            &draft_id,
            &AvailabilityUpdate::Disabled { reason: None },
        )
        .unwrap();
        assert!(disabled.disabled);
        assert_eq!(disabled.snoozed_until, None);
        assert!(find_public_version(&conn, &draft_id, None)
            .unwrap()
            .is_none());

        let resnoozed = set_availability(
            &mut conn,
            &draft_id,
            &AvailabilityUpdate::Snoozed {
                until: "2099-06-01T00:00:00Z".into(),
            },
        )
        .unwrap();
        assert!(!resnoozed.disabled);
        assert!(resnoozed.snoozed_until.is_some());

        let active = set_availability(&mut conn, &draft_id, &AvailabilityUpdate::Active).unwrap();
        assert!(!active.disabled);
        assert_eq!(active.snoozed_until, None);
        assert!(active.updated_at >= resnoozed.updated_at);

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
