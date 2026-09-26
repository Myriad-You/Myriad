//! Local music library HTTP: admin catalog CRUD + guest player/lyrics/audio.

use axum::{
    Json,
    body::Bytes,
    extract::{Path, State},
    http::{HeaderMap, StatusCode, header},
    response::{IntoResponse, Response},
};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::json;

use crate::error::HttpError;
use crate::extract::AdminClaims;
use crate::services::data_paths::paths;
use crate::services::local_music::{
    self, LOCAL_PLAYLIST_ID, LocalPlaylistInput, LocalTrackInput, build_player_playlist,
};
use crate::services::media::{
    MediaActor, MediaContext, MediaExposure, MediaService, MediaSource, NewMediaBytes,
};
use crate::services::memory_profile::{max_audio_bytes, note_image_limit};
use crate::services::music_player_view::{self, PlayerMusicSource};
use myriad_error::AppError;

fn local_http(status: StatusCode, error: impl Into<String>) -> HttpError {
    HttpError(AppError::from_status_u16(status.as_u16(), error.into()))
}

/// GET /api/local-music — admin catalog.
pub async fn list_local_tracks(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
) -> Result<Json<serde_json::Value>, HttpError> {
    let tracks = local_music::list_tracks(&db).await.map_err(|err| {
        tracing::error!(%err, "list local music");
        local_http(StatusCode::INTERNAL_SERVER_ERROR, "Failed to list local music")
    })?;
    Ok(Json(json!({ "tracks": tracks })))
}

/// POST /api/local-music — admin create from already-uploaded media ids.
pub async fn create_local_track(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Json(input): Json<LocalTrackInput>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let track = local_music::create_track(&db, input).await.map_err(|err| {
        tracing::warn!(%err, "create local music track");
        local_http(StatusCode::BAD_REQUEST, err.to_string())
    })?;
    Ok(Json(json!(track)))
}

/// PATCH /api/local-music/{id}
pub async fn update_local_track(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path(id): Path<i32>,
    Json(input): Json<LocalTrackInput>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let track = local_music::update_track(&db, id, input).await.map_err(|err| {
        tracing::warn!(%err, id, "update local music track");
        local_http(StatusCode::BAD_REQUEST, err.to_string())
    })?;
    Ok(Json(json!(track)))
}

/// DELETE /api/local-music/{id}
pub async fn delete_local_track(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    local_music::delete_track(&db, id).await.map_err(|err| {
        tracing::error!(%err, id, "delete local music track");
        local_http(StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete track")
    })?;
    Ok(Json(json!({ "ok": true })))
}

/// GET /api/local-music/playlists
pub async fn list_local_playlists(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
) -> Result<Json<serde_json::Value>, HttpError> {
    let playlists = local_music::list_playlists(&db).await.map_err(|err| {
        tracing::error!(%err, "list local playlists");
        local_http(StatusCode::INTERNAL_SERVER_ERROR, "Failed to list playlists")
    })?;
    Ok(Json(json!({ "playlists": playlists })))
}

/// POST /api/local-music/playlists
pub async fn create_local_playlist(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Json(input): Json<LocalPlaylistInput>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let playlist = local_music::create_playlist(&db, input).await.map_err(|err| {
        tracing::warn!(%err, "create local playlist");
        local_http(StatusCode::BAD_REQUEST, err.to_string())
    })?;
    Ok(Json(json!(playlist)))
}

/// PATCH /api/local-music/playlists/{id}
pub async fn update_local_playlist(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path(id): Path<i32>,
    Json(input): Json<LocalPlaylistInput>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let playlist = local_music::update_playlist(&db, id, input).await.map_err(|err| {
        tracing::warn!(%err, id, "update local playlist");
        local_http(StatusCode::BAD_REQUEST, err.to_string())
    })?;
    Ok(Json(json!(playlist)))
}

/// DELETE /api/local-music/playlists/{id}
pub async fn delete_local_playlist(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    local_music::delete_playlist(&db, id).await.map_err(|err| {
        tracing::error!(%err, id, "delete local playlist");
        local_http(StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete playlist")
    })?;
    Ok(Json(json!({ "ok": true })))
}

/// GET /api/proxy/music/local/playlist/{id} — guest, same shape as netease/qq.
pub async fn proxy_local_playlist(
    State(db): State<DatabaseConnection>,
    Path(playlist_id): Path<String>,
) -> Response {
    // Named playlists use numeric ids; "local"/"default" is the full catalog.
    let is_named = playlist_id
        .parse::<i32>()
        .map(|id| id > 0)
        .unwrap_or(false);
    if !is_named && playlist_id != LOCAL_PLAYLIST_ID && playlist_id != "default" {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Unknown local playlist", "code": "playlist_not_found"})),
        )
            .into_response();
    }
    let id = if playlist_id.is_empty() {
        LOCAL_PLAYLIST_ID.to_string()
    } else {
        playlist_id.clone()
    };
    let result = music_player_view::load_player_playlist(PlayerMusicSource::Local, &id, || {
        let db = db.clone();
        let pid = id.clone();
        async move {
            build_player_playlist(&db, &pid).await.map_err(|error| {
                tracing::error!(%error, "Failed to build local playlist");
                music_player_view::PlayerPlaylistError::FetchFailed
            })
        }
    })
    .await;
    match result {
        Ok(view) => (
            StatusCode::OK,
            [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
            Json(view.as_ref()),
        )
            .into_response(),
        Err(music_player_view::PlayerPlaylistError::RateLimited) => (
            StatusCode::TOO_MANY_REQUESTS,
            Json(json!({"error": "Too many requests"})),
        )
            .into_response(),
        Err(music_player_view::PlayerPlaylistError::FetchFailed) => (
            StatusCode::BAD_GATEWAY,
            Json(json!({"error": "Failed to fetch playlist", "code": "playlist_fetch_failed"})),
        )
            .into_response(),
    }
}

/// GET /api/proxy/music/local/lyrics/{id} — guest LRC text (iro-compatible shape).
pub async fn proxy_local_lyrics(
    State(db): State<DatabaseConnection>,
    Path(id): Path<i32>,
) -> Response {
    match local_music::track_lyrics(&db, id).await {
        Ok(lyrics) => {
            let lrc = lyrics.unwrap_or_default();
            (
                StatusCode::OK,
                [(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")],
                Json(json!({ "lrc": lrc, "code": 200 })),
            )
                .into_response()
        }
        Err(_) => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Track not found", "code": "track_not_found"})),
        )
            .into_response(),
    }
}

/// GET /api/proxy/music/local/audio/{id} — guest stream (same-origin, spectrum-safe).
pub async fn proxy_local_audio(
    State(db): State<DatabaseConnection>,
    Path(id): Path<i32>,
    headers: HeaderMap,
) -> Response {
    let media_id = match local_music::track_audio_media_id(&db, id).await {
        Ok(media_id) => media_id,
        Err(_) => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({"error": "Track not found", "code": "track_not_found"})),
            )
                .into_response();
        }
    };
    serve_media_bytes(&db, media_id, headers).await
}

/// GET /api/proxy/music/local/cover/{mediaId} — guest cover image.
pub async fn proxy_local_cover(
    State(db): State<DatabaseConnection>,
    Path(media_id): Path<i32>,
    headers: HeaderMap,
) -> Response {
    if local_music::cover_media_id(&db, media_id).await.is_err() {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Cover not found", "code": "cover_not_found"})),
        )
            .into_response();
    }
    serve_media_bytes(&db, media_id, headers).await
}

async fn serve_media_bytes(
    db: &DatabaseConnection,
    media_id: i32,
    headers: HeaderMap,
) -> Response {
    match crate::services::media::resolve_guest_media_bytes(db, media_id).await {
        Ok((mime, bytes)) => {
            let total = bytes.len() as u64;
            if let Some(spec) = headers
                .get(header::RANGE)
                .and_then(|v| v.to_str().ok())
                .and_then(parse_single_byte_range)
            {
                match resolve_byte_range(spec, total) {
                    ResolvedByteRange::Unsatisfiable => {
                        return Response::builder()
                            .status(StatusCode::RANGE_NOT_SATISFIABLE)
                            .header(header::CONTENT_RANGE, format!("bytes */{total}"))
                            .body(axum::body::Body::empty())
                            .unwrap_or_else(|_| StatusCode::RANGE_NOT_SATISFIABLE.into_response());
                    }
                    ResolvedByteRange::Ok { start, end } => {
                        let slice = bytes[start as usize..=end as usize].to_vec();
                        let len = slice.len() as u64;
                        return Response::builder()
                            .status(StatusCode::PARTIAL_CONTENT)
                            .header(header::CONTENT_TYPE, mime)
                            .header(header::ACCEPT_RANGES, "bytes")
                            .header(
                                header::CONTENT_RANGE,
                                format!("bytes {start}-{end}/{total}"),
                            )
                            .header(header::CONTENT_LENGTH, len.to_string())
                            .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                            .body(axum::body::Body::from(slice))
                            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());
                    }
                }
            }
            Response::builder()
                .status(StatusCode::OK)
                .header(header::CONTENT_TYPE, mime)
                .header(header::ACCEPT_RANGES, "bytes")
                .header(header::CONTENT_LENGTH, total.to_string())
                .header(header::ACCESS_CONTROL_ALLOW_ORIGIN, "*")
                .body(axum::body::Body::from(bytes))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Err(_) => StatusCode::NOT_FOUND.into_response(),
    }
}

/// A parsed `Range` request unit. Suffix form needs `total` before it becomes
/// absolute offsets (RFC 7233: `bytes=-N` is the last N bytes).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ByteRangeSpec {
    /// `bytes=start-end` or `bytes=start-` (end may be `u64::MAX` = open).
    Absolute { start: u64, end: u64 },
    /// `bytes=-suffix` — last `suffix` bytes of the representation.
    Suffix(u64),
}

/// Parse one `bytes=start-end` / `bytes=start-` / `bytes=-suffix` range.
/// Invalid specs (including `end < start` and `bytes=-0`) return `None` so the
/// Range header is ignored and the full body is served (RFC 7233 §3.1).
fn parse_single_byte_range(raw: &str) -> Option<ByteRangeSpec> {
    let spec = raw.trim().strip_prefix("bytes=")?;
    // Multi-range (`bytes=0-10,20-30`): only the first unit is served. RFC 7233
    // allows a server to ignore multi-range entirely (200); we answer 206 for
    // the first unit, which simple media clients use as a single range.
    let first = spec.split(',').next()?.trim();
    let (start_s, end_s) = first.split_once('-')?;
    let start_s = start_s.trim();
    let end_s = end_s.trim();
    if start_s.is_empty() {
        let suffix: u64 = end_s.parse().ok()?;
        if suffix == 0 {
            return None;
        }
        return Some(ByteRangeSpec::Suffix(suffix));
    }
    let start: u64 = start_s.parse().ok()?;
    if end_s.is_empty() {
        return Some(ByteRangeSpec::Absolute {
            start,
            end: u64::MAX,
        });
    }
    let end: u64 = end_s.parse().ok()?;
    // last-byte-pos < first-byte-pos is an invalid byte-range-spec.
    if end < start {
        return None;
    }
    Some(ByteRangeSpec::Absolute { start, end })
}

/// Outcome of mapping a [`ByteRangeSpec`] onto a representation of `total` bytes.
/// Invalid Range headers never reach this function — `parse_single_byte_range`
/// returns `None` and the full body is served (200).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResolvedByteRange {
    /// Syntactically valid but unsatisfiable — MUST answer 416 (RFC 7233 §4.4).
    Unsatisfiable,
    /// Inclusive absolute offsets inside `[0, total)`.
    Ok { start: u64, end: u64 },
}

/// Map a parsed range onto `[0, total)` using RFC 7233 suffix semantics.
fn resolve_byte_range(range: ByteRangeSpec, total: u64) -> ResolvedByteRange {
    if total == 0 {
        // A valid range over an empty representation is unsatisfiable.
        return ResolvedByteRange::Unsatisfiable;
    }
    match range {
        ByteRangeSpec::Suffix(suffix) => {
            let take = suffix.min(total);
            ResolvedByteRange::Ok {
                start: total - take,
                end: total - 1,
            }
        }
        ByteRangeSpec::Absolute { start, end } => {
            if start >= total {
                return ResolvedByteRange::Unsatisfiable;
            }
            let end = end.min(total - 1);
            // parse rejects end < start for closed ranges; open-ended ones
            // clamp above, so start > end cannot occur here.
            ResolvedByteRange::Ok { start, end }
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct UploadLocalTrackForm {
    pub title: Option<String>,
    pub artist: Option<String>,
    pub album: Option<String>,
    pub duration_ms: Option<i64>,
    pub lyrics: Option<String>,
    pub sort_order: Option<i32>,
}

/// POST /api/local-music/upload — admin multipart audio (+ optional cover).
///
/// Fields: `audio` (required), `cover` (optional), plus text metadata.
pub async fn upload_local_track(
    State(db): State<DatabaseConnection>,
    AdminClaims(claims): AdminClaims,
    mut multipart: axum::extract::Multipart,
) -> Result<Json<serde_json::Value>, HttpError> {
    let user_id: i32 = claims
        .durable_user_id()
        .ok_or_else(|| local_http(StatusCode::UNAUTHORIZED, "Invalid user ID"))?;
    let actor = MediaActor::admin(user_id).map_err(|err| HttpError(err.into()))?;
    let service = MediaService::from_data_paths(paths());

    let mut audio: Option<(String, String, Bytes)> = None;
    let mut cover: Option<(String, String, Bytes)> = None;
    let mut meta = UploadLocalTrackForm {
        title: None,
        artist: None,
        album: None,
        duration_ms: None,
        lyrics: None,
        sort_order: None,
    };

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|_| local_http(StatusCode::BAD_REQUEST, "Invalid multipart body"))?
    {
        let name = field.name().unwrap_or("").to_string();
        let filename = field.file_name().unwrap_or("").to_string();
        let mime = field
            .content_type()
            .map(|m| m.to_string())
            .unwrap_or_else(|| "application/octet-stream".into());
        let bytes = field
            .bytes()
            .await
            .map_err(|_| local_http(StatusCode::BAD_REQUEST, "Failed to read upload"))?;
        match name.as_str() {
            "audio" => audio = Some((filename, mime, bytes)),
            "cover" => cover = Some((filename, mime, bytes)),
            "title" => meta.title = Some(String::from_utf8_lossy(&bytes).to_string()),
            "artist" => meta.artist = Some(String::from_utf8_lossy(&bytes).to_string()),
            "album" => meta.album = Some(String::from_utf8_lossy(&bytes).to_string()),
            "duration_ms" => {
                meta.duration_ms = String::from_utf8_lossy(&bytes).trim().parse().ok()
            }
            "lyrics" => meta.lyrics = Some(String::from_utf8_lossy(&bytes).to_string()),
            "sort_order" => meta.sort_order = String::from_utf8_lossy(&bytes).trim().parse().ok(),
            _ => {}
        }
    }

    let (audio_name, audio_mime, audio_bytes) =
        audio.ok_or_else(|| local_http(StatusCode::BAD_REQUEST, "audio file is required"))?;
    // Browser Content-Type is unreliable (audio/mp3, octet-stream, …).
    let audio_mime = crate::services::media::resolve_upload_audio_mime(&audio_mime, &audio_name);
    let audio_mime_log = audio_mime.clone();
    let audio_name_log = audio_name.clone();
    // Local library only accepts mp3 / flac / ogg (tagged formats).
    let ext_guess = crate::services::media::audio_mime_from_filename(&audio_name)
        .map(|m| m.to_string())
        .unwrap_or_else(|| audio_mime.clone());
    if !matches!(
        ext_guess.as_str(),
        "audio/mpeg" | "audio/flac" | "audio/ogg"
    ) {
        return Err(local_http(
            StatusCode::BAD_REQUEST,
            "Only mp3 / flac / ogg are supported for local music",
        ));
    }
    let tags_source = audio_bytes.clone();

    let audio_asset = service
        .create_from_bytes(
            &db,
            MediaContext::site(actor.clone(), MediaSource::Upload),
            NewMediaBytes {
                max_bytes: max_audio_bytes(),
                bytes: audio_bytes,
                claimed_mime: audio_mime,
                filename: audio_name,
                derived_from_id: None,
                exposure: MediaExposure::Public,
            },
        )
        .await
        .map_err(|err| {
            tracing::warn!(%err, mime=%audio_mime_log, name=%audio_name_log, "local music audio upload");
            local_http(
                StatusCode::BAD_REQUEST,
                format!("{err} (mime={audio_mime_log}, file={audio_name_log})"),
            )
        })?;

    let cover_media_id = if let Some((cover_name, cover_mime, cover_bytes)) = cover {
        let cover_asset = service
            .create_from_bytes(
                &db,
                MediaContext::site(actor.clone(), MediaSource::Upload),
                NewMediaBytes {
                    max_bytes: note_image_limit(),
                    bytes: cover_bytes,
                    claimed_mime: cover_mime,
                    filename: cover_name,
                    derived_from_id: None,
                    exposure: MediaExposure::Public,
                },
            )
            .await
            .map_err(|err| {
                tracing::warn!(%err, "local music cover upload");
                local_http(StatusCode::BAD_REQUEST, err.to_string())
            })?;
        Some(cover_asset.id)
    } else {
        None
    };

    let ext_for_tags = match ext_guess.as_str() {
        "audio/mpeg" => "mp3",
        "audio/flac" => "flac",
        _ => "ogg",
    };
    let tags = crate::services::audio_tags::parse_audio_tags(&tags_source, ext_for_tags);
    // Embedded album art when no separate cover was uploaded.
    let mut cover_media_id = cover_media_id;
    if cover_media_id.is_none()
        && let Some((cover_mime, cover_bytes)) = tags.cover.clone()
    {
        let stem = audio_name_log
            .rsplit_once('.')
            .map(|(s, _)| s.to_string())
            .unwrap_or_else(|| audio_name_log.clone());
        match service
            .create_from_bytes(
                &db,
                MediaContext::site(actor.clone(), MediaSource::Generated),
                NewMediaBytes {
                    max_bytes: note_image_limit(),
                    bytes: cover_bytes.into(),
                    claimed_mime: cover_mime,
                    filename: format!("{stem}-cover"),
                    derived_from_id: Some(audio_asset.id),
                    exposure: MediaExposure::Public,
                },
            )
            .await
        {
            Ok(asset) => cover_media_id = Some(asset.id),
            Err(err) => tracing::warn!(%err, "local music embedded cover"),
        }
    }
    let title = meta
        .title
        .filter(|s| !s.trim().is_empty())
        .or(tags.title.clone())
        .unwrap_or_else(|| {
            audio_asset
                .name
                .rsplit_once('.')
                .map(|(stem, _)| stem.to_string())
                .unwrap_or_else(|| audio_asset.name.clone())
        })
        .trim()
        .to_string();
    let artist = meta
        .artist
        .filter(|s| !s.trim().is_empty())
        .or(tags.artist.clone())
        .unwrap_or_default();
    let album = meta
        .album
        .filter(|s| !s.trim().is_empty())
        .or(tags.album.clone())
        .unwrap_or_default();
    let lyrics = meta
        .lyrics
        .filter(|s| !s.trim().is_empty())
        .or(tags.lyrics.clone());
    let mut duration_ms = meta.duration_ms.unwrap_or(0);
    if duration_ms <= 0 {
        duration_ms = tags.duration_ms.unwrap_or(0);
    }
    // HTMLAudioElement / ID3 TLEN may report µs or s-as-ms; normalize to milliseconds.
    // Real songs are 1s–24h; values above one day in ms are almost certainly µs.
    const DAY_MS: i64 = 24 * 3600 * 1000;
    if duration_ms > DAY_MS && duration_ms < DAY_MS * 1000 {
        duration_ms /= 1000;
    }
    if duration_ms > DAY_MS {
        duration_ms = 0;
    }
    let input = LocalTrackInput {
        title,
        artist,
        album,
        duration_ms,
        audio_media_id: audio_asset.id,
        cover_media_id,
        lyrics,
        sort_order: meta.sort_order.unwrap_or(0),
        enabled: true,
    };
    let track = local_music::create_track(&db, input).await.map_err(|err| {
        tracing::warn!(%err, "local music track row");
        local_http(StatusCode::BAD_REQUEST, err.to_string())
    })?;
    Ok(Json(json!(track)))
}

#[cfg(test)]
mod tests {
    use super::{
        ByteRangeSpec, ResolvedByteRange, parse_single_byte_range, resolve_byte_range,
    };

    #[test]
    fn suffix_range_targets_the_tail() {
        // RFC 7233: bytes=-500 is the last 500 bytes, not the first 500.
        let spec = parse_single_byte_range("bytes=-500").unwrap();
        assert_eq!(spec, ByteRangeSpec::Suffix(500));
        assert_eq!(
            resolve_byte_range(spec, 1000),
            ResolvedByteRange::Ok {
                start: 500,
                end: 999
            }
        );
        // Suffix longer than the representation clamps to the whole body.
        assert_eq!(
            resolve_byte_range(spec, 200),
            ResolvedByteRange::Ok {
                start: 0,
                end: 199
            }
        );
    }

    #[test]
    fn absolute_and_open_end_ranges() {
        assert_eq!(
            parse_single_byte_range("bytes=0-499"),
            Some(ByteRangeSpec::Absolute { start: 0, end: 499 })
        );
        assert_eq!(
            parse_single_byte_range("bytes=500-"),
            Some(ByteRangeSpec::Absolute {
                start: 500,
                end: u64::MAX
            })
        );
        assert_eq!(
            resolve_byte_range(ByteRangeSpec::Absolute { start: 0, end: 499 }, 1000),
            ResolvedByteRange::Ok {
                start: 0,
                end: 499
            }
        );
        assert_eq!(
            resolve_byte_range(
                ByteRangeSpec::Absolute {
                    start: 500,
                    end: u64::MAX
                },
                1000
            ),
            ResolvedByteRange::Ok {
                start: 500,
                end: 999
            }
        );
    }

    #[test]
    fn unsatisfiable_ranges_must_416_not_200() {
        // Empty representation: any valid range is unsatisfiable → 416.
        assert_eq!(
            resolve_byte_range(ByteRangeSpec::Suffix(10), 0),
            ResolvedByteRange::Unsatisfiable
        );
        // start past EOF → 416 (not a silent 200 full-body).
        assert_eq!(
            resolve_byte_range(ByteRangeSpec::Absolute { start: 100, end: 200 }, 100),
            ResolvedByteRange::Unsatisfiable
        );
        assert_eq!(
            resolve_byte_range(
                ByteRangeSpec::Absolute {
                    start: 999,
                    end: u64::MAX
                },
                100
            ),
            ResolvedByteRange::Unsatisfiable
        );
    }

    #[test]
    fn invalid_specs_are_ignored_not_416() {
        // bytes=-0 and end<start are invalid byte-range-specs → ignore → 200.
        assert_eq!(parse_single_byte_range("bytes=-0"), None);
        assert_eq!(parse_single_byte_range("bytes=10-5"), None);
        assert_eq!(parse_single_byte_range("bytes=abc-def"), None);
        assert_eq!(parse_single_byte_range("items=0-1"), None);
        // Multi-range takes the first unit only.
        assert_eq!(
            parse_single_byte_range("bytes=0-10,20-30"),
            Some(ByteRangeSpec::Absolute { start: 0, end: 10 })
        );
    }
}
