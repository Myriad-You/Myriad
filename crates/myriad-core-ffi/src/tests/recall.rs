//! Recall's arithmetic, what a chat line keeps, and her hours on her clock:
//! through the C entry point and straight from the crates.

use chrono::{DateTime, Duration, Utc};
use myriad_agent_rules::{
    Concept,
    memory::{association, lexical, recall, strength},
};
use myriad_merope::{
    chat_history::{self, HistoryMessage},
    diary, timing,
};
use serde_json::{Value, json};

use super::{bits, micros, ok, text};

pub(super) const TESTED: &[&str] = &[
    "chat.history_lines",
    "chat.history_text",
    "chat.pause_text",
    "chat.safe_content",
    "diary.compact_summary",
    "diary.redact",
    "diary.should_write_chat",
    "memory.base_level",
    "memory.gap_in",
    "memory.of_row",
    "memory.overlap",
    "memory.readiness",
    "memory.score_all",
    "memory.spread",
    "timing.asleep_at",
    "timing.her_date_at",
    "timing.her_day_at",
    "timing.past_bedtime_at",
];

fn concept(name: &str, aliases: &[&str]) -> Concept {
    Concept {
        name: name.into(),
        aliases: aliases.iter().map(|alias| alias.to_string()).collect(),
    }
}

fn memories() -> Vec<(String, Vec<Concept>)> {
    vec![
        (
            "她家的猫叫年糕，很胖".into(),
            vec![concept("猫", &["cat", "喵"])],
        ),
        (
            "They went biking along the river".into(),
            vec![concept("骑车", &["bike"])],
        ),
        ("今天天气不错，去了公园".into(), vec![]),
        (
            "年糕又偷吃了".into(),
            vec![concept("猫", &[]), concept("年糕", &[])],
        ),
        ("下周考雅思".into(), vec![concept("雅思", &["IELTS"])]),
    ]
}

fn memories_json(memories: &[(String, Vec<Concept>)]) -> Value {
    json!(
        memories
            .iter()
            .map(|(text, concepts)| json!({ "text": text, "concepts": concepts }))
            .collect::<Vec<_>>()
    )
}

#[test]
fn recall_scores_match_the_crate_bit_for_bit() {
    let rows = memories();
    let documents: Vec<lexical::Document> = rows
        .iter()
        .map(|(text, concepts)| lexical::Document { text, concepts })
        .collect();
    for query in ["喵最近怎么样", "bikes", "天气", "雅思考得如何", "", "IELTS"] {
        let want = lexical::score_all(query, &documents);
        let got = ok(
            "memory.score_all",
            json!({ "query": query, "memories": memories_json(&rows) }),
        );
        let got = got["scores"].as_array().unwrap();
        assert_eq!(got.len(), want.len());
        for (got, want) in got.iter().zip(&want) {
            assert_eq!(got["strong"], want.strong, "{query}");
            assert_eq!(bits(&got["value"]), want.value.to_bits(), "{query}");
        }
        let gap = recall::gap_in(&documents, query);
        assert_eq!(
            ok(
                "memory.gap_in",
                json!({ "memories": memories_json(&rows), "query": query })
            )["gap"],
            gap.map_or(
                Value::Null,
                |(name, known)| json!({ "name": name, "known": known })
            ),
            "{query}"
        );
    }
    for (a, b) in [
        ("今天天气不错", "天气真不错"),
        ("cooking dinner", "cooked dinners"),
        ("", "x"),
    ] {
        let got = ok("memory.overlap", json!({ "a": a, "b": b }));
        assert_eq!(bits(&got["overlap"]), lexical::overlap(a, b).to_bits());
    }
}

#[test]
fn strength_and_spreading_match_the_crate() {
    for (uses, lifetime, since) in [
        (1, 30.0, 30.0),
        (3, 86_400.0, 3_600.0),
        (40, 3.0e7, 9.0e6),
        (2, 100.0, 99.5),
    ] {
        let base = strength::base_level(uses, lifetime, since);
        let got = ok(
            "memory.base_level",
            json!({ "uses": uses, "lifetime": lifetime, "since": since }),
        );
        assert_eq!(bits(&got["base"]), base.to_bits());
        let got = ok("memory.readiness", json!({ "base": base }));
        assert_eq!(bits(&got["readiness"]), strength::readiness(base).to_bits());
    }
    let now: DateTime<Utc> = "2026-10-09T08:00:00Z".parse().unwrap();
    for (made, count, last) in [
        (Duration::days(3), 0, None),
        (Duration::days(400), 12, Some(Duration::hours(5))),
        (Duration::seconds(-30), -4, None),
    ] {
        let want = strength::of_row(
            (now - made).fixed_offset(),
            count,
            last.map(|last| (now - last).fixed_offset()),
            now.fixed_offset(),
        );
        let got = ok(
            "memory.of_row",
            json!({
                "createdAt": micros(now - made),
                "accessCount": count,
                "lastAccessedAt": last.map(|last| micros(now - last)),
                "now": micros(now),
            }),
        );
        assert_eq!(bits(&got["readiness"]), want.to_bits());
    }
    let rows = memories();
    let at = |index: usize| now - Duration::minutes(7 * index as i64);
    let nodes: Vec<association::Node> = rows
        .iter()
        .enumerate()
        .map(|(index, (_, concepts))| association::Node {
            concepts,
            at: at(index).fixed_offset(),
        })
        .collect();
    let seeds = [1.0, 0.0, 0.0, 0.3, 0.0];
    let want = association::spread(&seeds, &nodes);
    let got = ok(
        "memory.spread",
        json!({
            "seeds": seeds,
            "nodes": rows.iter().enumerate().map(|(index, (_, concepts))| json!({ "concepts": concepts, "at": micros(at(index)) })).collect::<Vec<_>>(),
        }),
    );
    let got = got["activation"].as_array().unwrap();
    for (got, want) in got.iter().zip(&want) {
        assert_eq!(bits(got), want.to_bits());
    }
    let (status, _) = super::ffi("memory.spread", &json!({ "seeds": [1.0], "nodes": [] }));
    assert_eq!(status, crate::STATUS_BAD_INPUT);
}

#[test]
fn what_a_chat_line_keeps_matches_the_crate() {
    for (line, age) in [
        (
            "帮我查一下明天的天气 Bearer abc123 https://x.test/a?token=1",
            None,
        ),
        ("好的", Some(5)),
        ("帮我把这份报告整理成表格发给老板", Some(19)),
        ("帮我把这份报告整理成表格发给老板", Some(20)),
        ("{\"json\": true}", None),
    ] {
        assert_eq!(
            text("diary.compact_summary", json!({ "text": line })),
            Some(diary::compact_summary(line))
        );
        assert_eq!(
            text("diary.redact", json!({ "text": line })),
            Some(diary::redact_event_text(line))
        );
        assert_eq!(
            ok(
                "diary.should_write_chat",
                json!({ "text": line, "lastAgeMinutes": age })
            )["write"],
            diary::should_write_chat_diary(line, age)
        );
    }
}

#[test]
fn her_hours_are_read_on_the_zone_given() {
    let shanghai = chrono_tz::Asia::Shanghai;
    let new_york = chrono_tz::America::New_York;
    for hours in [0, 3, 7, 12, 17, 20, 23] {
        let now: DateTime<Utc> =
            "2026-10-08T16:00:00Z".parse::<DateTime<Utc>>().unwrap() + Duration::hours(hours);
        for (name, wants) in [
            (
                "Asia/Shanghai",
                (
                    timing::asleep_at(now, &shanghai),
                    timing::her_day_at(now, &shanghai),
                    timing::her_date_at(now, &shanghai),
                    timing::past_bedtime_at(now, &shanghai),
                ),
            ),
            (
                "America/New_York",
                (
                    timing::asleep_at(now, &new_york),
                    timing::her_day_at(now, &new_york),
                    timing::her_date_at(now, &new_york),
                    timing::past_bedtime_at(now, &new_york),
                ),
            ),
        ] {
            let req = json!({ "now": micros(now), "zone": name });
            assert_eq!(
                ok("timing.asleep_at", req.clone())["seconds"],
                json!(wants.0)
            );
            assert_eq!(
                ok("timing.her_day_at", req.clone()),
                json!({ "asleep": wants.1.asleep, "sinceUp": wants.1.since_up, "untilBed": wants.1.until_bed })
            );
            assert_eq!(
                ok("timing.her_date_at", req.clone())["date"],
                wants.2.format("%Y-%m-%d").to_string()
            );
            assert_eq!(text("timing.past_bedtime_at", req), wants.3);
        }
    }
}

#[test]
fn the_conversation_she_has_matches_the_crate() {
    let now: DateTime<Utc> = "2026-10-01T09:00:00Z".parse().unwrap();
    let rows: Vec<(&str, String, Option<DateTime<Utc>>)> = vec![
        ("user", "你好呀".into(), Some(now - Duration::hours(62))),
        (
            "assistant",
            "来啦[[wear:校服]]".into(),
            Some(now - Duration::hours(62)),
        ),
        ("assistant", "[输出数据: x]".into(), None),
        (
            "user",
            "我去上课了\n\n晚点聊".into(),
            Some(now - Duration::hours(13)),
        ),
        (
            "assistant",
            "难怪[[music:play]]\n[[sticker:hi]]".into(),
            None,
        ),
        ("user", "早".into(), Some(now)),
    ];
    let history: Vec<HistoryMessage> = rows
        .iter()
        .map(|(role, content, sent_at)| HistoryMessage {
            role,
            content,
            sent_at: *sent_at,
        })
        .collect();
    let input = json!({
        "history": rows.iter().map(|(role, content, sent_at)| json!({ "role": role, "content": content, "sentAt": sent_at.map(micros) })).collect::<Vec<_>>(),
    });
    assert_eq!(
        text("chat.history_text", input.clone()),
        Some(chat_history::chat_history_text(&history))
    );
    let lines: Vec<Value> = chat_history::chat_history_lines(&history)
        .into_iter()
        .map(|line| json!({ "index": line.index, "said": line.said, "pause": line.pause }))
        .collect();
    assert_eq!(ok("chat.history_lines", input)["lines"], json!(lines));
    for seconds in [
        0,
        3 * 3600,
        23 * 3600 + 3599,
        24 * 3600,
        49 * 3600,
        400 * 3600,
    ] {
        assert_eq!(
            text("chat.pause_text", json!({ "seconds": seconds })),
            Some(chat_history::pause_text(seconds))
        );
    }
    for (_, content, _) in &rows {
        assert_eq!(
            text("chat.safe_content", json!({ "content": content })),
            Some(chat_history::chat_safe_content(content))
        );
    }
}
