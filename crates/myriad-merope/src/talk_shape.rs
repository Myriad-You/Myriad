//! The shape of how people type in a chat, apart from what they say: how
//! many messages a turn goes out as, how long a message is, how often one
//! ends in a mark or carries an exclamation. People in the same place come
//! to type alike, and a speaker who does not stands out at a glance, so the
//! shape is measured the same way for the people in a place and for her.

use serde::{Deserialize, Serialize};

/// A turn: the messages one person sent in a row.
pub type Turn<'a> = Vec<&'a str>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shape {
    pub turns: usize,
    pub messages: usize,
    /// Messages a turn goes out as, on average.
    pub per_turn: f64,
    /// Share of turns that are a single message.
    pub single: f64,
    pub chars_median: usize,
    pub chars_p90: usize,
    /// Share of messages ending in a sentence mark.
    pub end_mark: f64,
    /// Share of messages with an exclamation mark.
    pub bang: f64,
}

/// Pictures, stickers and empty lines have no typing shape.
fn typed(message: &str) -> Option<&str> {
    let message = message.trim();
    (!message.is_empty() && !(message.starts_with('[') && message.ends_with(']')))
        .then_some(message)
}

fn ends_in_mark(message: &str) -> bool {
    message.chars().last().is_some_and(|last| {
        matches!(
            last,
            '。' | '！' | '？' | '!' | '?' | '.' | '…' | '~' | '～'
        )
    })
}

pub fn of_turns(turns: &[Turn<'_>]) -> Option<Shape> {
    let turns: Vec<Vec<&str>> = turns
        .iter()
        .map(|turn| turn.iter().filter_map(|message| typed(message)).collect())
        .filter(|turn: &Vec<&str>| !turn.is_empty())
        .collect();
    let messages: Vec<&str> = turns.iter().flatten().copied().collect();
    if messages.is_empty() {
        return None;
    }
    let mut chars: Vec<usize> = messages
        .iter()
        .map(|message| message.chars().count())
        .collect();
    chars.sort_unstable();
    let quantile = |q: f64| chars[((chars.len() as f64 * q) as usize).min(chars.len() - 1)];
    let share = |count: usize, of: usize| count as f64 / of as f64;
    Some(Shape {
        turns: turns.len(),
        messages: messages.len(),
        per_turn: share(messages.len(), turns.len()),
        single: share(turns.iter().filter(|turn| turn.len() == 1).count(), turns.len()),
        chars_median: quantile(0.5),
        chars_p90: quantile(0.9),
        end_mark: share(
            messages.iter().filter(|message| ends_in_mark(message)).count(),
            messages.len(),
        ),
        bang: share(
            messages
                .iter()
                .filter(|message| message.contains('！') || message.contains('!'))
                .count(),
            messages.len(),
        ),
    })
}

/// A chat's lines as turns: consecutive lines from the same speaker, no
/// more than `within` seconds apart, are one turn. Lines are (speaker,
/// seconds, text), oldest first.
pub fn turns_of<'a>(lines: &[(&str, i64, &'a str)], within: i64) -> Vec<Turn<'a>> {
    let mut turns: Vec<Turn<'a>> = Vec::new();
    let mut last: Option<(&str, i64)> = None;
    for &(speaker, at, text) in lines {
        match (last, turns.last_mut()) {
            (Some((was, then)), Some(turn)) if was == speaker && at - then <= within => {
                turn.push(text);
            }
            _ => turns.push(vec![text]),
        }
        last = Some((speaker, at));
    }
    turns
}

/// How far `hers` is from how people type in the same place: each measure
/// out of line, named, with the two values. Tolerances are loose on
/// purpose; a person is not the average of a room.
pub fn out_of_line(hers: &Shape, people: &Shape) -> Vec<String> {
    let mut off = Vec::new();
    if hers.per_turn > people.per_turn * 1.5 + 0.2 {
        off.push(format!(
            "messages a turn {:.2} vs {:.2}",
            hers.per_turn, people.per_turn
        ));
    }
    if hers.chars_median > people.chars_median * 2 + 2 {
        off.push(format!(
            "median message {} vs {} chars",
            hers.chars_median, people.chars_median
        ));
    }
    if hers.chars_p90 > people.chars_p90 * 2 + 4 {
        off.push(format!(
            "long messages (p90) {} vs {} chars",
            hers.chars_p90, people.chars_p90
        ));
    }
    if hers.end_mark > people.end_mark + 0.2 {
        off.push(format!(
            "ending in a mark {:.2} vs {:.2}",
            hers.end_mark, people.end_mark
        ));
    }
    if hers.bang > people.bang + 0.15 {
        off.push(format!("exclamations {:.2} vs {:.2}", hers.bang, people.bang));
    }
    off
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_is_what_one_person_sends_in_a_row() {
        let lines = [
            ("a", 0, "中午吃啥"),
            ("a", 4, "饿了"),
            ("b", 10, "拉面"),
            ("a", 12, "又是拉面"),
            ("a", 200, "算了"),
        ];
        let turns = turns_of(&lines, 60);
        assert_eq!(
            turns,
            vec![vec!["中午吃啥", "饿了"], vec!["拉面"], vec!["又是拉面"], vec!["算了"]]
        );
        let shape = of_turns(&turns).unwrap();
        assert_eq!(shape.turns, 4);
        assert_eq!(shape.messages, 5);
        assert!((shape.per_turn - 1.25).abs() < 1e-9);
        assert!((shape.single - 0.75).abs() < 1e-9);
        assert_eq!(shape.end_mark, 0.0);
    }

    #[test]
    fn pictures_and_stickers_have_no_typing_shape() {
        assert_eq!(of_turns(&[vec!["[图片]", "  "]]), None);
        let shape = of_turns(&[vec!["[表情：翻白眼]", "好！"]]).unwrap();
        assert_eq!(shape.messages, 1);
        assert_eq!(shape.bang, 1.0);
        assert_eq!(shape.end_mark, 1.0);
    }

    #[test]
    fn she_stands_out_when_she_types_unlike_the_room() {
        let people = of_turns(&[vec!["哈哈"], vec!["真的假的"], vec!["笑死", "绷不住了"]]).unwrap();
        let like = of_turns(&[vec!["那确实"], vec!["我也想去"]]).unwrap();
        assert!(out_of_line(&like, &people).is_empty());
        let unlike = of_turns(&[
            vec!["啊？！", "你这也太离谱了吧！", "到底怎么回事啊，快说清楚！"],
            vec!["不是吧！", "真的假的！", "我不信！"],
        ])
        .unwrap();
        let off = out_of_line(&unlike, &people);
        assert!(off.iter().any(|line| line.starts_with("messages a turn")));
        assert!(off.iter().any(|line| line.starts_with("ending in a mark")));
        assert!(off.iter().any(|line| line.starts_with("exclamations")));
    }
}
