//! Since when a thing on a node stands or ended, as she reads it on its
//! line, and the lines themselves.
//!
//! Two days can belong to one row of `node.standing`: when what it tells of
//! began (the row's own `since`, which the core writes as the day it
//! happened when that is not after the day she was told:
//! `myriad_nodes::stands::since_of`), and when they told her (the day she
//! learned the memories it rests on). Every other line she reads is dated
//! by the day it was said, and which of two lines is the later is told by
//! that day. So a line here always carries the day it was told, and the
//! day it began beside it only when that is another: told in November that
//! they left in August, it reads `since 2026-08, told 11-10`, and a fact of
//! September is plainly the earlier word.
//!
//! What stood on a node and has ended (a `node.standing` item whose latest
//! version is `ended`) keeps the fact that ended it as its words (the core's
//! `stands::decide`). Left unsaid, the node's older entries (`[08-21] 每周三
//! 晚上夜跑`) read as how things are; said first, with its day, they read as
//! what was before it ([`ended_lines`]). Only what she concluded is said: a
//! rule's item is what a device showed for a while, and its ending is
//! nothing about their life. A memory they had her forget takes the rows
//! resting on it with it (`super::forget`); one they took back or let go
//! keeps its rows, so it is the caller's `said` that says nothing for an
//! item resting on one, and it is left out.
//!
//! Days are written as a fact's is (`store::as_known`): month and day, the
//! year only when it is not this one.

use chrono::DateTime;
use myriad_nodes::stands;
use serde_json::Value;

use super::shown::Node;
use crate::services::agent::merope::clock::Zone;

/// Since when one thing stands, as its line says it: the day it began
/// (`since`: a day, month or year, or a moment) and the day she was told
/// (`learned`, the latest day she learned a memory it rests on), the second
/// alone when they are one day, it has no day of its own, or its own is
/// after the day she was told (nothing is told before it began: a row
/// built later than what it rests on). With nothing it rests on to say
/// when it was told (it names no memory, or none is kept any more), the
/// day its row was written (`at`) stands for the day told: the latest it
/// can have been. `year`: this one, left out of a whole day in it.
pub(super) fn dated(
    since: Option<&Value>,
    learned: Option<i64>,
    at: i64,
    zone: &Zone,
    year: i32,
) -> String {
    let day_of = |micros: i64| {
        DateTime::from_timestamp_micros(micros)
            .unwrap_or_default()
            .with_timezone(zone)
            .format("%Y-%m-%d")
            .to_string()
    };
    let began = since
        .and_then(|since| since.as_str())
        .map(str::trim)
        .filter(|day| stands::is_day(day))
        .map(str::to_string)
        .or_else(|| since.and_then(|since| since.as_i64()).map(day_of));
    let told = learned.map(day_of);
    match (began, told) {
        (Some(began), Some(told)) if began.as_str() < told.as_str() => {
            format!("since {}, told {}", short(&began, year), short(&told, year))
        }
        (_, Some(told)) => format!("since {}", short(&told, year)),
        // Nothing it rests on to say when it was told: no later than the
        // day its row was written.
        (began, None) => {
            let written = day_of(at);
            match began {
                Some(began) if began.as_str() < written.as_str() => {
                    format!(
                        "since {}, told {}",
                        short(&began, year),
                        short(&written, year)
                    )
                }
                _ => format!("since {}", short(&written, year)),
            }
        }
    }
}

/// A whole day of `year` without it, as a fact's day is written.
fn short(day: &str, year: i32) -> &str {
    match day.strip_prefix(&format!("{year}-")) {
        Some(rest) if day.len() == 10 => rest,
        _ => day,
    }
}

/// One thing that stands, as a line: what it is and since when (`dated`),
/// not a record of a day. Nothing in parentheses: beside a fact that is how
/// she says she came by it some other way than being told
/// (`store::as_known`), and one she was told is not to read as that. What a
/// rule wrote from what a device told says so, for it is only seen.
pub(super) fn now(text: &str, dated: &str, by_rule: bool) -> String {
    if by_rule {
        format!("now, {dated}, as their computer told it: {text}")
    } else {
        format!("now, {dated}: {text}")
    }
}

/// One ended thing, as a line: what no longer stands (`was`, the words it
/// stood in before it ended), since when, and the fact that ended it
/// (`text`, which is all the head keeps of it). Said so, the fact is not
/// read as the thing that ended (`ended: 搬到上海了` reads as the move
/// being over), nor the ending as the whole node's, beside what still
/// stands there. Its earlier words not kept (their row forgotten): only
/// that it is no longer as it stood.
pub(super) fn ended(text: &str, dated: &str, was: Option<&str>) -> String {
    match was {
        Some(was) => format!("no longer: {was}, {dated}: {text}"),
        None => format!("no longer as it stood here, {dated}: {text}"),
    }
}

/// One item of how their life stands, under its node's `name` when it has
/// one.
pub(super) fn resident(name: &str, text: &str, dated: &str) -> String {
    if name.is_empty() {
        format!("{dated}：{text}")
    } else {
        format!("{name}, {dated}：{text}")
    }
}

/// How many ended things of one node are said: the latest.
pub(super) const MOST_ENDED: usize = 2;

/// The ended things on `node`, newest first, at most `most`: the words of
/// each and its line, with since when and what stood before it (`said`, by
/// item; one with none is left out).
pub(super) fn ended_lines(
    node: &Node,
    said: impl Fn(&str) -> Option<(String, Option<String>)>,
    most: usize,
) -> Vec<(String, String)> {
    let Some(standing) = node.head.get("standing").and_then(|all| all.as_object()) else {
        return Vec::new();
    };
    let mut over: Vec<(i64, &str, &str)> = standing
        .iter()
        .filter(|(item, held)| {
            stands::whose_of(item).is_some()
                && held.get("author").and_then(|author| author.as_str()) != Some("rule")
                && held["value"]["ended"].as_bool() == Some(true)
        })
        .filter_map(|(item, held)| {
            let text = held["value"]["text"].as_str()?.trim();
            (!text.is_empty()).then_some((held.get("at")?.as_i64()?, item.as_str(), text))
        })
        .collect();
    over.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(b.1)));
    over.into_iter()
        .filter_map(|(_, item, text)| {
            let (dated, was) = said(item)?;
            Some((text.to_string(), ended(text, &dated, was.as_deref())))
        })
        .take(most)
        .collect()
}

#[cfg(test)]
mod tests {
    use chrono::TimeZone;
    use serde_json::json;

    use super::*;
    use crate::services::agent::merope::clock;

    fn micros(zone: &Zone, month: u32, day: u32) -> i64 {
        zone.with_ymd_and_hms(2026, month, day, 12, 0, 0)
            .unwrap()
            .timestamp_micros()
    }

    /// A line is as new as the day it was told, whenever its row was
    /// written and whenever what it tells of began: told in November that
    /// they left in August, the line says both, and the day told is the
    /// one a fact of September is older than.
    #[test]
    fn what_stands_carries_the_day_it_was_told() {
        let zone = clock::zone();
        let (told, written) = (micros(&zone, 11, 10), micros(&zone, 11, 16));
        // Began before it was told: both days, the day told last.
        for (began, shown) in [
            ("2026-08-11", "since 08-11, told 11-10"),
            ("2026-08", "since 2026-08, told 11-10"),
            ("2025", "since 2025, told 11-10"),
        ] {
            let dated = dated(Some(&json!(began)), Some(told), written, &zone, 2026);
            assert_eq!(dated, shown);
        }
        let began = json!(micros(&zone, 8, 3));
        assert_eq!(
            dated(Some(&began), Some(told), written, &zone, 2026),
            "since 08-03, told 11-10"
        );
        // One day, none of its own, one that is no day, or one after the
        // day told: the day told.
        for since in [
            Some(json!("2026-11-10")),
            Some(json!("2026-11-16")),
            None,
            Some(json!("lately")),
            Some(json!(null)),
        ] {
            assert_eq!(
                dated(since.as_ref(), Some(told), written, &zone, 2026),
                "since 11-10"
            );
        }
        // Nothing it rests on to say when: told no later than the day it
        // was written, which the line carries as every line does.
        assert_eq!(
            dated(Some(&json!("2026-08")), None, written, &zone, 2026),
            "since 2026-08, told 11-16"
        );
        assert_eq!(
            dated(Some(&json!("2026-12-01")), None, written, &zone, 2026),
            "since 11-16"
        );
        assert_eq!(dated(None, None, written, &zone, 2026), "since 11-16");
        // Another year is written out.
        assert_eq!(
            dated(None, Some(told), written, &zone, 2027),
            "since 2026-11-10"
        );
    }

    /// The lines say what stands and since when; none is a dated record,
    /// and none has a parenthesis.
    #[test]
    fn the_lines_are_no_dated_records() {
        let line = now("每晚练琴一小时", "since 08-11", false);
        assert_eq!(line, "now, since 08-11: 每晚练琴一小时");
        assert!(now("在写报告", "since 11-16", true).contains("their computer"));
        assert_eq!(
            ended("搬到上海了", "since 10-18", Some("住在北京朝阳")),
            "no longer: 住在北京朝阳, since 10-18: 搬到上海了"
        );
        assert_eq!(
            resident("工作", "在趣养做运营", "since 09-01"),
            "工作, since 09-01：在趣养做运营"
        );
        assert_eq!(
            resident("", "在趣养做运营", "since 09-01"),
            "since 09-01：在趣养做运营"
        );
        for line in [
            line,
            ended("x", "since 2025", None),
            ended("x", "since 2025", Some("y")),
            resident("a", "b", "since 2025"),
        ] {
            assert!(!line.starts_with('[') && !line.contains('('), "{line}");
        }
    }

    fn node(standing: serde_json::Value) -> Node {
        Node {
            id: "n".into(),
            kind: Some("topic".into()),
            name: Some("夜跑".into()),
            status: None,
            gone: false,
            head: json!({ "standing": standing }),
            facts: Vec::new(),
            heard: Vec::new(),
            latest: 0,
        }
    }

    /// What ended on a node is said, newest first, with since when; what
    /// still stands, a rule's item and one with no words are not, and no
    /// more than asked.
    #[test]
    fn what_ended_is_said_with_since_when() {
        let item = |n: usize| stands::item_id("them", "n", "m", n);
        let node = node(json!({
            (item(0)): {"at": 30, "author": "her",
                "value": {"text": "膝盖伤了，不夜跑了", "because": [], "ended": true}},
            (item(1)): {"at": 20, "author": "her",
                "value": {"text": "每周三晚上夜跑", "because": []}},
            (item(2)): {"at": 10, "author": "her",
                "value": {"text": "健身卡到期，没续", "because": [], "ended": true}},
            (item(3)): {"at": 40, "author": "her",
                "value": {"text": " ", "because": [], "ended": true}},
            "now": {"at": 50, "author": "rule",
                "value": {"text": "在写报告", "because": [], "ended": true}},
        }));
        // Of the first, the words it stood in are known; of the other, not.
        let first = item(0);
        let since = |item: &str| {
            let was = (item == first).then(|| "每周三晚上夜跑".to_string());
            Some(("since 10-18".to_string(), was))
        };
        let all = ended_lines(&node, since, MOST_ENDED);
        assert_eq!(all.len(), 2, "{all:?}");
        assert_eq!(all[0].0, "膝盖伤了，不夜跑了");
        assert_eq!(
            all[0].1,
            "no longer: 每周三晚上夜跑, since 10-18: 膝盖伤了，不夜跑了"
        );
        assert_eq!(
            all[1].1,
            "no longer as it stood here, since 10-18: 健身卡到期，没续"
        );
        assert_eq!(all[1].0, "健身卡到期，没续");
        assert_eq!(ended_lines(&node, since, 1).len(), 1);
        // No day to say: not said.
        assert!(ended_lines(&node, |_| None, MOST_ENDED).is_empty());
    }
}
