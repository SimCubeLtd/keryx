//! One entity per table, matching the existing columns exactly. Columns are
//! selected by name, so the differing column order of the two legacy SQLite
//! populations (inline versus ALTER-appended) is harmless.
//!
//! Timestamps are TEXT on both backends and stay `String` here. The two flag
//! columns are `bool`: INTEGER on SQLite, BOOLEAN on Postgres.

pub mod draft;
pub mod draft_version;
pub mod notification_delivery;
pub mod notification_event;
pub mod push_subscription;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connect::connect_sqlite_memory;
    use crate::migration::Migrator;
    use sea_orm::{
        ActiveModelTrait, ConnectionTrait, DatabaseConnection, DbBackend, EntityName, EntityTrait,
        IdenStatic, IntoActiveModel, Iterable, Statement,
    };
    use sea_orm_migration::MigratorTrait;

    async fn schema_columns(db: &DatabaseConnection, table: &str) -> Vec<String> {
        let mut names: Vec<String> = db
            .query_all_raw(Statement::from_string(
                DbBackend::Sqlite,
                format!("PRAGMA table_info({table})"),
            ))
            .await
            .unwrap()
            .iter()
            .map(|row| row.try_get_by_index::<String>(1).unwrap())
            .collect();
        names.sort();
        names
    }

    fn entity_columns<E: EntityTrait>() -> Vec<String> {
        let mut names: Vec<String> = E::Column::iter()
            .map(|column| column.as_str().to_string())
            .collect();
        names.sort();
        names
    }

    #[tokio::test]
    async fn every_entity_matches_its_table_column_for_column() {
        let db = connect_sqlite_memory().await.unwrap();
        Migrator::up(&db, None).await.unwrap();

        macro_rules! check {
            ($entity:ty) => {
                let table = <$entity>::default().table_name().to_string();
                assert_eq!(
                    entity_columns::<$entity>(),
                    schema_columns(&db, &table).await,
                    "{table}"
                );
            };
        }
        check!(draft::Entity);
        check!(draft_version::Entity);
        check!(push_subscription::Entity);
        check!(notification_event::Entity);
        check!(notification_delivery::Entity);
    }

    #[tokio::test]
    async fn flags_and_nulls_round_trip_through_the_entities() {
        let db = connect_sqlite_memory().await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        let now = "2026-09-01T10:00:00.000Z".to_string();

        let draft = draft::Model {
            id: "ab12cd34ef56".into(),
            title: "Plan".into(),
            description: None,
            current_version_id: None,
            repo_org: None,
            repo_name: None,
            repo_host: None,
            created_at: now.clone(),
            updated_at: now.clone(),
            deleted_at: None,
            disabled_at: None,
            disabled_reason: None,
            snoozed_until: None,
        };
        draft.clone().into_active_model().insert(&db).await.unwrap();

        let version = draft_version::Model {
            id: "V1aB2cD3eF4gH5iJ6kL7".into(),
            draft_id: draft.id.clone(),
            version_number: 1,
            object_key: "drafts/ab12cd34ef56/V1aB2cD3eF4gH5iJ6kL7.html".into(),
            content_hash: "4f9c1e".into(),
            file_size: 5_000_000_000,
            created_at: now,
            repo_org: None,
            repo_name: None,
            repo_host: None,
            source_ip: None,
            user_agent: None,
            cli_version: None,
            git_branch: None,
            git_commit_sha: None,
            git_commit_subject: None,
            git_dirty: None,
            original_filename: None,
            has_inline_script: true,
            external_image_hosts: "[]".into(),
        };
        version
            .clone()
            .into_active_model()
            .insert(&db)
            .await
            .unwrap();

        let read = draft_version::Entity::find_by_id(version.id.clone())
            .one(&db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(read, version);
        assert_eq!(
            draft::Entity::find().one(&db).await.unwrap().unwrap(),
            draft
        );
    }
}
