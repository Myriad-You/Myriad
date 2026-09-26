//! Local music library catalog. Audio bytes live in `media_assets`.

use sea_orm::entity::prelude::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, DeriveEntityModel, Serialize, Deserialize)]
#[sea_orm(table_name = "local_music_tracks")]
pub struct Model {
    #[sea_orm(primary_key)]
    pub id: i32,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: i64,
    pub audio_media_id: i32,
    pub cover_media_id: Option<i32>,
    #[sea_orm(column_type = "Text", nullable)]
    pub lyrics: Option<String>,
    pub sort_order: i32,
    pub enabled: bool,
    pub created_at: DateTimeWithTimeZone,
    pub updated_at: DateTimeWithTimeZone,
}

#[derive(Copy, Clone, Debug, EnumIter, DeriveRelation)]
pub enum Relation {}

impl ActiveModelBehavior for ActiveModel {}
