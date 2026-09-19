//! The parity gate. An existing database must keep working in place, with no
//! data loss and no observable change. Checked at every legacy user_version
//! (0, 1 and 2, with ALTER-appended columns) and against a database written
//! by the released 0.5.1 (every column inline): the two legacy populations.
//!
//! Level 1, schema: an adopted database has the same columns and indexes as
//! one the migrator builds from empty.
//! Level 2, data: adoption leaves every row exactly as the old rusqlite
//! upgrade path would have, and the old query layer reads the same answers
//! from both. Those answers are pinned in a golden file, so they outlive the
//! old code, and the SeaORM store must give the same answers through
//! DraftStore.
//!
//! Level 3, end to end through the real binary, is tests/legacy_database.rs
//! in the workspace root.

mod common;

use std::collections::BTreeMap;
use std::path::Path;

use common::{build_legacy, copy_released_db};
use keryx_db::adopt::open_sqlite;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement};

const TABLES: [&str; 5] = [
    "drafts",
    "draft_versions",
    "push_subscriptions",
    "notification_events",
    "notification_deliveries",
];

async fn rows(db: &DatabaseConnection, sql: &str) -> Vec<sea_orm::QueryResult> {
    db.query_all_raw(Statement::from_string(DbBackend::Sqlite, sql))
        .await
        .unwrap()
}

/// Columns by name, declared type, nullability, default and primary key
/// position, plus indexes with their columns. Deliberately not raw
/// sqlite_master text and not column order: the two legacy populations
/// already differ there, and entities select columns by name.
async fn schema_shape(db: &DatabaseConnection) -> BTreeMap<String, Vec<String>> {
    let mut shape = BTreeMap::new();
    for table in TABLES {
        let mut columns = Vec::new();
        for row in rows(db, &format!("PRAGMA table_info({table})")).await {
            columns.push(format!(
                "{} {} notnull={} default={:?} pk={}",
                row.try_get_by_index::<String>(1).unwrap(),
                row.try_get_by_index::<String>(2).unwrap(),
                row.try_get_by_index::<i64>(3).unwrap(),
                row.try_get_by_index::<Option<String>>(4).unwrap(),
                row.try_get_by_index::<i64>(5).unwrap(),
            ));
        }
        columns.sort();
        shape.insert(format!("{table} columns"), columns);

        let mut indexes = Vec::new();
        for row in rows(db, &format!("PRAGMA index_list({table})")).await {
            let name = row.try_get_by_index::<String>(1).unwrap();
            let mut indexed = Vec::new();
            for column in rows(db, &format!("PRAGMA index_info({name})")).await {
                indexed.push(column.try_get_by_index::<String>(2).unwrap());
            }
            // Auto-index names carry a creation ordinal; their columns are
            // what matters.
            let label = if name.starts_with("sqlite_autoindex_") {
                "auto".to_string()
            } else {
                name
            };
            indexes.push(format!(
                "{label} unique={} origin={} on ({})",
                row.try_get_by_index::<i64>(2).unwrap(),
                row.try_get_by_index::<String>(3).unwrap(),
                indexed.join(", "),
            ));
        }
        indexes.sort();
        shape.insert(format!("{table} indexes"), indexes);
    }
    shape
}

/// Every row of every table, columns by name, in a stable order.
async fn all_rows(db: &DatabaseConnection) -> BTreeMap<String, Vec<String>> {
    let mut dump = BTreeMap::new();
    for table in TABLES {
        let mut columns = Vec::new();
        for row in rows(db, &format!("PRAGMA table_info({table})")).await {
            columns.push(row.try_get_by_index::<String>(1).unwrap());
        }
        columns.sort();
        let select = columns
            .iter()
            .map(|c| format!("'{c}=' || COALESCE(quote(\"{c}\"), 'NULL')"))
            .collect::<Vec<_>>()
            .join(" || ' ' || ");
        let mut lines = Vec::new();
        for row in rows(db, &format!("SELECT {select} FROM {table}")).await {
            lines.push(row.try_get_by_index::<String>(0).unwrap());
        }
        lines.sort();
        dump.insert(table.to_string(), lines);
    }
    dump
}

/// What the old rusqlite query layer answers: the listing, and each draft's
/// summary and versions, serialised as the API serialises them.
fn old_query_layer_answers(path: &Path) -> serde_json::Value {
    let conn = keryx_db::open(path).unwrap();
    let ids: Vec<String> = {
        let mut statement = conn.prepare("SELECT id FROM drafts ORDER BY id").unwrap();
        let ids = statement.query_map([], |row| row.get(0)).unwrap();
        ids.collect::<Result<_, _>>().unwrap()
    };
    let details: BTreeMap<String, serde_json::Value> = ids
        .iter()
        .map(|id| {
            (
                id.clone(),
                serde_json::json!({
                    "summary": keryx_db::get_draft_summary(&conn, id).unwrap(),
                    "versions": keryx_db::list_versions(&conn, id).unwrap(),
                }),
            )
        })
        .collect();
    serde_json::json!({
        "listing": keryx_db::list_drafts(&conn).unwrap(),
        "drafts": details,
        "blobs": keryx_db::blob_records(&conn).unwrap().iter().map(|b| (b.object_key.clone(), b.content_hash.clone(), b.file_size)).collect::<Vec<_>>(),
    })
}

/// The same questions, asked of the SeaORM store.
async fn store_answers(path: &Path) -> serde_json::Value {
    use keryx_db::DraftStore;
    let ids = {
        let db = keryx_db::connect::connect_sqlite(path).await.unwrap();
        let ids = common::strings(&db, "SELECT id FROM drafts ORDER BY id").await;
        db.close().await.unwrap();
        ids
    };
    let (store, _) = keryx_db::SeaOrmStore::open_sqlite(path, false)
        .await
        .unwrap();
    let mut details = BTreeMap::new();
    for id in ids {
        details.insert(
            id.clone(),
            serde_json::json!({
                "summary": store.get_draft_summary(&id).await.unwrap(),
                "versions": store.list_versions(&id).await.unwrap(),
            }),
        );
    }
    serde_json::json!({
        "listing": store.list_drafts().await.unwrap(),
        "drafts": details,
        "blobs": store.blob_records().await.unwrap().iter().map(|b| (b.object_key.clone(), b.content_hash.clone(), b.file_size)).collect::<Vec<_>>(),
    })
}

fn golden(name: &str, actual: &serde_json::Value) {
    let path = format!(
        "{}/tests/fixtures/golden/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    let rendered = serde_json::to_string_pretty(actual).unwrap() + "\n";
    if std::env::var_os("KERYX_BLESS").is_some() {
        std::fs::create_dir_all(Path::new(&path).parent().unwrap()).unwrap();
        std::fs::write(&path, &rendered).unwrap();
    }
    let expected = std::fs::read_to_string(&path)
        .unwrap_or_else(|_| panic!("missing golden {path}; run once with KERYX_BLESS=1"));
    assert_eq!(rendered, expected, "{name} drifted from its golden file");
}

async fn fresh_shape(dir: &Path) -> BTreeMap<String, Vec<String>> {
    let (fresh, _) = open_sqlite(&dir.join("fresh.db"), false).await.unwrap();
    schema_shape(&fresh).await
}

async fn assert_parity(name: &str, legacy: &Path, dir: &Path) {
    // The same legacy file twice: once for each upgrade path.
    let old_way = dir.join(format!("{name}-old-way.db"));
    let new_way = dir.join(format!("{name}-new-way.db"));
    std::fs::copy(legacy, &old_way).unwrap();
    std::fs::copy(legacy, &new_way).unwrap();

    // Old way: rusqlite's open() runs init() and its upgrade steps.
    drop(keryx_db::open(&old_way).unwrap());
    // New way: adoption.
    let (adopted, _) = open_sqlite(&new_way, false).await.unwrap();

    // Level 1: the adopted schema is the schema the migrator builds.
    assert_eq!(
        schema_shape(&adopted).await,
        fresh_shape(dir).await,
        "{name}: schema"
    );

    // Level 2: every row is exactly what the old upgrade path produced...
    let old_db = keryx_db::connect::connect_sqlite(&old_way).await.unwrap();
    assert_eq!(
        all_rows(&adopted).await,
        all_rows(&old_db).await,
        "{name}: rows"
    );
    // ...and that old outcome is pinned too, so this holds once rusqlite is gone.
    golden(
        &format!("{name}.rows"),
        &serde_json::to_value(all_rows(&old_db).await).unwrap(),
    );
    adopted.close().await.unwrap();
    old_db.close().await.unwrap();

    // ...and the old query layer answers identically from both, as pinned.
    let answers = old_query_layer_answers(&new_way);
    assert_eq!(
        answers,
        old_query_layer_answers(&old_way),
        "{name}: answers"
    );
    golden(name, &answers);

    // And the SeaORM store, reading the adopted database, says the same.
    assert_eq!(
        store_answers(&new_way).await,
        answers,
        "{name}: DraftStore answers"
    );
}

#[tokio::test]
async fn parity_at_user_version_0() {
    let dir = tempfile::tempdir().unwrap();
    assert_parity(
        "user-version-0",
        &build_legacy(dir.path(), 0).await,
        dir.path(),
    )
    .await;
}

#[tokio::test]
async fn parity_at_user_version_1() {
    let dir = tempfile::tempdir().unwrap();
    assert_parity(
        "user-version-1",
        &build_legacy(dir.path(), 1).await,
        dir.path(),
    )
    .await;
}

#[tokio::test]
async fn parity_at_user_version_2() {
    let dir = tempfile::tempdir().unwrap();
    assert_parity(
        "user-version-2",
        &build_legacy(dir.path(), 2).await,
        dir.path(),
    )
    .await;
}

#[tokio::test]
async fn parity_for_a_database_written_by_0_5_1() {
    let dir = tempfile::tempdir().unwrap();
    assert_parity("released-0.5.1", &copy_released_db(dir.path()), dir.path()).await;
}
