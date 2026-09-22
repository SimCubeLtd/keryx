//! Add organisation metadata without rewriting any existing table.
use sea_orm_migration::prelude::*;

#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared(
                r#"
            CREATE TABLE tags (id TEXT PRIMARY KEY NOT NULL, name TEXT NOT NULL UNIQUE);
            CREATE TABLE draft_tags (
                draft_id TEXT NOT NULL REFERENCES drafts(id) ON DELETE CASCADE,
                tag_id TEXT NOT NULL REFERENCES tags(id) ON DELETE CASCADE,
                PRIMARY KEY (draft_id, tag_id)
            );
            CREATE INDEX draft_tags_tag_id_idx ON draft_tags(tag_id, draft_id);
        "#,
            )
            .await?;
        Ok(())
    }
    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE draft_tags; DROP TABLE tags;")
            .await?;
        Ok(())
    }
}
