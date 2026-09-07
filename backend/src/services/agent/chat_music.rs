//! Chat Lite may nudge the current player. Search and playlists stay in Work.

use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatMusicAction {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
}

impl ChatMusicAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Play => "play",
            Self::Pause => "pause",
            Self::Toggle => "toggle",
            Self::Next => "next",
            Self::Previous => "previous",
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
        "## 播放器\n{status}\n\
         要播、暂停、下一首、上一首时，在全文最后单独一行写 [[music:play]]、[[music:pause]]、\
         [[music:next]] 或 [[music:prev]]。搜歌、换歌单去办事档。不要念这一行。不操作就不要写。"
    )
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
    match inner.trim().to_ascii_lowercase().as_str() {
        "play" | "播放" | "唱" | "唱歌" => Some(ChatMusicAction::Play),
        "pause" | "暂停" | "停" => Some(ChatMusicAction::Pause),
        "toggle" => Some(ChatMusicAction::Toggle),
        "next" | "下一首" | "下一曲" => Some(ChatMusicAction::Next),
        "prev" | "previous" | "上一首" | "上一曲" => Some(ChatMusicAction::Previous),
        _ => None,
    }
}

fn player_status_line(music: Option<&Value>) -> String {
    let Some(music) = music else {
        return "现在没在放歌。".to_string();
    };
    let song = music.get("currentSong");
    if song.is_none() || song.is_some_and(Value::is_null) {
        return "现在没在放歌。".to_string();
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
        return "现在没在放歌。".to_string();
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
    let label = if playing { "正在播放" } else { "已暂停" };
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
    }

    #[test]
    fn player_section_hides_ids_and_keeps_search_in_work() {
        let section = format_chat_player_section(Some(&json!({
            "isPlaying": true,
            "currentSong": { "name": "星河", "artist": "A" }
        })));
        assert!(section.contains("正在播放：星河 — A"));
        assert!(section.contains("[[music:play]]"));
        assert!(section.contains("办事档"));
        assert!(!section.contains("playlist"));
        assert!(!section.contains("search"));
    }

    #[test]
    fn hold_cuts_an_open_live_marker() {
        assert_eq!(hold_incomplete_live_marker("行[[music:pl"), "行");
        assert_eq!(hold_incomplete_live_marker("行。"), "行。");
    }
}
