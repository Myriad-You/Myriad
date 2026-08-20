//! Per-addressee mood and activity for the site Agent.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "agent_addressee_state")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: i32,
    pub mood: f64,
    pub activity: String,
    pub do_not_disturb: bool,
    #[sea_orm(nullable)]
    pub last_user_message_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(nullable)]
    pub last_proactive_at: Option<DateTimeWithTimeZone>,
    /// Last time the departure decay was charged, so one silence window is only charged once.
    #[sea_orm(nullable)]
    pub last_departure_at: Option<DateTimeWithTimeZone>,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
