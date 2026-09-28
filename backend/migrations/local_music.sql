-- Local music library: an ordered catalog over uploaded audio in
-- media_assets, LRC text, and named playlists. Shared by migration 003 and
-- schema_check::ensure_local_music_tables; runs after media_asset_model.sql.
CREATE TABLE IF NOT EXISTS local_music_tracks (
    id SERIAL PRIMARY KEY,
    title TEXT NOT NULL,
    artist TEXT NOT NULL DEFAULT '',
    album TEXT NOT NULL DEFAULT '',
    duration_ms BIGINT NOT NULL DEFAULT 0,
    audio_media_id INTEGER NOT NULL REFERENCES media_assets(id) ON DELETE CASCADE,
    cover_media_id INTEGER REFERENCES media_assets(id) ON DELETE SET NULL,
    lyrics TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    enabled BOOLEAN NOT NULL DEFAULT true,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_local_music_tracks_order
    ON local_music_tracks (sort_order, id);
CREATE INDEX IF NOT EXISTS idx_local_music_tracks_enabled
    ON local_music_tracks (enabled, sort_order, id);

CREATE TABLE IF NOT EXISTS local_music_playlists (
    id SERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS local_music_playlist_tracks (
    playlist_id INTEGER NOT NULL REFERENCES local_music_playlists(id) ON DELETE CASCADE,
    track_id INTEGER NOT NULL REFERENCES local_music_tracks(id) ON DELETE CASCADE,
    sort_order INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (playlist_id, track_id)
);
CREATE INDEX IF NOT EXISTS idx_local_music_playlist_tracks_order
    ON local_music_playlist_tracks (playlist_id, sort_order, track_id);
