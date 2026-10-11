//! Unified Agent memory. One row is one remembered thing, typed, attributed and
//! bounded to the audience that was present when it was learned.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "agent_memories")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    /// The person this memory belongs to. `None` is the persona's own or
    /// community memory.
    pub user_id: Option<i32>,
    pub kind: String,
    pub content: String,
    pub evidence: Option<String>,
    pub speaker: String,
    pub source: String,
    pub venue: String,
    /// User ids present when it was learned (the original audience).
    #[sea_orm(column_type = "JsonBinary")]
    pub audience: Json,
    /// What it is about, each with the other names people use for it
    /// (`[{"name": "猫", "aliases": ["喵"]}]`). Recall matches these.
    #[sea_orm(column_type = "JsonBinary")]
    pub concepts: Json,
    #[sea_orm(column_type = "Double")]
    pub importance: f64,
    pub access_count: i32,
    pub last_accessed_at: Option<DateTimeWithTimeZone>,
    pub valid_from: DateTimeWithTimeZone,
    pub invalid_at: Option<DateTimeWithTimeZone>,
    pub invalid_reason: Option<String>,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
    /// The memory that took this one's place, written with its retirement:
    /// on the old row, so several old rows can point at one new one.
    pub superseded_by: Option<String>,
    /// The side that wrote it: `site`, or the device it came from.
    pub origin: String,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
