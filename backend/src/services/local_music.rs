//! Local music library: ordered catalog over `media_assets` audio.
//!
//! Projects to the same [`PlayerPlaylist`] shape as netease/qq so the
//! frontend player treats it as a third independent source.

use anyhow::{Result, anyhow};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, QueryFilter,
    QueryOrder, Set,
};
use serde::{Deserialize, Serialize};

use crate::models::entities::{
    local_music_playlist_tracks, local_music_playlists, local_music_tracks, media_assets,
};
use crate::services::music_player_view::{PlayerMusicSource, PlayerPlaylist, PlayerSong};

/// Canonical playlist id for the single on-site library (iro-style).
pub const LOCAL_PLAYLIST_ID: &str = "local";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalTrackInput {
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub duration_ms: i64,
    pub audio_media_id: i32,
    #[serde(default)]
    pub cover_media_id: Option<i32>,
    #[serde(default)]
    pub lyrics: Option<String>,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
}

fn default_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalTrackView {
    pub id: i32,
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_ms: i64,
    pub audio_media_id: i32,
    pub cover_media_id: Option<i32>,
    pub has_lyrics: bool,
    pub sort_order: i32,
    pub enabled: bool,
    pub audio_url: String,
    pub cover_url: Option<String>,
    /// File extension without dot (mp3/flac/…), for UI filter chips.
    pub ext: String,
    /// Original upload filename.
    pub filename: String,
    /// Audio file size in bytes.
    pub size_bytes: i64,
}

fn audio_url(track_id: i32) -> String {
    format!("/api/proxy/music/local/audio/{track_id}")
}

fn cover_url(cover_media_id: Option<i32>) -> Option<String> {
    cover_media_id.map(|id| format!("/api/proxy/music/local/cover/{id}"))
}

fn ext_from_mime_or_name(mime: &str, name: &str) -> String {
    let from_name = name
        .rsplit_once('.')
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .unwrap_or_default();
    if !from_name.is_empty() && from_name.len() <= 8 {
        return from_name;
    }
    match mime.split(';').next().unwrap_or(mime).trim() {
        "audio/mpeg" => "mp3".into(),
        "audio/mp4" => "m4a".into(),
        "audio/flac" => "flac".into(),
        "audio/wav" => "wav".into(),
        "audio/ogg" => "ogg".into(),
        "audio/aac" => "aac".into(),
        _ => from_name,
    }
}

fn to_view(
    row: local_music_tracks::Model,
    media: Option<&media_assets::Model>,
) -> LocalTrackView {
    let (mime, filename, size_bytes) = match media {
        Some(m) => (m.mime.clone(), m.name.clone(), m.size),
        None => (String::new(), String::new(), 0),
    };
    LocalTrackView {
        id: row.id,
        title: row.title,
        artist: row.artist,
        album: row.album,
        duration_ms: row.duration_ms,
        audio_media_id: row.audio_media_id,
        cover_media_id: row.cover_media_id,
        has_lyrics: row.lyrics.as_deref().is_some_and(|s| !s.trim().is_empty()),
        sort_order: row.sort_order,
        enabled: row.enabled,
        audio_url: audio_url(row.id),
        cover_url: cover_url(row.cover_media_id),
        ext: ext_from_mime_or_name(&mime, &filename),
        filename,
        size_bytes,
    }
}

async fn load_media_map(
    db: &DatabaseConnection,
    ids: &[i32],
) -> Result<std::collections::HashMap<i32, media_assets::Model>> {
    use sea_orm::Condition;
    if ids.is_empty() {
        return Ok(Default::default());
    }
    let rows = media_assets::Entity::find()
        .filter(Condition::any().add(media_assets::Column::Id.is_in(ids.to_vec())))
        .all(db)
        .await?;
    Ok(rows.into_iter().map(|m| (m.id, m)).collect())
}

pub async fn list_tracks(db: &DatabaseConnection) -> Result<Vec<LocalTrackView>> {
    let rows = local_music_tracks::Entity::find()
        .order_by_asc(local_music_tracks::Column::SortOrder)
        .order_by_asc(local_music_tracks::Column::Id)
        .all(db)
        .await?;
    let media_ids: Vec<i32> = rows.iter().map(|r| r.audio_media_id).collect();
    let media = load_media_map(db, &media_ids).await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            let m = media.get(&row.audio_media_id);
            to_view(row, m)
        })
        .collect())
}

pub async fn create_track(
    db: &DatabaseConnection,
    input: LocalTrackInput,
) -> Result<LocalTrackView> {
    if input.title.trim().is_empty() {
        return Err(anyhow!("title is required"));
    }
    // Reject dangling or wrong-class media so the player never 404s on day one.
    ensure_media_mime_prefix(db, input.audio_media_id, "audio", "audio/").await?;
    if let Some(cover_id) = input.cover_media_id {
        ensure_media_mime_prefix(db, cover_id, "cover", "image/").await?;
    }
    let row = local_music_tracks::ActiveModel {
        title: Set(input.title.trim().to_string()),
        artist: Set(input.artist.trim().to_string()),
        album: Set(input.album.trim().to_string()),
        duration_ms: Set(input.duration_ms.max(0)),
        audio_media_id: Set(input.audio_media_id),
        cover_media_id: Set(input.cover_media_id),
        lyrics: Set(normalize_lyrics(input.lyrics)),
        sort_order: Set(input.sort_order),
        enabled: Set(input.enabled),
        ..Default::default()
    }
    .insert(db)
    .await?;
    let media_id = row.audio_media_id;
    let media = load_media_map(db, &[media_id]).await?;
    Ok(to_view(row, media.get(&media_id)))
}

pub async fn update_track(
    db: &DatabaseConnection,
    id: i32,
    input: LocalTrackInput,
) -> Result<LocalTrackView> {
    let existing = local_music_tracks::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow!("track not found"))?;
    if input.title.trim().is_empty() {
        return Err(anyhow!("title is required"));
    }
    ensure_media_mime_prefix(db, input.audio_media_id, "audio", "audio/").await?;
    if let Some(cover_id) = input.cover_media_id {
        ensure_media_mime_prefix(db, cover_id, "cover", "image/").await?;
    }
    let old_audio_media_id = existing.audio_media_id;
    let old_cover_media_id = existing.cover_media_id;
    let mut active: local_music_tracks::ActiveModel = existing.into();
    active.title = Set(input.title.trim().to_string());
    active.artist = Set(input.artist.trim().to_string());
    active.album = Set(input.album.trim().to_string());
    active.duration_ms = Set(input.duration_ms.max(0));
    active.audio_media_id = Set(input.audio_media_id);
    active.cover_media_id = Set(input.cover_media_id);
    active.lyrics = Set(normalize_lyrics(input.lyrics));
    active.sort_order = Set(input.sort_order);
    active.enabled = Set(input.enabled);
    active.updated_at = Set(chrono::Utc::now().into());
    let row = active.update(db).await?;
    if old_audio_media_id != row.audio_media_id {
        release_media_if_unused(db, Some(old_audio_media_id)).await;
    }
    if old_cover_media_id != row.cover_media_id {
        release_media_if_unused(db, old_cover_media_id).await;
    }
    let media_id = row.audio_media_id;
    let media = load_media_map(db, &[media_id]).await?;
    Ok(to_view(row, media.get(&media_id)))
}

pub async fn delete_track(db: &DatabaseConnection, id: i32) -> Result<()> {
    let Some(row) = local_music_tracks::Entity::find_by_id(id).one(db).await? else {
        return Ok(());
    };
    let audio_media_id = row.audio_media_id;
    let cover_media_id = row.cover_media_id;
    local_music_playlist_tracks::Entity::delete_many()
        .filter(local_music_playlist_tracks::Column::TrackId.eq(id))
        .exec(db)
        .await?;
    local_music_tracks::Entity::delete_by_id(id).exec(db).await?;
    release_media_if_unused(db, Some(audio_media_id)).await;
    release_media_if_unused(db, cover_media_id).await;
    Ok(())
}

/// Delete the stored media file once no local track still points at it.
/// Best-effort: a pending unlink is retried by media recovery; an in-use
/// asset (e.g. also cited by a note) is left alone and logged.
async fn release_media_if_unused(db: &DatabaseConnection, media_id: Option<i32>) {
    let Some(media_id) = media_id else {
        return;
    };
    let still_used = local_music_tracks::Entity::find()
        .filter(
            Condition::any()
                .add(local_music_tracks::Column::AudioMediaId.eq(media_id))
                .add(local_music_tracks::Column::CoverMediaId.eq(media_id)),
        )
        .one(db)
        .await;
    match still_used {
        Ok(Some(_)) => return,
        Ok(None) => {}
        Err(err) => {
            tracing::warn!(%err, media_id, "local music media ref check");
            return;
        }
    }
    let service =
        crate::services::media::MediaService::from_data_paths(&crate::services::data_paths::paths());
    match service.delete(db, media_id).await {
        Ok(crate::services::media::DeleteOutcome::Deleted) => {}
        Ok(crate::services::media::DeleteOutcome::PendingRetry) => {
            tracing::warn!(media_id, "local music media delete pending retry");
        }
        Err(crate::services::media::MediaError::Missing) => {}
        Err(err) => tracing::warn!(%err, media_id, "local music media delete"),
    }
}

pub async fn track_lyrics(db: &DatabaseConnection, id: i32) -> Result<Option<String>> {
    let row = local_music_tracks::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow!("track not found"))?;
    Ok(normalize_lyrics(row.lyrics))
}

pub async fn track_audio_media_id(db: &DatabaseConnection, id: i32) -> Result<i32> {
    let row = local_music_tracks::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow!("track not found"))?;
    Ok(row.audio_media_id)
}

/// Cover bytes are only public when some local track actually references this media as cover.
pub async fn cover_media_id(db: &DatabaseConnection, media_id: i32) -> Result<i32> {
    ensure_media_mime_prefix(db, media_id, "cover", "image/").await?;
    let referenced = local_music_tracks::Entity::find()
        .filter(local_music_tracks::Column::CoverMediaId.eq(media_id))
        .one(db)
        .await?
        .is_some();
    if !referenced {
        return Err(anyhow!("cover not referenced by any track"));
    }
    Ok(media_id)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPlaylistInput {
    pub name: String,
    #[serde(default)]
    pub sort_order: i32,
    #[serde(default)]
    pub track_ids: Vec<i32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalPlaylistView {
    pub id: i32,
    pub name: String,
    pub sort_order: i32,
    pub track_ids: Vec<i32>,
}

pub async fn list_playlists(db: &DatabaseConnection) -> Result<Vec<LocalPlaylistView>> {
    let playlists = local_music_playlists::Entity::find()
        .order_by_asc(local_music_playlists::Column::SortOrder)
        .order_by_asc(local_music_playlists::Column::Id)
        .all(db)
        .await?;
    let links = local_music_playlist_tracks::Entity::find()
        .order_by_asc(local_music_playlist_tracks::Column::PlaylistId)
        .order_by_asc(local_music_playlist_tracks::Column::SortOrder)
        .all(db)
        .await?;
    let mut by_playlist: std::collections::HashMap<i32, Vec<i32>> = Default::default();
    for link in links {
        by_playlist
            .entry(link.playlist_id)
            .or_default()
            .push(link.track_id);
    }
    Ok(playlists
        .into_iter()
        .map(|p| LocalPlaylistView {
            id: p.id,
            name: p.name,
            sort_order: p.sort_order,
            track_ids: by_playlist.remove(&p.id).unwrap_or_default(),
        })
        .collect())
}

async fn replace_playlist_tracks(
    db: &DatabaseConnection,
    playlist_id: i32,
    track_ids: &[i32],
) -> Result<()> {
    local_music_playlist_tracks::Entity::delete_many()
        .filter(local_music_playlist_tracks::Column::PlaylistId.eq(playlist_id))
        .exec(db)
        .await?;
    for (index, track_id) in track_ids.iter().enumerate() {
        // Drop unknown track ids instead of failing the whole save.
        let exists = local_music_tracks::Entity::find_by_id(*track_id)
            .one(db)
            .await?
            .is_some();
        if !exists {
            continue;
        }
        local_music_playlist_tracks::ActiveModel {
            playlist_id: Set(playlist_id),
            track_id: Set(*track_id),
            sort_order: Set(index as i32),
        }
        .insert(db)
        .await?;
    }
    Ok(())
}

pub async fn create_playlist(
    db: &DatabaseConnection,
    input: LocalPlaylistInput,
) -> Result<LocalPlaylistView> {
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(anyhow!("playlist name is required"));
    }
    let row = local_music_playlists::ActiveModel {
        name: Set(name),
        sort_order: Set(input.sort_order),
        ..Default::default()
    }
    .insert(db)
    .await?;
    replace_playlist_tracks(db, row.id, &input.track_ids).await?;
    Ok(LocalPlaylistView {
        id: row.id,
        name: row.name,
        sort_order: row.sort_order,
        track_ids: input.track_ids,
    })
}

pub async fn update_playlist(
    db: &DatabaseConnection,
    id: i32,
    input: LocalPlaylistInput,
) -> Result<LocalPlaylistView> {
    let existing = local_music_playlists::Entity::find_by_id(id)
        .one(db)
        .await?
        .ok_or_else(|| anyhow!("playlist not found"))?;
    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err(anyhow!("playlist name is required"));
    }
    let mut active: local_music_playlists::ActiveModel = existing.into();
    active.name = Set(name);
    active.sort_order = Set(input.sort_order);
    active.updated_at = Set(chrono::Utc::now().into());
    let row = active.update(db).await?;
    replace_playlist_tracks(db, id, &input.track_ids).await?;
    Ok(LocalPlaylistView {
        id: row.id,
        name: row.name,
        sort_order: row.sort_order,
        track_ids: input.track_ids,
    })
}

pub async fn delete_playlist(db: &DatabaseConnection, id: i32) -> Result<()> {
    local_music_playlist_tracks::Entity::delete_many()
        .filter(local_music_playlist_tracks::Column::PlaylistId.eq(id))
        .exec(db)
        .await?;
    local_music_playlists::Entity::delete_by_id(id).exec(db).await?;
    Ok(())
}

/// Player view: either a named playlist or the full enabled catalog (`local`).
pub async fn build_player_playlist(
    db: &DatabaseConnection,
    playlist_id: &str,
) -> Result<PlayerPlaylist> {
    let (rows, resolved_id) = if playlist_id.is_empty()
        || playlist_id == LOCAL_PLAYLIST_ID
        || playlist_id == "default"
    {
        let rows = local_music_tracks::Entity::find()
            .filter(local_music_tracks::Column::Enabled.eq(true))
            .order_by_asc(local_music_tracks::Column::SortOrder)
            .order_by_asc(local_music_tracks::Column::Id)
            .all(db)
            .await?;
        (rows, LOCAL_PLAYLIST_ID.to_string())
    } else {
        let Ok(pl_id) = playlist_id.parse::<i32>() else {
            return Err(anyhow!("unknown local playlist"));
        };
        let playlist = local_music_playlists::Entity::find_by_id(pl_id)
            .one(db)
            .await?
            .ok_or_else(|| anyhow!("playlist not found"))?;
        let links = local_music_playlist_tracks::Entity::find()
            .filter(local_music_playlist_tracks::Column::PlaylistId.eq(pl_id))
            .order_by_asc(local_music_playlist_tracks::Column::SortOrder)
            .all(db)
            .await?;
        let mut rows = Vec::new();
        for link in links {
            if let Some(row) = local_music_tracks::Entity::find_by_id(link.track_id)
                .one(db)
                .await?
                && row.enabled
            {
                rows.push(row);
            }
        }
        (rows, playlist.id.to_string())
    };
    let songs = rows
        .into_iter()
        .map(|row| PlayerSong {
            id: row.id.to_string(),
            name: row.title,
            artist: if row.artist.is_empty() {
                "Unknown".to_string()
            } else {
                row.artist
            },
            album: row.album,
            cover: cover_url(row.cover_media_id).unwrap_or_default(),
            duration: row.duration_ms / 1000,
            is_vip: false,
        })
        .collect();
    Ok(PlayerPlaylist {
        code: 200,
        source: PlayerMusicSource::Local,
        playlist_id: resolved_id,
        songs,
    })
}

fn normalize_lyrics(lyrics: Option<String>) -> Option<String> {
    lyrics
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

async fn ensure_media_mime_prefix(
    db: &DatabaseConnection,
    media_id: i32,
    role: &str,
    prefix: &str,
) -> Result<()> {
    let found = media_assets::Entity::find_by_id(media_id).one(db).await?;
    let row = found.ok_or_else(|| anyhow!("{role} media {media_id} not found"))?;
    if !row.mime.to_ascii_lowercase().starts_with(prefix) {
        return Err(anyhow!(
            "{role} media {media_id} is not an {} asset",
            prefix.trim_end_matches('/')
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audio_url_is_stable_for_player() {
        assert_eq!(audio_url(12), "/api/proxy/music/local/audio/12");
    }

    #[test]
    fn empty_lyrics_are_dropped() {
        assert_eq!(normalize_lyrics(Some("  \n".into())), None);
        assert_eq!(normalize_lyrics(Some("[00:01.00] hi ".into())).as_deref(), Some("[00:01.00] hi"));
    }

    #[test]
    fn delete_track_releases_audio_and_cover_media() {
        let src = include_str!("local_music.rs");
        let body = src
            .split("pub async fn delete_track(")
            .nth(1)
            .and_then(|rest| rest.split("\npub async fn ").next())
            .expect("delete_track");
        assert!(body.contains("release_media_if_unused"));
        assert!(body.contains("audio_media_id"));
        assert!(body.contains("cover_media_id"));
        assert!(body.contains("local_music_playlist_tracks"));
    }

    #[test]
    fn update_track_releases_replaced_media() {
        let src = include_str!("local_music.rs");
        let body = src
            .split("pub async fn update_track(")
            .nth(1)
            .and_then(|rest| rest.split("\npub async fn ").next())
            .expect("update_track");
        assert!(body.contains("old_audio_media_id"));
        assert!(body.contains("old_cover_media_id"));
        assert!(body.contains("release_media_if_unused"));
    }

    #[test]
    fn release_skips_media_still_bound_to_a_track() {
        let src = include_str!("local_music.rs");
        let body = src
            .split("async fn release_media_if_unused(")
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .expect("release_media_if_unused");
        assert!(body.contains("AudioMediaId.eq"));
        assert!(body.contains("CoverMediaId.eq"));
        assert!(body.contains("MediaService::from_data_paths"));
        assert!(body.contains("service.delete"));
    }
}
