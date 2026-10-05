//! A song from the site's playlist, heard from its recording.
//!
//! The recording is heard through `hearing`: what happens in its sound and
//! its lyrics on the timeline. She feels from that, and afterwards says of
//! the song only what she heard in it. Not told what such moments tend to do
//! to listeners, nor pushed to tell how it sounded each time: with both, every
//! note came back as the same "it lit up, it burst open". Without the recording she has only its words,
//! and knows it; without either, she heard nothing of it.

use sea_orm::DatabaseConnection;
use serde_json::Value;

use super::{Carry, Intake, Thing};
use crate::services::music_player_view::{PlayerMusicSource, PlayerPlaylistError, PlayerSong};

/// Songs offered at a time.
pub const OFFERED: usize = 6;
/// A heard song carries its timeline and lyrics.
const HEARD_CHARS: usize = 12_000;
const WORDS_CHARS: usize = 2_500;

const HEARD: &str = "You heard it: the material is what happens in its sound, measured from the recording, from start to end, with its lyrics where they are sung. \
That is how the song went for you. \
The measures are of the whole sound: they cannot tell a voice from the instruments, so say nothing of how it is sung or played, or by which. \
Say it as a listener would, by the moment, the line or the feeling, not as a readout of the measures. ";
/// Said with a heard song that has no words. Its name is then the only
/// words about it, and what the name calls up came back as what she heard
/// (a piece called "Ocean of Memories" heard as the sea), whatever she was
/// told about names; so she writes of it as a piece without words by its
/// artist, and its name is kept with her note.
const WORDLESS: &str = "It has no words: what you have of it is how it went. Its name is kept with your note; you need not name it. ";

/// A song without words, as she has it while she writes of it.
fn wordless(by: Option<&str>) -> String {
    match by {
        Some(by) => format!("a piece without words by {by}"),
        None => "a piece without words".to_string(),
    }
}
const WORDS_ONLY: &str = "The recording would not load, so you only had its words; you did not hear how it sounds, and do not pretend to. ";
const NOTHING: &str = "The recording would not load and it had no words to read: you neither heard nor read any of it, and do not pretend to. ";

/// The songs on the site's playlist that she can hear (not paywalled).
pub async fn options(db: &DatabaseConnection) -> Vec<Thing> {
    let (enabled, source, playlist) = {
        let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        (
            config.music_enabled.clone(),
            config.music_source.clone(),
            config.music_playlist_id.clone(),
        )
    };
    let setting = |value: Option<String>, name: &str| {
        value
            .or_else(|| std::env::var(name).ok())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    if setting(enabled, "MUSIC_ENABLED").as_deref() == Some("false") {
        return Vec::new();
    }
    let Some(playlist) = setting(playlist, "MUSIC_PLAYLIST_ID") else {
        return Vec::new();
    };
    let source = match setting(source, "MUSIC_SOURCE").as_deref() {
        Some("qq") => PlayerMusicSource::Qq,
        Some("local") => PlayerMusicSource::Local,
        _ => PlayerMusicSource::Netease,
    };
    let loaded = match source {
        // Only what the site's own player already loaded.
        PlayerMusicSource::Qq => {
            crate::services::music_player_view::get_cached_player_playlist(source, &playlist).await
        }
        PlayerMusicSource::Local => {
            let db = db.clone();
            let pid = playlist.clone();
            crate::services::music_player_view::load_player_playlist(
                source,
                &playlist,
                || async move {
                    crate::services::local_music::build_player_playlist(&db, &pid)
                        .await
                        .map_err(|_| PlayerPlaylistError::FetchFailed)
                },
            )
            .await
            .ok()
        }
        PlayerMusicSource::Netease => {
            let Ok(id) = playlist.parse::<i64>() else {
                return Vec::new();
            };
            crate::services::music_player_view::load_player_playlist(
                source,
                &playlist,
                || async move {
                    crate::services::netease_service::NeteaseService::new()
                        .fetch_player_playlist(id)
                        .await
                        .map_err(|_| PlayerPlaylistError::FetchFailed)
                },
            )
            .await
            .ok()
        }
    };
    loaded
        .map(|playlist| {
            playlist
                .songs
                .iter()
                .filter(|song| !song.is_vip && song.duration > 0)
                .map(|song| song_thing(song, source))
                .collect()
        })
        .unwrap_or_default()
}

fn song_thing(song: &PlayerSong, source: PlayerMusicSource) -> Thing {
    Thing::Song {
        id: song.id.clone(),
        source: source.as_str().to_string(),
        name: song.name.clone(),
        artist: song.artist.clone(),
        album: song.album.clone(),
        cover: song.cover.clone(),
        // The player view keeps seconds.
        duration_ms: song.duration.saturating_mul(1000),
    }
}

/// A song's words, without timestamps.
async fn lyrics(db: &DatabaseConnection, thing: &Thing) -> Option<String> {
    let Thing::Song { id, source, .. } = thing else {
        return None;
    };
    let lrc = crate::services::agent::merope::hearing::timed_lyrics(db, source, id).await?;
    Some(plain_lyrics(&lrc))
}

fn plain_lyrics(lrc: &str) -> String {
    lrc.lines()
        .map(|line| {
            let mut rest = line.trim();
            while rest.starts_with('[') {
                match rest.find(']') {
                    Some(end) => rest = rest[end + 1..].trim_start(),
                    None => break,
                }
            }
            rest.trim()
        })
        .filter(|line| !line.is_empty() && !line.contains(" : ") && !line.contains('：'))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Shown with a song she hears: the songs that moved her lately, measured
/// the same way, so she can hold this one up against them if she cares to.
const MOVED_LATELY: &str = "Songs that moved you lately, measured the same way";
/// How many, and how far back.
const MOVED_SHOWN: usize = 3;
const MOVED_DAYS: i64 = 60;

/// The songs that moved her lately, as they sounded (what she kept of
/// hearing each), the latest few, each once, not `this` one.
async fn moved_by_lately(db: &DatabaseConnection, this: &str) -> Vec<String> {
    use crate::services::agent::memory::unified;
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let since = chrono::Utc::now() - chrono::Duration::days(MOVED_DAYS);
    // Faded is still hers; a row taken away (not faded) is not.
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT e->'thing' AS thing, e->>'heard' AS heard \
             FROM (SELECT created_at, evidence::jsonb AS e FROM agent_memories \
               WHERE user_id IS NULL AND venue = $1 AND source = $2 AND created_at >= $3 \
                 AND evidence IS JSON AND (invalid_at IS NULL OR invalid_reason = 'faded')) own \
             WHERE e->'thing'->>'kind' = 'song' AND e->>'reaction' = 'moved' \
               AND e->>'heard' IS NOT NULL \
             ORDER BY created_at DESC LIMIT 20",
            [
                unified::OWN_VENUE.into(),
                unified::OWN_EXPERIENCE.into(),
                since.fixed_offset().into(),
            ],
        ))
        .await
        .unwrap_or_default();
    let mut seen = std::collections::HashSet::from([this.to_string()]);
    rows.iter()
        .filter_map(|row| {
            let thing: Thing = serde_json::from_value(row.try_get("", "thing").ok()?).ok()?;
            let heard: String = row.try_get("", "heard").ok()?;
            let Thing::Song { name, artist, .. } = &thing else {
                return None;
            };
            seen.insert(thing.key())
                .then(|| format!("「{name}」 by {artist}: {heard}"))
        })
        .take(MOVED_SHOWN)
        .collect()
}

/// The song as it reached her: heard from its recording, or only its
/// words, or nothing.
pub async fn intake(db: &DatabaseConnection, thing: &Thing) -> Intake {
    let hearing = crate::services::agent::merope::hearing::sheet_for(db, &thing.key(), thing).await;
    if let Some(sheet) = hearing {
        let words = !sheet.lyrics.is_empty();
        let how = if words {
            HEARD.to_string()
        } else {
            format!("{HEARD}{WORDLESS}")
        };
        let mut intake = Intake::plain(Some(sheet.describe()), HEARD_CHARS, how);
        if !words {
            intake.called = Some(wordless(thing.by()));
        }
        intake.carry = Carry::Heard(sheet);
        let moved = moved_by_lately(db, &thing.key()).await;
        if !moved.is_empty() {
            intake
                .alongside
                .push((MOVED_LATELY.to_string(), moved.join("\n")));
        }
        return intake;
    }
    match lyrics(db, thing)
        .await
        .filter(|words| !words.trim().is_empty())
    {
        Some(words) => Intake::plain(Some(words), WORDS_CHARS, WORDS_ONLY),
        None => Intake {
            reached: false,
            ..Intake::plain(None, WORDS_CHARS, NOTHING)
        },
    }
}

/// Where she is in the song right now, from what she has heard of it.
pub fn so_far(thing: &Thing, seconds_in: f32) -> String {
    match crate::services::agent::merope::hearing::heard(&thing.key()) {
        Some(sheet) => format!(" {}", sheet.so_far(seconds_in.max(0.0))),
        // Still being heard, or the recording would not load.
        None => " How it sounds has not reached you.".to_string(),
    }
}

/// Whether their player is on the song she is listening to right now.
pub fn listening_along(thing: &Thing, music: Option<&Value>) -> bool {
    let Thing::Song { name, .. } = thing else {
        return false;
    };
    let Some(music) = music else {
        return false;
    };
    let playing = music.get("isPlaying").and_then(Value::as_bool) == Some(true);
    let theirs = music
        .pointer("/currentSong/name")
        .or_else(|| music.pointer("/currentSong/title"))
        .and_then(Value::as_str)
        .map(str::trim);
    playing && theirs == Some(name.trim())
}

/// For the player section of a private chat: whether they are already
/// listening with her, or how she can put her song on for them.
pub fn player_line(thing: &Thing, music: Option<&Value>) -> Option<&'static str> {
    if !matches!(thing, Thing::Song { .. }) {
        return None;
    }
    Some(if listening_along(thing, music) {
        "Their player is on the song you are listening to: you are listening to it together right now."
    } else {
        "If they want to listen with you, put [[music:join]] on its own last line: it puts the song you are listening to on their player, where you are in it. Only when they want it."
    })
}

#[cfg(test)]
pub(crate) fn probe_intake(what: &str, material: Option<&str>) -> Intake {
    match material {
        Some(material) if material.contains("How it goes:") && !material.contains('「') => {
            let mut intake = Intake::plain(None, HEARD_CHARS, format!("{HEARD}{WORDLESS}"));
            intake.called = Some(wordless(what.split_once("」 by ").map(|(_, by)| by)));
            intake
        }
        Some(material) if material.contains("How it goes:") => {
            Intake::plain(None, HEARD_CHARS, HEARD)
        }
        Some(_) => Intake::plain(None, WORDS_CHARS, WORDS_ONLY),
        None => Intake {
            reached: false,
            ..Intake::plain(None, WORDS_CHARS, NOTHING)
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn song(name: &str) -> Thing {
        Thing::Song {
            id: "1".into(),
            source: "netease".into(),
            name: name.into(),
            artist: "周杰伦".into(),
            album: String::new(),
            cover: String::new(),
            duration_ms: 269_000,
        }
    }

    #[test]
    fn lyrics_lose_their_timestamps_and_credits() {
        let lrc = "[00:00.00] 作词 : 周杰伦\n[00:01.00] 作曲 : 周杰伦\n[00:25.10]故事的小黄花\n[00:28.00][01:10.00]从出生那年就飘着\n[00:30.00]";
        assert_eq!(plain_lyrics(lrc), "故事的小黄花\n从出生那年就飘着");
    }

    #[test]
    fn how_a_song_reached_her_is_said_plainly() {
        assert!(HEARD.contains("not as a readout"));
        assert!(WORDS_ONLY.contains("you only had its words"));
        assert!(NOTHING.contains("neither heard nor read"));
        let what = "listening to the song 「Ocean of Memories」 by 深澤秀行";
        assert!(!probe_intake(what, None).reached);
        let sung = probe_intake(what, Some("How it goes:\n  0:06 「凌晨四点」"));
        assert!(sung.how.starts_with("You heard it") && sung.called.is_none());
        // Without words, she has it as a piece by its artist, not by a name
        // whose images would come back as what she heard.
        let wordless = probe_intake(what, Some("How it goes:\n  1:20 swells +11 dB"));
        assert!(wordless.how.contains("It has no words"));
        assert_eq!(
            wordless.called.as_deref(),
            Some("a piece without words by 深澤秀行")
        );
        assert!(!HEARD.contains("research") && !HEARD.contains("bright"));
    }

    #[test]
    fn they_can_listen_along_when_they_want_to() {
        let thing = song("晴天");
        let along = json!({"isPlaying": true, "currentSong": {"name": "晴天", "artist": "周杰伦"}});
        assert!(listening_along(&thing, Some(&along)));
        assert!(
            player_line(&thing, Some(&along))
                .unwrap()
                .contains("together right now")
        );
        let paused = json!({"isPlaying": false, "currentSong": {"name": "晴天"}});
        assert!(!listening_along(&thing, Some(&paused)));
        assert!(
            player_line(&thing, None)
                .unwrap()
                .contains("[[music:join]]")
        );
        let note = Thing::Note {
            item_id: 1,
            title: "t".into(),
        };
        assert!(player_line(&note, None).is_none());
    }
}
