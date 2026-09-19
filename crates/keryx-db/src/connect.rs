//! Opening a SeaORM connection. SeaORM sets no SQLite pragmas of its own, so
//! everything the rusqlite era relied on is set here, explicitly.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use sea_orm::sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sea_orm::{ConnectOptions, Database, DatabaseConnection};

/// How long a writer waits on a locked database before giving up. rusqlite's
/// default, which is what Keryx has always run with.
const SQLITE_BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// WAL, foreign keys on, a busy timeout, create if missing. Omitting any of
/// these fails silently: no WAL, and purge stops cascading.
fn sqlite_pragmas(options: SqliteConnectOptions) -> SqliteConnectOptions {
    options
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .foreign_keys(true)
        .busy_timeout(SQLITE_BUSY_TIMEOUT)
}

/// Open the SQLite database at `path`, creating it and its directory if
/// missing. A pool of one connection, which is exactly the single mutex-held
/// connection Keryx has always had.
pub async fn connect_sqlite(path: &Path) -> Result<DatabaseConnection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating database directory {}", parent.display()))?;
    }
    let path = path
        .to_str()
        .with_context(|| format!("database path {} is not valid UTF-8", path.display()))?;
    connect_sqlite_url(&format!("sqlite://{path}"))
        .await
        .with_context(|| format!("opening database {path}"))
}

/// A private in-memory database, for tests.
#[cfg(any(test, feature = "test-support"))]
pub async fn connect_sqlite_memory() -> Result<DatabaseConnection> {
    connect_sqlite_url("sqlite::memory:").await
}

async fn connect_sqlite_url(url: &str) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(url);
    options
        .max_connections(1)
        .sqlx_logging(false)
        .map_sqlx_sqlite_opts(sqlite_pragmas);
    Ok(Database::connect(options).await?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, DbBackend, Statement};

    async fn pragma(db: &DatabaseConnection, name: &str) -> String {
        let row = db
            .query_one_raw(Statement::from_string(
                DbBackend::Sqlite,
                format!("PRAGMA {name}"),
            ))
            .await
            .unwrap()
            .unwrap();
        row.try_get_by_index::<String>(0)
            .or_else(|_| row.try_get_by_index::<i64>(0).map(|n| n.to_string()))
            .unwrap()
    }

    #[tokio::test]
    async fn a_freshly_opened_sqlite_database_has_wal_and_foreign_keys_on() {
        let dir = tempfile::tempdir().unwrap();
        let db = connect_sqlite(&dir.path().join("nested/keryx.db"))
            .await
            .unwrap();
        assert_eq!(pragma(&db, "journal_mode").await, "wal");
        assert_eq!(pragma(&db, "foreign_keys").await, "1");
        assert_eq!(pragma(&db, "busy_timeout").await, "5000");
    }
}
