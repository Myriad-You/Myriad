//! The conversation as she has it in front of her in a chat: what was said,
//! one message per line with who said it, without the directives and Work
//! artifacts riding on a stored line.

use chrono::{DateTime, Utc};

const WORK_ARTIFACT_PREFIXES: &[&str] = &[
    "[输出数据:",
    "[展示类型:",
    "[前端动作:",
    "[确认",
    "[output data:",
    "[display type:",
    "[frontend action:",
    "[confirm:",
];

/// Drop Work task metadata / confirmation / frontend-action injections that
/// may already be sitting on an assistant line, and the wear, music and
/// sticker directives at the end of hers.
pub fn chat_safe_content(content: &str) -> String {
    let spoken = crate::split_chat_wear_directive(content).0;
    let spoken = crate::chat_music::split_chat_music_directive(&spoken).0;
    let spoken = crate::stickers::split_sticker_directive(&spoken).0;
    spoken
        .lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !WORK_ARTIFACT_PREFIXES
                .iter()
                .any(|prefix| trimmed.starts_with(prefix))
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

/// The conversation as far back as she would still have it in front of
/// her: the latest messages, up to this many or this much text.
pub const HISTORY_MESSAGES: usize = 40;
pub const HISTORY_CHARS: usize = 5000;

/// A pause in a conversation this long is marked, so what was said days
/// ago does not read as if just said.
pub const MARKED_PAUSE_SECS: i64 = 3 * 3600;

/// How long a pause was, plainly: "13 hours", "3 days".
pub fn pause_text(seconds: i64) -> String {
    let hours = seconds / 3600;
    match hours {
        ..=23 => format!("{} hours", hours.max(1)),
        _ => match hours / 24 {
            1 => "1 day".to_string(),
            days => format!("{days} days"),
        },
    }
}

/// One message of the conversation, oldest first in a history.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HistoryMessage<'a> {
    /// Who said it, as the line reads: `user`, `assistant`.
    pub role: &'a str,
    /// What was stored, directives and all; `chat_safe_content` is applied.
    pub content: &'a str,
    /// When it was sent, if known.
    pub sent_at: Option<DateTime<Utc>>,
}

/// One line of the conversation as she has it: which message of the
/// history it is, what was said in it, as one line, and the pause marked
/// before it, if one was.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryLine {
    pub index: usize,
    pub said: String,
    pub pause: Option<String>,
}

/// The messages she still has in front of her, oldest first: the latest
/// `HISTORY_MESSAGES`, or as many as fit in `HISTORY_CHARS`, a message sent
/// as several lines read as one, empty ones left out, and a pause of
/// `MARKED_PAUSE_SECS` or more marked before the message after it.
pub fn chat_history_lines(history: &[HistoryMessage]) -> Vec<HistoryLine> {
    let mut kept: Vec<(usize, String, Option<DateTime<Utc>>)> = Vec::new();
    let mut chars = 0;
    for (index, message) in history.iter().enumerate().rev().take(HISTORY_MESSAGES) {
        let said = chat_safe_content(message.content)
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .collect::<Vec<_>>()
            .join(" / ");
        if said.is_empty() {
            continue;
        }
        chars += format!("{}：{said}", message.role).chars().count();
        if chars > HISTORY_CHARS && !kept.is_empty() {
            break;
        }
        kept.push((index, said, message.sent_at));
    }
    kept.reverse();
    let mut lines = Vec::with_capacity(kept.len());
    let mut previous: Option<DateTime<Utc>> = None;
    for (index, said, at) in kept {
        let pause = match (previous, at) {
            (Some(before), Some(at)) if (at - before).num_seconds() >= MARKED_PAUSE_SECS => Some(
                format!("（{} later）", pause_text((at - before).num_seconds())),
            ),
            _ => None,
        };
        if at.is_some() {
            previous = at;
        }
        lines.push(HistoryLine { index, said, pause });
    }
    lines
}

/// One message per line, each with who said it: a message sent as several
/// lines reads as one. Long pauses between them are marked; how long since
/// the last of them is told apart (see `format_since_section`), so the whole
/// stands against now.
pub fn chat_history_text(history: &[HistoryMessage]) -> String {
    let mut lines = Vec::new();
    for line in chat_history_lines(history) {
        lines.extend(line.pause);
        lines.push(format!("{}：{}", history[line.index].role, line.said));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn message<'a>(
        role: &'a str,
        content: &'a str,
        sent_at: Option<DateTime<Utc>>,
    ) -> HistoryMessage<'a> {
        HistoryMessage {
            role,
            content,
            sent_at,
        }
    }

    #[test]
    fn long_pauses_in_a_conversation_are_marked() {
        let now: DateTime<Utc> = "2026-10-01T09:00:00Z".parse().unwrap();
        let at = |hours_ago: i64| Some(now - chrono::Duration::hours(hours_ago));
        let history = [
            message("user", "你好呀", at(62)),
            message("assistant", "来啦", at(62)),
            message("user", "我去上课了", at(13)),
            message("assistant", "难怪见不着人", at(13)),
            message("user", "早", at(0)),
        ];
        assert_eq!(
            chat_history_text(&history),
            "user：你好呀\nassistant：来啦\n（2 days later）\nuser：我去上课了\nassistant：难怪见不着人\n（13 hours later）\nuser：早"
        );
        // A message without a time is measured from the last one with one.
        let gaps = [
            message("user", "在吗", at(30)),
            message("assistant", "在", None),
            message("user", "哦", at(1)),
        ];
        assert_eq!(
            chat_history_text(&gaps),
            "user：在吗\nassistant：在\n（1 day later）\nuser：哦"
        );
        // The same, as lines that say which message each is.
        let lines = chat_history_lines(&gaps);
        assert_eq!(
            lines.iter().map(|line| line.index).collect::<Vec<_>>(),
            [0, 1, 2]
        );
        assert_eq!(lines[2].pause.as_deref(), Some("（1 day later）"));
        assert_eq!(lines[1].said, "在");
        // Without times, nothing is made up.
        let plain = [
            message("user", "在吗", None),
            message("assistant", "在", None),
        ];
        assert_eq!(chat_history_text(&plain), "user：在吗\nassistant：在");
        assert_eq!(pause_text(3 * 3600), "3 hours");
        assert_eq!(pause_text(60), "1 hours");
        assert_eq!(pause_text(72 * 3600), "3 days");
    }

    #[test]
    fn the_conversation_reaches_back_by_length_and_each_line_says_who() {
        let lines: Vec<String> = (0..30).map(|index| format!("第{index}句")).collect();
        let mut history: Vec<HistoryMessage> = lines
            .iter()
            .map(|line| message("user", line, None))
            .collect();
        history.push(message("assistant", "哈？\n大早上的\n\n算你快", None));
        let text = chat_history_text(&history);
        assert!(text.contains("user：第5句"));
        assert!(text.ends_with("assistant：哈？ / 大早上的 / 算你快"));
        let long: Vec<String> = (0..40)
            .map(|index| format!("{index}{}", "长".repeat(400)))
            .collect();
        let long: Vec<HistoryMessage> = long
            .iter()
            .map(|line| message("user", line, None))
            .collect();
        let text = chat_history_text(&long);
        assert!(text.chars().count() <= HISTORY_CHARS + 500);
        assert!(text.contains("user：39"));
        // Only the latest messages, however short.
        let many: Vec<String> = (0..50).map(|index| format!("第{index}句")).collect();
        let many: Vec<HistoryMessage> = many
            .iter()
            .map(|line| message("user", line, None))
            .collect();
        let text = chat_history_text(&many);
        assert_eq!(text.lines().count(), HISTORY_MESSAGES);
        assert!(text.starts_with("user：第10句\n"));
    }

    #[test]
    fn what_rides_on_a_line_is_not_said() {
        assert_eq!(chat_safe_content("行啊。\n[[wear:舞台装]]"), "行啊。");
        assert_eq!(chat_safe_content("唱。\n[[music:play]]"), "唱。");
        assert_eq!(
            chat_safe_content(
                "已经整理好了。\n[输出数据: {}]\n  [display type: table]\n[确认: cnf_1]"
            ),
            "已经整理好了。"
        );
        let history = [
            message("assistant", "[[music:next]]", None),
            message("user", "嗯", None),
        ];
        assert_eq!(
            chat_history_text(&history),
            "user：嗯",
            "nothing said is no line"
        );
    }
}
