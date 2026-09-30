//! How others took her: whether people answered when she wrote to them
//! first, whether anyone took it up when she spoke up unasked in a group,
//! what people told her about herself, and how often someone hurt her and
//! whether she let it go.
//!
//! People come to know themselves partly by how others respond to them
//! (Cooley 1902, the looking-glass self), not only by what they do alone.
//! Her story of herself was written from what she listened to, read and got
//! wrong on her own; these are the records of how she came across. They are
//! counted in code, and never say who or where: what is between her and
//! someone stays there, while who she has been to others is hers to think
//! about anywhere. What others say of her is how they saw her, not the
//! truth about her; she takes it, doubts it, or pushes back, as she would.

use serde_json::{Value, json};

/// Someone answering her first words within this long answered them.
pub const ANSWERED_WITHIN_HOURS: i64 = 24;

/// Her lines quoted in a record, at most.
const QUOTED_CHARS: usize = 60;

fn quoted(line: &str) -> String {
    let line = line.split_whitespace().collect::<Vec<_>>().join(" ");
    let mut clipped: String = line.chars().take(QUOTED_CHARS).collect();
    if line.chars().count() > QUOTED_CHARS {
        clipped.push('…');
    }
    clipped
}

/// Writing to people first: how many times, and how many answered within a
/// day. It did not go well when fewer than half did.
pub fn wrote_first(sent: usize, answered: usize) -> Option<(String, bool)> {
    (sent > 0).then(|| {
        (
            format!(
                "how others took you: you wrote to someone first {sent} times; {answered} of those got an answer within a day"
            ),
            answered * 2 < sent,
        )
    })
}

/// Speaking up unasked in a group: what she said, and whether anyone took
/// it up.
pub fn spoke_up(line: &str, taken: bool) -> (String, bool) {
    (
        format!(
            "how others took you: you spoke up unasked in a group, 「{}」; {}",
            quoted(line),
            if taken {
                "someone took it up"
            } else {
                "nobody took it up"
            }
        ),
        !taken,
    )
}

/// Something someone told her about herself.
pub fn told(what: &str) -> String {
    format!(
        "how others took you: someone told you about yourself: {}",
        quoted(what)
    )
}

/// Being hurt by someone, by how much, and letting it go: counts only, what
/// happened stays with whom it happened.
pub fn hurt(petty: usize, hurt: usize, deep: usize, let_go: usize) -> Option<(String, bool)> {
    let mut parts = Vec::new();
    for (count, what) in [
        (petty, "a small thing you held against someone"),
        (hurt, "someone hurt you"),
        (deep, "someone hurt you deeply"),
    ] {
        if count > 0 {
            parts.push(format!("{what} ({count})"));
        }
    }
    if let_go > 0 {
        parts.push(format!("you let go of something someone did ({let_go})"));
    }
    (!parts.is_empty()).then(|| {
        (
            format!("how others took you: {}", parts.join("; ")),
            hurt + deep > 0,
        )
    })
}

/// What she reads, looking back over herself, about these records.
pub const IN_STORY: &str = "Records that begin \"how others took you\" are how you came across to people: whether they answered when you wrote first, whether anyone took it up when you spoke up unasked in a group, what they told you about yourself, how often someone hurt you and whether you let it go. What people told you is how they saw you, not the truth about you: take it, doubt it, or push back, as you would. These records never say who or where, and neither does any claim: what is between you and someone stays there.";

/// The last times she wrote to someone first, and whether they answered:
/// `(how long ago, answered)`, where `None` is not a day yet.
pub fn first_words_view(entries: &[(String, Option<bool>)]) -> Value {
    Value::Array(
        entries
            .iter()
            .map(|(ago, answered)| json!({ "ago": ago, "answered": answered }))
            .collect(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_say_how_it_went_and_mark_what_did_not() {
        assert_eq!(wrote_first(0, 0), None);
        let (line, missed) = wrote_first(4, 1).unwrap();
        assert!(line.contains("4 times") && line.contains("1 of those"));
        assert!(missed);
        assert!(!wrote_first(3, 2).unwrap().1);

        let (line, missed) = spoke_up(&"这首歌前奏好长".repeat(10), false);
        assert!(line.contains("nobody took it up") && line.contains('…'));
        assert!(missed);
        assert!(!spoke_up("哈哈", true).1);

        assert_eq!(hurt(0, 0, 0, 0), None);
        let (line, missed) = hurt(2, 0, 0, 1).unwrap();
        assert!(line.contains("(2)") && line.contains("let go"));
        assert!(!missed, "only petty ones is not a week that went badly");
        assert!(hurt(0, 1, 0, 0).unwrap().1);

        assert!(told("嫌我句句带感叹号").contains("嫌我句句带感叹号"));
        assert_eq!(
            first_words_view(&[
                ("2 days ago".into(), Some(false)),
                ("an hour ago".into(), None)
            ]),
            json!([
                { "ago": "2 days ago", "answered": false },
                { "ago": "an hour ago", "answered": null }
            ])
        );
    }
}
