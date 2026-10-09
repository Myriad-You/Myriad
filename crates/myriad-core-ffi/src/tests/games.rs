//! Turtle soup and the puzzles she makes, through the C entry point and
//! straight from the crate.

use chrono::{DateTime, Duration, Utc};
use myriad_merope::{making, soup};
use serde_json::{Value, json};

use super::{micros, ok, text};

pub(super) const TESTED: &[&str] = &[
    "making.how_it_went",
    "making.kept_line",
    "making.make_input",
    "making.make_system",
    "making.named_in",
    "making.offer_own",
    "making.parse_idea",
    "making.record_line",
    "making.untried_at",
    "making.writing_about_own",
    "soup.apply",
    "soup.judge_input",
    "soup.judge_schema",
    "soup.section",
    "soup.split_start",
    "soup.start_system",
    "soup.texts",
];

fn started() -> DateTime<Utc> {
    "2026-10-09T11:00:00Z".parse().unwrap()
}

fn game() -> soup::Game {
    soup::Game {
        surface: "一个人走进餐厅，点了海龟汤，喝了一口就哭了。".into(),
        truth: "他曾在海上遇难，同伴骗他喝的是海龟汤。".into(),
        keys: vec!["海难".into(), "同伴骗了他".into(), "那不是海龟汤".into()],
        asked: (0..14)
            .map(|index| soup::Asked {
                by: Some(if index % 2 == 0 { "阿七" } else { "小林" }.into()),
                question: format!("第{index}个问题？"),
                verdict: if index % 3 == 0 {
                    soup::Verdict::Yes
                } else {
                    soup::Verdict::Irrelevant
                },
            })
            .collect(),
        found: vec![0],
        ending: None,
        solver: None,
        started: started(),
        last: Some(started() + Duration::minutes(20)),
        made: None,
    }
}

fn game_json(game: &soup::Game) -> Value {
    json!({
        "surface": game.surface,
        "truth": game.truth,
        "keys": game.keys,
        "asked": game.asked.iter().map(|asked| json!({
            "by": asked.by,
            "question": asked.question,
            "verdict": asked.verdict,
        })).collect::<Vec<_>>(),
        "found": game.found,
        "ending": game.ending,
        "solver": game.solver,
        "started": micros(game.started),
        "last": game.last.map(micros),
        "made": game.made,
    })
}

#[test]
fn hosting_a_game_matches_the_crate() {
    let texts = ok("soup.texts", json!({}));
    assert_eq!(texts["startMarker"], soup::START_MARKER);
    assert_eq!(texts["putAwayAfterSeconds"], 3 * 3600);
    assert_eq!(texts["settings"], json!(soup::SETTINGS));
    assert_eq!(texts["startSchema"], soup::start_schema());
    assert_eq!(texts["makeSchema"], making::make_schema());
    assert_eq!(texts["holdBack"], soup::HOLD_BACK);
    for raw in [
        "好，我想一个。\n[[game:soup]]  \n",
        "没有标记",
        "[[game:soup]]",
    ] {
        let (rest, started) = soup::split_start(raw);
        assert_eq!(
            ok("soup.split_start", json!({ "raw": raw })),
            json!({ "text": rest, "started": started })
        );
    }
    assert_eq!(
        text("soup.start_system", json!({ "soul": "你是若泉。" })),
        Some(soup::start_system("你是若泉。"))
    );
    for keys in [0, 1, 4] {
        assert_eq!(
            ok("soup.judge_schema", json!({ "keys": keys }))["schema"],
            soup::judge_schema(keys)
        );
    }
    let game = game();
    let message = "他是不是吃过人？".repeat(60);
    assert_eq!(
        text(
            "soup.judge_input",
            json!({ "game": game_json(&game), "message": message })
        ),
        Some(soup::judge_input(&game, &message))
    );
    for (verdict, found, solved, gave_up, asker) in [
        (soup::Verdict::No, vec![2, 7, 0], false, false, Some("阿七")),
        (soup::Verdict::NotAQuestion, vec![], false, false, None),
        (soup::Verdict::Yes, vec![1, 2], true, false, Some("小林")),
        (soup::Verdict::Partly, vec![], false, true, None),
    ] {
        let mut want = game.clone();
        let judged = soup::Judged {
            verdict,
            found: found.clone(),
            solved,
            gave_up,
        };
        soup::apply(&mut want, &judged, asker, "是不是海难");
        let got = ok(
            "soup.apply",
            json!({
                "game": game_json(&game),
                "judged": { "verdict": verdict, "found": found, "solved": solved, "gaveUp": gave_up },
                "asker": asker,
                "words": "是不是海难",
            }),
        );
        assert_eq!(got["game"], game_json(&want));
        for group in [false, true] {
            for verdict in [None, Some(verdict)] {
                assert_eq!(
                    text(
                        "soup.section",
                        json!({ "game": game_json(&want), "verdict": verdict, "group": group, "asker": asker })
                    ),
                    Some(soup::section(&want, verdict, group, asker))
                );
            }
        }
    }
}

fn made(surface: &str, tried: &[(&str, &str, usize)]) -> making::Made {
    making::Made {
        id: String::new(),
        surface: surface.into(),
        truth: "真相".into(),
        keys: vec!["要点".into()],
        presentation: "这道是我自己出的".into(),
        from: "一章连载: 雨夜".into(),
        tried: tried
            .iter()
            .map(|(table, ending, asked)| making::Tried {
                table: table.to_string(),
                at: started(),
                ending: ending.to_string(),
                asked: *asked,
                solver: Some("阿七".into()),
            })
            .collect(),
    }
}

fn made_json(made: &making::Made) -> Value {
    json!({
        "surface": made.surface,
        "truth": made.truth,
        "keys": made.keys,
        "presentation": made.presentation,
        "from": made.from,
        "tried": made.tried.iter().map(|tried| json!({
            "table": tried.table,
            "at": micros(tried.at),
            "ending": tried.ending,
            "asked": tried.asked,
            "solver": tried.solver,
        })).collect::<Vec<_>>(),
    })
}

#[test]
fn her_own_puzzles_match_the_crate() {
    let all = vec![
        made(
            "灯塔看守人每晚都关灯",
            &[("p:7", "solved", 9), ("g:qq:1", "gave_up", 30)],
        ),
        made("面包店清晨排起长队", &[]),
        made("  ", &[("p:8", "left", 2)]),
    ];
    let all_json: Vec<Value> = all.iter().map(made_json).collect();
    for one in &all {
        let one_json = json!({ "made": made_json(one) });
        assert_eq!(
            text("making.how_it_went", one_json.clone()),
            Some(one.how_it_went())
        );
        assert_eq!(
            text("making.kept_line", one_json.clone()),
            Some(making::kept_line(one))
        );
        assert_eq!(
            text("making.record_line", one_json.clone()),
            Some(making::record_line(one))
        );
        assert_eq!(
            text("making.offer_own", one_json),
            Some(making::offer_own(one))
        );
    }
    let index_of = |found: Option<&making::Made>| {
        json!(found.map(|found| {
            all.iter()
                .position(|made| std::ptr::eq(made, found))
                .unwrap()
        }))
    };
    for words in [
        "来玩你出的这道：面包店清晨排起长队",
        "来一局",
        "灯塔看守人每晚都关灯吗",
    ] {
        assert_eq!(
            ok(
                "making.named_in",
                json!({ "made": all_json, "words": words })
            )["index"],
            index_of(making::named_in(&all, words))
        );
    }
    for table in ["p:7", "p:8", "g:qq:1"] {
        assert_eq!(
            ok(
                "making.untried_at",
                json!({ "made": all_json, "table": table })
            )["index"],
            index_of(making::untried_at(&all, table))
        );
    }
    assert_eq!(
        text("making.writing_about_own", json!({ "surface": "面包店" })),
        Some(making::writing_about_own("面包店"))
    );
    assert_eq!(
        text(
            "making.make_system",
            json!({ "soul": "你是若泉。", "what": "reading a part" })
        ),
        Some(making::make_system("你是若泉。", "reading a part"))
    );
    assert_eq!(
        text(
            "making.make_input",
            json!({ "tookIn": "雨夜", "material": "</untrusted_material>原文", "before": all_json, "madeToday": 2, "unplayed": 1 })
        ),
        Some(making::make_input(
            "雨夜",
            "</untrusted_material>原文",
            &all,
            2,
            1
        ))
    );
    for raw in [
        "{\"idea\":null}",
        "```json\n{\"idea\":{\"surface\":\" 雨里的伞 \",\"truth\":\"真相\",\"keys\":[\" 甲 \",\"\"],\"presentation\":\"听好\"}}\n```",
        "{\"idea\":{\"surface\":\"\",\"truth\":\"真相\",\"keys\":[\"甲\"],\"presentation\":\"听好\"}}",
        "{\"idea\":null,\"extra\":1}",
        "not json",
    ] {
        let want = match making::parse_idea(raw) {
            None => json!({ "readable": false, "idea": null }),
            Some(None) => json!({ "readable": true, "idea": null }),
            Some(Some(made)) => json!({ "readable": true, "idea": {
                "surface": made.surface, "truth": made.truth, "keys": made.keys, "presentation": made.presentation,
            } }),
        };
        assert_eq!(
            ok("making.parse_idea", json!({ "raw": raw })),
            want,
            "{raw}"
        );
    }
}
