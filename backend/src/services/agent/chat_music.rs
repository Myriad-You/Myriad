//! Chat Lite may nudge the current player. Search and playlists stay in Work.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatMusicAction {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    /// Put the song she is listening to on their player, where she is in it.
    Join,
    /// Put a song she heard and liked on their player: its number in the
    /// songs offered to her this turn.
    Share(u8),
}

impl ChatMusicAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Toggle => "toggle",
            Self::Next => "next",
            Self::Previous => "previous",
            Self::Join => "join",
            Self::Share(_) => "share",
        }
    }
}

const OPEN: &str = "[[music:";
const CLOSE: &str = "]]";

pub fn peel_chat_live_reply(
    raw: &str,
) -> (
    String,
    Option<myriad_merope::WearDirective>,
    Option<ChatMusicAction>,
) {
    let (spoken, wear) = myriad_merope::split_chat_wear_directive(raw);
    let (spoken, music) = split_chat_music_directive(&spoken);
    (spoken, wear, music)
}

pub fn split_chat_music_directive(raw: &str) -> (String, Option<ChatMusicAction>) {
    let mut spoken = raw.to_string();
    let mut action = None;
    while let Some((rest, inner)) = take_music_marker(&spoken) {
        spoken = rest;
        if let Some(next) = parse_music_inner(&inner) {
            action = Some(next);
        }
    }
    (collapse(&spoken), action)
}

pub fn format_chat_player_section(music: Option<&Value>) -> String {
    let status = player_status_line(music);
    format!(
        "## Player\n{status}\n\
         To play, pause, skip next, or skip previous, put [[music:play]], [[music:pause]], \
         [[music:next]], or [[music:prev]] on its own last line. Skipping, you do not know what comes next: say nothing about it. Finding tracks and switching the queue belong in Work. Do not read that line aloud. Omit it if you are not changing playback."
    )
}

/// The songs she could play them this turn, remembered for this
/// conversation so `[[music:share N]]` finds the one she saw.
pub async fn offer_songs(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    session_id: &str,
) -> Option<String> {
    use crate::services::agent::merope::doing;
    let (songs, lines): (Vec<_>, Vec<_>) = doing::songs_to_share(db).await.into_iter().unzip();
    doing::offer_songs(user_id, session_id, songs);
    myriad_merope::speaking::format_share_section(&lines)
}

/// What the player is told to do. A song to share is the one she was
/// offered under that number; with none, there is nothing to do.
pub fn control_event(
    action: ChatMusicAction,
    user_id: i32,
    session_id: Option<&str>,
) -> Option<crate::services::agent::AgentProgressEvent> {
    let song = match action {
        ChatMusicAction::Share(number) => {
            let thing = crate::services::agent::merope::doing::offered_song(
                user_id,
                session_id.unwrap_or(""),
                number,
            )?;
            Some(serde_json::to_value(thing).ok()?)
        }
        _ => None,
    };
    Some(crate::services::agent::AgentProgressEvent::MusicControl {
        action: action.as_str().to_string(),
        song,
    })
}

pub fn hold_incomplete_live_marker(spoken: &str) -> &str {
    if let Some(at) = spoken.rfind("[[") {
        if !spoken[at..].contains("]]") {
            return &spoken[..at];
        }
    }
    myriad_merope::hold_incomplete_wear_marker(spoken)
}

fn take_music_marker(text: &str) -> Option<(String, String)> {
    let start = text.find(OPEN)?;
    let inner_at = start + OPEN.len();
    let after = text.get(inner_at..)?;
    let close_at = after.find(CLOSE)?;
    let inner = after[..close_at].trim().to_string();
    let end = inner_at + close_at + CLOSE.len();
    let mut spoken = String::with_capacity(text.len().saturating_sub(end - start));
    spoken.push_str(&text[..start]);
    spoken.push_str(&text[end..]);
    Some((spoken, inner))
}

fn parse_music_inner(inner: &str) -> Option<ChatMusicAction> {
    let inner = inner.trim().to_ascii_lowercase();
    for prefix in ["share", "分享"] {
        if let Some(number) = inner.strip_prefix(prefix) {
            let number = number.trim_start_matches([' ', ':', '：']).trim();
            return number
                .parse::<u8>()
                .ok()
                .filter(|number| *number > 0)
                .map(ChatMusicAction::Share);
        }
    }
    match inner.as_str() {
        "play" | "播放" | "唱" | "唱歌" => Some(ChatMusicAction::Play),
        "pause" | "暂停" | "停" => Some(ChatMusicAction::Pause),
        "toggle" => Some(ChatMusicAction::Toggle),
        "next" | "下一首" | "下一曲" => Some(ChatMusicAction::Next),
        "prev" | "previous" | "上一首" | "上一曲" => Some(ChatMusicAction::Previous),
        "join" | "一起听" => Some(ChatMusicAction::Join),
        _ => None,
    }
}

fn player_status_line(music: Option<&Value>) -> String {
    let Some(music) = music else {
        return "Nothing is playing.".to_string();
    };
    let song = music.get("currentSong");
    if song.is_none() || song.is_some_and(Value::is_null) {
        return "Nothing is playing.".to_string();
    }
    let song = song.unwrap();
    let name: String = song
        .get("name")
        .or_else(|| song.get("title"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(80)
        .collect();
    if name.trim().is_empty() {
        return "Nothing is playing.".to_string();
    }
    let artist: String = song
        .get("artist")
        .and_then(Value::as_str)
        .unwrap_or("")
        .chars()
        .take(80)
        .collect();
    let playing = music
        .get("isPlaying")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let label = if playing { "Playing" } else { "Paused" };
    if artist.trim().is_empty() {
        format!("{label}：{}", name.trim())
    } else {
        format!("{label}：{} — {}", name.trim(), artist.trim())
    }
}

fn collapse(raw: &str) -> String {
    let mut lines = Vec::new();
    let mut blank = false;
    for line in raw.lines() {
        if line.trim().is_empty() {
            if !lines.is_empty() {
                blank = true;
            }
            continue;
        }
        if blank {
            lines.push(String::new());
            blank = false;
        }
        lines.push(line.trim_end().to_string());
    }
    lines.join("\n").trim().to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn music_marker_is_stripped_and_parsed() {
        let (spoken, action) = split_chat_music_directive("行，我唱给你听。\n[[music:play]]");
        assert_eq!(spoken, "行，我唱给你听。");
        assert_eq!(action, Some(ChatMusicAction::Play));
        let (spoken, action) = split_chat_music_directive("下一首\n[[music:next]]");
        assert_eq!(spoken, "下一首");
        assert_eq!(action, Some(ChatMusicAction::Next));
        assert_eq!(split_chat_music_directive("你好").1, None);
        let (spoken, action) = split_chat_music_directive("来，一起听。\n[[music:join]]");
        assert_eq!(spoken, "来，一起听。");
        assert_eq!(action, Some(ChatMusicAction::Join));
        let (spoken, action) = split_chat_music_directive("放给你听。\n[[music:share 2]]");
        assert_eq!(spoken, "放给你听。");
        assert_eq!(action, Some(ChatMusicAction::Share(2)));
        assert_eq!(
            split_chat_music_directive("[[music:share:3]]").1,
            Some(ChatMusicAction::Share(3))
        );
        assert_eq!(split_chat_music_directive("[[music:share 0]]").1, None);
        assert_eq!(split_chat_music_directive("[[music:share x]]").1, None);
    }

    #[test]
    fn player_section_hides_ids_and_keeps_search_in_work() {
        let section = format_chat_player_section(Some(&json!({
            "isPlaying": true,
            "currentSong": { "name": "星河", "artist": "A" }
        })));
        assert!(section.contains("Playing：星河 — A"));
        assert!(section.contains("[[music:play]]"));
        assert!(section.contains("Work"));
        assert!(!section.contains("playlist"));
        assert!(!section.contains("search"));
    }

    #[test]
    fn she_shares_only_a_song_she_was_offered_in_this_conversation() {
        use crate::services::agent::merope::doing::{Thing, offer_songs};
        let user = -94_201;
        assert!(control_event(ChatMusicAction::Share(1), user, Some("s")).is_none());
        offer_songs(
            user,
            "s",
            vec![Thing::Song {
                id: "186016".into(),
                source: "netease".into(),
                name: "晴天".into(),
                artist: "周杰伦".into(),
                album: String::new(),
                cover: String::new(),
                duration_ms: 269_000,
            }],
        );
        let Some(crate::services::agent::AgentProgressEvent::MusicControl { action, song }) =
            control_event(ChatMusicAction::Share(1), user, Some("s"))
        else {
            panic!("the offered song is shared");
        };
        assert_eq!(action, "share");
        let song = song.unwrap();
        assert_eq!(
            (song["kind"].as_str(), song["id"].as_str()),
            (Some("song"), Some("186016"))
        );
        assert_eq!(song["durationMs"], 269_000);
        assert!(control_event(ChatMusicAction::Share(2), user, Some("s")).is_none());
        assert!(control_event(ChatMusicAction::Share(1), user, Some("other")).is_none());
        assert!(matches!(
            control_event(ChatMusicAction::Next, user, None),
            Some(crate::services::agent::AgentProgressEvent::MusicControl { song: None, .. })
        ));
    }

    #[test]
    fn skipping_she_does_not_describe_the_next_song() {
        assert!(format_chat_player_section(None).contains("you do not know what comes next"));
    }

    #[test]
    fn hold_cuts_an_open_live_marker() {
        assert_eq!(hold_incomplete_live_marker("行[[music:pl"), "行");
        assert_eq!(hold_incomplete_live_marker("行。"), "行。");
    }
}
