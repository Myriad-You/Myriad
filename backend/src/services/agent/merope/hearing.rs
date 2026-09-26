//! Hearing a song she listens to on her own.
//!
//! The recording itself is fetched from where the site's player gets it
//! (NetEase, or the site's local library) with its timed lyrics, and heard
//! through `myriad_listening`: what happens in its sound, measured; the
//! lyrics on its timeline; what listening research says moments like those
//! tend to do. That sheet is what she feels from, in her own model, and
//! what she can say about the song afterwards. Without the recording she has
//! only its words, and knows it.
//!
//! One song is heard at a time: hearing starts when she puts it on, and the
//! sheet stays until the next song.

use std::sync::{Arc, LazyLock, Mutex};
use std::time::Duration;

use myriad_listening::ListeningSheet;
use sea_orm::DatabaseConnection;

use super::doing::Thing;
use crate::services::music_player_view::PlayerMusicSource;

const FETCH_TIMEOUT: Duration = Duration::from_secs(90);

/// The song heard now, by its key.
static HEARD: LazyLock<Mutex<Option<(String, Arc<ListeningSheet>)>>> =
    LazyLock::new(|| Mutex::new(None));

/// The sheet of the song she is hearing, once it is heard.
pub fn heard(key: &str) -> Option<Arc<ListeningSheet>> {
    HEARD
        .lock()
        .ok()?
        .as_ref()
        .filter(|(heard, _)| heard == key)
        .map(|(_, sheet)| sheet.clone())
}

/// Start hearing a song as she puts it on.
pub fn start(db: DatabaseConnection, key: String, thing: Thing) {
    if let Ok(mut heard) = HEARD.lock() {
        *heard = None;
    }
    tokio::spawn(async move {
        if let Some(sheet) = hear(&db, &thing).await {
            if let Ok(mut heard) = HEARD.lock() {
                *heard = Some((key, Arc::new(sheet)));
            }
        }
    });
}

/// The song's sheet: the one already heard, or heard now.
pub async fn sheet_for(
    db: &DatabaseConnection,
    key: &str,
    thing: &Thing,
) -> Option<Arc<ListeningSheet>> {
    if let Some(sheet) = heard(key) {
        return Some(sheet);
    }
    let sheet = Arc::new(hear(db, thing).await?);
    if let Ok(mut heard) = HEARD.lock() {
        *heard = Some((key.to_string(), sheet.clone()));
    }
    Some(sheet)
}

/// A new persona has heard nothing.
pub(super) fn forget() {
    if let Ok(mut heard) = HEARD.lock() {
        *heard = None;
    }
}

async fn hear(db: &DatabaseConnection, thing: &Thing) -> Option<ListeningSheet> {
    let Thing::Song { id, source, .. } = thing else {
        return None;
    };
    let fetched = tokio::time::timeout(FETCH_TIMEOUT, async {
        let lrc = timed_lyrics(db, source, id).await;
        let audio = recording(db, source, id).await;
        audio.map(|(bytes, ext)| (bytes, ext, lrc))
    })
    .await;
    let Ok(Some((bytes, ext, lrc))) = fetched else {
        tracing::info!(song = %id, "[Merope] could not get the recording to hear");
        return None;
    };
    let heard = tokio::task::spawn_blocking(move || {
        myriad_listening::listen_to_bytes(bytes, ext, lrc.as_deref())
    })
    .await;
    match heard {
        Ok(Ok(sheet)) => Some(sheet),
        Ok(Err(error)) => {
            tracing::info!(song = %id, %error, "[Merope] could not hear the recording");
            None
        }
        Err(error) => {
            tracing::warn!(song = %id, %error, "[Merope] hearing a song failed");
            None
        }
    }
}

/// The song's timed lyrics (LRC), if it has any.
pub async fn timed_lyrics(db: &DatabaseConnection, source: &str, id: &str) -> Option<String> {
    let lrc = if source == PlayerMusicSource::Netease.as_str() {
        let data = crate::services::netease_service::NeteaseService::new()
            .fetch_lyrics(id.parse().ok()?)
            .await
            .ok()?;
        data.pointer("/lrc/lyric")?.as_str()?.to_string()
    } else if source == PlayerMusicSource::Local.as_str() {
        crate::services::local_music::track_lyrics(db, id.parse().ok()?)
            .await
            .ok()??
    } else {
        return None;
    };
    Some(lrc).filter(|lrc| !lrc.trim().is_empty())
}

/// The recording's bytes and a hint of its format.
async fn recording(
    db: &DatabaseConnection,
    source: &str,
    id: &str,
) -> Option<(Vec<u8>, Option<&'static str>)> {
    if source == PlayerMusicSource::Netease.as_str() {
        let url = crate::services::netease_service::NeteaseService::new()
            .fetch_audio_url(id.parse().ok()?)
            .await
            .ok()?;
        let response = crate::services::http_client::MEDIA_FETCH_CLIENT
            .get(&url.url)
            .header("Referer", "https://music.163.com/")
            .timeout(FETCH_TIMEOUT)
            .send()
            .await
            .ok()?
            .error_for_status()
            .ok()?;
        let ext = extension(
            response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .unwrap_or("audio/mpeg"),
        );
        let bytes = crate::services::outbound_security::read_limited_body(
            response,
            crate::services::memory_profile::max_audio_bytes(),
        )
        .await
        .ok()?;
        Some((bytes, ext))
    } else if source == PlayerMusicSource::Local.as_str() {
        let media_id = crate::services::local_music::track_audio_media_id(db, id.parse().ok()?)
            .await
            .ok()?;
        let (mime, bytes) = crate::services::media::resolve_guest_media_bytes(db, media_id)
            .await
            .ok()?;
        Some((bytes, extension(&mime)))
    } else {
        None
    }
}

fn extension(mime: &str) -> Option<&'static str> {
    let mime = mime.to_ascii_lowercase();
    Some(match mime.split(';').next()?.trim() {
        "audio/mpeg" | "audio/mp3" => "mp3",
        "audio/flac" | "audio/x-flac" => "flac",
        "audio/mp4" | "audio/m4a" | "audio/x-m4a" | "audio/aac" => "m4a",
        "audio/ogg" | "audio/vorbis" => "ogg",
        "audio/wav" | "audio/x-wav" | "audio/wave" => "wav",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_format_is_guessed_from_its_type() {
        assert_eq!(extension("audio/mpeg"), Some("mp3"));
        assert_eq!(extension("Audio/FLAC; charset=binary"), Some("flac"));
        assert_eq!(extension("application/octet-stream"), None);
    }
}
