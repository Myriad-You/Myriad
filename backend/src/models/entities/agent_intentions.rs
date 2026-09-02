//! Durable consciousness intentions.
//!
//! Work proposals are persisted here before they may be accepted and handed
//! to the normal Agent Work runtime.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "agent_intentions")]
pub struct Model {
    #[sea_orm(primary_key, auto_increment = false)]
    pub id: String,
    pub user_id: i32,
    pub source_event_id: String,
    #[sea_orm(column_type = "Text")]
    pub summary: String,
    pub reason_code: String,
    pub status: String,
    #[sea_orm(column_type = "JsonBinary")]
    pub proposal: Json,
    #[sea_orm(nullable)]
    pub work_session_id: Option<String>,
    #[sea_orm(nullable)]
    pub work_run_id: Option<String>,
    #[sea_orm(column_type = "Text", nullable)]
    pub result_summary: Option<String>,
    pub expires_at: Option<DateTimeWithTimeZone>,
    pub accept_source: String,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
