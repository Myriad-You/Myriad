//! Following a book, writing first, and changing clothes in chat: through
//! the C entry point and straight from the crate.

use chrono::{DateTime, Duration, Utc};
use myriad_merope::{library, reach, serial, threads};
use serde_json::{Value, json};

use super::{micros, ok, text};

pub(super) const TESTED: &[&str] = &[
    "library.csv_records",
    "library.from_aozora",
    "library.from_gutenberg",
    "outfit.after_reply",
    "outfit.hold_wear",
    "outfit.looks_from_profile",
    "outfit.resolve",
    "outfit.resolve_directive",
    "outfit.split_wear",
    "outfit.wardrobe_section",
    "reach.as_text",
    "reach.judge_system",
    "reach.parse_judged",
    "reach.reason",
    "reach.route",
    "reach.texts",
    "reach.wrote_before",
    "reach.writing_first",
    "serial.advance",
    "serial.clean_aozora",
    "serial.clean_gutenberg",
    "serial.looking_back",
    "serial.out",
    "serial.parts",
    "serial.texts",
    "serial.view",
];

fn now() -> DateTime<Utc> {
    "2026-10-09T16:30:00Z".parse().unwrap()
}

const AOZORA: &str = "吾輩は猫である\n夏目漱石\n\n-------------------------------------------------------\n【テキスト中に現れる記号について】\n《》：ルビ\n-------------------------------------------------------\n\n　吾輩《わがはい》は猫である。名前はまだ無い。\n　どこで生れたかとんと見当《けんとう》がつかぬ。［＃「つかぬ」に傍点］\n\n底本：「夏目漱石全集1」ちくま文庫\n";
const GUTENBERG: &str = "The Project Gutenberg eBook\n*** START OF THE PROJECT GUTENBERG EBOOK THE HOUND ***\n[Illustration]\nCHAPTER I.\n\nMr. Sherlock Holmes.\n*** END OF THE PROJECT GUTENBERG EBOOK THE HOUND ***\nlicense";

#[test]
fn a_book_one_part_a_day_matches_the_crate() {
    let texts = ok("serial.texts", json!({}));
    assert_eq!(texts["how"], serial::HOW);
    assert_eq!(texts["judgeSchema"], serial::judge_schema());
    assert_eq!(texts["catalog"], json!(*serial::CATALOG));
    assert_eq!(
        texts["asks"],
        json!(
            serial::asks()
                .into_iter()
                .map(|(name, schema)| json!([name, schema]))
                .collect::<Vec<_>>()
        )
    );
    assert_eq!(
        text("serial.clean_aozora", json!({ "raw": AOZORA })),
        Some(serial::clean_aozora(AOZORA))
    );
    assert_eq!(
        text("serial.clean_gutenberg", json!({ "raw": GUTENBERG })),
        Some(serial::clean_gutenberg(GUTENBERG))
    );
    let long = "　一つの段落です。".repeat(400) + "\n" + &"　次の段落。\n".repeat(900);
    for lang in ["ja", "en", "zh"] {
        assert_eq!(
            ok("serial.parts", json!({ "text": long, "lang": lang }))["parts"],
            json!(serial::parts(&long, lang))
        );
    }
    let following = serial::Following {
        id: "pg-2852".into(),
        next: 1,
        total: 5,
        started: now() - Duration::hours(17),
        guess: Some("狗是假的".into()),
        knew_it: false,
    };
    let following_json = json!({
        "id": following.id, "next": following.next, "total": following.total,
        "started": micros(following.started), "guess": following.guess, "knewIt": following.knew_it,
    });
    for (zone, want) in [
        (
            "Asia/Shanghai",
            following.out(now(), &chrono_tz::Asia::Shanghai),
        ),
        ("UTC", following.out(now(), &chrono_tz::UTC)),
        (
            "-10:00",
            following.out(now(), &chrono::FixedOffset::west_opt(10 * 3600).unwrap()),
        ),
    ] {
        assert_eq!(
            ok(
                "serial.out",
                json!({ "following": following_json, "now": micros(now()), "zone": zone })
            )["out"],
            want,
            "{zone}"
        );
    }
    for (id, index, go_on, guess) in [
        ("pg-2852", 1, true, Some(" 下一章他会出现 ")),
        ("pg-2852", 4, true, None),
        ("aozora-773", 0, false, Some("  ")),
        ("aozora-773", 0, true, None),
    ] {
        let mut want = vec![following.clone()];
        let ended = serial::advance(
            &mut want,
            serial::Read {
                id,
                index,
                total: 5,
                guess: guess.map(str::to_string),
                go_on,
                knew_it: index == 0,
            },
            now(),
        );
        let got = ok(
            "serial.advance",
            json!({
                "all": [following_json],
                "read": { "id": id, "index": index, "total": 5, "guess": guess, "goOn": go_on, "knewIt": index == 0 },
                "now": micros(now()),
            }),
        );
        assert_eq!(got["ended"], json!(ended));
        let want: Vec<Value> = want
            .iter()
            .map(|f| json!({ "id": f.id, "next": f.next, "total": f.total, "started": micros(f.started), "guess": f.guess, "knewIt": f.knew_it }))
            .collect();
        assert_eq!(got["all"], json!(want));
    }
    let work = serial::CATALOG[0].clone();
    for (work, index, total) in [(Some(&work), 2, 9), (Some(&work), 0, 0), (None, 0, 3)] {
        assert_eq!(
            ok(
                "serial.view",
                json!({ "work": work, "index": index, "total": total })
            )["view"],
            Value::Object(serial::view(work, index, total))
        );
    }
    let guessed = serial::Guessed {
        said: "他是凶手".into(),
        held: serial::Held::No,
        happened: "他只是路过".into(),
        remembered: false,
    };
    for (guessed, ended) in [
        (None, None),
        (Some(&guessed), Some(serial::Ended::Finished)),
        (Some(&guessed), Some(serial::Ended::LetGo)),
    ] {
        let (line, wrong) = serial::looking_back(guessed, ended);
        assert_eq!(
            ok(
                "serial.looking_back",
                json!({ "guessed": guessed, "ended": ended })
            ),
            json!({ "line": line, "wrong": wrong })
        );
    }
}

#[test]
fn the_library_reads_as_the_crate_does() {
    let csv = "\u{feff}Text#,Type,Issued,Title,Language,Authors,Subjects,LoCC,Bookshelves\n\
120,Text,2006-01-12,Treasure Island,en,\"Stevenson, Robert Louis, 1850-1894\",\"Adventure stories\",PZ,\"Category: Novels\"\n\
2,Text,1899-01-01,\"Punch, Volume 1\",en,,Humor,AP101,\n\
27166,Text,2008-11-03,吶喊,zh,\"Lu, Xun, 1881-1936\",Chinese fiction,PL,\n";
    assert_eq!(
        ok("library.csv_records", json!({ "text": csv }))["records"],
        json!(library::csv_records(csv))
    );
    assert_eq!(
        ok("library.from_gutenberg", json!({ "csv": csv }))["works"],
        json!(library::from_gutenberg(csv))
    );
    let aozora = "作品ID,作品名,作品名読み,ソート用読み,副題,副題読み,原題,初出,分類番号,文字遣い種別,作品著作権フラグ,公開日,最終更新日,図書カードURL,人物ID,姓,名,姓読み,名読み,姓読みソート用,名読みソート用,姓ローマ字,名ローマ字,役割フラグ,生年月日,没年月日,人物著作権フラグ,底本名1,底本出版社名1,底本初版発行年1,入力に使用した版1,校正に使用した版1,底本の親本名1,底本の親本出版社名1,底本の親本初版発行年1,底本名2,底本出版社名2,底本初版発行年2,入力に使用した版2,校正に使用した版2,底本の親本名2,底本の親本出版社名2,底本の親本初版発行年2,入力者,校正者,テキストファイルURL,テキストファイル最終更新日,テキストファイル符号化方式,テキストファイル文字集合,テキストファイル修正回数,XHTML/HTMLファイルURL,XHTML/HTMLファイル最終更新日,XHTML/HTMLファイル符号化方式,XHTML/HTMLファイル文字集合,XHTML/HTMLファイル修正回数\n\
000789,吾輩は猫である,,,,,,,NDC 913,新字新仮名,なし,1999-09-21,2014-09-17,https://www.aozora.gr.jp/cards/000148/card789.html,000148,夏目,漱石,,,,,,,著者,1867-02-09,1916-12-09,なし,,,,,,,,,,,,,,,,,,,https://www.aozora.gr.jp/cards/000148/files/789_ruby_5639.zip,,,,,,,,,\n";
    assert_eq!(
        ok("library.from_aozora", json!({ "csv": aozora }))["works"],
        json!(library::from_aozora(aozora))
    );
}

fn thread(id: &str, due: Option<DateTime<Utc>>, hers: bool) -> threads::Thread {
    threads::Thread {
        id: id.into(),
        about: "考试".into(),
        then: "问问".into(),
        due,
        hers,
    }
}

#[test]
fn writing_first_matches_the_crate() {
    let texts = ok("reach.texts", json!({}));
    assert_eq!(texts["judgeSchema"], reach::judge_schema());
    assert_eq!(
        texts["notMidTalkSeconds"],
        reach::NOT_MID_TALK.num_seconds()
    );
    for (panel_open, on_site, route) in [
        (true, true, "Stay"),
        (false, true, "Site"),
        (false, false, "Away"),
    ] {
        assert_eq!(
            ok(
                "reach.route",
                json!({ "panelOpen": panel_open, "onSite": on_site })
            )["route"],
            route
        );
    }
    let now = now();
    let all = [
        thread("a", Some(now - Duration::hours(1)), false),
        thread("b", Some(now + Duration::hours(1)), false),
        thread("c", None, true),
        thread("d", Some(now + Duration::hours(3)), true),
    ];
    let as_json = |threads: &[threads::Thread]| -> Vec<Value> {
        threads
            .iter()
            .map(|t| json!({ "id": t.id, "about": t.about, "then": t.then, "due": t.due.map(micros), "hers": t.hers }))
            .collect()
    };
    let tell = vec!["读完了一本书".to_string()];
    for (threads, last, to_try) in [
        (&all[..], Some(now - Duration::minutes(30)), None),
        (
            &all[..],
            Some(now - Duration::days(3)),
            Some("灯塔 (not tried)"),
        ),
        (&all[1..2], Some(now - Duration::hours(5)), None),
        (&all[..], None, Some("灯塔")),
    ] {
        let want = reach::reason(threads, last, tell.clone(), to_try.map(str::to_string), now);
        let got = ok(
            "reach.reason",
            json!({
                "threads": as_json(threads),
                "last": last.map(micros),
                "toTell": tell,
                "toTry": to_try,
                "now": micros(now),
            }),
        );
        let ids =
            |threads: &[threads::Thread]| threads.iter().map(|t| t.id.clone()).collect::<Vec<_>>();
        assert_eq!(
            got["reason"],
            want.map_or(Value::Null, |reason| json!({
                "key": reason.key(),
                "due": ids(&reason.due),
                "wished": ids(&reason.wished),
                "daysSince": reason.days_since,
                "toTell": reason.to_tell,
                "toTry": reason.to_try,
            }))
        );
    }
    assert_eq!(
        text("reach.judge_system", json!({ "soul": "你是若泉。" })),
        Some(reach::judge_system("你是若泉。"))
    );
    for raw in [
        "{\"reach_out\":true,\"about\":\"  考试怎么样 \"}",
        "{\"reach_out\":false,\"about\":null}",
        "{\"reach_out\":true,\"about\":\"\"}",
        "{\"reach_out\":true}",
        "nonsense",
    ] {
        let want = match reach::parse_judged(raw) {
            None => json!({ "readable": false, "about": null }),
            Some(about) => json!({ "readable": true, "about": about }),
        };
        assert_eq!(
            ok("reach.parse_judged", json!({ "raw": raw })),
            want,
            "{raw}"
        );
    }
    let lines = vec![
        (
            "2 days ago".to_string(),
            "考试  怎么样了\n还紧张吗".to_string(),
        ),
        ("a week ago".to_string(), "长".repeat(100)),
    ];
    assert_eq!(
        text("reach.wrote_before", json!({ "lines": lines })),
        reach::wrote_before(&lines)
    );
    assert_eq!(text("reach.wrote_before", json!({ "lines": [] })), None);
    for last_talked in [None, Some("3 days ago")] {
        assert_eq!(
            text(
                "reach.writing_first",
                json!({ "about": "考试", "lastTalked": last_talked })
            ),
            Some(reach::writing_first("考试", last_talked))
        );
    }
    for raw in [
        "考试怎么样？[[wear:校服]]（笑）\n\n还好吗(小声",
        "（动作）",
        &"字".repeat(300),
    ] {
        assert_eq!(
            text("reach.as_text", json!({ "raw": raw })),
            reach::as_text(raw),
            "{raw}"
        );
    }
}

fn profile() -> Value {
    json!({
        "activeOutfitId": "default",
        "wardrobe": [
            { "id": "default", "clothingStyle": "everyday", "portraitAssetId": "p0" },
            { "id": "uni", "name": "校服", "clothingStyle": "uniform", "rigAssetId": "r1",
              "outfit": { "construction": "水手领 短裙 蝴蝶结", "top": "白色水手服" } },
            { "id": "stage", "clothingStyle": "idol", "portraitAssetId": "p2", "generationFingerprint": "f" },
            { "id": "full", "clothingStyle": "formal", "portraitAssetId": "p3", "profile": "fullBody" },
        ],
    })
}

#[test]
fn changing_clothes_in_chat_matches_the_crate() {
    let profile = profile();
    let crate_looks = myriad_merope::looks_from_visual_profile(&profile);
    let looks = ok("outfit.looks_from_profile", json!({ "profile": profile }))["looks"].clone();
    let look_json = |look: &myriad_merope::WardrobeLook| {
        json!({
            "id": look.id, "label": look.label, "clothingStyle": look.clothing_style,
            "portraitAssetId": look.portrait_asset_id, "rigAssetId": look.rig_asset_id,
            "generationFingerprint": look.generation_fingerprint, "hints": look.hints,
        })
    };
    assert_eq!(
        looks,
        json!(crate_looks.iter().map(look_json).collect::<Vec<_>>())
    );
    for overlay in [None, Some("uni")] {
        assert_eq!(
            text(
                "outfit.wardrobe_section",
                json!({ "looks": looks, "worn": "default", "overlay": overlay })
            ),
            myriad_merope::format_chat_wardrobe_section(&crate_looks, "default", overlay)
        );
    }
    let directive_json = |directive: &Option<myriad_merope::WearDirective>| match directive {
        None => Value::Null,
        Some(myriad_merope::WearDirective::Revert) => json!({ "kind": "revert" }),
        Some(myriad_merope::WearDirective::Label(label)) => {
            json!({ "kind": "label", "label": label })
        }
    };
    for raw in [
        "好呀\n\n\n[[wear:校服]]",
        "换回来啦⟦wear:revert⟧",
        "没有",
        "[[wear:a]] 再 [[wear:舞台]]",
    ] {
        let (spoken, directive) = myriad_merope::split_chat_wear_directive(raw);
        assert_eq!(
            ok("outfit.split_wear", json!({ "raw": raw })),
            json!({ "spoken": spoken, "directive": directive_json(&directive) }),
            "{raw}"
        );
    }
    for spoken in ["好的[[we", "好的⟦wear", "好的"] {
        assert_eq!(
            text("outfit.hold_wear", json!({ "spoken": spoken })),
            Some(myriad_merope::hold_incomplete_wear_marker(spoken).to_string())
        );
    }
    let decision_json = |decided: myriad_merope::OverlayDecision| match decided {
        myriad_merope::OverlayDecision::Unchanged => json!({ "kind": "unchanged" }),
        myriad_merope::OverlayDecision::Clear => json!({ "kind": "clear" }),
        myriad_merope::OverlayDecision::Wear(id) => json!({ "kind": "wear", "id": id }),
    };
    for input in [
        "换上校服给我看",
        "换回来吧",
        "换一套别的",
        "今天天气不错",
        "想看你穿舞台装",
    ] {
        for overlay in [None, Some("uni")] {
            assert_eq!(
                ok(
                    "outfit.resolve",
                    json!({ "input": input, "looks": looks, "worn": "default", "overlay": overlay })
                )["decision"],
                decision_json(myriad_merope::resolve_chat_outfit_overlay(
                    input,
                    &crate_looks,
                    "default",
                    overlay
                )),
                "{input} {overlay:?}"
            );
        }
        for (spoken, marker) in [
            ("好呀", None),
            ("不用换，已经穿了", None),
            ("好", Some(myriad_merope::WearDirective::Revert)),
        ] {
            let want = myriad_merope::wear_directive_after_reply(input, spoken, marker.clone());
            assert_eq!(
                ok(
                    "outfit.after_reply",
                    json!({ "input": input, "spoken": spoken, "marker": directive_json(&marker) })
                )["directive"],
                directive_json(&want)
            );
            if let Some(directive) = &want {
                assert_eq!(
                    ok(
                        "outfit.resolve_directive",
                        json!({ "directive": directive_json(&want), "looks": looks, "worn": "default", "overlay": "stage" })
                    )["decision"],
                    decision_json(myriad_merope::resolve_wear_directive(
                        directive,
                        &crate_looks,
                        "default",
                        Some("stage")
                    ))
                );
            }
        }
    }
}
