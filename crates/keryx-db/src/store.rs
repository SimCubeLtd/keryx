//! The metadata seam. The server holds an `Arc<dyn DraftStore>` and never
//! names a backend, exactly as it holds an `Arc<dyn BlobBackend>`.
//! [`SeaOrmStore`] is the one implementation: every query is written once and
//! runs on SQLite or Postgres, chosen at runtime by the connection.

use std::path::Path;

use anyhow::{Context, Result};
use async_trait::async_trait;
use chrono::Utc;
use sea_orm::sea_query::{Alias, Expr, ExprTrait, OnConflict, Query, QueryStatementBuilder};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseConnection,
    DatabaseTransaction, DbBackend, DbErr, EntityTrait, FromQueryResult, JoinType, Order,
    QueryFilter, QueryOrder, QuerySelect, RelationTrait, SqliteTransactionMode, Statement,
    TransactionOptions, TransactionTrait, TryInsertResult,
};

use keryx_core::ids::{new_draft_id, new_internal_id};
use keryx_core::now;
use keryx_core::types::{
    AvailabilityUpdate, DraftSummary, NotificationEvent, NotificationKind, PushSubscriptionInput,
    PushSubscriptionSummary, VersionInfo,
};

use crate::adopt::{self, Adoption};
use crate::entity::{
    draft, draft_version, notification_delivery, notification_event, push_subscription,
};
use crate::types::{
    normalize_wake_time, AvailabilityError, BlobRecord, NewUpload, PendingDelivery, ServedVersion,
    UploadError, UploadOutcome, DEFAULT_DISABLE_REASON,
};

/// Draft and version metadata, availability, and the notification outbox.
#[async_trait]
pub trait DraftStore: Send + Sync {
    /// A trivial query, for `/healthz`.
    async fn ping(&self) -> Result<()>;

    /// Resolve where an upload lands: the existing live draft it names, or a
    /// freshly minted draft id. Answers `(draft_id, created)`. A cheap read,
    /// so the caller can write the blob afterwards with no transaction open.
    async fn resolve_upload_target(
        &self,
        draft_id: Option<String>,
    ) -> Result<(String, bool), UploadError>;

    /// Record the metadata for a blob the caller has already written.
    ///
    /// Ordering invariant: the blob lands before this transaction commits, so
    /// a crash leaves at most an orphan blob and never a version row pointing
    /// at nothing. The draft can be deleted or purged after
    /// `resolve_upload_target`, so an existing draft is re-checked inside the
    /// transaction; on `DraftNotFound` the caller removes its blob.
    async fn record_upload(&self, upload: NewUpload<'_>) -> Result<UploadOutcome, UploadError>;

    /// Every blob any version row points at, including versions of
    /// soft-deleted and disabled drafts: those rows still own their objects.
    async fn blob_records(&self) -> Result<Vec<BlobRecord>>;

    /// A publicly servable version: the draft must exist and be neither
    /// deleted nor disabled. `version` of None means the current version.
    async fn find_public_version(
        &self,
        draft_id: &str,
        version: Option<i64>,
    ) -> Result<Option<ServedVersion>>;

    /// Every live draft, newest first. Snoozed drafts are included; callers
    /// derive the display state. `public_url` and `raw_url` are filled in by
    /// the server layer.
    async fn list_drafts(&self) -> Result<Vec<DraftSummary>>;
    async fn get_draft_summary(&self, draft_id: &str) -> Result<Option<DraftSummary>>;
    /// A draft's versions, newest first.
    async fn list_versions(&self, draft_id: &str) -> Result<Vec<VersionInfo>>;

    /// Soft-delete: versions stay but the draft stops serving and leaves
    /// listings. False when there was no live draft to delete.
    async fn soft_delete_draft(&self, draft_id: &str) -> Result<bool>;
    /// Hard delete. Answers the removed versions' object keys so the caller
    /// can delete the blobs, or None when the draft id does not exist at all.
    /// Soft-deleted drafts can be purged; that is the point.
    async fn purge_draft(&self, draft_id: &str) -> Result<Option<Vec<String>>>;
    /// Hard-delete everything soft-deleted. Answers the number of drafts
    /// removed and the object keys of their blobs.
    async fn purge_deleted_drafts(&self) -> Result<(usize, Vec<String>)>;

    /// The one mutation that changes availability. Each state clears the
    /// fields of the others, so a row is never both snoozed and disabled, and
    /// every manual transition bumps `updated_at`.
    async fn set_availability(
        &self,
        draft_id: &str,
        update: &AvailabilityUpdate,
    ) -> Result<DraftSummary, AvailabilityError>;

    /// Store an event and queue one delivery per subscription that opted in
    /// to its kind. Idempotent by key: a repeated key changes nothing and
    /// answers false.
    async fn record_event(&self, event: &NotificationEvent) -> Result<bool>;
    async fn get_push_subscription(
        &self,
        endpoint: &str,
    ) -> Result<Option<PushSubscriptionSummary>>;
    /// Insert or refresh a browser subscription by endpoint. Keys are always
    /// replaced; preferences change only when the caller sends them.
    async fn upsert_push_subscription(
        &self,
        input: &PushSubscriptionInput,
    ) -> Result<PushSubscriptionSummary>;
    async fn remove_push_subscription(&self, endpoint: &str) -> Result<bool>;
    async fn remove_push_subscription_by_id(&self, id: &str) -> Result<()>;

    /// Deliveries due at `now`, earliest first.
    async fn due_deliveries(&self, now: &str, limit: usize) -> Result<Vec<PendingDelivery>>;
    /// The delivery is finished, whether it succeeded or was given up on.
    async fn delivery_done(&self, event_key: &str, subscription_id: &str) -> Result<()>;
    async fn delivery_retry(
        &self,
        event_key: &str,
        subscription_id: &str,
        attempts: i64,
        next_attempt_at: &str,
    ) -> Result<()>;
    async fn next_delivery_at(&self) -> Result<Option<String>>;
    /// Turn every expired snooze that has not woken yet into a Plan woke
    /// event. The wake key carries the snooze timestamp, so this is safe on
    /// every pass and after a restart; nothing on the draft row changes.
    async fn record_due_wakes(&self, now: &str) -> Result<Vec<NotificationEvent>>;
    /// The nearest future wake time across live, snoozed drafts.
    async fn next_wake_at(&self, now: &str) -> Result<Option<String>>;
}

/// Which database to open. SQLite at a path is the default; a Postgres URL
/// is the opt-in that lets Keryx run with no persistent volume.
#[derive(Debug, Clone)]
pub enum DatabaseConfig {
    Sqlite {
        path: std::path::PathBuf,
        /// Snapshot a legacy database before adopting it.
        backup: bool,
        pool_size: Option<u32>,
    },
    Postgres {
        /// `postgres://...`, with TLS set through `sslmode` and `sslrootcert`.
        url: String,
        pool_size: Option<u32>,
    },
}

impl DatabaseConfig {
    /// Where this is, safe to print: never the credentials in a URL.
    pub fn describe(&self) -> String {
        match self {
            DatabaseConfig::Sqlite { path, .. } => path.display().to_string(),
            DatabaseConfig::Postgres { url, .. } => crate::connect::redact_url(url),
        }
    }
}

pub struct SeaOrmStore {
    db: DatabaseConnection,
}

impl SeaOrmStore {
    /// Open the database `config` names, migrating it first. On SQLite that
    /// includes adopting a legacy database in place.
    pub async fn open(config: &DatabaseConfig) -> Result<(Self, Adoption)> {
        match config {
            DatabaseConfig::Sqlite {
                path,
                backup,
                pool_size,
            } => {
                let (db, adoption) = adopt::open_sqlite_pooled(path, *backup, *pool_size).await?;
                Ok((Self { db }, adoption))
            }
            DatabaseConfig::Postgres { url, pool_size } => {
                let db = crate::connect::connect_postgres(url, *pool_size, None).await?;
                let adoption = adopt::migrate_postgres(&db).await?;
                Ok((Self { db }, adoption))
            }
        }
    }

    /// Open the SQLite database at `path` with the default pool. `backup`
    /// snapshots a legacy database before its first write.
    pub async fn open_sqlite(path: &Path, backup: bool) -> Result<(Self, Adoption)> {
        Self::open(&DatabaseConfig::Sqlite {
            path: path.to_path_buf(),
            backup,
            pool_size: None,
        })
        .await
    }

    /// A private, empty store for one test. In-memory SQLite by default. With
    /// `KERYX_TEST_DATABASE_URL` set to a Postgres URL, a fresh schema in that
    /// database instead, so the same suite runs against both backends.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn open_test() -> Self {
        use crate::connect::{connect_postgres, connect_sqlite_memory};
        let Some(url) = std::env::var("KERYX_TEST_DATABASE_URL")
            .ok()
            .filter(|url| !url.is_empty())
        else {
            let db = connect_sqlite_memory()
                .await
                .expect("opening in-memory SQLite");
            adopt::adopt(&db, None)
                .await
                .expect("migrating in-memory SQLite");
            return Self { db };
        };

        let schema = format!("keryx_test_{}", new_internal_id().to_lowercase());
        let admin = connect_postgres(&url, Some(1), None)
            .await
            .expect("connecting to the test Postgres");
        admin
            .execute_unprepared(&format!("CREATE SCHEMA \"{schema}\""))
            .await
            .expect("creating a test schema");
        admin.close().await.expect("closing the admin connection");

        let db = connect_postgres(&url, None, Some(&schema))
            .await
            .expect("connecting to the test schema");
        adopt::migrate_postgres(&db)
            .await
            .expect("migrating the test schema");
        Self { db }
    }

    /// The underlying connection, for tests that need to look at raw rows.
    #[cfg(any(test, feature = "test-support"))]
    pub fn connection(&self) -> &DatabaseConnection {
        &self.db
    }

    /// One value from one row of raw SQL, for tests that assert on rows no
    /// store method exposes. `None` when the value is NULL or no row matched.
    #[cfg(any(test, feature = "test-support"))]
    pub async fn peek<T: sea_orm::TryGetable>(&self, sql: &str) -> Option<T> {
        self.db
            .query_one_raw(Statement::from_string(self.db.get_database_backend(), sql))
            .await
            .expect("peek query")
            .and_then(|row| row.try_get_by_index::<Option<T>>(0).expect("peek column"))
    }

    /// Every write transaction begins immediate. A deferred read-then-write
    /// transaction fails with SQLITE_BUSY regardless of the busy timeout, and
    /// record_upload is exactly that shape. Ignored on Postgres.
    async fn begin_write(&self) -> Result<DatabaseTransaction, DbErr> {
        self.db
            .begin_with_options(TransactionOptions {
                sqlite_transaction_mode: Some(SqliteTransactionMode::Immediate),
                ..Default::default()
            })
            .await
    }
}

impl From<DbErr> for UploadError {
    fn from(error: DbErr) -> Self {
        UploadError::Other(error.into())
    }
}

impl From<DbErr> for AvailabilityError {
    fn from(error: DbErr) -> Self {
        AvailabilityError::Other(error.into())
    }
}

/// The listing row: a draft, its current version, and its version count.
#[derive(FromQueryResult)]
struct SummaryRow {
    draft_id: String,
    title: String,
    description: Option<String>,
    repo_org: Option<String>,
    repo_name: Option<String>,
    repo_host: Option<String>,
    created_at: String,
    updated_at: String,
    disabled_at: Option<String>,
    latest_version_number: Option<i64>,
    latest_version_at: Option<String>,
    latest_git_branch: Option<String>,
    latest_git_commit_sha: Option<String>,
    latest_git_commit_subject: Option<String>,
    latest_git_dirty: Option<bool>,
    version_count: i64,
    snoozed_until: Option<String>,
}

impl From<SummaryRow> for DraftSummary {
    fn from(row: SummaryRow) -> Self {
        DraftSummary {
            draft_id: row.draft_id,
            title: row.title,
            description: row.description,
            repo_org: row.repo_org,
            repo_name: row.repo_name,
            repo_host: row.repo_host,
            created_at: row.created_at,
            updated_at: row.updated_at,
            disabled: row.disabled_at.is_some(),
            latest_version_number: row.latest_version_number,
            latest_version_at: row.latest_version_at,
            latest_git_branch: row.latest_git_branch,
            latest_git_commit_sha: row.latest_git_commit_sha,
            latest_git_commit_subject: row.latest_git_commit_subject,
            latest_git_dirty: row.latest_git_dirty,
            version_count: row.version_count,
            snoozed_until: row.snoozed_until,
            public_url: String::new(),
            raw_url: String::new(),
        }
    }
}

/// Live drafts joined to their current version, with a correlated count of
/// all versions. Repository and git fields come from the current version.
fn summaries() -> sea_orm::Select<draft::Entity> {
    use draft::Column as D;
    use draft_version::Column as V;
    let current = Alias::new("cv");
    let counted = Alias::new("counted");

    let version_count = Query::select()
        .expr(Expr::col((counted.clone(), V::Id)).count())
        .from_as(draft_version::Entity, counted.clone())
        .and_where(Expr::col((counted, V::DraftId)).equals((draft::Entity, D::Id)))
        .to_owned();
    let current_version = draft::Entity::belongs_to(draft_version::Entity)
        .from(D::CurrentVersionId)
        .to(V::Id)
        .into();
    let cv = |column: V| Expr::col((current.clone(), column));

    draft::Entity::find()
        .select_only()
        .column_as(D::Id, "draft_id")
        .column(D::Title)
        .column(D::Description)
        .expr_as(cv(V::RepoOrg), "repo_org")
        .expr_as(cv(V::RepoName), "repo_name")
        .expr_as(cv(V::RepoHost), "repo_host")
        .column(D::CreatedAt)
        .column(D::UpdatedAt)
        .column(D::DisabledAt)
        .expr_as(cv(V::VersionNumber), "latest_version_number")
        .expr_as(cv(V::CreatedAt), "latest_version_at")
        .expr_as(cv(V::GitBranch), "latest_git_branch")
        .expr_as(cv(V::GitCommitSha), "latest_git_commit_sha")
        .expr_as(cv(V::GitCommitSubject), "latest_git_commit_subject")
        .expr_as(cv(V::GitDirty), "latest_git_dirty")
        .expr_as(
            sea_orm::sea_query::SimpleExpr::SubQuery(
                None,
                Box::new(version_count.into_sub_query_statement()),
            ),
            "version_count",
        )
        .column(D::SnoozedUntil)
        .join_as(JoinType::LeftJoin, current_version, current)
        .filter(D::DeletedAt.is_null())
}

async fn draft_summary<C: ConnectionTrait>(db: &C, draft_id: &str) -> Result<Option<DraftSummary>> {
    Ok(summaries()
        .filter(draft::Column::Id.eq(draft_id))
        .into_model::<SummaryRow>()
        .one(db)
        .await?
        .map(DraftSummary::from))
}

/// Store an event and queue its deliveries on the caller's connection, which
/// is a transaction whenever the event belongs to a larger change.
async fn record_event_on<C: ConnectionTrait>(
    db: &C,
    event: &NotificationEvent,
) -> Result<bool, DbErr> {
    let inserted = notification_event::Entity::insert(notification_event::ActiveModel {
        key: Set(event.key.clone()),
        kind: Set(event.kind.as_str().to_string()),
        draft_id: Set(event.draft_id.clone()),
        title: Set(event.title.clone()),
        body: Set(event.body.clone()),
        target: Set(event.target.clone()),
        created_at: Set(event.created_at.clone()),
    })
    .on_conflict_do_nothing()
    .exec_without_returning(db)
    .await?;
    if !matches!(inserted, TryInsertResult::Inserted(n) if n > 0) {
        return Ok(false);
    }

    // `events` is a JSON array of kind names stored as text.
    let opted_in: Vec<String> = push_subscription::Entity::find()
        .select_only()
        .column(push_subscription::Column::Id)
        .filter(push_subscription::Column::Events.like(format!("%\"{}\"%", event.kind.as_str())))
        .into_tuple()
        .all(db)
        .await?;
    if !opted_in.is_empty() {
        notification_delivery::Entity::insert_many(opted_in.into_iter().map(|subscription_id| {
            notification_delivery::ActiveModel {
                event_key: Set(event.key.clone()),
                subscription_id: Set(subscription_id),
                attempts: Set(0),
                next_attempt_at: Set(event.created_at.clone()),
            }
        }))
        .exec_without_returning(db)
        .await?;
    }
    Ok(true)
}

fn subscription_summary(model: push_subscription::Model) -> PushSubscriptionSummary {
    PushSubscriptionSummary {
        id: model.id,
        endpoint: model.endpoint,
        events: serde_json::from_str(&model.events).unwrap_or_default(),
        updated_at: model.updated_at,
    }
}

/// A due delivery joined to its event and subscription.
#[derive(FromQueryResult)]
struct DeliveryRow {
    key: String,
    kind: String,
    draft_id: String,
    title: String,
    body: String,
    target: String,
    event_created_at: String,
    subscription_id: String,
    endpoint: String,
    p256dh: String,
    auth: String,
    attempts: i64,
}

#[async_trait]
impl DraftStore for SeaOrmStore {
    async fn ping(&self) -> Result<()> {
        let backend: DbBackend = self.db.get_database_backend();
        self.db
            .query_one_raw(Statement::from_string(backend, "SELECT 1"))
            .await?;
        Ok(())
    }

    async fn resolve_upload_target(
        &self,
        draft_id: Option<String>,
    ) -> Result<(String, bool), UploadError> {
        let Some(id) = draft_id else {
            return Ok((new_draft_id(), true));
        };
        let live = draft::Entity::find_by_id(id)
            .filter(draft::Column::DeletedAt.is_null())
            .one(&self.db)
            .await?;
        match live {
            Some(draft) => Ok((draft.id, false)),
            None => Err(UploadError::DraftNotFound),
        }
    }

    async fn record_upload(&self, upload: NewUpload<'_>) -> Result<UploadOutcome, UploadError> {
        let tx = self.begin_write().await?;
        let timestamp = now();
        let draft_id = upload.draft_id;
        let created = upload.created;
        let version_id = upload.version_id;

        let existing = if created {
            None
        } else {
            let mut live = draft::Entity::find_by_id(draft_id.clone())
                .filter(draft::Column::DeletedAt.is_null());
            // MAX + 1 can collide between concurrent uploads to one draft.
            // UNIQUE (draft_id, version_number) makes that an error rather
            // than corruption; locking the draft row means a user never sees
            // it. SQLite has no row locks and needs none: the immediate
            // transaction already serialises writers.
            if tx.get_database_backend() == DbBackend::Postgres {
                live = live.lock_exclusive();
            }
            match live.one(&tx).await? {
                Some(draft) => Some(draft),
                None => return Err(UploadError::DraftNotFound),
            }
        };

        let version_number = if created {
            1
        } else {
            let highest: Option<Option<i64>> = draft_version::Entity::find()
                .select_only()
                .expr(draft_version::Column::VersionNumber.max())
                .filter(draft_version::Column::DraftId.eq(draft_id.clone()))
                .into_tuple()
                .one(&tx)
                .await?;
            highest.flatten().unwrap_or(0) + 1
        };

        let title = upload
            .title_from_html
            .clone()
            .or_else(|| existing.as_ref().map(|draft| draft.title.clone()))
            .or_else(|| upload.filename.clone())
            .unwrap_or_else(|| "Untitled Draft".to_string());
        let image_hosts_json = serde_json::to_string(upload.external_image_hosts)
            .map_err(|e| UploadError::Other(e.into()))?;
        let m = upload.metadata;

        if created {
            draft::ActiveModel {
                id: Set(draft_id.clone()),
                title: Set(title.clone()),
                description: Set(upload.description.clone()),
                current_version_id: Set(None),
                repo_org: Set(m.repo_org.clone()),
                repo_name: Set(m.repo_name.clone()),
                repo_host: Set(m.repo_host.clone()),
                created_at: Set(timestamp.clone()),
                updated_at: Set(timestamp.clone()),
                deleted_at: Set(None),
                disabled_at: Set(None),
                disabled_reason: Set(None),
                snoozed_until: Set(None),
            }
            .insert(&tx)
            .await?;
        }

        draft_version::ActiveModel {
            id: Set(version_id.clone()),
            draft_id: Set(draft_id.clone()),
            version_number: Set(version_number),
            object_key: Set(upload.object_key),
            content_hash: Set(keryx_core::sha256_hex(upload.html)),
            file_size: Set(upload.html.len() as i64),
            created_at: Set(timestamp.clone()),
            repo_org: Set(m.repo_org.clone()),
            repo_name: Set(m.repo_name.clone()),
            repo_host: Set(m.repo_host.clone()),
            source_ip: Set(upload.source_ip),
            user_agent: Set(upload.user_agent),
            cli_version: Set(m.cli_version.clone()),
            git_branch: Set(m.git_branch.clone()),
            git_commit_sha: Set(m.git_commit_sha.clone()),
            git_commit_subject: Set(m.git_commit_subject.clone()),
            git_dirty: Set(m.git_dirty),
            original_filename: Set(upload.filename),
            has_inline_script: Set(upload.has_inline_script),
            external_image_hosts: Set(image_hosts_json),
        }
        .insert(&tx)
        .await?;

        // The draft always follows its newest upload. A description is only
        // replaced when this upload brings one.
        let mut point_at_version = draft::Entity::update_many()
            .col_expr(
                draft::Column::CurrentVersionId,
                Expr::value(version_id.clone()),
            )
            .col_expr(draft::Column::Title, Expr::value(title.clone()))
            .col_expr(draft::Column::RepoOrg, Expr::value(m.repo_org.clone()))
            .col_expr(draft::Column::RepoName, Expr::value(m.repo_name.clone()))
            .col_expr(draft::Column::RepoHost, Expr::value(m.repo_host.clone()))
            .col_expr(draft::Column::UpdatedAt, Expr::value(timestamp.clone()))
            .filter(draft::Column::Id.eq(draft_id.clone()));
        if let Some(description) = upload.description {
            point_at_version =
                point_at_version.col_expr(draft::Column::Description, Expr::value(description));
        }
        point_at_version.exec(&tx).await?;

        let event = if created {
            NotificationEvent::published(&draft_id, &title, &version_id, &timestamp)
        } else {
            NotificationEvent::revised(&draft_id, &title, &version_id, version_number, &timestamp)
        };
        record_event_on(&tx, &event).await?;
        tx.commit().await?;

        Ok(UploadOutcome {
            draft_id,
            version_id,
            version_number,
            title,
            created,
        })
    }

    async fn blob_records(&self) -> Result<Vec<BlobRecord>> {
        let versions = draft_version::Entity::find()
            .order_by_asc(draft_version::Column::ObjectKey)
            .all(&self.db)
            .await?;
        Ok(versions
            .into_iter()
            .map(|version| BlobRecord {
                object_key: version.object_key,
                content_hash: version.content_hash,
                file_size: version.file_size,
            })
            .collect())
    }

    async fn find_public_version(
        &self,
        draft_id: &str,
        version: Option<i64>,
    ) -> Result<Option<ServedVersion>> {
        let Some(draft) = draft::Entity::find_by_id(draft_id)
            .filter(draft::Column::DeletedAt.is_null())
            .filter(draft::Column::DisabledAt.is_null())
            .one(&self.db)
            .await?
        else {
            return Ok(None);
        };

        let found = match (version, draft.current_version_id) {
            (Some(n), _) => {
                draft_version::Entity::find()
                    .filter(draft_version::Column::DraftId.eq(draft.id.clone()))
                    .filter(draft_version::Column::VersionNumber.eq(n))
                    .one(&self.db)
                    .await?
            }
            (None, Some(current)) => {
                draft_version::Entity::find_by_id(current)
                    .one(&self.db)
                    .await?
            }
            (None, None) => None,
        };
        Ok(found.map(|version| ServedVersion {
            draft_id: draft.id,
            version_number: version.version_number,
            object_key: version.object_key,
            created_at: version.created_at,
        }))
    }

    async fn list_drafts(&self) -> Result<Vec<DraftSummary>> {
        Ok(summaries()
            .order_by(draft::Column::UpdatedAt, Order::Desc)
            .into_model::<SummaryRow>()
            .all(&self.db)
            .await?
            .into_iter()
            .map(DraftSummary::from)
            .collect())
    }

    async fn get_draft_summary(&self, draft_id: &str) -> Result<Option<DraftSummary>> {
        draft_summary(&self.db, draft_id).await
    }

    async fn list_versions(&self, draft_id: &str) -> Result<Vec<VersionInfo>> {
        let versions = draft_version::Entity::find()
            .filter(draft_version::Column::DraftId.eq(draft_id))
            .order_by_desc(draft_version::Column::VersionNumber)
            .all(&self.db)
            .await?;
        Ok(versions
            .into_iter()
            .map(|version| VersionInfo {
                id: version.id,
                version_number: version.version_number,
                created_at: version.created_at,
                repo_org: version.repo_org,
                repo_name: version.repo_name,
                repo_host: version.repo_host,
                git_branch: version.git_branch,
                git_commit_sha: version.git_commit_sha,
                git_commit_subject: version.git_commit_subject,
                git_dirty: version.git_dirty,
                file_size: version.file_size,
                original_filename: version.original_filename,
            })
            .collect())
    }

    async fn soft_delete_draft(&self, draft_id: &str) -> Result<bool> {
        let timestamp = now();
        let changed = draft::Entity::update_many()
            .col_expr(draft::Column::DeletedAt, Expr::value(timestamp.clone()))
            .col_expr(draft::Column::UpdatedAt, Expr::value(timestamp))
            .filter(draft::Column::Id.eq(draft_id))
            .filter(draft::Column::DeletedAt.is_null())
            .exec(&self.db)
            .await?;
        Ok(changed.rows_affected > 0)
    }

    async fn purge_draft(&self, draft_id: &str) -> Result<Option<Vec<String>>> {
        let tx = self.begin_write().await?;
        if draft::Entity::find_by_id(draft_id)
            .one(&tx)
            .await?
            .is_none()
        {
            return Ok(None);
        }
        let keys: Vec<String> = draft_version::Entity::find()
            .select_only()
            .column(draft_version::Column::ObjectKey)
            .filter(draft_version::Column::DraftId.eq(draft_id))
            .into_tuple()
            .all(&tx)
            .await?;
        draft_version::Entity::delete_many()
            .filter(draft_version::Column::DraftId.eq(draft_id))
            .exec(&tx)
            .await?;
        draft::Entity::delete_by_id(draft_id).exec(&tx).await?;
        tx.commit().await?;
        Ok(Some(keys))
    }

    async fn purge_deleted_drafts(&self) -> Result<(usize, Vec<String>)> {
        let tx = self.begin_write().await?;
        let soft_deleted = || {
            Query::select()
                .column(draft::Column::Id)
                .from(draft::Entity)
                .and_where(draft::Column::DeletedAt.is_not_null())
                .to_owned()
        };
        let keys: Vec<String> = draft_version::Entity::find()
            .select_only()
            .column(draft_version::Column::ObjectKey)
            .filter(draft_version::Column::DraftId.in_subquery(soft_deleted()))
            .into_tuple()
            .all(&tx)
            .await?;
        draft_version::Entity::delete_many()
            .filter(draft_version::Column::DraftId.in_subquery(soft_deleted()))
            .exec(&tx)
            .await?;
        let removed = draft::Entity::delete_many()
            .filter(draft::Column::DeletedAt.is_not_null())
            .exec(&tx)
            .await?;
        tx.commit().await?;
        Ok((removed.rows_affected as usize, keys))
    }

    async fn set_availability(
        &self,
        draft_id: &str,
        update: &AvailabilityUpdate,
    ) -> Result<DraftSummary, AvailabilityError> {
        let tx = self.begin_write().await?;
        let timestamp = now();

        let Some(previous) = draft::Entity::find_by_id(draft_id)
            .filter(draft::Column::DeletedAt.is_null())
            .one(&tx)
            .await?
        else {
            return Err(AvailabilityError::DraftNotFound);
        };
        let was_disabled = previous.disabled_at.is_some();

        let none = || Expr::value(Option::<String>::None);
        let (disabled_at, disabled_reason, snoozed_until) = match update {
            AvailabilityUpdate::Active => (none(), none(), none()),
            AvailabilityUpdate::Snoozed { until } => {
                let until = normalize_wake_time(until, Utc::now())?;
                (none(), none(), Expr::value(until))
            }
            AvailabilityUpdate::Disabled { reason } => (
                Expr::value(timestamp.clone()),
                Expr::value(reason.as_deref().unwrap_or(DEFAULT_DISABLE_REASON)),
                none(),
            ),
        };
        draft::Entity::update_many()
            .col_expr(draft::Column::DisabledAt, disabled_at)
            .col_expr(draft::Column::DisabledReason, disabled_reason)
            .col_expr(draft::Column::SnoozedUntil, snoozed_until)
            .col_expr(draft::Column::UpdatedAt, Expr::value(timestamp.clone()))
            .filter(draft::Column::Id.eq(draft_id))
            .exec(&tx)
            .await?;

        // Only a change of serving state is activity: snoozing and unsnoozing
        // are the owner's own attention management.
        let event = match update {
            AvailabilityUpdate::Disabled { .. } if !was_disabled => Some(
                NotificationEvent::disabled(draft_id, &previous.title, &timestamp),
            ),
            AvailabilityUpdate::Active if was_disabled => Some(NotificationEvent::enabled(
                draft_id,
                &previous.title,
                &timestamp,
            )),
            _ => None,
        };
        if let Some(event) = event {
            record_event_on(&tx, &event).await?;
        }

        let summary = draft_summary(&tx, draft_id)
            .await?
            .ok_or(AvailabilityError::DraftNotFound)?;
        tx.commit().await?;
        Ok(summary)
    }

    async fn record_event(&self, event: &NotificationEvent) -> Result<bool> {
        let tx = self.begin_write().await?;
        let recorded = record_event_on(&tx, event).await?;
        tx.commit().await?;
        Ok(recorded)
    }

    async fn get_push_subscription(
        &self,
        endpoint: &str,
    ) -> Result<Option<PushSubscriptionSummary>> {
        Ok(push_subscription::Entity::find()
            .filter(push_subscription::Column::Endpoint.eq(endpoint))
            .one(&self.db)
            .await?
            .map(subscription_summary))
    }

    async fn upsert_push_subscription(
        &self,
        input: &PushSubscriptionInput,
    ) -> Result<PushSubscriptionSummary> {
        use push_subscription::Column as S;
        let timestamp = now();
        let chosen = input
            .events
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let everything = serde_json::to_string(&NotificationKind::ALL)?;

        // Keys are always replaced. Preferences are only replaced when sent,
        // so a bare re-subscribe keeps what the user picked.
        let mut refreshed = vec![S::P256dh, S::Auth, S::UpdatedAt];
        if chosen.is_some() {
            refreshed.push(S::Events);
        }
        push_subscription::Entity::insert(push_subscription::ActiveModel {
            id: Set(new_internal_id()),
            endpoint: Set(input.endpoint.clone()),
            p256dh: Set(input.keys.p256dh.clone()),
            auth: Set(input.keys.auth.clone()),
            events: Set(chosen.unwrap_or(everything)),
            created_at: Set(timestamp.clone()),
            updated_at: Set(timestamp),
        })
        .on_conflict(
            OnConflict::column(S::Endpoint)
                .update_columns(refreshed)
                .to_owned(),
        )
        .exec_without_returning(&self.db)
        .await?;
        self.get_push_subscription(&input.endpoint)
            .await?
            .context("subscription was not stored")
    }

    async fn remove_push_subscription(&self, endpoint: &str) -> Result<bool> {
        let removed = push_subscription::Entity::delete_many()
            .filter(push_subscription::Column::Endpoint.eq(endpoint))
            .exec(&self.db)
            .await?;
        Ok(removed.rows_affected > 0)
    }

    async fn remove_push_subscription_by_id(&self, id: &str) -> Result<()> {
        push_subscription::Entity::delete_by_id(id)
            .exec(&self.db)
            .await?;
        Ok(())
    }

    async fn due_deliveries(&self, now: &str, limit: usize) -> Result<Vec<PendingDelivery>> {
        use notification_delivery::Column as D;
        use notification_event::Column as E;
        use push_subscription::Column as S;
        let rows = notification_delivery::Entity::find()
            .select_only()
            .column_as(E::Key, "key")
            .column_as(E::Kind, "kind")
            .column_as(E::DraftId, "draft_id")
            .column_as(E::Title, "title")
            .column_as(E::Body, "body")
            .column_as(E::Target, "target")
            .column_as(E::CreatedAt, "event_created_at")
            .column_as(S::Id, "subscription_id")
            .column_as(S::Endpoint, "endpoint")
            .column_as(S::P256dh, "p256dh")
            .column_as(S::Auth, "auth")
            .column_as(D::Attempts, "attempts")
            .join(
                JoinType::InnerJoin,
                notification_delivery::Relation::Event.def(),
            )
            .join(
                JoinType::InnerJoin,
                notification_delivery::Relation::Subscription.def(),
            )
            .filter(D::NextAttemptAt.lte(now))
            .order_by_asc(D::NextAttemptAt)
            .limit(limit as u64)
            .into_model::<DeliveryRow>()
            .all(&self.db)
            .await?;

        rows.into_iter()
            .map(|row| {
                let kind = NotificationKind::parse(&row.kind)
                    .with_context(|| format!("unknown notification kind {:?}", row.kind))?;
                Ok(PendingDelivery {
                    event: NotificationEvent {
                        key: row.key,
                        kind,
                        draft_id: row.draft_id,
                        title: row.title,
                        body: row.body,
                        target: row.target,
                        created_at: row.event_created_at,
                    },
                    subscription_id: row.subscription_id,
                    endpoint: row.endpoint,
                    p256dh: row.p256dh,
                    auth: row.auth,
                    attempts: row.attempts,
                })
            })
            .collect()
    }

    async fn delivery_done(&self, event_key: &str, subscription_id: &str) -> Result<()> {
        notification_delivery::Entity::delete_by_id((
            event_key.to_string(),
            subscription_id.to_string(),
        ))
        .exec(&self.db)
        .await?;
        Ok(())
    }

    async fn delivery_retry(
        &self,
        event_key: &str,
        subscription_id: &str,
        attempts: i64,
        next_attempt_at: &str,
    ) -> Result<()> {
        use notification_delivery::Column as D;
        notification_delivery::Entity::update_many()
            .col_expr(D::Attempts, Expr::value(attempts))
            .col_expr(D::NextAttemptAt, Expr::value(next_attempt_at))
            .filter(D::EventKey.eq(event_key))
            .filter(D::SubscriptionId.eq(subscription_id))
            .exec(&self.db)
            .await?;
        Ok(())
    }

    async fn next_delivery_at(&self) -> Result<Option<String>> {
        let earliest: Option<Option<String>> = notification_delivery::Entity::find()
            .select_only()
            .expr(notification_delivery::Column::NextAttemptAt.min())
            .into_tuple()
            .one(&self.db)
            .await?;
        Ok(earliest.flatten())
    }

    async fn record_due_wakes(&self, now: &str) -> Result<Vec<NotificationEvent>> {
        let tx = self.begin_write().await?;
        let expired = draft::Entity::find()
            .filter(draft::Column::DeletedAt.is_null())
            .filter(draft::Column::DisabledAt.is_null())
            .filter(draft::Column::SnoozedUntil.is_not_null())
            .filter(draft::Column::SnoozedUntil.lte(now))
            .all(&tx)
            .await?;
        let candidates: Vec<NotificationEvent> = expired
            .iter()
            .filter_map(|draft| {
                let until = draft.snoozed_until.as_deref()?;
                Some(NotificationEvent::woke(&draft.id, &draft.title, until, now))
            })
            .collect();

        // An expired snooze stays on its row for good, so skip the ones whose
        // wake is already recorded rather than re-attempting every pass.
        let already: Vec<String> = if candidates.is_empty() {
            Vec::new()
        } else {
            notification_event::Entity::find()
                .select_only()
                .column(notification_event::Column::Key)
                .filter(
                    notification_event::Column::Key
                        .is_in(candidates.iter().map(|event| event.key.clone())),
                )
                .into_tuple()
                .all(&tx)
                .await?
        };

        let mut woke = Vec::new();
        for event in candidates {
            if !already.contains(&event.key) && record_event_on(&tx, &event).await? {
                woke.push(event);
            }
        }
        tx.commit().await?;
        Ok(woke)
    }

    async fn next_wake_at(&self, now: &str) -> Result<Option<String>> {
        let nearest: Option<Option<String>> = draft::Entity::find()
            .select_only()
            .expr(draft::Column::SnoozedUntil.min())
            .filter(draft::Column::DeletedAt.is_null())
            .filter(draft::Column::DisabledAt.is_null())
            .filter(draft::Column::SnoozedUntil.gt(now))
            .into_tuple()
            .one(&self.db)
            .await?;
        Ok(nearest.flatten())
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
