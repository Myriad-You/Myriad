use sea_orm_migration::prelude::*;

/// Local music library (007).
///
/// Independent player source alongside netease/qq. Audio bytes live in
/// `media_assets`; this table is the ordered catalog + LRC text (iro-style).
#[derive(DeriveMigrationName)]
pub struct Migration;

#[async_trait::async_trait]
impl MigrationTrait for Migration {
    async fn up(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        let db = manager.get_connection();
        db.execute_unprepared(
            r#"
            CREATE TABLE IF NOT EXISTS local_music_tracks (
                id                SERIAL PRIMARY KEY,
                title             TEXT NOT NULL,
                artist            TEXT NOT NULL DEFAULT '',
                album             TEXT NOT NULL DEFAULT '',
                duration_ms       BIGINT NOT NULL DEFAULT 0,
                audio_media_id    INTEGER NOT NULL REFERENCES media_assets(id) ON DELETE CASCADE,
                cover_media_id    INTEGER REFERENCES media_assets(id) ON DELETE SET NULL,
                lyrics            TEXT,
                sort_order        INTEGER NOT NULL DEFAULT 0,
                enabled           BOOLEAN NOT NULL DEFAULT true,
                created_at        TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at        TIMESTAMPTZ NOT NULL DEFAULT NOW()
            )
            "#,
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_local_music_tracks_order \
             ON local_music_tracks (sort_order, id)",
        )
        .await?;
        db.execute_unprepared(
            "CREATE INDEX IF NOT EXISTS idx_local_music_tracks_enabled \
             ON local_music_tracks (enabled, sort_order, id)",
        )
        .await?;
        Ok(())
    }

    async fn down(&self, manager: &SchemaManager) -> Result<(), DbErr> {
        manager
            .get_connection()
            .execute_unprepared("DROP TABLE IF EXISTS local_music_tracks")
            .await?;
        Ok(())
    }
}
