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
//! Days are written as a fact's is (`store::as_known`): month and day, the
//! year only when it is not this one.

use chrono::DateTime;
use myriad_nodes::stands;
use serde_json::Value;

use crate::services::agent::merope::clock::Zone;

/// Since when one thing stands, as its line says it: the day it began
/// (`since`: a day, month or year, or a moment) and the day she was told
/// (`learned`, the latest day she learned a memory it rests on), the second
/// alone when they are one day, it has no day of its own, or its own is
/// after the day she was told (nothing is told before it began: a row
/// built later than what it rests on); with nothing it
/// rests on still held, its own day; only with neither, the day the row was
/// written (`at`). `year`: this one, left out of a whole day in it.
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
        (Some(began), None) => format!("since {}", short(&began, year)),
        (None, None) => format!("since {}", short(&day_of(at), year)),
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

/// One ended thing, as a line: what ended it and since when.
pub(super) fn ended(text: &str, dated: &str) -> String {
    format!("ended, {dated}: {text}")
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
        // Nothing it rests on still held: its own day; with neither, the
        // day it was written.
        assert_eq!(
            dated(Some(&json!("2026-08")), None, written, &zone, 2026),
            "since 2026-08"
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
            ended("搬到上海了", "since 10-18"),
            "ended, since 10-18: 搬到上海了"
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
            ended("x", "since 2025"),
            resident("a", "b", "since 2025"),
        ] {
            assert!(!line.starts_with('[') && !line.contains('('), "{line}");
        }
    }
}
