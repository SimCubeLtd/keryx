//! Adopting a legacy database in place.

mod common;

use common::{build_legacy, copy_released_db, integer, strings};
use keryx_db::adopt::{open_sqlite, Adoption};

/// Backup snapshots in `dir`, ignoring SQLite's own -wal and -shm files.
fn backups_in(dir: &std::path::Path) -> usize {
    std::fs::read_dir(dir)
        .unwrap()
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            name.contains(".backup-") && !name.ends_with("-wal") && !name.ends_with("-shm")
        })
        .count()
}

#[tokio::test]
async fn a_fresh_database_is_created_by_the_migrator_not_adopted() {
    let dir = tempfile::tempdir().unwrap();
    let (db, adoption) = open_sqlite(&dir.path().join("new.db"), true).await.unwrap();
    assert_eq!(adoption, Adoption::Fresh);
    assert_eq!(
        integer(&db, "SELECT COUNT(*) FROM seaql_migrations").await,
        1
    );
    assert_eq!(
        backups_in(dir.path()),
        0,
        "a fresh database needs no backup"
    );
}

#[tokio::test]
async fn adoption_is_idempotent_and_writes_exactly_one_baseline_row() {
    for user_version in [0, 1, 2] {
        let dir = tempfile::tempdir().unwrap();
        let path = build_legacy(dir.path(), user_version).await;

        let (db, first) = open_sqlite(&path, false).await.unwrap();
        assert!(
            matches!(first, Adoption::Adopted { from_user_version, .. } if from_user_version == i64::from(user_version)),
            "{first:?}"
        );
        db.close().await.unwrap();

        let (db, second) = open_sqlite(&path, false).await.unwrap();
        assert_eq!(second, Adoption::Managed, "user_version {user_version}");
        assert_eq!(
            strings(&db, "SELECT version FROM seaql_migrations").await,
            ["m0001_baseline"]
        );
        // The pragma is left alone, so an older Keryx still understands the file.
        assert_eq!(
            integer(&db, "PRAGMA user_version").await,
            i64::from(user_version)
        );
    }
}

#[tokio::test]
async fn adoption_reports_exactly_what_it_changed() {
    let dir = tempfile::tempdir().unwrap();

    let (_, adoption) = open_sqlite(&build_legacy(dir.path(), 0).await, false)
        .await
        .unwrap();
    let Adoption::Adopted { changes, .. } = adoption else {
        panic!()
    };
    assert_eq!(
        changes,
        [
            "created table push_subscriptions",
            "created table notification_events",
            "created table notification_deliveries",
            "added draft_versions.repo_org",
            "added draft_versions.repo_name",
            "added draft_versions.repo_host",
            "backfilled repository provenance onto current versions",
            "added drafts.snoozed_until",
        ]
    );

    // A database written by 0.5.1 is already in the current shape.
    let (_, adoption) = open_sqlite(&copy_released_db(dir.path()), false)
        .await
        .unwrap();
    let Adoption::Adopted {
        from_user_version,
        changes,
        backup,
    } = adoption
    else {
        panic!()
    };
    assert_eq!(from_user_version, 2);
    assert_eq!(changes, Vec::<String>::new());
    assert_eq!(backup, None);
}

#[tokio::test]
async fn a_legacy_database_is_backed_up_before_its_first_write() {
    let dir = tempfile::tempdir().unwrap();
    let path = build_legacy(dir.path(), 0).await;

    let (db, adoption) = open_sqlite(&path, true).await.unwrap();
    let Adoption::Adopted {
        backup: Some(backup),
        ..
    } = adoption
    else {
        panic!("expected a backup: {adoption:?}")
    };
    db.close().await.unwrap();
    assert!(backup
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with("legacy-v0.db.backup-"));

    // The snapshot is the database as it was: legacy shape, all rows, untracked.
    let (snapshot, _) = (
        keryx_db::connect::connect_sqlite(&backup).await.unwrap(),
        (),
    );
    assert_eq!(
        integer(&snapshot, "SELECT COUNT(*) FROM draft_versions").await,
        4
    );
    assert!(strings(
        &snapshot,
        "SELECT name FROM sqlite_master WHERE name = 'seaql_migrations'"
    )
    .await
    .is_empty());
    assert!(!strings(
        &snapshot,
        "SELECT name FROM pragma_table_info('draft_versions')"
    )
    .await
    .contains(&"repo_org".to_string()));

    // Once managed, opening again takes no further backup.
    let (_, again) = open_sqlite(&path, true).await.unwrap();
    assert_eq!(again, Adoption::Managed);
    assert_eq!(backups_in(dir.path()), 1);
}
