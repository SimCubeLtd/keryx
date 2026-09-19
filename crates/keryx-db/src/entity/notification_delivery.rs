use sea_orm::entity::prelude::*;

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel)]
#[sea_orm(table_name = "notification_deliveries")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub event_key: String,
    #[sea_orm(primary_key, auto_increment = false)]
    pub subscription_id: String,
    pub attempts: i64,
    pub next_attempt_at: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {
    #[sea_orm(
        belongs_to = "super::notification_event::Entity",
        from = "Column::EventKey",
        to = "super::notification_event::Column::Key"
    )]
    Event,
    #[sea_orm(
        belongs_to = "super::push_subscription::Entity",
        from = "Column::SubscriptionId",
        to = "super::push_subscription::Column::Id"
    )]
    Subscription,
}

impl Related<super::notification_event::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Event.def()
    }
}

impl Related<super::push_subscription::Entity> for Entity {
    fn to() -> RelationDef {
        Relation::Subscription.def()
    }
}

impl ActiveModelBehavior for ActiveModel {}
