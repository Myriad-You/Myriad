//! How her lines differ from the people's in a place, found from the lines
//! themselves. What people in a chat app really say is barely in any model's
//! training data, so neither the model's sense of normal nor ours can tell
//! her; the people she talks with can. Every message is cut into the pieces
//! a way of talking shows in (how it starts, how it ends, and a short message
//! whole), the pieces are counted for her and for them, and the ones she uses
//! far more than they do are told to her as counts. Nothing is picked out in
//! advance, and no reason is given: what she makes of it is hers.
//!
//! Only counts of short pieces are kept, never a message.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Pieces counted in a message: where, and what.
const STARTS: [usize; 2] = [1, 2];
const ENDS: [usize; 3] = [1, 2, 3];
/// A message this short is also counted whole.
const WHOLE_UP_TO: usize = 4;

/// How many messages, and how many had each piece.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Counts {
    pub messages: u32,
    pub pieces: HashMap<String, u32>,
}

/// The pieces of one message, each once: `start:`, `end:` or `whole:` and
/// the characters. Pictures and blank lines have none.
pub fn pieces(message: &str) -> Vec<String> {
    let message = message.trim();
    if message.is_empty() || (message.starts_with('[') && message.ends_with(']')) {
        return Vec::new();
    }
    let chars: Vec<char> = message.chars().collect();
    let mut out = Vec::new();
    for length in STARTS {
        if chars.len() > length {
            out.push(format!(
                "start:{}",
                chars[..length].iter().collect::<String>()
            ));
        }
    }
    for length in ENDS {
        if chars.len() > length {
            out.push(format!(
                "end:{}",
                chars[chars.len() - length..].iter().collect::<String>()
            ));
        }
    }
    if chars.len() <= WHOLE_UP_TO {
        out.push(format!("whole:{message}"));
    }
    out.sort();
    out.dedup();
    out
}

impl Counts {
    pub fn add(&mut self, message: &str) {
        let pieces = pieces(message);
        if pieces.is_empty() {
            return;
        }
        self.messages += 1;
        for piece in pieces {
            *self.pieces.entry(piece).or_default() += 1;
        }
    }

    pub fn of<'a>(messages: impl IntoIterator<Item = &'a str>) -> Self {
        let mut counts = Self::default();
        for message in messages {
            counts.add(message);
        }
        counts
    }

    /// Add what `other` counted.
    pub fn merge(&mut self, other: &Counts) {
        self.messages += other.messages;
        for (piece, count) in &other.pieces {
            *self.pieces.entry(piece.clone()).or_default() += count;
        }
    }

    /// Keep the counts from growing without end: past `most` messages, all
    /// are halved, so older talk weighs less and rare pieces drop out.
    pub fn keep_within(&mut self, most: u32) {
        if self.messages <= most {
            return;
        }
        self.messages /= 2;
        self.pieces.retain(|_, count| {
            *count /= 2;
            *count > 0
        });
    }
}

/// A piece she uses far more than the people there.
#[derive(Debug, Clone, PartialEq)]
pub struct Overused {
    pub piece: String,
    pub hers: u32,
    pub her_messages: u32,
    pub theirs: u32,
    pub their_messages: u32,
    pub z: f64,
}

/// Messages of theirs it takes to say what is usual there.
const THEIRS_AT_LEAST: u32 = 20;
/// Messages of hers with the piece before it is hers at all.
const HERS_AT_LEAST: u32 = 3;
/// How far apart the two shares must be: a two-proportion z, and her share
/// at least this many times theirs.
const Z_AT_LEAST: f64 = 2.5;
const TIMES_AT_LEAST: f64 = 3.0;
const TOLD: usize = 3;

/// The pieces she uses far more than they do, most telling first; a piece
/// inside one already told (「哈」 in 「哈哈」, at the same end) is left out.
pub fn overused(hers: &Counts, theirs: &Counts) -> Vec<Overused> {
    if theirs.messages < THEIRS_AT_LEAST || hers.messages < HERS_AT_LEAST {
        return Vec::new();
    }
    let (h_total, t_total) = (f64::from(hers.messages), f64::from(theirs.messages));
    let mut found: Vec<Overused> = hers
        .pieces
        .iter()
        .filter(|(_, count)| **count >= HERS_AT_LEAST)
        .filter_map(|(piece, count)| {
            let theirs_count = theirs.pieces.get(piece).copied().unwrap_or(0);
            let p_hers = (f64::from(*count) + 0.5) / (h_total + 1.0);
            let p_theirs = (f64::from(theirs_count) + 0.5) / (t_total + 1.0);
            let pooled = (f64::from(*count + theirs_count) + 1.0) / (h_total + t_total + 2.0);
            let spread = (pooled * (1.0 - pooled) * (1.0 / h_total + 1.0 / t_total)).sqrt();
            let z = (p_hers - p_theirs) / spread;
            (z >= Z_AT_LEAST && p_hers >= TIMES_AT_LEAST * p_theirs).then(|| Overused {
                piece: piece.clone(),
                hers: *count,
                her_messages: hers.messages,
                theirs: theirs_count,
                their_messages: theirs.messages,
                z,
            })
        })
        .collect();
    // Exact ties go by the piece itself: the counts are a hash map, and its
    // order is not the same twice.
    found.sort_by(|left, right| {
        right
            .z
            .total_cmp(&left.z)
            .then_with(|| right.piece.chars().count().cmp(&left.piece.chars().count()))
            .then_with(|| left.piece.cmp(&right.piece))
    });
    let mut told: Vec<Overused> = Vec::new();
    for candidate in found {
        let (place, text) = candidate.piece.split_once(':').unwrap_or(("", ""));
        let overlaps = told.iter().any(|kept| {
            let (kept_place, kept_text) = kept.piece.split_once(':').unwrap_or(("", ""));
            kept_place == place && (kept_text.contains(text) || text.contains(kept_text))
        });
        if !overlaps {
            told.push(candidate);
        }
        if told.len() == TOLD {
            break;
        }
    }
    told
}

/// What she is told, when anything: counts, no reasons.
pub fn describe(found: &[Overused]) -> Option<String> {
    if found.is_empty() {
        return None;
    }
    let lines: Vec<String> = found
        .iter()
        .map(|item| {
            let (place, text) = item.piece.split_once(':').unwrap_or(("", &item.piece));
            let how = match place {
                "start" => format!("started with 「{text}」"),
                "end" => format!("ended with 「{text}」"),
                _ => format!("were just 「{text}」"),
            };
            format!(
                "- {} of your {} messages {how}; {} of their {}.",
                item.hers, item.her_messages, item.theirs, item.their_messages
            )
        })
        .collect();
    Some(format!(
        "## Your lines and theirs\nSet side by side with the people you talk with here, your messages differ like this:\n{}",
        lines.join("\n")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The day the group asked her why she kept going 哈哈: what they said
    /// that day and what she said, nothing picked out in advance.
    #[test]
    fn what_she_says_that_they_never_do_is_found_from_the_lines() {
        let hers = Counts::of([
            "？",
            "干嘛突然这么叫 有事直说，少来这套哈哈",
            "我发的啊，就上面那句",
            "怎么，被盗号了？哈哈",
            "没啊，我说我发的那句",
            "你们这反应才像被盗号了吧哈哈",
            "笑什么，刚才那句确实是我发的",
            "别给我扣盗号帽子！哈哈",
            "调吧调吧，调完再来验我",
        ]);
        let theirs = Counts::of([
            "你什么时候",
            "签证办下来先",
            "真的在办吗",
            "真的",
            "还在办光之国的十年签",
            "wp已经正式扬掉了",
            "真的吗",
            "已经换了吗",
            "blog.已经重定向了",
            "刚换完",
            "想念我了吗",
            "宝宝，晚安喵",
            "晚安猪咪",
            "宝宝，可以给我一个😚吗？",
            "宝宝",
            "这是谁发的",
            "？",
            "被盗号了？",
            "神经模型",
            "一直哈哈干嘛",
            "我调调",
        ]);
        let found = overused(&hers, &theirs);
        assert_eq!(found[0].piece, "end:哈哈");
        assert_eq!((found[0].hers, found[0].theirs), (4, 0));
        // 「哈」 at the end is the same thing, not told twice.
        assert!(!found.iter().any(|item| item.piece == "end:哈"));
        let told = describe(&found).unwrap();
        assert!(told.contains("- 4 of your 9 messages ended with 「哈哈」; 0 of their 21."));
        // A lone ？ they send too is not hers.
        assert!(!found.iter().any(|item| item.piece == "whole:？"));
    }

    #[test]
    fn what_they_say_too_or_too_little_talk_tells_nothing() {
        let theirs: Vec<String> = (0..30).map(|index| format!("第{index}句哈哈")).collect();
        let theirs = Counts::of(theirs.iter().map(String::as_str));
        let hers = Counts::of(["好哈哈", "行哈哈", "来了哈哈", "嗯哈哈"]);
        assert!(overused(&hers, &theirs).is_empty());
        let few_of_theirs = Counts::of(["真的", "好"]);
        assert!(overused(&hers, &few_of_theirs).is_empty());
        assert_eq!(describe(&[]), None);
        // Pictures are not typed; short messages count whole; counts halve.
        assert!(pieces("[图片]").is_empty());
        assert!(pieces("？").contains(&"whole:？".to_string()));
        let mut many = Counts::of(["好的", "好的"]);
        many.keep_within(1);
        assert_eq!(many.messages, 1);
        assert_eq!(many.pieces.get("whole:好的"), Some(&1));
    }

    #[test]
    fn pieces_as_telling_as_each_other_are_told_in_one_order() {
        // 「呀」 and 「嘛」 end as many of her lines and none of theirs: the
        // same z, the same length.
        let hers = Counts::of(["好呀", "去呀", "来呀", "行嘛", "说嘛", "看嘛"]);
        let theirs: Vec<String> = (0..30).map(|index| format!("第{index}句")).collect();
        let theirs = Counts::of(theirs.iter().map(String::as_str));
        let told: Vec<String> = overused(&hers, &theirs)
            .into_iter()
            .map(|item| item.piece)
            .collect();
        assert_eq!(told, ["end:呀", "end:嘛"]);
    }
}
