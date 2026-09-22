use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "draft_tags")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub draft_id: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub tag_id: String,
}
#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}
impl ActiveModelBehavior for ActiveModel {}
