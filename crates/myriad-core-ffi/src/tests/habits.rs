//! How she and they type, and what in her day is worth raising: through
//! the C entry point and straight from the crate.

use std::collections::BTreeMap;

use myriad_merope::{contrast, style, vitals};
use serde_json::{Value, json};

use super::{bits, ok, text};

pub(super) const TESTED: &[&str] = &[
    "contrast.describe",
    "contrast.overused",
    "contrast.pieces",
    "style.noticed",
    "style.unlike",
    "vitals.alerts",
];

fn overused_json(found: &[contrast::Overused]) -> Value {
    json!(
        found
            .iter()
            .map(|item| json!({
                "piece": item.piece,
                "hers": item.hers,
                "herMessages": item.her_messages,
                "theirs": item.theirs,
                "theirMessages": item.their_messages,
                "z": item.z,
            }))
            .collect::<Vec<_>>()
    )
}

#[test]
fn her_lines_beside_theirs_match_the_crate() {
    for message in [
        "",
        " [图片] ",
        "哈",
        "哈哈哈哈",
        "好的呀～",
        "  嗯嗯，明天见！ ",
    ] {
        assert_eq!(
            ok("contrast.pieces", json!({ "message": message }))["pieces"],
            json!(contrast::pieces(message)),
            "{message}"
        );
    }
    let hers: Vec<String> = (0..12)
        .map(|i| {
            if i % 3 == 0 {
                format!("哈哈{i}～")
            } else {
                format!("好呀{i}～")
            }
        })
        .collect();
    let theirs: Vec<String> = (0..40).map(|i| format!("收到{i}。")).collect();
    let hers = contrast::Counts::of(hers.iter().map(String::as_str));
    let theirs = contrast::Counts::of(theirs.iter().map(String::as_str));
    let want = contrast::overused(&hers, &theirs);
    assert!(!want.is_empty());
    let got = ok(
        "contrast.overused",
        json!({ "hers": hers, "theirs": theirs }),
    );
    assert_eq!(got["overused"], overused_json(&want));
    for (got, want) in got["overused"].as_array().unwrap().iter().zip(&want) {
        assert_eq!(bits(&got["z"]), want.z.to_bits());
    }
    assert_eq!(
        text("contrast.describe", json!({ "overused": got["overused"] })),
        contrast::describe(&want)
    );
    assert_eq!(text("contrast.describe", json!({ "overused": [] })), None);
}

#[test]
fn how_unlike_them_matches_the_crate() {
    let history: Vec<String> = (0..140)
        .map(|i| match i % 4 {
            0 => format!("好的{i}"),
            1 => format!("哈哈哈 那就这样吧{i}"),
            2 => format!("明天{i}点见？"),
            _ => format!("嗯嗯 收到{i}～"),
        })
        .collect();
    let recent = vec![
        "Dear Sir, I am writing to inform you of an urgent matter.".to_string(),
        "Please transfer the funds at your earliest convenience.".to_string(),
    ];
    let want = style::unlike(&history, &recent);
    let got = ok(
        "style.unlike",
        json!({ "history": history, "recent": recent }),
    );
    // Gram weights are summed in hash order: equal to rounding.
    let (got, want) = (got["z"].as_f64().unwrap(), want.unwrap());
    assert!((got - want).abs() <= want.abs() * 1e-12, "{got} {want}");
    assert_eq!(
        ok(
            "style.unlike",
            json!({ "history": ["好"], "recent": ["好"] })
        )["z"],
        Value::Null
    );
    assert_eq!(
        ok("style.noticed", json!({})),
        json!({
            "knownAfter": style::KNOWN_AFTER,
            "tellsAfter": style::TELLS_AFTER,
            "noticedAt": style::NOTICED_AT,
            "text": style::NOTICED,
        })
    );
}

fn day(name: &str, calls: u64, kept: &[(&str, u64)]) -> vitals::Day {
    vitals::Day {
        day: name.into(),
        calls,
        failed_calls: calls / 5,
        kept: kept
            .iter()
            .map(|(source, count)| (source.to_string(), *count))
            .collect::<BTreeMap<_, _>>(),
        unreadable: u64::from(name.ends_with('9')),
        notes_lean_on: vec![("其实".into(), 0.42)],
        replies_lean_on: vec![("哈哈".into(), 0.2)],
        openers_lean_on: vec![("你回来啦".into(), 0.5)],
        replies_asking: Some(0.75),
        reply_p90: Some(41.6),
        proactive: 3,
        proactive_answered: 0,
        learned: Some(0),
        ..vitals::Day::default()
    }
}

#[test]
fn alerts_match_the_crate() {
    let before = vec![
        day("2026-10-06", 30, &[("diary", 2), ("own", 1)]),
        day("2026-10-07", 25, &[("diary", 1), ("own", 3)]),
        day("2026-10-08", 28, &[("diary", 4), ("own", 1)]),
    ];
    let today = day("2026-10-09", 90, &[("diary", 1)]);
    let want = vitals::alerts(&today, &before);
    assert!(want.len() > 5);
    let got = ok("vitals.alerts", json!({ "today": today, "before": before }));
    assert_eq!(got["alerts"], json!(want));
}
