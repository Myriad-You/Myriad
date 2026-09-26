//! Player playlist view: vendor JSON → the fields the music player actually uses.
//!
//! `NeteaseService::fetch_playlist` stays fat — platform liked-songs still needs
//! full tracks. This module is the player-proxy boundary only.

use myriad_platform_utils::netease::ensure_https_url;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Arc, Weak};
use std::time::{Duration, Instant};
use tokio::sync::{OnceCell, RwLock};

use super::netease_service::MUSIC_CACHE;

type PlaylistResult = Result<Arc<PlayerPlaylist>, PlayerPlaylistError>;
type PlaylistLoad = OnceCell<PlaylistResult>;

static PLAYER_PLAYLIST_LOADS: Lazy<RwLock<HashMap<String, Weak<PlaylistLoad>>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlayerPlaylistError {
    RateLimited,
    FetchFailed,
}

pub const PLAYER_PLAYLIST_CACHE_TTL: Duration = Duration::from_secs(604_800);
pub const PLAYER_PLAYLIST_CACHE_VERSION: &str = "v1";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlayerMusicSource {
    Netease,
    Qq,
    Local,
}

impl PlayerMusicSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Netease => "netease",
            Self::Qq => "qq",
            Self::Local => "local",
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
) -> Option<Arc<PlayerPlaylist>> {
    let key = player_playlist_cache_key(source, playlist_id);
    MUSIC_CACHE.write().await.get_player(&key)
}

/// Concurrent callers share the result, including errors and snapshots too big to cache.
/// OnceCell lets a waiter resume loading if the initiating request is cancelled.
pub async fn load_player_playlist<F, Fut>(
    source: PlayerMusicSource,
    playlist_id: &str,
    fetch: F,
) -> PlaylistResult
where
    F: FnOnce() -> Fut,
    Fut: std::future::Future<Output = Result<PlayerPlaylist, PlayerPlaylistError>>,
{
    // Local catalog mutates on upload/delete/edit — never serve a stale cache.
    if source == PlayerMusicSource::Local {
        return Ok(Arc::new(fetch().await?));
    }
    if let Some(playlist) = get_cached_player_playlist(source, playlist_id).await {
        return Ok(playlist);
    }
    let key = player_playlist_cache_key(source, playlist_id);
    let load = {
        let mut loads = PLAYER_PLAYLIST_LOADS.write().await;
        loads.retain(|_, load| load.strong_count() > 0);
        if let Some(load) = loads.get(&key).and_then(Weak::upgrade) {
            load
        } else {
            let load = Arc::new(OnceCell::new());
            loads.insert(key.clone(), Arc::downgrade(&load));
            load
        }
    };
    let result = load
        .get_or_init(|| async {
            if let Some(playlist) = get_cached_player_playlist(source, playlist_id).await {
                return Ok(playlist);
            }
            let playlist = Arc::new(fetch().await?);
            MUSIC_CACHE.write().await.insert_player(
                key.clone(),
                Arc::clone(&playlist),
                Instant::now() + PLAYER_PLAYLIST_CACHE_TTL,
            );
            Ok(playlist)
        })
        .await
        .clone();
    let mut loads = PLAYER_PLAYLIST_LOADS.write().await;
    if loads
        .get(&key)
        .is_some_and(|current| current.ptr_eq(&Arc::downgrade(&load)))
    {
        loads.remove(&key);
    }
    result
}

#[cfg(test)]
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
    // Both QQ endpoints use zero for success. Reject upstream errors before
    // looking at the payload so an error cannot become a cached empty playlist.
    for field in ["code", "subcode"] {
        if let Some(code) = upstream.get(field)
            && code.as_i64() != Some(0)
        {
            return Err(QqPlaylistError::Invalid);
        }
    }
    // v8 nests cdlist under data; retain the legacy response shape as well.
    let cdlist = upstream
        .pointer("/data/cdlist")
        .or_else(|| upstream.get("cdlist"))
        .and_then(Value::as_array)
        .filter(|list| !list.is_empty())
        .ok_or(QqPlaylistError::Invalid)?;
    let songlist = cdlist[0]
        .get("songlist")
        .and_then(Value::as_array)
        .ok_or(QqPlaylistError::Invalid)?;
    Ok(PlayerPlaylist {
        code: 200,
        source: PlayerMusicSource::Qq,
        playlist_id: playlist_id.to_string(),
        songs: songlist.iter().filter_map(project_qq_track).collect(),
    })
}

#[cfg(test)]
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
    // Playback/lyrics resolve a song MID, not the numeric song id or media_mid.
    // An empty upstream url is expected: playback URLs are resolved on demand.
    let id = json_id(song.get("mid").or_else(|| song.get("songmid"))?)?;
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
        .or_else(|| song.pointer("/album/mid"))
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
            song.pointer("/pay/pay_play")
                .or_else(|| song.pointer("/pay/payplay"))
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

#[cfg(test)]
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
            player_playlist_cache_key(PlayerMusicSource::Local, "local"),
            "playlist_player:v1:local:local"
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
    fn qq_v8_projects_mid_nested_album_and_empty_url() {
        let upstream = json!({
            "code": 0,
            "subcode": 0,
            "data": {"cdlist": [{"songlist": [
                {
                    "id": 123,
                    "mid": "000M3Yxt2tIuHZ",
                    "name": "Free song",
                    "singer": [{"name": "A"}, {"name": "B"}],
                    "album": {"name": "Album", "mid": "0011AmNK31mn3Z"},
                    "file": {"media_mid": "differentMediaMid"},
                    "interval": 123,
                    "pay": {"pay_play": 0, "pay_month": 1, "pay_down": 1},
                    "url": ""
                },
                {
                    "id": 456,
                    "mid": "004aJoYT3ia7al",
                    "name": "Paid song",
                    "pay": {"pay_play": 1},
                    "url": ""
                }
            ]}]}
        });
        let view = project_qq_player_playlist("99", &upstream).expect("valid v8 playlist");
        assert_eq!(view.code, 200);
        assert_eq!(view.source, PlayerMusicSource::Qq);
        assert_eq!(view.playlist_id, "99");
        assert_eq!(view.songs.len(), 2);
        assert_eq!(
            view.songs[0],
            PlayerSong {
                id: "000M3Yxt2tIuHZ".into(),
                name: "Free song".into(),
                artist: "A, B".into(),
                album: "Album".into(),
                cover: "https://y.gtimg.cn/music/photo_new/T002R300x300M0000011AmNK31mn3Z.jpg".into(),
                duration: 123,
                is_vip: false,
            }
        );
        assert_eq!(view.songs[1].id, "004aJoYT3ia7al");
        assert!(view.songs[1].is_vip);
    }

    #[test]
    fn qq_v8_empty_songlist_is_ok() {
        let upstream = json!({
            "code": 0,
            "subcode": 0,
            "data": {"cdlist": [{"songlist": []}]}
        });
        let view = project_qq_player_playlist("99", &upstream).unwrap();
        assert!(view.songs.is_empty());
    }

    #[test]
    fn qq_upstream_error_codes_are_not_cached_as_success() {
        for (code, subcode) in [(1, 0), (0, 1), (-1, 0)] {
            let upstream = json!({
                "code": code,
                "subcode": subcode,
                "data": {"cdlist": [{"songlist": []}]},
                "cdlist": [{"songlist": []}]
            });
            assert_eq!(
                project_qq_player_playlist("99", &upstream),
                Err(QqPlaylistError::Invalid)
            );
        }
    }

    #[test]
    fn qq_missing_or_malformed_songlist_is_invalid() {
        for playlist in [json!({}), json!({"songlist": null}), json!({"songlist": {}})] {
            for upstream in [
                json!({"code": 0, "data": {"cdlist": [playlist.clone()]}}),
                json!({"code": 0, "cdlist": [playlist.clone()]}),
            ] {
                assert_eq!(
                    project_qq_player_playlist("99", &upstream),
                    Err(QqPlaylistError::Invalid)
                );
            }
        }
    }

    #[test]
    fn qq_numeric_id_is_not_a_playable_mid() {
        let upstream = json!({"cdlist": [{"songlist": [
            {"id": 123},
            {"id": 456, "mid": ""},
            {"id": 789, "songmid": "   "},
            {"id": 101, "mid": " 000M3Yxt2tIuHZ "}
        ]}]});
        let view = project_qq_player_playlist("99", &upstream).unwrap();
        assert_eq!(view.songs.len(), 1);
        assert_eq!(view.songs[0].id, "000M3Yxt2tIuHZ");
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
    fn empty_playlist(id: &str) -> PlayerPlaylist {
        PlayerPlaylist {
            code: 200,
            source: PlayerMusicSource::Netease,
            playlist_id: id.into(),
            songs: vec![],
        }
    }

    async fn wait_for_loaders(id: &str, count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let joined = PLAYER_PLAYLIST_LOADS
                    .read()
                    .await
                    .get(&player_playlist_cache_key(PlayerMusicSource::Netease, id))
                    .is_some_and(|load| load.strong_count() >= count);
                if joined {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("callers joined the same load");
    }

    #[tokio::test]
    async fn concurrent_misses_share_success_and_failure_then_allow_retry() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        for (count, success_id, failure_id) in [
            (1, "coalesced-success-1", "coalesced-failure-1"),
            (10, "coalesced-success-10", "coalesced-failure-10"),
            (100, "coalesced-success-100", "coalesced-failure-100"),
        ] {
            for fail in [false, true] {
                let id = if fail { failure_id } else { success_id };
                let calls = Arc::new(AtomicUsize::new(0));
                let release = Arc::new(tokio::sync::Notify::new());
                let mut requests = Vec::new();
                for _ in 0..count {
                    let calls = calls.clone();
                    let release = release.clone();
                    requests.push(tokio::spawn(async move {
                        load_player_playlist(PlayerMusicSource::Netease, id, || async {
                            calls.fetch_add(1, Ordering::SeqCst);
                            release.notified().await;
                            if fail {
                                Err(PlayerPlaylistError::FetchFailed)
                            } else {
                                Ok(empty_playlist(id))
                            }
                        })
                        .await
                    }));
                }
                wait_for_loaders(id, count).await;
                assert_eq!(calls.load(Ordering::SeqCst), 1);
                release.notify_waiters();
                let results = tokio::time::timeout(
                    Duration::from_secs(5),
                    futures::future::join_all(requests),
                )
                .await
                .unwrap()
                .into_iter()
                .map(Result::unwrap)
                .collect::<Vec<_>>();
                assert_eq!(calls.load(Ordering::SeqCst), 1);
                if fail {
                    assert!(
                        results
                            .iter()
                            .all(|result| *result == Err(PlayerPlaylistError::FetchFailed))
                    );
                } else {
                    let first = results[0].as_ref().unwrap();
                    assert!(
                        results
                            .iter()
                            .all(|result| Arc::ptr_eq(first, result.as_ref().unwrap()))
                    );
                }
                let next = load_player_playlist(PlayerMusicSource::Netease, id, || async {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(empty_playlist(id))
                })
                .await
                .unwrap();
                assert_eq!(calls.load(Ordering::SeqCst), if fail { 2 } else { 1 });
                if !fail {
                    assert!(Arc::ptr_eq(&next, results[0].as_ref().unwrap()));
                }
                assert!(
                    !PLAYER_PLAYLIST_LOADS
                        .read()
                        .await
                        .contains_key(&player_playlist_cache_key(PlayerMusicSource::Netease, id))
                );
            }
        }
    }

    #[tokio::test]
    async fn cancelled_loader_does_not_strand_waiters_or_other_keys() {
        let (started, ready) = tokio::sync::oneshot::channel();
        let leader = tokio::spawn(async {
            load_player_playlist(PlayerMusicSource::Netease, "cancelled-load", || async {
                let _ = started.send(());
                std::future::pending::<Result<PlayerPlaylist, PlayerPlaylistError>>().await
            })
            .await
        });
        ready.await.unwrap();
        let waiter = tokio::spawn(async {
            load_player_playlist(PlayerMusicSource::Netease, "cancelled-load", || async {
                Ok(empty_playlist("cancelled-load"))
            })
            .await
        });
        wait_for_loaders("cancelled-load", 2).await;
        load_player_playlist(PlayerMusicSource::Netease, "independent-load", || async {
            Ok(empty_playlist("independent-load"))
        })
        .await
        .unwrap();
        leader.abort();
        assert!(leader.await.unwrap_err().is_cancelled());
        tokio::time::timeout(Duration::from_secs(5), waiter)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
    }
}
