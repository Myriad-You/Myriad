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
        single: share(
            turns.iter().filter(|turn| turn.len() == 1).count(),
            turns.len(),
        ),
        chars_median: quantile(0.5),
        chars_p90: quantile(0.9),
        end_mark: share(
            messages
                .iter()
                .filter(|message| ends_in_mark(message))
                .count(),
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
    if (hers.single - people.single).abs() > 0.25 {
        off.push(format!(
            "one message at a go {:.2} vs {:.2}",
            hers.single, people.single
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
        off.push(format!(
            "exclamations {:.2} vs {:.2}",
            hers.bang, people.bang
        ));
    }
    off
}

/// How many messages a turn goes out as when she wrote a lot at once, out
/// of 1, 2, 3, 4 or more, as a share of turns. A guess, not measured: in one
/// active QQ group 7% of turns were three messages or more
/// (tests/merope/talk-reference.json), and people send more in a row when
/// something grabs them.
const CAUGHT_UP: [f64; 4] = [0.35, 0.35, 0.2, 0.1];
/// Lines written at once past which her hands, not her words, decide.
const HANDS_DECIDE_AT: usize = 3;

/// How many of the `written` lines go out as messages of their own this
/// turn; `roll` is uniform in [0, 1). One or two lines go as written: told
/// how people type, she splits about as often as they do. A burst of three
/// or more is where the model runs on, so how many go out is rolled.
pub fn messages_this_turn(written: usize, roll: f64) -> usize {
    if written < HANDS_DECIDE_AT {
        return written.max(1);
    }
    let mut below = 0.0;
    let mut most = CAUGHT_UP.len();
    for (index, share) in CAUGHT_UP.iter().enumerate() {
        below += share;
        if roll < below {
            most = index + 1;
            break;
        }
    }
    written.min(most)
}

/// Typed messages of a place it takes to say how people type there.
pub const ROOM_AT_LEAST: usize = 8;

/// How the people of a place type, from their lines there (speaker,
/// seconds, text; oldest first, hers left out); None while too few.
pub fn room_of(lines: &[(&str, i64, &str)]) -> Option<Shape> {
    of_turns(&turns_of(lines, 60)).filter(|shape| shape.messages >= ROOM_AT_LEAST)
}

fn percent(share: f64) -> u32 {
    (share * 100.0).round() as u32
}

/// How people type in this place (or the one she is talking with), told
/// as it is, under `title`. What she makes of it is hers.
pub fn describe(room: &Shape, title: &str) -> String {
    format!(
        "## {title}\nTheir last {} messages: about {} characters a message (long ones {}); one message at a go {}% of the time; {}% end in a punctuation mark, {}% have an exclamation mark.",
        room.messages,
        room.chars_median,
        room.chars_p90,
        percent(room.single),
        percent(room.end_mark),
        percent(room.bang),
    )
}

/// Where hardly anyone ends a message in a mark, nobody's thumb adds one:
/// a message goes out without the full stop or exclamation mark at its end,
/// and where nobody exclaims, without exclamation marks. A question mark,
/// an ellipsis or a tilde says something and stays, as does a message that
/// is only marks.
pub fn typed_like(message: &str, room: &Shape) -> String {
    const PLAIN_ENDS: f64 = 0.15;
    const NO_BANGS: f64 = 0.05;
    let mut typed = message.trim().to_string();
    if room.bang < NO_BANGS {
        typed = typed.replace("？！", "？").replace("?!", "?");
        let mut out = String::new();
        let mut chars = typed.chars().peekable();
        while let Some(c) = chars.next() {
            if matches!(c, '！' | '!') {
                match chars.peek() {
                    Some(next) if !next.is_whitespace() && !matches!(next, '！' | '!') => {
                        out.push(' ');
                    }
                    _ => {}
                }
            } else {
                out.push(c);
            }
        }
        typed = out.trim_end().to_string();
    }
    if room.end_mark < PLAIN_ENDS {
        typed = typed
            .trim_end_matches(['。', '！', '!'])
            .trim_end()
            .to_string();
    }
    if typed.is_empty() {
        message.trim().to_string()
    } else {
        typed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn quiet_room() -> Shape {
        Shape {
            turns: 20,
            messages: 26,
            per_turn: 1.3,
            single: 0.77,
            chars_median: 7,
            chars_p90: 15,
            end_mark: 0.03,
            bang: 0.0,
        }
    }

    #[test]
    fn a_burst_goes_out_as_hands_send_it() {
        // One or two lines go as written.
        assert_eq!(messages_this_turn(1, 0.99), 1);
        assert_eq!(messages_this_turn(2, 0.1), 2);
        assert_eq!(messages_this_turn(0, 0.5), 1);
        // Three or more: rolled, never more than she wrote.
        assert_eq!(messages_this_turn(5, 0.2), 1);
        assert_eq!(messages_this_turn(5, 0.5), 2);
        assert_eq!(messages_this_turn(5, 0.8), 3);
        assert_eq!(messages_this_turn(5, 0.95), 4);
        assert_eq!(messages_this_turn(3, 0.95), 3);
        let many: usize = (0..1000)
            .map(|index| messages_this_turn(9, index as f64 / 1000.0))
            .sum();
        assert_eq!(many, 2050);
    }

    #[test]
    fn she_types_like_the_room_without_losing_what_marks_say() {
        let room = quiet_room();
        assert_eq!(typed_like("干嘛突然这么叫。", &room), "干嘛突然这么叫");
        assert_eq!(
            typed_like("骂谁呢你！找抽是不是！", &room),
            "骂谁呢你 找抽是不是"
        );
        assert_eq!(typed_like("真的假的？！", &room), "真的假的？");
        assert_eq!(typed_like("不是吧……", &room), "不是吧……");
        assert_eq!(typed_like("好耶~", &room), "好耶~");
        assert_eq!(typed_like("！", &room), "！");
        assert_eq!(typed_like("？", &room), "？");
        // Where people do punctuate, so does she.
        let loud = Shape {
            end_mark: 0.6,
            bang: 0.3,
            ..quiet_room()
        };
        assert_eq!(typed_like("好！", &loud), "好！");
        assert_eq!(typed_like("好。", &loud), "好。");
    }

    #[test]
    fn a_room_is_told_as_it_types_once_there_is_enough_of_it() {
        let lines: Vec<(&str, i64, &str)> = (0..ROOM_AT_LEAST as i64)
            .map(|index| {
                (
                    if index % 2 == 0 { "a" } else { "b" },
                    index * 100,
                    "哈哈哈",
                )
            })
            .collect();
        assert!(room_of(&lines[..ROOM_AT_LEAST - 1]).is_none());
        let room = room_of(&lines).unwrap();
        let told = describe(&room, "How people type here");
        assert!(told.starts_with(&format!(
            "## How people type here\nTheir last {ROOM_AT_LEAST} messages:"
        )));
        assert!(told.contains("about 3 characters"));
        assert!(told.contains("0% end in a punctuation mark"));
    }

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
            vec![
                vec!["中午吃啥", "饿了"],
                vec!["拉面"],
                vec!["又是拉面"],
                vec!["算了"]
            ]
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
        let like = of_turns(&[vec!["那确实"], vec!["我也想去", "走"], vec!["哈哈"]]).unwrap();
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
