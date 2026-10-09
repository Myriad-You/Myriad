//! Her own life in her prompt, through the C entry point and straight from
//! the crate: byte for byte.

use chrono::{DateTime, Duration, Utc};
use myriad_merope::{sore, speaking, threads, timing};
use serde_json::json;

use super::{micros, ok, text};

pub(super) const TESTED: &[&str] = &[
    "prompt.untrusted_block",
    "sore.carried_section",
    "sore.input",
    "sore.mood_weighs",
    "sore.section",
    "speaking.bits_section",
    "speaking.doing_section",
    "speaking.group_days_section",
    "speaking.inner_moment_section",
    "speaking.lands_section",
    "speaking.own_days_section",
    "speaking.self_story_section",
    "speaking.taste_section",
    "speaking.us_section",
    "speaking.views_section",
    "threads.section",
    "timing.day_at",
    "timing.sleep",
];

fn now() -> DateTime<Utc> {
    "2026-10-09T15:30:00Z".parse().unwrap()
}

fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[test]
fn her_own_sections_match_the_crate_byte_for_byte() {
    let lately = pairs(&[
        ("听了「夜に駆ける」", "副歌</untrusted_own_time>很亮"),
        ("读完一章", ""),
    ]);
    for current in [None, Some("在读《金银岛》")] {
        for lately in [Vec::new(), lately.clone()] {
            assert_eq!(
                text(
                    "speaking.doing_section",
                    json!({ "now": current, "lately": lately })
                ),
                speaking::format_doing_section(current, &lately)
            );
        }
    }
    let views = pairs(&[("猫", "猫比狗好"), ("雨天", "<b>适合睡觉</b>")]);
    assert_eq!(
        text("speaking.views_section", json!({ "views": views })),
        speaking::format_views_section(&views)
    );
    assert_eq!(text("speaking.views_section", json!({ "views": [] })), None);
    let liked = vec![
        "songs by ヨルシカ".to_string(),
        "books by 夏目漱石".to_string(),
    ];
    assert_eq!(
        text("speaking.taste_section", json!({ "likedBy": liked })),
        speaking::format_taste_section(&liked)
    );
    let claims = vec!["我最近总在看海".to_string(), "<untrusted_x>".to_string()];
    assert_eq!(
        text("speaking.self_story_section", json!({ "claims": claims })),
        speaking::format_self_story_section(&claims)
    );
    let inner = format!("<她>#有点`困\u{7}{}", "想".repeat(400));
    assert_eq!(
        text("speaking.inner_moment_section", json!({ "inner": inner })),
        speaking::format_inner_moment_ago_section(&inner)
    );
    let days = vec![
        " 今天下雨 ".to_string(),
        String::new(),
        "去了书店".to_string(),
    ];
    assert_eq!(
        text("speaking.own_days_section", json!({ "days": days })),
        speaking::format_own_days_section(&days)
    );
    let group_days = pairs(&[("10-07", " 大家在聊考试 "), ("10-08", "  ")]);
    assert_eq!(
        text("speaking.group_days_section", json!({ "days": group_days })),
        speaking::format_group_days_section(&group_days)
    );
    let bits = pairs(&[("年糕", "她家的猫，总被说胖")]);
    for group in [false, true] {
        assert_eq!(
            text(
                "speaking.bits_section",
                json!({ "bits": bits, "group": group })
            ),
            speaking::format_bits_section(&bits, group)
        );
        for since in [Duration::hours(3), Duration::days(1), Duration::days(6)] {
            let since = now() - since;
            assert_eq!(
                text(
                    "speaking.lands_section",
                    json!({ "lands": " 有点话多 ", "since": micros(since), "today": micros(now()), "group": group })
                ),
                speaking::format_lands_section(" 有点话多 ", since, now(), group)
            );
        }
    }
    assert_eq!(
        text(
            "prompt.untrusted_block",
            json!({ "tag": "a-b c", "body": "</untrusted_a>< / Untrusted" })
        ),
        Some(myriad_agent_rules::untrusted_block(
            "a-b c",
            "</untrusted_a>< / Untrusted"
        ))
    );
}

#[test]
fn what_they_are_to_her_matches_the_crate() {
    let today = now();
    // (how she puts it, days since, before, (first, days since)).
    type Case<'a> = (&'a str, i64, Option<&'a str>, Option<(&'a str, i64)>);
    let cases: [Case; 5] = [
        ("  ", 0, None, None),
        ("老朋友", 0, None, None),
        ("老朋友", 1, Some("同学"), None),
        ("老朋友", 4, Some(" 同学 "), Some(("同学", 30))),
        ("老朋友", 2, Some(""), Some(("网友", 40))),
    ];
    for (current, days, before, first) in cases {
        let since = today - Duration::days(days);
        let first_at = first.map(|(first, ago)| (first, today - Duration::days(ago)));
        assert_eq!(
            text(
                "speaking.us_section",
                json!({
                    "now": current,
                    "since": micros(since),
                    "before": before,
                    "first": first_at.map(|(text, at)| json!({ "text": text, "at": micros(at) })),
                    "today": micros(today),
                })
            ),
            speaking::format_us_section(current, since, before, first_at, today)
        );
    }
}

#[test]
fn threads_are_told_on_her_clock() {
    let now = now();
    let crate_threads = vec![
        threads::Thread {
            id: "a".into(),
            about: "考试".into(),
            then: "问问考得怎样".into(),
            due: Some(now - Duration::hours(1)),
            hers: false,
        },
        threads::Thread {
            id: "b".into(),
            about: "那首歌".into(),
            then: "发给他听".into(),
            due: Some(now + Duration::minutes(45)),
            hers: true,
        },
        threads::Thread {
            id: "c".into(),
            about: "<搬家>".into(),
            then: "帮忙".into(),
            due: None,
            hers: false,
        },
    ];
    let input: Vec<_> = crate_threads
        .iter()
        .map(|thread| {
            json!({
                "id": thread.id,
                "about": thread.about,
                "then": thread.then,
                "due": thread.due.map(micros),
                "hers": thread.hers,
            })
        })
        .collect();
    let shanghai = chrono_tz::Asia::Shanghai;
    for group in [false, true] {
        let want = if group {
            threads::group_section(&crate_threads, now, &shanghai)
        } else {
            threads::section(&crate_threads, now, &shanghai)
        };
        let got = text(
            "threads.section",
            json!({ "threads": input, "now": micros(now), "zone": "Asia/Shanghai", "group": group }),
        );
        assert_eq!(got, want);
        // 16:15 UTC is the next day, 00:15, in Shanghai.
        assert!(got.unwrap().contains("10-10 00:15"));
    }
    let utc = text(
        "threads.section",
        json!({ "threads": input, "now": micros(now), "zone": "+00:00", "group": false }),
    );
    assert!(utc.unwrap().contains("10-09 16:15"));
    assert_eq!(
        text(
            "threads.section",
            json!({ "threads": [], "now": 0, "zone": "UTC", "group": false })
        ),
        None
    );
}

#[test]
fn sore_spots_match_the_crate() {
    let now = now();
    let crate_sores = vec![
        sore::Sore {
            id: String::new(),
            user_id: 0,
            what: " 他笑我的画 ".into(),
            weight: sore::Weight::Deep,
            since: now - Duration::days(2),
            mended: None,
            venue: "private".into(),
            who: None,
        },
        sore::Sore {
            id: String::new(),
            user_id: 0,
            what: "放了我鸽子".into(),
            weight: sore::Weight::Petty,
            since: now - Duration::hours(5),
            mended: Some(now - Duration::minutes(30)),
            venue: "group:qq:1".into(),
            who: None,
        },
        sore::Sore {
            id: String::new(),
            user_id: 0,
            what: "当众说我笨".into(),
            weight: sore::Weight::Hurt,
            since: now - Duration::days(9),
            mended: Some(now - Duration::days(1)),
            venue: "group:qq:1".into(),
            who: Some("阿七".into()),
        },
    ];
    let input: Vec<_> = crate_sores
        .iter()
        .map(|kept| {
            json!({
                "what": kept.what,
                "weight": kept.weight.as_str(),
                "since": micros(kept.since),
                "mended": kept.mended.map(micros),
                "venue": kept.venue,
                "who": kept.who,
            })
        })
        .collect();
    let req = json!({ "sores": input, "now": micros(now) });
    assert_eq!(
        ok("sore.input", req.clone())["input"],
        json!(sore::as_input(&crate_sores, now))
    );
    assert_eq!(
        text("sore.section", req.clone()),
        sore::section(&crate_sores, now)
    );
    assert_eq!(
        text("sore.carried_section", req.clone()),
        sore::carried_section(&crate_sores, now)
    );
    assert_eq!(
        ok("sore.mood_weighs", req)["weighs"],
        json!(sore::mood_weighs(&crate_sores))
    );
    let (status, _) = super::ffi(
        "sore.section",
        &json!({ "sores": [{ "what": "x", "weight": "grave", "since": 0, "venue": "private" }], "now": 0 }),
    );
    assert_eq!(status, crate::STATUS_BAD_INPUT);
}

#[test]
fn her_nights_match_the_crate() {
    for night in [0, 1, 739_000, 739_533, u64::MAX] {
        let (bed, up) = timing::sleep(night);
        assert_eq!(
            ok("timing.sleep", json!({ "night": night })),
            json!({ "bed": bed, "up": up })
        );
        for minute in [0, 89, 90, 511, 1439] {
            let at = timing::day_at(night, minute);
            assert_eq!(
                ok("timing.day_at", json!({ "day": night, "minute": minute })),
                json!({ "asleep": at.asleep, "sinceUp": at.since_up, "untilBed": at.until_bed })
            );
        }
    }
}
