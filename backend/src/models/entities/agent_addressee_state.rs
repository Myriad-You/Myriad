//! Per-addressee mood and activity for the site Agent.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "agent_addressee_state")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub user_id: i32,
    pub mood: f64,
    pub arousal: f64,
    pub emotion: f64,
    pub emotion_arousal: f64,
    pub activity: String,
    pub activity_updated_at: DateTimeWithTimeZone,
    pub do_not_disturb: bool,
    #[sea_orm(nullable)]
    pub dnd_start_minute: Option<i32>,
    #[sea_orm(nullable)]
    pub dnd_end_minute: Option<i32>,
    #[sea_orm(nullable)]
    pub last_user_message_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(nullable)]
    pub last_proactive_at: Option<DateTimeWithTimeZone>,
    #[sea_orm(nullable)]
    pub music_mood_credited_at: Option<DateTimeWithTimeZone>,
    pub mood_settled_at: DateTimeWithTimeZone,
    pub emotion_settled_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
