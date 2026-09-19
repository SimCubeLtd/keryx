//! Opening a SQLite database that may predate SeaORM.
//!
//! People are already running Keryx. Their `keryx.db` was built by
//! hand-rolled upgrades keyed on `PRAGMA user_version` and has no
//! `seaql_migrations` table. It does not need its data moved; it needs its
//! history acknowledged. On open:
//!
//! - no tables at all: a fresh database, so run the migrator;
//! - `seaql_migrations` present: already managed, so run what is pending;
//! - tables but no `seaql_migrations`: a legacy database. Back it up, bring
//!   it to the current shape with the two old conditional upgrade steps,
//!   record the baseline as applied, then run whatever is genuinely pending.
//!
//! `user_version` is read but never written. An older Keryx opening the same
//! file still finds a schema it understands, which keeps downgrade safe.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DatabaseConnection, DbBackend, Statement,
    TransactionOptions, TransactionTrait,
};
use sea_orm_migration::{seaql_migrations, MigratorTrait};

use crate::connect::connect_sqlite;
use crate::migration::{Migrator, BASELINE_NAME, SQLITE_BASELINE};

/// What opening the database found and did, for the server log, so a support
/// question has an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Adoption {
    /// An empty database; the migrator built the schema.
    Fresh,
    /// Already tracked by `seaql_migrations`.
    Managed,
    /// A legacy database, adopted in place just now.
    Adopted {
        /// The `user_version` it was found at: 0, 1 or 2.
        from_user_version: i64,
        /// The `VACUUM INTO` snapshot taken first, unless backups were off.
        backup: Option<PathBuf>,
        /// Every schema change made, in order. Empty for a database already
        /// in the current shape.
        changes: Vec<String>,
    },
}

impl std::fmt::Display for Adoption {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Adoption::Fresh => write!(f, "new database, schema created"),
            Adoption::Managed => write!(f, "schema up to date"),
            Adoption::Adopted {
                from_user_version,
                backup,
                changes,
            } => {
                write!(
                    f,
                    "adopted a legacy database at user_version {from_user_version}"
                )?;
                match backup {
                    Some(path) => write!(f, "; backup at {}", path.display())?,
                    None => write!(f, "; no backup taken")?,
                }
                if changes.is_empty() {
                    write!(f, "; schema already current")
                } else {
                    write!(f, "; {}", changes.join("; "))
                }
            }
        }
    }
}

/// Open the SQLite database at `path`, adopting it first if it is legacy.
/// `backup` takes a consistent snapshot before the first write to a legacy
/// database; it is on by default at the call site.
pub async fn open_sqlite(path: &Path, backup: bool) -> Result<(DatabaseConnection, Adoption)> {
    open_sqlite_pooled(path, backup, None).await
}

/// [`open_sqlite`] with an explicit pool size.
pub async fn open_sqlite_pooled(
    path: &Path,
    backup: bool,
    pool_size: Option<u32>,
) -> Result<(DatabaseConnection, Adoption)> {
    let db = connect_sqlite(path, pool_size).await?;
    let backup_target = backup.then(|| backup_path(path));
    let adoption = adopt(&db, backup_target.as_deref()).await?;
    Ok((db, adoption))
}

/// Bring an open SQLite connection under migration management. Idempotent:
/// a second call finds `seaql_migrations` and only runs pending migrations.
pub async fn adopt(db: &DatabaseConnection, backup_to: Option<&Path>) -> Result<Adoption> {
    let has_schema = table_exists(db, "drafts").await?;
    let managed = table_exists(db, "seaql_migrations").await?;

    let adoption = if managed {
        Adoption::Managed
    } else if !has_schema {
        Adoption::Fresh
    } else {
        adopt_legacy(db, backup_to).await?
    };
    Migrator::up(db, None)
        .await
        .context("running database migrations")?;
    Ok(adoption)
}

async fn adopt_legacy(db: &DatabaseConnection, backup_to: Option<&Path>) -> Result<Adoption> {
    let from_user_version = query_i64(db, "PRAGMA user_version").await?;

    // Keryx runs in WAL mode, so a plain file copy can miss committed pages
    // still in the -wal file. VACUUM INTO writes a consistent snapshot. It
    // cannot run inside a transaction, so it comes first.
    if let Some(target) = backup_to {
        let target_sql = target
            .to_str()
            .context("backup path is not valid UTF-8")?
            .replace('\'', "''");
        db.execute_unprepared(&format!("VACUUM INTO '{target_sql}'"))
            .await
            .with_context(|| format!("backing up the database to {}", target.display()))?;
    }

    // One immediate transaction: either the database is fully adopted or it
    // is untouched. SQLite DDL is transactional.
    let tx = db
        .begin_with_options(TransactionOptions {
            sqlite_transaction_mode: Some(sea_orm::SqliteTransactionMode::Immediate),
            ..Default::default()
        })
        .await?;
    let mut changes = Vec::new();

    // Tables that arrived after this database was created, exactly as init()
    // used to add them: the create block is IF NOT EXISTS throughout.
    let tables_before = table_names(&tx).await?;
    tx.execute_unprepared(SQLITE_BASELINE).await?;
    for table in table_names(&tx).await? {
        if !tables_before.contains(&table) {
            changes.push(format!("created table {table}"));
        }
    }

    // Old schema version 1: repository provenance moves onto versions.
    if from_user_version < 1 {
        let columns = table_columns(&tx, "draft_versions").await?;
        for column in ["repo_org", "repo_name", "repo_host"] {
            if !columns.iter().any(|existing| existing == column) {
                tx.execute_unprepared(&format!(
                    "ALTER TABLE draft_versions ADD COLUMN {column} TEXT"
                ))
                .await?;
                changes.push(format!("added draft_versions.{column}"));
            }
        }
        // Older databases kept provenance only on the draft row. Preserve it
        // on the version that was current at upgrade time.
        tx.execute_unprepared(BACKFILL_VERSION_PROVENANCE).await?;
        changes.push("backfilled repository provenance onto current versions".to_string());
    }

    // Old schema version 2: snooze, a nullable wake time on the draft row.
    if from_user_version < 2
        && !table_columns(&tx, "drafts")
            .await?
            .iter()
            .any(|column| column == "snoozed_until")
    {
        tx.execute_unprepared("ALTER TABLE drafts ADD COLUMN snoozed_until TEXT")
            .await?;
        changes.push("added drafts.snoozed_until".to_string());
    }

    // The schema now is the baseline. Say so, so the migrator never replays it.
    Migrator::install(&tx).await?;
    seaql_migrations::ActiveModel {
        version: Set(BASELINE_NAME.to_string()),
        applied_at: Set(chrono::Utc::now().timestamp()),
    }
    .insert(&tx)
    .await?;
    tx.commit().await?;

    Ok(Adoption::Adopted {
        from_user_version,
        backup: backup_to.map(Path::to_path_buf),
        changes,
    })
}

const BACKFILL_VERSION_PROVENANCE: &str = r#"
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
"#;

/// `keryx.db` becomes `keryx.db.backup-20260919T183000Z`, next to the original.
fn backup_path(path: &Path) -> PathBuf {
    let stamp = chrono::Utc::now().format("%Y%m%dT%H%M%SZ");
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(format!(".backup-{stamp}"));
    path.with_file_name(name)
}

async fn table_exists<C: ConnectionTrait>(db: &C, table: &str) -> Result<bool> {
    Ok(table_names(db).await?.iter().any(|name| name == table))
}

async fn table_names<C: ConnectionTrait>(db: &C) -> Result<Vec<String>> {
    query_strings(db, "SELECT name FROM sqlite_master WHERE type = 'table'", 0).await
}

async fn table_columns<C: ConnectionTrait>(db: &C, table: &str) -> Result<Vec<String>> {
    query_strings(db, &format!("PRAGMA table_info({table})"), 1).await
}

async fn query_strings<C: ConnectionTrait>(db: &C, sql: &str, index: usize) -> Result<Vec<String>> {
    let rows = db
        .query_all_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await?;
    rows.iter()
        .map(|row| Ok(row.try_get_by_index::<String>(index)?))
        .collect()
}

async fn query_i64<C: ConnectionTrait>(db: &C, sql: &str) -> Result<i64> {
    let row = db
        .query_one_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await?
        .with_context(|| format!("{sql} returned no row"))?;
    Ok(row.try_get_by_index::<i64>(0)?)
}

/// An arbitrary, fixed key for the migration advisory lock ("KERYX").
const POSTGRES_MIGRATION_LOCK: i64 = 0x4B_45_52_59_58;

/// Migrate a Postgres database. There is no legacy to adopt: Postgres
/// databases have only ever been created by the migrator.
///
/// SeaORM's migrator takes no lock, and a rolling update starts a new pod
/// while the old one runs. So the migrator runs inside a transaction that
/// holds a transaction-scoped advisory lock: a second pod waits here, then
/// finds nothing pending.
pub async fn migrate_postgres(db: &DatabaseConnection) -> Result<Adoption> {
    let tx = db.begin().await?;
    tx.execute_unprepared(&format!(
        "SELECT pg_advisory_xact_lock({POSTGRES_MIGRATION_LOCK})"
    ))
    .await
    .context("taking the migration lock")?;
    let pending = Migrator::get_pending_migrations(&tx).await?.len();
    let total = Migrator::migrations().len();
    Migrator::up(&tx, None)
        .await
        .context("running database migrations")?;
    tx.commit().await?;
    Ok(if pending == total {
        Adoption::Fresh
    } else {
        Adoption::Managed
    })
}
