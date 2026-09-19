//! The one baseline migration: the whole schema, as `init()` has always
//! created it. The old user_version 1 and 2 upgrade steps are not migrations,
//! because their columns are already in this block; they live in `adopt`.

use sea_orm_migration::prelude::*;
use sea_orm_migration::sea_orm::DbBackend;

/// The SQLite schema, verbatim from the rusqlite era. Existing databases were
/// created from exactly this text, and parity is judged against it, so it
/// must never be reformatted or "improved". Idempotent by construction.
pub const SQLITE_BASELINE: &str = r#"
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

        CREATE TABLE IF NOT EXISTS push_subscriptions (
            id TEXT PRIMARY KEY,
            endpoint TEXT NOT NULL UNIQUE,
            p256dh TEXT NOT NULL,
            auth TEXT NOT NULL,
            events TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS notification_events (
            key TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            draft_id TEXT NOT NULL,
            title TEXT NOT NULL,
            body TEXT NOT NULL,
            target TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS notification_deliveries (
            event_key TEXT NOT NULL REFERENCES notification_events(key) ON DELETE CASCADE,
            subscription_id TEXT NOT NULL REFERENCES push_subscriptions(id) ON DELETE CASCADE,
            attempts INTEGER NOT NULL DEFAULT 0,
            next_attempt_at TEXT NOT NULL,
            PRIMARY KEY (event_key, subscription_id)
        );

        CREATE INDEX IF NOT EXISTS notification_deliveries_due_idx ON notification_deliveries(next_attempt_at);
        "#;

/// The same tables on Postgres. Timestamps stay TEXT on purpose: the RFC 3339
/// millisecond format orders lexically, and every query relies on that. Only
/// booleans diverge (BOOLEAN for SQLite's INTEGER), and integer columns are
/// BIGINT because a Postgres INTEGER is 32-bit where SQLite's is 64.
pub const POSTGRES_BASELINE: &str = r#"
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
            version_number BIGINT NOT NULL,
            object_key TEXT NOT NULL,
            content_hash TEXT NOT NULL,
            file_size BIGINT NOT NULL,
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
            git_dirty BOOLEAN,
            original_filename TEXT,
            has_inline_script BOOLEAN NOT NULL DEFAULT FALSE,
            external_image_hosts TEXT NOT NULL DEFAULT '[]',
            UNIQUE (draft_id, version_number)
        );

        CREATE INDEX IF NOT EXISTS draft_versions_draft_id_idx ON draft_versions(draft_id);
        CREATE INDEX IF NOT EXISTS drafts_updated_at_idx ON drafts(updated_at);

        CREATE TABLE IF NOT EXISTS push_subscriptions (
            id TEXT PRIMARY KEY,
            endpoint TEXT NOT NULL UNIQUE,
            p256dh TEXT NOT NULL,
            auth TEXT NOT NULL,
            events TEXT NOT NULL,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS notification_events (
            key TEXT PRIMARY KEY,
            kind TEXT NOT NULL,
            draft_id TEXT NOT NULL,
            title TEXT NOT NULL,
            body TEXT NOT NULL,
            target TEXT NOT NULL,
            created_at TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS notification_deliveries (
            event_key TEXT NOT NULL REFERENCES notification_events(key) ON DELETE CASCADE,
            subscription_id TEXT NOT NULL REFERENCES push_subscriptions(id) ON DELETE CASCADE,
            attempts BIGINT NOT NULL DEFAULT 0,
            next_attempt_at TEXT NOT NULL,
            PRIMARY KEY (event_key, subscription_id)
        );

        CREATE INDEX IF NOT EXISTS notification_deliveries_due_idx ON notification_deliveries(next_attempt_at);
        "#;

pub struct Migration;

impl MigrationName for Migration {
    fn name(&self) -> &str {
        "m0001_baseline"
    }
}

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let sql = match manager.get_database_backend() {
            DbBackend::Sqlite => SQLITE_BASELINE,
            DbBackend::Postgres => POSTGRES_BASELINE,
            backend => {
                return Err(DbErr::Migration(format!(
                    "Keryx supports SQLite and Postgres, not {backend:?}"
                )))
            }
        };
        manager.get_connection().execute_unprepared(sql).await?;
        Ok(())
    }

    /// SeaORM only wraps migrations in a transaction by default on Postgres.
    fn use_transaction(&self) -> Option<bool> {
        Some(true)
    }
}
