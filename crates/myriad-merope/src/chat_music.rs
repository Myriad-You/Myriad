//! The player line at the end of a chat reply, the rules of it: Chat Lite
//! may nudge the current player with `[[music:…]]`. Search and playlists
//! stay in Work.

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
}
