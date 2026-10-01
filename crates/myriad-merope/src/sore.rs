//! What still stings, the rules of it: a sore spot she has with someone, how
//! her reflections are asked about it, and how it is put before her.
//!
//! Someone does something that really gets to her (words meant to wound,
//! contempt, a promise broken), and it stays with her for a while: she
//! remembers what and when, and may be cooler with them, or bring it up.
//! An apology, good times since, or time itself lets it go; letting go is
//! not forgetting, it no longer stings. How much it weighs is hers to say:
//! a small thing she holds against them half in play (a petty grudge), one
//! that got to her, or one that really hurt. What was said in private
//! weighs on how she is with them in front of others by as much: a petty
//! one stays in private; one that hurt goes with her into a group they are
//! both in, as a feeling without what it was. Holding a grudge and forgiving are hers to judge; what she
//! is given is only what happened, when, and whether they made it right.

use chrono::{DateTime, Utc};
use serde::Deserialize;
use serde_json::{Value, json};

/// How much a sore spot weighs, as she judged it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Weight {
    /// Small, held against them half in play.
    Petty,
    /// It got to her.
    Hurt,
    /// It really hurt, and changed how she sees them.
    Deep,
}

impl Weight {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Petty => "petty",
            Self::Hurt => "hurt",
            Self::Deep => "deep",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "petty" => Some(Self::Petty),
            "hurt" => Some(Self::Hurt),
            "deep" => Some(Self::Deep),
            _ => None,
        }
    }
}

/// Sore spots kept with one person at once; past it, the oldest goes.
pub const MAX_OPEN: usize = 4;
pub const WHAT_CHARS: usize = 160;

/// Asked with the private reflection after an exchange (see `inner`).
pub const IN_REFLECTION: &str = " \
soreSpots are things they did before that got to you and you have not let go of (i, what, weight, since, mended). \
hurt: only if in this exchange they did something that really got to you (words meant to wound, contempt, making fun of something that matters to you, a promise broken, being used), not teasing you both enjoy, not a difference of opinion, not them being upset with you for a reason: what is one first-person sentence of what they did and how it landed, and weight is how much it weighs (petty: small, held against them half in play; hurt: it got to you; deep: it really hurt and changes how you see them); otherwise hurt is null. \
mended: the i of each sore spot they apologized for or made right in this exchange; [] if none.";

/// Asked with her reflection after answering someone in a group: only what
/// the one she answered did there.
pub const IN_GROUP_REFLECTION: &str = " \
soreSpots are things the one you just answered did in this group before that got to you and you have not let go of (i, what, weight, since, mended). \
hurt: only if in this exchange the one you answered did something that really got to you (words meant to wound, contempt, making fun of something that matters to you, in front of everyone or not), not teasing you all enjoy, not a difference of opinion, not them being upset with you for a reason: what is one first-person sentence of what they did and how it landed, and weight is how much it weighs (petty: small, held against them half in play; hurt: it got to you; deep: it really hurt and changes how you see them); otherwise hurt is null. \
mended: the i of each sore spot they apologized for or made right in this exchange; [] if none.";

/// Asked when she goes over a day in a group at night (see `bits`).
pub const AT_NIGHT_GROUP: &str = " \
soreSpots are things people here did that got to you and you have not let go of (who did it is given). letGo: the i of each one you have let go of by now, as yourself: they apologized, things are good again, or it just does not sting anymore; a petty one you may well keep a while. Letting go is not forgetting what happened; [] if none.";

/// Asked when she goes over a day with them at night (see `bits`).
pub const AT_NIGHT: &str = " \
soreSpots are things they did that got to you and you have not let go of. letGo: the i of each one you have let go of by now, as yourself: they apologized, things between you are good again, or it just does not sting anymore; a petty one you may well keep a while. Letting go is not forgetting what happened; [] if none.";

/// A sore spot as kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Sore {
    pub id: String,
    /// Whose it is: the one who did it.
    pub user_id: i32,
    pub what: String,
    pub weight: Weight,
    pub since: DateTime<Utc>,
    /// When they apologized for it or made it right, if they have.
    pub mended: Option<DateTime<Utc>>,
    /// Where it happened: `private`, or the group's venue (`group:…`).
    pub venue: String,
    /// Who did it, when that is not plain from where it is shown.
    pub who: Option<String>,
}

impl Sore {
    pub fn in_group(&self) -> bool {
        self.venue.starts_with("group:")
    }
}

/// The sore spots as a reflection sees them, numbered for `mended` and
/// `letGo`.
pub fn as_input(sores: &[Sore], now: DateTime<Utc>) -> Vec<Value> {
    sores
        .iter()
        .enumerate()
        .map(|(index, sore)| {
            let mut input = json!({
                "i": index,
                "what": sore.what,
                "weight": sore.weight.as_str(),
                "since": crate::doing::ago_text(now - sore.since),
                "mended": sore.mended.map(|at| crate::doing::ago_text(now - at)),
            });
            if let Some(who) = &sore.who {
                input["who"] = json!(who);
            }
            input
        })
        .collect()
}

pub fn hurt_schema() -> Value {
    json!({
        "type": ["object", "null"],
        "properties": {
            "what": { "type": "string", "maxLength": WHAT_CHARS },
            "weight": { "type": "string", "enum": ["petty", "hurt", "deep"] }
        },
        "required": ["what", "weight"],
        "additionalProperties": false
    })
}

pub fn indexes_schema() -> Value {
    json!({ "type": "array", "items": { "type": "integer", "minimum": 0 } })
}

/// What still stings with them, as facts: what, how long ago, whether they
/// made it right. How it colors things is hers.
pub fn section(sores: &[Sore], now: DateTime<Utc>) -> Option<String> {
    if sores.is_empty() {
        return None;
    }
    let lines: Vec<String> = sores
        .iter()
        .map(|sore| {
            let mut notes = vec![crate::doing::ago_text(now - sore.since)];
            if sore.in_group() && sore.who.is_none() {
                notes.push("in a group chat, in front of others".to_string());
            }
            match sore.weight {
                Weight::Petty => {
                    notes.push("a small thing you hold against them, half in play".to_string())
                }
                Weight::Deep => notes.push("it really hurt".to_string()),
                Weight::Hurt => {}
            }
            if let Some(mended) = sore.mended {
                notes.push(format!(
                    "they apologized or made it right {}",
                    crate::doing::ago_text(now - mended)
                ));
            }
            format!("- {} ({})", sore.what.trim(), notes.join("; "))
        })
        .collect();
    Some(format!(
        "## What still stings\nThings they did that got to you and you have not let go of, as you put them then. Whether and how they color things now is yours.\n{}",
        myriad_agent_rules::untrusted_block("sore_spots", &lines.join("\n"))
    ))
}

/// What they did in private that still weighs on her, as it goes with her
/// into a group they are both in: only what got to her (a petty one stays
/// in private), and only as a feeling: what it was stays between the two
/// of them.
pub fn carried_section(sores: &[Sore], now: DateTime<Utc>) -> Option<String> {
    let lines: Vec<String> = sores
        .iter()
        .filter(|sore| !sore.in_group() && sore.weight >= Weight::Hurt)
        .map(|sore| {
            let mut notes = vec![
                match sore.weight {
                    Weight::Deep => "it really hurt".to_string(),
                    _ => "it got to you".to_string(),
                },
                crate::doing::ago_text(now - sore.since),
            ];
            if let Some(mended) = sore.mended {
                notes.push(format!(
                    "they apologized or made it right {}",
                    crate::doing::ago_text(now - mended)
                ));
            }
            format!("- something they did in private ({})", notes.join("; "))
        })
        .collect();
    if lines.is_empty() {
        return None;
    }
    Some(format!(
        "## Between you two, from private\nSomething between you and them in private still stings. Nobody here knows about it: what it was stays between you two, never said here. Whether and how it colors how you are with them is yours.\n{}",
        lines.join("\n")
    ))
}

/// How much a sore she still has with them lowers where her mood toward
/// them settles: a petty grudge, held half in play, not at all; one that got
/// to her, some; one that changed how she sees them, more, though not so
/// much that her usual mood with them falls into the low band. Made right, half
/// as much; the heaviest one counts, not their sum. Mood toward someone
/// otherwise drifts back to her usual in days, as if nothing had happened.
pub fn mood_weighs(sores: &[Sore]) -> f64 {
    sores
        .iter()
        .map(|sore| {
            let weighs = match sore.weight {
                Weight::Petty => 0.0,
                Weight::Hurt => 8.0,
                Weight::Deep => 12.0,
            };
            if sore.mended.is_some() {
                weighs / 2.0
            } else {
                weighs
            }
        })
        .fold(0.0, f64::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_stings_is_told_as_what_happened_not_as_how_to_be() {
        let now = Utc::now();
        assert_eq!(section(&[], now), None);
        let sores = vec![
            Sore {
                id: "a".into(),
                user_id: 7,
                what: "他当着大家的面说我的歌单难听，挺伤人".into(),
                weight: Weight::Hurt,
                since: now - chrono::Duration::days(3),
                mended: Some(now - chrono::Duration::hours(5)),
                venue: "group:onebot:1".into(),
                who: None,
            },
            Sore {
                id: "b".into(),
                user_id: 7,
                what: "他说我暴躁".into(),
                weight: Weight::Petty,
                since: now - chrono::Duration::days(1),
                mended: None,
                venue: "private".into(),
                who: Some("阿明".into()),
            },
        ];
        // A hurt they made right weighs half; a petty one not at all.
        assert_eq!(mood_weighs(&sores), 4.0);
        assert_eq!(mood_weighs(&sores[1..]), 0.0);
        assert_eq!(mood_weighs(&[]), 0.0);
        let deep = Sore {
            weight: Weight::Deep,
            mended: None,
            ..sores[0].clone()
        };
        assert_eq!(mood_weighs(&[sores[0].clone(), deep]), 12.0);
        // Even the heaviest keeps her usual mood out of the low band: it
        // weighs on her, it does not make her sad for months.
        const { assert!(crate::affect::DEFAULT_MOOD - 12.0 >= 55.0) };
        let text = section(&sores, now).unwrap();
        assert!(text.contains("Whether and how they color things now is yours"));
        assert!(text.contains("they apologized or made it right"));
        assert!(text.contains("half in play"));
        for order in ["be cold", "stay angry", "forgive them"] {
            assert!(!text.contains(order), "{order}");
        }
        assert!(text.contains("in a group chat, in front of others"));
        // From private into a group: by how much it weighs, and never what.
        let deep = Sore {
            id: "c".into(),
            user_id: 7,
            what: "他把我私下说的话转给了别人".into(),
            weight: Weight::Deep,
            since: now - chrono::Duration::days(2),
            mended: None,
            venue: "private".into(),
            who: None,
        };
        let carried = carried_section(&[sores[1].clone(), deep.clone()], now).unwrap();
        assert!(carried.contains("it really hurt"));
        assert!(!carried.contains("转给了别人"));
        assert!(carried.contains("never said here"));
        assert_eq!(
            carried.matches("- something they did in private").count(),
            1
        );
        assert_eq!(
            carried_section(&[sores[0].clone(), sores[1].clone()], now),
            None
        );
        assert_eq!(Weight::parse("deep"), Some(Weight::Deep));
        assert!(IN_REFLECTION.contains("deep: it really hurt"));
        let input = as_input(&sores, now);
        assert_eq!(input[1]["i"], 1);
        assert_eq!(input[1]["who"], "阿明");
        assert!(input[0].get("who").is_none());
        assert!(IN_GROUP_REFLECTION.contains("the one you just answered"));
        assert!(AT_NIGHT_GROUP.contains("who did it is given"));
        assert_eq!(input[1]["mended"], Value::Null);
        assert!(IN_REFLECTION.contains("not teasing you both enjoy"));
        assert!(AT_NIGHT.contains("Letting go is not forgetting"));
    }
}
