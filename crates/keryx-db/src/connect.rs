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

/// One connection on SQLite: exactly the single mutex-held connection Keryx
/// has always had. Raise it only on evidence.
pub const DEFAULT_SQLITE_POOL: u32 = 1;
/// Keryx is a small tenant on a shared cluster's connection budget.
pub const DEFAULT_POSTGRES_POOL: u32 = 4;

/// Open the SQLite database at `path`, creating it and its directory if
/// missing.
pub async fn connect_sqlite(path: &Path, pool_size: Option<u32>) -> Result<DatabaseConnection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("creating database directory {}", parent.display()))?;
    }
    let path = path
        .to_str()
        .with_context(|| format!("database path {} is not valid UTF-8", path.display()))?;
    connect_sqlite_url(&format!("sqlite://{path}"), pool_size)
        .await
        .with_context(|| format!("opening database {path}"))
}

/// A private in-memory database, for tests.
#[cfg(any(test, feature = "test-support"))]
pub async fn connect_sqlite_memory() -> Result<DatabaseConnection> {
    // More than one connection to `:memory:` would be more than one database.
    connect_sqlite_url("sqlite::memory:", None).await
}

async fn connect_sqlite_url(url: &str, pool_size: Option<u32>) -> Result<DatabaseConnection> {
    let mut options = ConnectOptions::new(url);
    options
        .max_connections(pool_size.unwrap_or(DEFAULT_SQLITE_POOL).max(1))
        .sqlx_logging(false)
        .map_sqlx_sqlite_opts(sqlite_pragmas);
    Ok(Database::connect(options).await?)
}

/// Connect to Postgres. TLS is configured in the URL, with `sslmode` and
/// `sslrootcert`; a configured root certificate is added on top of the
/// built-in roots, which is what a private cluster CA needs.
///
/// `test_before_acquire` pings a pooled connection before handing it out, so
/// a failover heals without a restart, and the timeouts keep a dead primary
/// from parking requests forever. Connect to the read-write service directly:
/// a transaction-mode pooler breaks prepared statement caching.
pub async fn connect_postgres(
    url: &str,
    pool_size: Option<u32>,
    schema: Option<&str>,
) -> Result<DatabaseConnection> {
    if !is_postgres_url(url) {
        anyhow::bail!("the database URL must start with postgres:// or postgresql://");
    }
    let mut options = ConnectOptions::new(url);
    options
        .max_connections(pool_size.unwrap_or(DEFAULT_POSTGRES_POOL).max(1))
        .test_before_acquire(true)
        .connect_timeout(Duration::from_secs(10))
        .acquire_timeout(Duration::from_secs(10))
        .idle_timeout(Duration::from_secs(300))
        .sqlx_logging(false);
    if let Some(schema) = schema {
        options.set_schema_search_path(schema);
    }
    // The URL can carry a password, so it never goes into an error.
    Database::connect(options)
        .await
        .with_context(|| format!("connecting to {}", redact_url(url)))
}

pub fn is_postgres_url(url: &str) -> bool {
    url.starts_with("postgres://") || url.starts_with("postgresql://")
}

/// The URL with its credentials and query string removed, safe to print.
pub fn redact_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return "the configured database".to_string();
    };
    let rest = rest.split(['?', '#']).next().unwrap_or_default();
    let (authority, path) = match rest.split_once('/') {
        Some((authority, path)) => (authority, format!("/{path}")),
        None => (rest, String::new()),
    };
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    format!("{scheme}://{host}{path}")
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
        let db = connect_sqlite(&dir.path().join("nested/keryx.db"), None)
            .await
            .unwrap();
        assert_eq!(pragma(&db, "journal_mode").await, "wal");
        assert_eq!(pragma(&db, "foreign_keys").await, "1");
        assert_eq!(pragma(&db, "busy_timeout").await, "5000");
    }

    #[test]
    fn a_database_url_is_never_printed_with_its_credentials() {
        assert_eq!(
            redact_url("postgres://keryx:s3cret@db.internal:5432/keryx?sslmode=verify-full&sslrootcert=/ca.pem"),
            "postgres://db.internal:5432/keryx"
        );
        assert_eq!(redact_url("postgresql://db/keryx"), "postgresql://db/keryx");
        assert_eq!(redact_url("postgres://u:p%40ss@host"), "postgres://host");
        assert_eq!(redact_url("nonsense"), "the configured database");
        assert!(is_postgres_url("postgresql://db/keryx") && !is_postgres_url("mysql://db/keryx"));
    }
}
