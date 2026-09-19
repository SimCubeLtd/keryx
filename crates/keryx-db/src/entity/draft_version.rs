use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "draft_versions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub draft_id: String,
    pub version_number: i64,
    pub object_key: String,
    pub content_hash: String,
    pub file_size: i64,
    pub created_at: String,
    pub repo_org: Option<String>,
    pub repo_name: Option<String>,
    pub repo_host: Option<String>,
    pub source_ip: Option<String>,
    pub user_agent: Option<String>,
    pub cli_version: Option<String>,
    pub git_branch: Option<String>,
    pub git_commit_sha: Option<String>,
    pub git_commit_subject: Option<String>,
    pub git_dirty: Option<bool>,
    pub original_filename: Option<String>,
    pub has_inline_script: bool,
    /// A JSON array of host names, stored as text.
    pub external_image_hosts: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::draft::Entity",
        from = "Column::DraftId",
        to = "super::draft::Column::Id"
    )]
    Draft,
}

impl Related<super::draft::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Draft.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
