//! The store's regression suite, ported from the rusqlite era unchanged in
//! meaning, plus what only the new store can be asked.

use super::*;
use crate::entity::{draft, draft_version, notification_event};
use keryx_core::types::UploadMetadata;
use sea_orm::sea_query::Expr;
use sea_orm::PaginatorTrait;
use sea_orm::{ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, QueryOrder};

async fn event_kinds(store: &SeaOrmStore, draft_id: &str) -> Vec<(String, String)> {
    notification_event::Entity::find()
        .filter(notification_event::Column::DraftId.eq(draft_id))
        .order_by_asc(notification_event::Column::CreatedAt)
        .order_by_asc(notification_event::Column::Kind)
        .all(store.connection())
        .await
        .unwrap()
        .into_iter()
        .map(|event| (event.kind, event.target))
        .collect()
}

/// The whole upload sequence a caller performs, minus the blob write:
/// resolve the target, mint the ids and key, record the metadata.
async fn record(
    store: &SeaOrmStore,
    html: &str,
    draft_id: Option<String>,
    meta: &UploadMetadata,
) -> Result<UploadOutcome, UploadError> {
    let (draft_id, created) = store.resolve_upload_target(draft_id).await?;
    let version_id = new_internal_id();
    store
        .record_upload(new_upload(html, draft_id, created, version_id, meta))
        .await
}

fn new_upload<'a>(
    html: &'a str,
    draft_id: String,
    created: bool,
    version_id: String,
    meta: &'a UploadMetadata,
) -> NewUpload<'a> {
    NewUpload {
        html,
        filename: Some("plan.html".into()),
        object_key: format!("drafts/{draft_id}/{version_id}.html"),
        draft_id,
        created,
        version_id,
        description: None,
        title_from_html: Some("Test".into()),
        metadata: meta,
        source_ip: None,
        user_agent: None,
        has_inline_script: false,
        external_image_hosts: &[],
    }
}

#[tokio::test]
async fn upload_versioning_and_delete_flow() {
    let store = SeaOrmStore::open_test().await;
    let meta = UploadMetadata::default();

    let first = record(&store, "<title>Test</title>v1", None, &meta)
        .await
        .unwrap();
    assert!(first.created);
    assert_eq!(first.version_number, 1);

    let second = record(
        &store,
        "<title>Test</title>v2",
        Some(first.draft_id.clone()),
        &meta,
    )
    .await
    .unwrap();
    assert!(!second.created);
    assert_eq!(second.version_number, 2);

    let current = store
        .find_public_version(&first.draft_id, None)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(current.version_number, 2);
    assert!(current
        .object_key
        .ends_with(&format!("{}.html", second.version_id)));

    let v1 = store
        .find_public_version(&first.draft_id, Some(1))
        .await
        .unwrap()
        .unwrap();
    assert!(v1
        .object_key
        .ends_with(&format!("{}.html", first.version_id)));

    let drafts = store.list_drafts().await.unwrap();
    assert_eq!(drafts.len(), 1);
    assert_eq!(drafts[0].version_count, 2);

    assert!(store.soft_delete_draft(&first.draft_id).await.unwrap());
    assert!(store
        .find_public_version(&first.draft_id, None)
        .await
        .unwrap()
        .is_none());
    assert!(store.list_drafts().await.unwrap().is_empty());
}

#[tokio::test]
async fn purge_removes_rows_and_reports_blob_keys() {
    let store = SeaOrmStore::open_test().await;
    let meta = UploadMetadata::default();

    let first = record(&store, "<title>Test</title>v1", None, &meta)
        .await
        .unwrap();
    record(
        &store,
        "<title>Test</title>v2",
        Some(first.draft_id.clone()),
        &meta,
    )
    .await
    .unwrap();

    assert!(store.purge_draft("missing").await.unwrap().is_none());

    // Purge a live draft directly by id.
    let keys = store.purge_draft(&first.draft_id).await.unwrap().unwrap();
    assert_eq!(keys.len(), 2);
    assert!(store.list_drafts().await.unwrap().is_empty());
    assert!(store
        .find_public_version(&first.draft_id, None)
        .await
        .unwrap()
        .is_none());

    // Housekeeping purge collects soft-deleted drafts.
    let second = record(&store, "<title>Test</title>x", None, &meta)
        .await
        .unwrap();
    store.soft_delete_draft(&second.draft_id).await.unwrap();
    let (count, keys) = store.purge_deleted_drafts().await.unwrap();
    assert_eq!(count, 1);
    assert_eq!(keys.len(), 1);
    assert!(store.purge_draft(&second.draft_id).await.unwrap().is_none());
}

#[tokio::test]
async fn repository_and_branch_provenance_are_versioned() {
    let store = SeaOrmStore::open_test().await;
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

    let first = record(&store, "<title>Test</title>v1", None, &first_meta)
        .await
        .unwrap();
    record(
        &store,
        "<title>Test</title>v2",
        Some(first.draft_id.clone()),
        &second_meta,
    )
    .await
    .unwrap();

    let summary = store
        .get_draft_summary(&first.draft_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(summary.repo_host.as_deref(), Some("gitlab.com"));
    assert_eq!(summary.repo_org.as_deref(), Some("acme-labs"));
    assert_eq!(summary.repo_name.as_deref(), Some("widgets-next"));
    assert_eq!(
        summary.latest_git_branch.as_deref(),
        Some("feature/dashboard")
    );

    let versions = store.list_versions(&first.draft_id).await.unwrap();
    assert_eq!(versions[0].repo_host.as_deref(), Some("gitlab.com"));
    assert_eq!(versions[0].repo_org.as_deref(), Some("acme-labs"));
    assert_eq!(versions[0].git_branch.as_deref(), Some("feature/dashboard"));
    assert_eq!(versions[1].repo_host.as_deref(), Some("github.com"));
    assert_eq!(versions[1].repo_org.as_deref(), Some("acme"));
    assert_eq!(versions[1].git_branch.as_deref(), Some("main"));
}

#[tokio::test]
async fn latest_summary_does_not_inherit_repository_from_an_older_version() {
    let store = SeaOrmStore::open_test().await;
    let recorded = UploadMetadata {
        repo_org: Some("acme".into()),
        repo_name: Some("widgets".into()),
        repo_host: Some("github.com".into()),
        git_branch: Some("main".into()),
        ..UploadMetadata::default()
    };

    let first = record(&store, "<title>Test</title>v1", None, &recorded)
        .await
        .unwrap();
    record(
        &store,
        "<title>Test</title>v2",
        Some(first.draft_id.clone()),
        &UploadMetadata::default(),
    )
    .await
    .unwrap();

    let summary = store
        .get_draft_summary(&first.draft_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(summary.repo_org, None);
    assert_eq!(summary.repo_name, None);
    assert_eq!(summary.repo_host, None);
    assert_eq!(summary.latest_git_branch, None);
}

#[tokio::test]
async fn availability_transitions_are_exclusive_and_validated() {
    let store = SeaOrmStore::open_test().await;
    let meta = UploadMetadata::default();
    let draft_id = record(&store, "<title>Test</title>v1", None, &meta)
        .await
        .unwrap()
        .draft_id;

    assert!(matches!(
        store
            .set_availability("missing", &AvailabilityUpdate::Active)
            .await,
        Err(AvailabilityError::DraftNotFound)
    ));

    let snoozed = store
        .set_availability(
            &draft_id,
            &AvailabilityUpdate::Snoozed {
                until: "2099-01-01T09:00:00+01:00".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        snoozed.snoozed_until.as_deref(),
        Some("2099-01-01T08:00:00.000Z")
    );
    assert!(!snoozed.disabled);
    assert!(store
        .find_public_version(&draft_id, None)
        .await
        .unwrap()
        .is_some());
    assert!(store
        .find_public_version(&draft_id, Some(1))
        .await
        .unwrap()
        .is_some());

    for bad in ["2000-01-01T00:00:00Z", "tomorrow", ""] {
        assert!(matches!(
            store
                .set_availability(
                    &draft_id,
                    &AvailabilityUpdate::Snoozed { until: bad.into() }
                )
                .await,
            Err(AvailabilityError::InvalidWakeTime(_))
        ));
    }

    // A rejected transition leaves the previous state untouched, and a
    // new version never changes availability.
    record(
        &store,
        "<title>Test</title>v2",
        Some(draft_id.clone()),
        &meta,
    )
    .await
    .unwrap();
    let unchanged = store.get_draft_summary(&draft_id).await.unwrap().unwrap();
    assert_eq!(
        unchanged.snoozed_until.as_deref(),
        Some("2099-01-01T08:00:00.000Z")
    );

    let disabled = store
        .set_availability(&draft_id, &AvailabilityUpdate::Disabled { reason: None })
        .await
        .unwrap();
    assert!(disabled.disabled);
    assert_eq!(disabled.snoozed_until, None);
    assert!(store
        .find_public_version(&draft_id, None)
        .await
        .unwrap()
        .is_none());

    let resnoozed = store
        .set_availability(
            &draft_id,
            &AvailabilityUpdate::Snoozed {
                until: "2099-06-01T00:00:00Z".into(),
            },
        )
        .await
        .unwrap();
    assert!(!resnoozed.disabled);
    assert!(resnoozed.snoozed_until.is_some());

    let active = store
        .set_availability(&draft_id, &AvailabilityUpdate::Active)
        .await
        .unwrap();
    assert!(!active.disabled);
    assert_eq!(active.snoozed_until, None);
    assert!(active.updated_at >= resnoozed.updated_at);
}

fn subscription(endpoint: &str, events: Option<Vec<NotificationKind>>) -> PushSubscriptionInput {
    PushSubscriptionInput {
        endpoint: endpoint.into(),
        keys: keryx_core::types::PushKeys {
            p256dh: "BPUBLIC".into(),
            auth: "AUTH".into(),
        },
        events,
    }
}

#[tokio::test]
async fn uploads_and_serving_changes_record_events_for_opted_in_subscriptions() {
    let store = SeaOrmStore::open_test().await;
    let meta = UploadMetadata::default();
    let everything = store
        .upsert_push_subscription(&subscription("https://push.test/a", None))
        .await
        .unwrap();
    assert_eq!(everything.events, NotificationKind::ALL.to_vec());
    let revisions_only = store
        .upsert_push_subscription(&subscription(
            "https://push.test/b",
            Some(vec![NotificationKind::Revised]),
        ))
        .await
        .unwrap();

    let first = record(&store, "<title>Test</title>v1", None, &meta)
        .await
        .unwrap();
    let draft_id = first.draft_id.clone();
    record(
        &store,
        "<title>Test</title>v2",
        Some(draft_id.clone()),
        &meta,
    )
    .await
    .unwrap();
    store
        .set_availability(
            &draft_id,
            &AvailabilityUpdate::Snoozed {
                until: "2099-01-01T00:00:00Z".into(),
            },
        )
        .await
        .unwrap();
    store
        .set_availability(&draft_id, &AvailabilityUpdate::Active)
        .await
        .unwrap();
    store
        .set_availability(&draft_id, &AvailabilityUpdate::Disabled { reason: None })
        .await
        .unwrap();
    store
        .set_availability(
            &draft_id,
            &AvailabilityUpdate::Disabled {
                reason: Some("again".into()),
            },
        )
        .await
        .unwrap();
    store
        .set_availability(&draft_id, &AvailabilityUpdate::Active)
        .await
        .unwrap();

    let mut kinds = event_kinds(&store, &draft_id).await;
    kinds.sort();
    assert_eq!(
        kinds,
        vec![
            (
                "disabled".to_string(),
                format!("/?draft={draft_id}&view=disabled")
            ),
            (
                "enabled".to_string(),
                format!("/?draft={draft_id}&view=active")
            ),
            ("published".to_string(), format!("/d/{draft_id}")),
            ("revised".to_string(), format!("/d/{draft_id}/v/2")),
        ]
    );

    let due = store
        .due_deliveries("2099-01-01T00:00:00.000Z", 50)
        .await
        .unwrap();
    let mut addressed: Vec<(String, String)> = due
        .iter()
        .map(|delivery| {
            (
                delivery.subscription_id.clone(),
                delivery.event.kind.as_str().to_string(),
            )
        })
        .collect();
    addressed.sort();
    let mut expected = vec![
        (everything.id.clone(), "disabled".to_string()),
        (everything.id.clone(), "enabled".to_string()),
        (everything.id.clone(), "published".to_string()),
        (everything.id.clone(), "revised".to_string()),
        (revisions_only.id.clone(), "revised".to_string()),
    ];
    expected.sort();
    assert_eq!(addressed, expected);

    // Preferences change only when sent; keys always refresh.
    let updated = store
        .upsert_push_subscription(&subscription("https://push.test/b", None))
        .await
        .unwrap();
    assert_eq!(updated.id, revisions_only.id);
    assert_eq!(updated.events, vec![NotificationKind::Revised]);
    assert!(store
        .remove_push_subscription("https://push.test/b")
        .await
        .unwrap());
    assert!(!store
        .remove_push_subscription("https://push.test/b")
        .await
        .unwrap());
    assert_eq!(
        store
            .due_deliveries("2099-01-01T00:00:00.000Z", 50)
            .await
            .unwrap()
            .len(),
        4
    );
}

#[tokio::test]
async fn a_due_snooze_wakes_exactly_once_without_touching_the_draft() {
    let store = SeaOrmStore::open_test().await;
    let draft_id = record(
        &store,
        "<title>Test</title>v1",
        None,
        &UploadMetadata::default(),
    )
    .await
    .unwrap()
    .draft_id;
    store
        .upsert_push_subscription(&subscription("https://push.test/a", None))
        .await
        .unwrap();

    // Snoozes are validated as future on write, so age one directly.
    draft::Entity::update_many()
        .col_expr(
            draft::Column::SnoozedUntil,
            Expr::value("2026-01-01T09:00:00.000Z"),
        )
        .filter(draft::Column::Id.eq(draft_id.clone()))
        .exec(store.connection())
        .await
        .unwrap();
    assert_eq!(
        store
            .next_wake_at("2026-01-01T08:00:00.000Z")
            .await
            .unwrap()
            .as_deref(),
        Some("2026-01-01T09:00:00.000Z")
    );
    assert!(store
        .record_due_wakes("2026-01-01T08:59:59.999Z")
        .await
        .unwrap()
        .is_empty());

    let woke = store
        .record_due_wakes("2026-01-01T09:00:00.000Z")
        .await
        .unwrap();
    assert_eq!(woke.len(), 1);
    assert_eq!(woke[0].kind, NotificationKind::Woke);
    assert_eq!(woke[0].target, format!("/d/{draft_id}"));
    // A later pass, or a restart, finds nothing new to send.
    assert!(store
        .record_due_wakes("2026-01-02T00:00:00.000Z")
        .await
        .unwrap()
        .is_empty());
    assert_eq!(
        store
            .next_wake_at("2026-01-02T00:00:00.000Z")
            .await
            .unwrap(),
        None
    );
    let row = store.get_draft_summary(&draft_id).await.unwrap().unwrap();
    assert_eq!(
        row.snoozed_until.as_deref(),
        Some("2026-01-01T09:00:00.000Z")
    );
    assert_eq!(row.availability(), keryx_core::types::Availability::Active);
    assert_eq!(
        store
            .due_deliveries("2099-01-01T00:00:00.000Z", 50)
            .await
            .unwrap()
            .iter()
            .filter(|d| d.event.kind == NotificationKind::Woke)
            .count(),
        1
    );
}

#[tokio::test]
async fn a_draft_purged_between_resolve_and_record_is_not_found() {
    let store = SeaOrmStore::open_test().await;
    let meta = UploadMetadata::default();
    let first = record(&store, "<p>v1</p>", None, &meta).await.unwrap();

    // The caller resolved the target, then the draft went away while the
    // blob was being written.
    let (draft_id, created) = store
        .resolve_upload_target(Some(first.draft_id.clone()))
        .await
        .unwrap();
    assert!(!created);
    store.purge_draft(&draft_id).await.unwrap().unwrap();

    let version_id = new_internal_id();
    let result = store
        .record_upload(NewUpload {
            html: "<p>v2</p>",
            filename: None,
            object_key: format!("drafts/{draft_id}/{version_id}.html"),
            draft_id: draft_id.clone(),
            created,
            version_id,
            description: None,
            title_from_html: None,
            metadata: &meta,
            source_ip: None,
            user_agent: None,
            has_inline_script: false,
            external_image_hosts: &[],
        })
        .await;
    assert!(matches!(result, Err(UploadError::DraftNotFound)));
    let versions = draft_version::Entity::find()
        .count(store.connection())
        .await
        .unwrap();
    assert_eq!(versions, 0, "the rejected upload must leave no version row");
}

#[tokio::test]
async fn unknown_target_draft_is_not_found() {
    let store = SeaOrmStore::open_test().await;
    let result = record(
        &store,
        "<p>x</p>",
        Some("nope".into()),
        &UploadMetadata::default(),
    )
    .await;
    assert!(matches!(result, Err(UploadError::DraftNotFound)));
}

#[tokio::test]
async fn ping_answers_and_blob_records_cover_every_version() {
    let store = SeaOrmStore::open_test().await;
    store.ping().await.unwrap();
    let meta = UploadMetadata::default();
    let first = record(&store, "<p>v1</p>", None, &meta).await.unwrap();
    record(
        &store,
        "<p>version two</p>",
        Some(first.draft_id.clone()),
        &meta,
    )
    .await
    .unwrap();
    // Soft-deleted drafts still own their blobs.
    store.soft_delete_draft(&first.draft_id).await.unwrap();

    let records = store.blob_records().await.unwrap();
    assert_eq!(records.len(), 2);
    let sizes: Vec<i64> = records.iter().map(|record| record.file_size).collect();
    assert!(sizes.contains(&9) && sizes.contains(&18), "{sizes:?}");
    assert!(records
        .iter()
        .any(|r| r.content_hash == keryx_core::sha256_hex("<p>v1</p>")));
}

/// Every write transaction begins immediate, so concurrent uploads queue
/// on the busy timeout instead of failing with SQLITE_BUSY.
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_uploads_surface_no_sqlite_busy() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("keryx.db");
    let (store, _) = SeaOrmStore::open_sqlite(&path, false).await.unwrap();
    let store = std::sync::Arc::new(store);
    let meta = UploadMetadata::default();
    let draft_id = record(&store, "<p>v1</p>", None, &meta)
        .await
        .unwrap()
        .draft_id;

    // A second handle on the same file, as a concurrent process would be.
    let (other, _) = SeaOrmStore::open_sqlite(&path, false).await.unwrap();
    let other = std::sync::Arc::new(other);

    let mut uploads = Vec::new();
    for i in 0..24 {
        let store = if i % 2 == 0 {
            store.clone()
        } else {
            other.clone()
        };
        let draft_id = draft_id.clone();
        uploads.push(tokio::spawn(async move {
            let meta = UploadMetadata::default();
            record(&store, "<p>again</p>", Some(draft_id), &meta)
                .await
                .map(|outcome| outcome.version_number)
        }));
    }
    let mut numbers = Vec::new();
    for upload in uploads {
        numbers.push(upload.await.unwrap().expect("no upload may fail"));
    }
    numbers.sort_unstable();
    assert_eq!(
        numbers,
        (2..=25).collect::<Vec<i64>>(),
        "version numbers are gapless and unique"
    );
}

/// On Postgres MAX + 1 can collide between concurrent uploads; the draft row
/// lock means no caller ever sees it. On SQLite the immediate transaction
/// serialises them. Either way: no failures, no gaps, no duplicates.
#[tokio::test(flavor = "multi_thread")]
async fn concurrent_uploads_to_one_draft_number_their_versions_without_gaps() {
    let store = std::sync::Arc::new(SeaOrmStore::open_test().await);
    let meta = UploadMetadata::default();
    let draft_id = record(&store, "<p>v1</p>", None, &meta)
        .await
        .unwrap()
        .draft_id;

    let mut uploads = Vec::new();
    for _ in 0..16 {
        let (store, draft_id) = (store.clone(), draft_id.clone());
        uploads.push(tokio::spawn(async move {
            let meta = UploadMetadata::default();
            record(&store, "<p>again</p>", Some(draft_id), &meta)
                .await
                .map(|outcome| outcome.version_number)
        }));
    }
    let mut numbers = Vec::new();
    for upload in uploads {
        numbers.push(upload.await.unwrap().expect("no upload may fail"));
    }
    numbers.sort_unstable();
    assert_eq!(numbers, (2..=17).collect::<Vec<i64>>());
}

/// A rolling update starts a new pod while the old one runs. Both migrate;
/// the advisory lock makes the second wait and then find nothing pending.
#[tokio::test(flavor = "multi_thread")]
async fn two_migrators_started_together_against_postgres_both_succeed() {
    let Some(url) = std::env::var("KERYX_TEST_DATABASE_URL")
        .ok()
        .filter(|url| !url.is_empty())
    else {
        return; // Postgres only; SQLite has one process per file.
    };
    let schema = format!("keryx_test_{}", new_internal_id().to_lowercase());
    let admin = crate::connect::connect_postgres(&url, Some(1), None)
        .await
        .unwrap();
    admin
        .execute_unprepared(&format!("CREATE SCHEMA \"{schema}\""))
        .await
        .unwrap();

    let mut pods = Vec::new();
    for _ in 0..4 {
        let (url, schema) = (url.clone(), schema.clone());
        pods.push(tokio::spawn(async move {
            let db = crate::connect::connect_postgres(&url, Some(2), Some(&schema))
                .await
                .unwrap();
            crate::adopt::migrate_postgres(&db).await
        }));
    }
    let mut created = 0;
    for pod in pods {
        if pod.await.unwrap().expect("every migrator must succeed") == Adoption::Fresh {
            created += 1;
        }
    }
    assert_eq!(created, 1, "exactly one pod creates the schema");
}

// Compare complete records, including timestamps, blob pointers and the outbox.
async fn document_records(
    store: &SeaOrmStore,
) -> (
    Vec<draft::Model>,
    Vec<draft_version::Model>,
    Vec<notification_event::Model>,
    Vec<notification_delivery::Model>,
    Vec<push_subscription::Model>,
) {
    let db = store.connection();
    (
        draft::Entity::find()
            .order_by_asc(draft::Column::Id)
            .all(db)
            .await
            .unwrap(),
        draft_version::Entity::find()
            .order_by_asc(draft_version::Column::Id)
            .all(db)
            .await
            .unwrap(),
        notification_event::Entity::find().all(db).await.unwrap(),
        notification_delivery::Entity::find().all(db).await.unwrap(),
        push_subscription::Entity::find().all(db).await.unwrap(),
    )
}

#[tokio::test]
async fn tags_only_write_organisation_metadata_and_survive_document_changes() {
    let store = SeaOrmStore::open_test().await;
    store
        .upsert_push_subscription(&subscription("https://push.test/tags", None))
        .await
        .unwrap();
    let first = record(
        &store,
        "<title>Tagged</title>exact original bytes",
        None,
        &UploadMetadata::default(),
    )
    .await
    .unwrap();
    let id = &first.draft_id;
    assert!(store.dashboard_tags().await.unwrap().assignments.is_empty());
    let before = document_records(&store).await;
    let tags = store
        .attach_tag(id, "  Release   Planning  ")
        .await
        .unwrap();
    assert_eq!(tags[0].name, "release planning");
    assert!(!before.2.is_empty());
    assert!(!before.3.is_empty());
    assert!(!before.4.is_empty());
    assert_eq!(document_records(&store).await, before);
    assert_eq!(
        store.attach_tag(id, "RELEASE PLANNING").await.unwrap(),
        tags
    );
    assert_eq!(document_records(&store).await, before);
    let other = record(&store, "other", None, &UploadMetadata::default())
        .await
        .unwrap();
    let before = document_records(&store).await;
    assert_eq!(
        store
            .attach_tag(&other.draft_id, "release planning")
            .await
            .unwrap(),
        tags
    );
    assert_eq!(document_records(&store).await, before);
    assert!(store.detach_tag(id, &tags[0].id).await.unwrap().is_empty());
    assert_eq!(document_records(&store).await, before);
    assert!(store.detach_tag(id, &tags[0].id).await.unwrap().is_empty());
    assert_eq!(document_records(&store).await, before);
    assert_eq!(store.dashboard_tags().await.unwrap().catalogue, tags);
    store.attach_tag(id, "release planning").await.unwrap();
    record(
        &store,
        "updated bytes",
        Some(id.clone()),
        &UploadMetadata::default(),
    )
    .await
    .unwrap();
    store
        .set_availability(id, &AvailabilityUpdate::Disabled { reason: None })
        .await
        .unwrap();
    assert_eq!(store.dashboard_tags().await.unwrap().assignments[id], tags);
    store
        .set_availability(
            id,
            &AvailabilityUpdate::Snoozed {
                until: "2099-01-01T00:00:00Z".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(store.dashboard_tags().await.unwrap().assignments[id], tags);
    store.soft_delete_draft(id).await.unwrap();
    assert!(!store
        .dashboard_tags()
        .await
        .unwrap()
        .assignments
        .contains_key(id));
    assert!(matches!(
        store.attach_tag(id, "nope").await,
        Err(TagError::DraftNotFound)
    ));
    assert!(matches!(
        store.detach_tag(id, &tags[0].id).await,
        Err(TagError::DraftNotFound)
    ));
    store.purge_deleted_drafts().await.unwrap();
    assert!(draft_tag::Entity::find()
        .filter(draft_tag::Column::DraftId.eq(id))
        .all(store.connection())
        .await
        .unwrap()
        .is_empty());
    store.purge_draft(&other.draft_id).await.unwrap();
    assert!(draft_tag::Entity::find()
        .all(store.connection())
        .await
        .unwrap()
        .is_empty());
    assert_eq!(store.dashboard_tags().await.unwrap().catalogue, tags);
}

#[tokio::test]
async fn tag_limits_and_concurrent_assignments_are_atomic() {
    let store = SeaOrmStore::open_test().await;
    let first = record(&store, "one", None, &UploadMetadata::default())
        .await
        .unwrap();
    let second = record(&store, "two", None, &UploadMetadata::default())
        .await
        .unwrap();
    let before = document_records(&store).await;
    let (a, b, c) = tokio::join!(
        store.attach_tag(&first.draft_id, "Shared"),
        store.attach_tag(&first.draft_id, " shared "),
        store.attach_tag(&second.draft_id, "SHARED"),
    );
    assert_eq!(a.as_ref().unwrap(), b.as_ref().unwrap());
    assert_eq!(a.unwrap(), c.unwrap());
    for invalid in [
        "",
        "   ",
        "a\tb",
        "x\n",
        "é",
        "<tag>",
        "a/b",
        "abcdefghijklmnopqrstuvwxyz1234567",
    ] {
        assert!(
            matches!(
                store.attach_tag(&first.draft_id, invalid).await,
                Err(TagError::Invalid(_))
            ),
            "{invalid:?}"
        );
    }
    store
        .attach_tag(&first.draft_id, "abcdefghijklmnopqrstuvwxyz123456")
        .await
        .unwrap();
    for n in 0..17 {
        store
            .attach_tag(&first.draft_id, &format!("tag-{n}"))
            .await
            .unwrap();
    }
    let (a, b) = tokio::join!(
        store.attach_tag(&first.draft_id, "last-a"),
        store.attach_tag(&first.draft_id, "last-b")
    );
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert!(matches!(a.err().or(b.err()), Some(TagError::Invalid(_))));
    assert_eq!(
        store
            .attach_tag(&first.draft_id, "shared")
            .await
            .unwrap()
            .len(),
        20
    );
    assert_eq!(store.dashboard_tags().await.unwrap().catalogue.len(), 20);
    assert_eq!(document_records(&store).await, before);
    assert!(matches!(
        store.attach_tag("missing", "valid").await,
        Err(TagError::DraftNotFound)
    ));
}

#[tokio::test]
async fn populated_baseline_upgrade_preserves_records_and_tags_persist_on_reopen() {
    use sea_orm_migration::MigratorTrait;
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("upgrade.sqlite");
    let db = crate::connect::connect_sqlite(&path, None).await.unwrap();
    crate::migration::Migrator::up(&db, Some(1)).await.unwrap();
    let store = SeaOrmStore { db };
    let uploaded = record(
        &store,
        "immutable content",
        None,
        &UploadMetadata::default(),
    )
    .await
    .unwrap();
    let before = document_records(&store).await;
    crate::migration::Migrator::up(store.connection(), None)
        .await
        .unwrap();
    assert_eq!(document_records(&store).await, before);
    assert!(store.dashboard_tags().await.unwrap().catalogue.is_empty());
    let tags = store
        .attach_tag(&uploaded.draft_id, "persisted")
        .await
        .unwrap();
    assert_eq!(document_records(&store).await, before);
    store.db.close().await.unwrap();
    let (store, _) = SeaOrmStore::open_sqlite(&path, false).await.unwrap();
    assert_eq!(
        store.dashboard_tags().await.unwrap().assignments[&uploaded.draft_id],
        tags
    );
    assert_eq!(document_records(&store).await, before);
}

#[tokio::test]
async fn tags_upgrade_a_populated_baseline_on_either_backend() {
    use sea_orm_migration::MigratorTrait;
    let store = SeaOrmStore::open_test().await;
    // Only this test's fresh database/schema is rolled back to the baseline.
    crate::migration::Migrator::down(store.connection(), Some(1))
        .await
        .unwrap();
    record(&store, "baseline", None, &UploadMetadata::default())
        .await
        .unwrap();
    let before = document_records(&store).await;
    crate::migration::Migrator::up(store.connection(), None)
        .await
        .unwrap();
    assert_eq!(document_records(&store).await, before);
    assert!(store.dashboard_tags().await.unwrap().catalogue.is_empty());
    assert!(store.dashboard_tags().await.unwrap().assignments.is_empty());
}
