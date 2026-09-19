//! Schema migrations, tracked by SeaORM in `seaql_migrations`.

mod m0001_baseline;

use sea_orm_migration::prelude::*;

pub use m0001_baseline::{POSTGRES_BASELINE, SQLITE_BASELINE};

/// The name recorded in `seaql_migrations` for the baseline. Adoption writes
/// this row by hand for a legacy database that already has the schema.
pub const BASELINE_NAME: &str = "m0001_baseline";

pub struct Migrator;

#[async_trait::async_trait]
impl MigratorTrait for Migrator {
    fn migrations() -> Vec<Box<dyn MigrationTrait>> {
        vec![Box::new(m0001_baseline::Migration)]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::connect_sqlite_memory;
    use sea_orm_migration::sea_orm::{ConnectionTrait, DbBackend, Statement};

    #[tokio::test]
    async fn the_migrator_builds_the_schema_once_and_records_one_baseline_row() {
        let db = connect_sqlite_memory().await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        Migrator::up(&db, None).await.unwrap();

        let names = |sql: &'static str| {
            let db = &db;
            async move {
                db.query_all_raw(Statement::from_string(DbBackend::Sqlite, sql))
                    .await
                    .unwrap()
                    .iter()
                    .map(|row| row.try_get_by_index::<String>(0).unwrap())
                    .collect::<Vec<_>>()
            }
        };
        assert_eq!(
            names("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name").await,
            [
                "draft_versions",
                "drafts",
                "notification_deliveries",
                "notification_events",
                "push_subscriptions",
                "seaql_migrations",
            ]
        );
        assert_eq!(
            names("SELECT version FROM seaql_migrations").await,
            [BASELINE_NAME]
        );
    }
}
