//! Legacy database fixtures, built the way history built them: the first
//! release's schema, then the old upgrade steps as plain SQL. No Keryx code
//! is involved in building them, so they stay valid whatever the crate does.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use keryx_db::connect::connect_sqlite;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

const V0_SCHEMA: &str = include_str!("../fixtures/legacy_v0_schema.sql");
const TO_V1: &str = include_str!("../fixtures/legacy_to_v1.sql");
const TO_V2: &str = include_str!("../fixtures/legacy_to_v2.sql");
const SEED: &str = include_str!("../fixtures/legacy_seed.sql");
const SEED_V2: &str = include_str!("../fixtures/legacy_seed_v2.sql");

/// A database written by the released 0.5.1 binary: every column inline in
/// its stored SQL, user_version 2. The other legacy population.
pub const RELEASED_0_5_1_DB: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/keryx-0.5.1.db");

/// Build a seeded legacy database at `user_version` 0, 1 or 2 and close it.
/// Levels 1 and 2 carry ALTER-appended columns, as an upgraded database does.
pub async fn build_legacy(dir: &Path, user_version: u8) -> PathBuf {
    let path = dir.join(format!("legacy-v{user_version}.db"));
    let db = connect_sqlite(&path).await.unwrap();
    db.execute_unprepared(V0_SCHEMA).await.unwrap();
    db.execute_unprepared(SEED).await.unwrap();
    if user_version >= 1 {
        db.execute_unprepared(TO_V1).await.unwrap();
    }
    if user_version >= 2 {
        db.execute_unprepared(TO_V2).await.unwrap();
        // Releases from 0.5.0 created the push tables on every start.
        db.execute_unprepared(keryx_db::migration::SQLITE_BASELINE)
            .await
            .unwrap();
        db.execute_unprepared(SEED_V2).await.unwrap();
    }
    db.close().await.unwrap();
    path
}

pub fn copy_released_db(dir: &Path) -> PathBuf {
    let path = dir.join("keryx-0.5.1.db");
    std::fs::copy(RELEASED_0_5_1_DB, &path).unwrap();
    path
}

pub async fn strings(db: &DatabaseConnection, sql: &str) -> Vec<String> {
    db.query_all_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await
        .unwrap()
        .iter()
        .map(|row| row.try_get_by_index::<String>(0).unwrap())
        .collect()
}

pub async fn integer(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await
        .unwrap()
        .unwrap()
        .try_get_by_index::<i64>(0)
        .unwrap()
}
