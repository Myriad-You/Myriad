use sea_orm_migration::prelude::*;

/// Named local playlists over uploaded tracks (008).
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            CREATE TABLE IF NOT EXISTS local_music_playlists (
                id           SERIAL PRIMARY KEY,
                name         TEXT NOT NULL,
                sort_order   INTEGER NOT NULL DEFAULT 0,
                created_at   TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .await?;
        db.execute_unprepared(
            r#"
            CREATE TABLE IF NOT EXISTS local_music_playlist_tracks (
                playlist_id  INTEGER NOT NULL REFERENCES local_music_playlists(id) ON DELETE CASCADE,
                track_id     INTEGER NOT NULL REFERENCES local_music_tracks(id) ON DELETE CASCADE,
                sort_order   INTEGER NOT NULL DEFAULT 0,
                PRIMARY KEY (playlist_id, track_id)
            )
            "#,
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_local_music_playlist_tracks_order \
             ON local_music_playlist_tracks (playlist_id, sort_order, track_id)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared("DROP TABLE IF EXISTS local_music_playlist_tracks")
            .await?;
        db.execute_unprepared("DROP TABLE IF EXISTS local_music_playlists")
            .await?;
        Ok(())
    }
}
