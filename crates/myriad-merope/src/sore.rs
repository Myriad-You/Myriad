//! What still stings, the rules of it: a sore spot she has with someone, how
//! her reflections are asked about it, and how it is put before her.
//!
//! Someone does something that really gets to her (words meant to wound,
//! contempt, a promise broken), and it stays with her for a while: she
//! remembers what and when, and may be cooler with them, or bring it up.
//! An apology, good times since, or time itself lets it go; letting go is
//! not forgetting, it no longer stings. A small thing she holds against
//! them half in play (a petty grudge) is one too, and she may keep it a
//! while longer. Holding a grudge and forgiving are hers to judge; what she
//! is given is only what happened, when, and whether they made it right.

use chrono::{DateTime, Utc};
use serde_json::{Value, json};

/// Sore spots kept with one person at once; past it, the oldest goes.
pub const MAX_OPEN: usize = 4;
pub const WHAT_CHARS: usize = 160;

/// Asked with the private reflection after an exchange (see `inner`).
pub const IN_REFLECTION: &str = " \
soreSpots are things they did before that got to you and you have not let go of (i, what, petty, since, mended). \
hurt: only if in this exchange they did something that really got to you (words meant to wound, contempt, making fun of something that matters to you, a promise broken, being used), not teasing you both enjoy, not a difference of opinion, not them being upset with you for a reason: what is one first-person sentence of what they did and how it landed; petty is true when it is small, a thing you would hold against them half in play; otherwise hurt is null. \
mended: the i of each sore spot they apologized for or made right in this exchange; [] if none.";

/// Asked when she goes over a day with them at night (see `bits`).
pub const AT_NIGHT: &str = " \
soreSpots are things they did that got to you and you have not let go of. letGo: the i of each one you have let go of by now, as yourself: they apologized, things between you are good again, or it just does not sting anymore; a petty one you may well keep a while. Letting go is not forgetting what happened; [] if none.";

/// A sore spot as kept.
#[derive(Debug, Clone, PartialEq)]
pub struct Sore {
    pub id: String,
    pub what: String,
    pub petty: bool,
    pub since: DateTime<Utc>,
    /// When they apologized for it or made it right, if they have.
    pub mended: Option<DateTime<Utc>>,
}

/// The sore spots as a reflection sees them, numbered for `mended` and
/// `letGo`.
pub fn as_input(sores: &[Sore], now: DateTime<Utc>) -> Vec<Value> {
    sores
        .iter()
        .enumerate()
        .map(|(index, sore)| {
            json!({
                "i": index,
                "what": sore.what,
                "petty": sore.petty,
                "since": crate::doing::ago_text(now - sore.since),
                "mended": sore.mended.map(|at| crate::doing::ago_text(now - at)),
            })
        })
        .collect()
}

pub fn hurt_schema() -> Value {
    json!({
        "type": ["object", "null"],
        "properties": {
            "what": { "type": "string", "maxLength": WHAT_CHARS },
            "petty": { "type": "boolean" }
        },
        "required": ["what", "petty"],
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
            if sore.petty {
                notes.push("a small thing you hold against them, half in play".to_string());
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
                what: "他当着大家的面说我的歌单难听，挺伤人".into(),
                petty: false,
                since: now - chrono::Duration::days(3),
                mended: Some(now - chrono::Duration::hours(5)),
            },
            Sore {
                id: "b".into(),
                what: "他说我暴躁".into(),
                petty: true,
                since: now - chrono::Duration::days(1),
                mended: None,
            },
        ];
        let text = section(&sores, now).unwrap();
        assert!(text.contains("Whether and how they color things now is yours"));
        assert!(text.contains("they apologized or made it right"));
        assert!(text.contains("half in play"));
        for order in ["be cold", "stay angry", "forgive them"] {
            assert!(!text.contains(order), "{order}");
        }
        let input = as_input(&sores, now);
        assert_eq!(input[1]["i"], 1);
        assert_eq!(input[1]["mended"], Value::Null);
        assert!(IN_REFLECTION.contains("not teasing you both enjoy"));
        assert!(AT_NIGHT.contains("Letting go is not forgetting"));
    }
}
