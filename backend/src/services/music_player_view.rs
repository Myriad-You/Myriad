//! Player playlist view: vendor JSON → the fields the music player actually uses.
//!
//! `NeteaseService::fetch_playlist` stays fat — platform liked-songs still needs
//! full tracks. This module is the player-proxy boundary only.

use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, OwnedMutexGuard, RwLock};

use super::netease_service::{CacheEntry, MUSIC_CACHE};
use super::netease_utils::ensure_https_url;

static PLAYER_PLAYLIST_LOADS: Lazy<RwLock<HashMap<String, Weak<Mutex<()>>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

pub const PLAYER_PLAYLIST_CACHE_TTL: Duration = Duration::from_secs(604_800);
pub const PLAYER_PLAYLIST_CACHE_VERSION: &str = "v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerMusicSource {
    Netease,
    Qq,
}

impl PlayerMusicSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Netease => "netease",
            Self::Qq => "qq",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerSong {
    pub id: String,
    pub name: String,
    pub artist: String,
    pub album: String,
    pub cover: String,
    pub duration: i64,
    pub is_vip: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlayerPlaylist {
    pub code: i64,
    pub source: PlayerMusicSource,
    pub playlist_id: String,
    pub songs: Vec<PlayerSong>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QqPlaylistError {
    Invalid,
}

pub fn player_playlist_cache_key(source: PlayerMusicSource, playlist_id: &str) -> String {
    format!(
        "playlist_player:{}:{}:{}",
        PLAYER_PLAYLIST_CACHE_VERSION,
        source.as_str(),
        playlist_id
    )
}

pub async fn get_cached_player_playlist(
    source: PlayerMusicSource,
    playlist_id: &str,
) -> Option<PlayerPlaylist> {
    let key = player_playlist_cache_key(source, playlist_id);
    let mut cache = MUSIC_CACHE.write().await;
    let entry = cache.get(&key)?;
    serde_json::from_value(entry.data.clone()).ok()
}

pub async fn set_cached_player_playlist(playlist: &PlayerPlaylist) {
    let Ok(data) = serde_json::to_value(playlist) else {
        return;
    };
    let key = player_playlist_cache_key(playlist.source, &playlist.playlist_id);
    let mut cache = MUSIC_CACHE.write().await;
    cache.insert(
        key,
        CacheEntry {
            data,
            expires_at: Instant::now() + PLAYER_PLAYLIST_CACHE_TTL,
        },
    );
}

/// Serialize upstream fetches for one player playlist. Callers must re-check
/// the cache after acquiring this lock so a stampede shares one fat parse.
pub async fn lock_player_playlist_load(
    source: PlayerMusicSource,
    playlist_id: &str,
) -> OwnedMutexGuard<()> {
    let key = player_playlist_cache_key(source, playlist_id);
    let lock = {
        let mut locks = PLAYER_PLAYLIST_LOADS.write().await;
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(&key).and_then(Weak::upgrade) {
            lock
        } else {
            let lock = Arc::new(Mutex::new(()));
            locks.insert(key, Arc::downgrade(&lock));
            lock
        }
    };
    lock.lock_owned().await
}

pub fn project_netease_player_playlist(playlist_id: &str, upstream: &Value) -> PlayerPlaylist {
    let tracks = upstream
        .pointer("/playlist/tracks")
        .or_else(|| upstream.pointer("/result/playlist/tracks"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    PlayerPlaylist {
        code: 200,
        source: PlayerMusicSource::Netease,
        playlist_id: playlist_id.to_string(),
        songs: tracks.iter().filter_map(project_netease_track).collect(),
    }
}

pub fn project_qq_player_playlist(
    playlist_id: &str,
    upstream: &Value,
) -> Result<PlayerPlaylist, QqPlaylistError> {
    let cdlist = upstream
        .get("cdlist")
        .and_then(Value::as_array)
        .filter(|list| !list.is_empty())
        .ok_or(QqPlaylistError::Invalid)?;
    let songlist = cdlist[0]
        .get("songlist")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or(&[]);
    Ok(PlayerPlaylist {
        code: 200,
        source: PlayerMusicSource::Qq,
        playlist_id: playlist_id.to_string(),
        songs: songlist.iter().filter_map(project_qq_track).collect(),
    })
}

fn project_netease_track(track: &Value) -> Option<PlayerSong> {
    let id = json_id(track.get("id")?)?;
    let name = string_field(track, "name");
    let artist = named_join(
        track
            .get("ar")
            .or_else(|| track.get("artists"))
            .and_then(Value::as_array)
            .map(Vec::as_slice),
    );
    let album_obj = track.get("al").or_else(|| track.get("album"));
    let album = album_obj
        .and_then(|album| album.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let cover_raw = album_obj
        .and_then(|album| {
            album
                .get("picUrl")
                .or_else(|| album.get("blurPicUrl"))
                .and_then(Value::as_str)
        })
        .unwrap_or("");
    let cover = if cover_raw.is_empty() {
        String::new()
    } else {
        ensure_https_url(cover_raw)
    };
    let duration_ms = track
        .get("dt")
        .or_else(|| track.get("duration"))
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    let is_vip = track
        .get("isVip")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            let fee = track.get("fee").and_then(Value::as_i64).unwrap_or(0);
            fee == 1 || fee == 4
        });
    Some(PlayerSong {
        id,
        name,
        artist,
        album,
        cover,
        duration: duration_ms / 1000,
        is_vip,
    })
}

fn project_qq_track(song: &Value) -> Option<PlayerSong> {
    let id = json_id(song.get("songmid").or_else(|| song.get("id"))?)?;
    let name = song
        .get("songname")
        .or_else(|| song.get("name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let artist = named_join(
        song.get("singer")
            .and_then(Value::as_array)
            .map(Vec::as_slice),
    );
    let album = song
        .get("albumname")
        .or_else(|| song.pointer("/album/name"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let cover = song
        .get("albummid")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|mid| !mid.is_empty())
        .map(|mid| format!("https://y.gtimg.cn/music/photo_new/T002R300x300M000{mid}.jpg"))
        .unwrap_or_default();
    let duration = song
        .get("interval")
        .and_then(Value::as_i64)
        .unwrap_or(0)
        .max(0);
    let is_vip = song
        .get("isVip")
        .and_then(Value::as_bool)
        .unwrap_or_else(|| {
            song.pointer("/pay/payplay")
                .and_then(Value::as_i64)
                .unwrap_or(0)
                > 0
        });
    Some(PlayerSong {
        id,
        name,
        artist,
        album,
        cover,
        duration,
        is_vip,
    })
}

fn json_id(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    } else if let Some(n) = value.as_i64() {
        Some(n.to_string())
    } else {
        value.as_u64().map(|n| n.to_string())
    }
}

fn string_field(value: &Value, key: &str) -> String {
    value
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn named_join(list: Option<&[Value]>) -> String {
    let Some(items) = list else {
        return "Unknown".to_string();
    };
    let names: Vec<&str> = items
        .iter()
        .filter_map(|item| item.get("name").and_then(Value::as_str))
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .collect();
    if names.is_empty() {
        "Unknown".to_string()
    } else {
        names.join(", ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn cache_key_is_versioned_per_source() {
        assert_eq!(
            player_playlist_cache_key(PlayerMusicSource::Netease, "42"),
            "playlist_player:v1:netease:42"
        );
        assert_eq!(
            player_playlist_cache_key(PlayerMusicSource::Qq, "99"),
            "playlist_player:v1:qq:99"
        );
    }

    #[test]
    fn netease_projects_player_fields_and_upgrades_cover() {
        let upstream = json!({
            "playlist": {
                "tracks": [
                    {
                        "id": 111,
                        "name": "Song",
                        "ar": [{"name": "A"}, {"name": "B"}],
                        "al": {
                            "name": "Album",
                            "picUrl": "http://p1.music.126.net/x.jpg"
                        },
                        "dt": 240_000,
                        "fee": 1
                    },
                    {
                        "id": "222",
                        "name": "Free",
                        "artists": [{"name": "C"}],
                        "album": {
                            "name": "X",
                            "blurPicUrl": "https://p1.music.126.net/y.jpg"
                        },
                        "duration": 61_000,
                        "isVip": false
                    },
                    {
                        "name": "no id"
                    }
                ]
            }
        });
        let view = project_netease_player_playlist("42", &upstream);
        assert_eq!(view.code, 200);
        assert_eq!(view.source, PlayerMusicSource::Netease);
        assert_eq!(view.playlist_id, "42");
        assert_eq!(view.songs.len(), 2);
        assert_eq!(
            view.songs[0],
            PlayerSong {
                id: "111".into(),
                name: "Song".into(),
                artist: "A, B".into(),
                album: "Album".into(),
                cover: "https://p1.music.126.net/x.jpg".into(),
                duration: 240,
                is_vip: true,
            }
        );
        assert_eq!(view.songs[1].id, "222");
        assert_eq!(view.songs[1].artist, "C");
        assert_eq!(view.songs[1].duration, 61);
        assert!(!view.songs[1].is_vip);
        assert_eq!(view.songs[1].cover, "https://p1.music.126.net/y.jpg");
    }

    #[test]
    fn netease_empty_tracks_is_empty_view() {
        let view = project_netease_player_playlist("1", &json!({"playlist": {"tracks": []}}));
        assert!(view.songs.is_empty());
        let missing = project_netease_player_playlist("1", &json!({}));
        assert!(missing.songs.is_empty());
    }

    #[test]
    fn qq_projects_payplay_and_skips_blank_mid() {
        let upstream = json!({
            "cdlist": [{
                "songlist": [
                    {
                        "songmid": "001abcXY",
                        "songname": "Q",
                        "singer": [{"name": "S"}],
                        "albumname": "Al",
                        "albummid": "MID",
                        "interval": 180,
                        "pay": {"payplay": 1}
                    },
                    { "name": "no mid" }
                ]
            }]
        });
        let view = project_qq_player_playlist("99", &upstream).expect("valid cdlist");
        assert_eq!(view.source, PlayerMusicSource::Qq);
        assert_eq!(view.songs.len(), 1);
        assert_eq!(
            view.songs[0],
            PlayerSong {
                id: "001abcXY".into(),
                name: "Q".into(),
                artist: "S".into(),
                album: "Al".into(),
                cover: "https://y.gtimg.cn/music/photo_new/T002R300x300M000MID.jpg".into(),
                duration: 180,
                is_vip: true,
            }
        );
    }

    #[test]
    fn qq_missing_cdlist_is_invalid() {
        assert_eq!(
            project_qq_player_playlist("99", &json!({"code": 0})),
            Err(QqPlaylistError::Invalid)
        );
        assert_eq!(
            project_qq_player_playlist("99", &json!({"cdlist": []})),
            Err(QqPlaylistError::Invalid)
        );
    }

    #[test]
    fn qq_empty_songlist_is_ok() {
        let view =
            project_qq_player_playlist("99", &json!({"cdlist": [{"songlist": []}]})).unwrap();
        assert!(view.songs.is_empty());
    }

    #[test]
    fn player_playlist_serializes_camel_case() {
        let view = PlayerPlaylist {
            code: 200,
            source: PlayerMusicSource::Netease,
            playlist_id: "1".into(),
            songs: vec![PlayerSong {
                id: "1".into(),
                name: "n".into(),
                artist: "a".into(),
                album: "b".into(),
                cover: "c".into(),
                duration: 1,
                is_vip: true,
            }],
        };
        let value = serde_json::to_value(&view).unwrap();
        assert_eq!(value["playlistId"], "1");
        assert_eq!(value["source"], "netease");
        assert_eq!(value["songs"][0]["isVip"], true);
    }

    #[tokio::test]
    async fn player_playlist_load_lock_serializes_the_same_key() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let inside = Arc::new(AtomicUsize::new(0));
        let max_inside = Arc::new(AtomicUsize::new(0));
        let mut tasks = Vec::new();
        for _ in 0..8 {
            let inside = inside.clone();
            let max_inside = max_inside.clone();
            tasks.push(tokio::spawn(async move {
                let _guard =
                    lock_player_playlist_load(PlayerMusicSource::Netease, "25247131").await;
                let now = inside.fetch_add(1, Ordering::SeqCst) + 1;
                max_inside.fetch_max(now, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(15)).await;
                inside.fetch_sub(1, Ordering::SeqCst);
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }
        assert_eq!(max_inside.load(Ordering::SeqCst), 1);
    }
}
