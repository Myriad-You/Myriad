//! Expected schema for local music library tables (migrations 007–008).
use super::types::{ColumnDef, TableDef};

pub(crate) fn tables() -> Vec<TableDef> {
    vec![
        TableDef {
            name: "local_music_tracks".to_string(),
            columns: vec![
                ColumnDef::new("id", "integer"),
                ColumnDef::new("title", "text").not_null(),
                ColumnDef::new("artist", "text").not_null().default_value("''"),
                ColumnDef::new("album", "text").not_null().default_value("''"),
                ColumnDef::new("duration_ms", "bigint")
                    .not_null()
                    .default_value("0"),
                ColumnDef::new("audio_media_id", "integer").not_null(),
                ColumnDef::new("cover_media_id", "integer"),
                ColumnDef::new("lyrics", "text"),
                ColumnDef::new("sort_order", "integer")
                    .not_null()
                    .default_value("0"),
                ColumnDef::new("enabled", "boolean")
                    .not_null()
                    .default_value("true"),
                ColumnDef::new("created_at", "timestamp with time zone")
                    .not_null()
                    .default_value("CURRENT_TIMESTAMP"),
                ColumnDef::new("updated_at", "timestamp with time zone")
                    .not_null()
                    .default_value("CURRENT_TIMESTAMP"),
            ],
        },
        TableDef {
            name: "local_music_playlists".to_string(),
            columns: vec![
                ColumnDef::new("id", "integer"),
                ColumnDef::new("name", "text").not_null(),
                ColumnDef::new("sort_order", "integer")
                    .not_null()
                    .default_value("0"),
                ColumnDef::new("created_at", "timestamp with time zone")
                    .not_null()
                    .default_value("CURRENT_TIMESTAMP"),
                ColumnDef::new("updated_at", "timestamp with time zone")
                    .not_null()
                    .default_value("CURRENT_TIMESTAMP"),
            ],
        },
        TableDef {
            name: "local_music_playlist_tracks".to_string(),
            columns: vec![
                ColumnDef::new("playlist_id", "integer").not_null(),
                ColumnDef::new("track_id", "integer").not_null(),
                ColumnDef::new("sort_order", "integer")
                    .not_null()
                    .default_value("0"),
            ],
        },
    ]
}
