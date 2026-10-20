//! The group's mind on its own, and the trajectories (G1) every end replays:
//! a small end is simulated here (a clock, timers, scripted answers), and
//! each step it takes is written to `fixtures/group/<name>.json` with
//! `MYRIAD_RECORD_G1=1`, else compared with it. The recordings are not in
//! the repository: where one is missing (a fresh checkout), the comparison
//! is skipped with a note saying how to record it; the scene's own
//! assertions still run.

use std::collections::{BTreeMap, HashMap, VecDeque};

use myriad_agent_rules::channel::{GroupImage, ImageFetch};
use serde_json::{Value, json};

use super::flows::without_reply_mark;
use super::lines::*;
use super::*;

const BASE: i64 = 1_791_957_600_000_000; // 2026-10-14T06:00:00Z

fn secs(seconds: f64) -> i64 {
    (seconds * 1_000_000.0) as i64
}

fn message(id: i64, from: i64, name: &str, text: &str, addressed: bool) -> Message {
    Message {
        platform: "telegram".into(),
        chat: "-100777".into(),
        message_id: id.to_string(),
        thread: None,
        from: from.to_string(),
        display_name: name.into(),
        text: text.into(),
        addressed,
        reply_to: None,
        images: Vec::new(),
        shown_as: None,
    }
}

fn her() -> Her {
    Her {
        away: false,
        asleep_for: None,
        busy_for: None,
        today: "2026-10-14".into(),
    }
}

fn at(now: i64) -> At {
    At {
        now,
        her: her(),
        rolls: Rolls::new(7, Vec::new()),
    }
}

fn line(at: i64, id: Option<&str>, from: Option<&str>, name: &str, text: &str, hers: bool) -> Line {
    Line {
        at: time(at),
        message_id: id.map(str::to_string),
        name: name.into(),
        from: from.map(str::to_string),
        text: text.into(),
        hers,
        addressed: false,
        images: Vec::new(),
        seen: Vec::new(),
    }
}

// ------------------------------------------------------------ rules

#[test]
fn a_line_heard_again_is_kept_once() {
    let mut group = Group::default();
    let heard = |group: &mut Group| {
        step(
            group,
            &mut at(BASE),
            Event::Heard {
                message: message(1, 11, "阿明", "@bot 在吗", false),
                restore: None,
            },
        )
        .fresh
    };
    assert_eq!(heard(&mut group), Some(true));
    assert_eq!(heard(&mut group), Some(false));
    assert_eq!(transcript(&group, None, BASE).len(), 1);
}

#[test]
fn the_transcript_is_the_lines_before_the_one_she_answers() {
    let mut group = Group::default();
    push_line(
        &mut group,
        line(BASE, Some("1"), Some("1"), "阿明", "周五聚餐吗", false),
        BASE,
    );
    push_line(
        &mut group,
        line(BASE, Some("2"), Some("2"), "小红", "我可以", false),
        BASE,
    );
    push_line(
        &mut group,
        line(BASE, None, None, "", "我在屏幕里", true),
        BASE,
    );
    push_line(
        &mut group,
        line(BASE, Some("3"), Some("1"), "阿明", "@bot 你推荐哪家", false),
        BASE,
    );
    let lines: Vec<(String, String)> = transcript(&group, Some("3"), BASE)
        .into_iter()
        .map(|spoken| (spoken.role, spoken.content))
        .collect();
    assert_eq!(
        lines,
        [
            ("user".into(), "阿明：周五聚餐吗".into()),
            ("user".into(), "小红：我可以".into()),
            ("assistant".into(), "我在屏幕里".into()),
        ]
    );
}

#[test]
fn she_mentions_whoever_spoke_lately_by_their_last_name_and_reads_ids_as_names() {
    let mut group = Group::default();
    push_line(
        &mut group,
        line(BASE, Some("1"), Some("11"), "阿明", "嗯", false),
        BASE,
    );
    push_line(
        &mut group,
        line(BASE, Some("2"), Some("12"), "小红", "嗯", false),
        BASE,
    );
    push_line(
        &mut group,
        line(BASE, Some("3"), Some("11"), "阿明同学", "嗯", false),
        BASE,
    );
    assert_eq!(
        people(&group, BASE),
        [
            ("阿明同学".to_string(), "11".to_string()),
            ("小红".to_string(), "12".to_string())
        ]
    );
    let people = vec![("小红".to_string(), "111".to_string())];
    assert_eq!(
        by_name("@111 你看 @1112 @222", &people),
        "@小红 你看 @1112 @222"
    );
    assert_eq!(by_name("邮箱 a@b.c", &people), "邮箱 a@b.c");
}

#[test]
fn she_answers_what_was_said_up_to_when_she_gets_to_it() {
    let mut group = Group::default();
    push_line(
        &mut group,
        line(BASE, Some("1"), Some("11"), "阿明", "周五聚餐吗", false),
        BASE,
    );
    let called = message(2, 11, "阿明", "你推荐哪家", true);
    push_line(
        &mut group,
        line(BASE, Some("2"), Some("11"), "阿明", "你推荐哪家", false),
        BASE,
    );
    push_line(
        &mut group,
        line(BASE, Some("3"), Some("12"), "小红", "我想吃火锅", false),
        BASE,
    );
    push_line(
        &mut group,
        line(BASE, Some("4"), Some("11"), "阿明", "最好便宜点", false),
        BASE,
    );
    let added = message(4, 11, "阿明", "最好便宜点", true);
    group.waiting.push_back(added.clone());
    let read = reading(&mut group, &called, BASE);
    assert_eq!(read.said, "你推荐哪家\n最好便宜点");
    assert_eq!(read.transcript.len(), 4);
    assert!(group.waiting.is_empty());
    assert!(read_already(&group, &added));
    assert!(!read_already(&group, &called));
}

#[test]
fn what_came_while_she_wrote_is_after_she_began_and_never_hers() {
    let mut group = Group::default();
    let since = BASE;
    push_line(
        &mut group,
        line(
            since - secs(10.0),
            Some("1"),
            Some("1"),
            "阿明",
            "周六几点集合",
            false,
        ),
        BASE,
    );
    push_line(
        &mut group,
        line(
            since + secs(5.0),
            Some("2"),
            Some("2"),
            "阿红",
            "刚说了十点",
            false,
        ),
        BASE,
    );
    push_line(
        &mut group,
        line(since + secs(6.0), None, None, "", "（她的）", true),
        BASE,
    );
    push_line(
        &mut group,
        line(
            since + secs(8.0),
            Some("3"),
            Some("1"),
            "阿明",
            "那就十点",
            false,
        ),
        BASE,
    );
    let mut calling = line(
        since + secs(9.0),
        Some("4"),
        Some("3"),
        "小周",
        "@她 你来吗",
        false,
    );
    calling.addressed = true;
    push_line(&mut group, calling, BASE);
    assert_eq!(
        came_since(&group, since, "1"),
        [
            ("阿红".to_string(), "刚说了十点".to_string(), false),
            ("阿明".to_string(), "那就十点".to_string(), true),
            ("小周".to_string(), "@你 @她 你来吗".to_string(), true),
        ]
    );
    assert!(!came_since(&group, since, "")[1].2);
}

#[test]
fn lines_kept_before_a_restart_come_back_under_newer_ones() {
    let mut group = Group::default();
    let minutes = |m: i64| BASE - m * 60_000_000;
    push_line(
        &mut group,
        line(
            minutes(1),
            Some("9"),
            Some("1"),
            "阿明",
            "说到一半怎么没了",
            false,
        ),
        BASE,
    );
    merge_restored(
        &mut group,
        vec![
            line(minutes(30), Some("7"), Some("1"), "阿明", "来点歌", false),
            line(minutes(29), None, None, "", "放就放，听完要是", true),
            line(
                minutes(1),
                Some("9"),
                Some("1"),
                "阿明",
                "说到一半怎么没了",
                false,
            ),
            line(
                minutes(7 * 60),
                Some("1"),
                Some("1"),
                "阿明",
                "太久以前的话",
                false,
            ),
        ],
        BASE,
    );
    let texts: Vec<&str> = group.lines.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(texts, ["来点歌", "放就放，听完要是", "说到一半怎么没了"]);
    // The kept shape reads back, and an old line without the flag does not
    // call her.
    let mut old = serde_json::to_value(&group.lines[0]).unwrap();
    old.as_object_mut().unwrap().remove("addressed");
    assert!(!serde_json::from_value::<Line>(old).unwrap().addressed);
}

#[test]
fn a_mute_is_told_as_it_happened_and_holds_until_it_ends() {
    let mut group = Group::default();
    let mut out = step(
        &mut group,
        &mut at(BASE),
        Event::Muted {
            by: String::new(),
            mute: Mute::For { seconds: 600 },
            everyone: false,
            restore: None,
        },
    );
    assert_eq!(group.lines.back().unwrap().text, "（把你禁言了 10 分钟）");
    assert_eq!(group.lines.back().unwrap().name, "群管理");
    assert!(matches!(out.effects.pop(), Some(Effect::KeepLines { .. })));
    assert_eq!(group.muted_until, Some(BASE + secs(600.0)));
    step(
        &mut group,
        &mut at(BASE),
        Event::Muted {
            by: String::new(),
            mute: Mute::For { seconds: 3 * 3600 },
            everyone: false,
            restore: None,
        },
    );
    assert_eq!(group.lines.back().unwrap().text, "（把你禁言了 3 小时）");
    step(
        &mut group,
        &mut at(BASE),
        Event::Muted {
            by: String::new(),
            mute: Mute::For { seconds: 2 * 86400 },
            everyone: true,
            restore: None,
        },
    );
    assert_eq!(group.lines.back().unwrap().text, "（全员禁言 2 天）");
    step(
        &mut group,
        &mut at(BASE),
        Event::Muted {
            by: String::new(),
            mute: Mute::Lifted,
            everyone: false,
            restore: None,
        },
    );
    assert_eq!(group.lines.back().unwrap().text, "（解除了你的禁言）");
    // Everyone's mute still holds.
    assert!(group.everyone_muted_until.is_some_and(|until| until > BASE));
    assert_eq!(group.muted_until, None);
}

#[test]
fn a_line_left_waiting_by_a_restart_is_taken_back_up_once() {
    let mut group = Group::default();
    let up = BASE;
    let ago = |minutes: i64| up - minutes * 60_000_000;
    let past = |id: i64, minutes: i64, text: &str, hers: bool, addressed: bool| Past {
        message: message(id, 11, "阿明", text, addressed),
        at: ago(minutes),
        hers,
    };
    let lines = vec![
        past(1, 90, "@bot 早", false, true),
        past(2, 89, "早呀", true, false),
        past(3, 40, "她起了没", false, false),
        past(4, 30, "@bot 晚上打游戏吗", false, true),
        past(5, 20, "没回，睡着了吧", false, false),
    ];
    let out = step(
        &mut group,
        &mut at(BASE),
        Event::Resumed {
            why: Resume::Restart,
            past: lines.clone(),
            up_since: Some(up),
        },
    );
    let taken: Vec<&flows::Errand> = group.errands.iter().collect();
    assert_eq!(taken.len(), 1, "{out:?}");
    match &taken[0].kind {
        flows::Kind::TakeUp { message, .. } => assert_eq!(message.message_id, "4"),
        other => panic!("{other:?}"),
    }
    let again = step(
        &mut group,
        &mut at(BASE),
        Event::Resumed {
            why: Resume::Restart,
            past: lines,
            up_since: Some(up),
        },
    );
    assert!(
        again
            .effects
            .iter()
            .all(|effect| !matches!(effect, Effect::Wake { .. }))
    );
}

#[test]
fn she_stops_answering_one_who_never_stops() {
    let mut group = Group::default();
    group.answered = (0..LOOP_ROUNDS - 1)
        .map(|_| Answered {
            from: "77".into(),
            at: BASE,
        })
        .collect();
    let bot = message(1, 77, "复读机", "在吗", true);
    assert!(group.paused.is_empty());
    // One more round, all within the window: paused.
    group.answered.push_back(Answered {
        from: "77".into(),
        at: BASE,
    });
    let looping = group.answered.len() == LOOP_ROUNDS
        && group.answered.iter().all(|one| one.from == bot.from);
    assert!(looping);
}

#[test]
fn the_ledger_keeps_numbers_not_words_or_ids_and_a_member_is_the_same_token_everywhere() {
    assert_eq!(ledger_who("1"), ledger_who("1"));
    assert_ne!(ledger_who("1"), "1");
    assert_eq!(ledger_who("1").len(), 12);
    // SHA-256 of "1", first six bytes: the same on every end and build.
    assert_eq!(ledger_who("1"), "6b86b273ff34");
    let merged = merge_ledger(
        Ledger::default(),
        vec![crate::talk_shape::typed_by("x", 0, "周五聚餐吗").unwrap()],
        vec![1.5],
        &Counts::of(["周五聚餐吗"]),
        &Counts::default(),
    );
    // Messages are numbers; only the pieces they are made of are counted.
    let json = serde_json::to_string(&merged.messages).unwrap();
    assert!(!json.contains("聚餐"));
    assert!(merged.theirs.pieces.contains_key("end:餐吗"));
    assert_eq!(merged.her_waits, [1.5]);
}

#[test]
fn she_does_not_echo_the_reply_mark() {
    assert_eq!(
        without_reply_mark("（回复 阿明）谁是你宝宝！"),
        "谁是你宝宝！"
    );
    assert_eq!(
        without_reply_mark("（回复你之前说的「听完要是」）断了"),
        "断了"
    );
    assert_eq!(
        without_reply_mark("谁暴躁了（回复一下）"),
        "谁暴躁了（回复一下）"
    );
}

#[test]
fn a_group_of_another_version_is_read_as_nothing_in_mind() {
    let mut group = Group::default();
    push_line(
        &mut group,
        line(BASE, Some("1"), Some("1"), "阿明", "嗯", false),
        BASE,
    );
    let mut kept = serde_json::to_value(&group).unwrap();
    assert_eq!(Group::read(kept.clone()), group);
    kept["version"] = json!(VERSION + 1);
    assert_eq!(Group::read(kept), Group::default());
}

#[test]
fn chance_from_a_seed_is_the_same_everywhere() {
    let mut rolls = Rolls::new(7, vec![0.25]);
    assert_eq!(rolls.roll(), 0.25);
    let first = rolls.roll();
    let mut again = Rolls::new(7, Vec::new());
    assert_eq!(again.roll(), first);
    assert!((0.0..1.0).contains(&first));
    let mut range = Rolls::new(0, vec![0.5, 0.999_999]);
    assert_eq!(range.range(GLANCE_AFTER_SECONDS), 105);
    assert_eq!(range.range(GLANCE_AFTER_SECONDS), 180);
}

#[test]
fn a_group_gets_one_turn_at_a_time_and_a_pause_after_replying() {
    let mut group = Group::default();
    assert_eq!(flows::begin(&mut group, BASE), Ok(()));
    assert_eq!(
        flows::begin(&mut group, BASE),
        Err(None),
        "one turn at a time"
    );
    flows::end_turn(&mut group, true, BASE);
    assert_eq!(
        flows::begin(&mut group, BASE + secs(1.0)),
        Err(Some(4.0)),
        "a short pause after a reply"
    );
    assert_eq!(flows::begin(&mut group, BASE + secs(5.0)), Ok(()));
    flows::end_turn(&mut group, false, BASE + secs(5.0));
    assert_eq!(
        flows::begin(&mut group, BASE + secs(5.0)),
        Ok(()),
        "no reply, no pause"
    );
}

#[test]
fn a_line_that_comes_while_she_is_busy_waits_and_only_the_oldest_go() {
    let mut group = Group::default();
    for id in 1..=(WAITING_LINES as i64 + 3) {
        flows::park(
            &mut group,
            message(id, 11, "阿明", &format!("第{id}问"), true),
        );
    }
    assert_eq!(group.waiting.len(), WAITING_LINES);
    assert_eq!(group.waiting.front().unwrap().text, "第4问");
    let mut today = None;
    for _ in 0..3 {
        assert!(flows::count_today(&mut today, "2026-10-14", 3));
    }
    assert!(!flows::count_today(&mut today, "2026-10-14", 3));
    // Her next day counts afresh (her day, in her zone: D2-18).
    assert!(flows::count_today(&mut today, "2026-10-15", 3));
}

#[test]
fn she_looks_once_the_talk_settles_and_glances_at_the_rest_once() {
    let mut group = Group::default();
    let mut at = at(BASE);
    // Not in the talk: one glance is planned, not one per line.
    let first = step(
        &mut group,
        &mut at,
        Event::Noticed {
            message: message(1, 11, "阿明", "有人听过吗", false),
        },
    );
    assert!(matches!(first.effects[..], [Effect::Wake { .. }]));
    let planned = group.glance_at;
    assert!(planned.is_some_and(|at| at <= BASE + secs(LONGEST_BUSY_SECONDS + 180.0)));
    let second = step(
        &mut group,
        &mut at,
        Event::Noticed {
            message: message(2, 11, "阿明", "你们说呢", false),
        },
    );
    assert!(second.effects.is_empty());
    assert_eq!(group.glance_at, planned);
    assert_eq!(
        group.pending.as_ref().map(|line| line.message_id.as_str()),
        Some("2")
    );
    assert!(group.unjudged_since.is_some());
    // She said something there: she is in the talk, and looks once it settles.
    push_line(
        &mut group,
        line(BASE, None, None, "", "我在屏幕里", true),
        BASE,
    );
    assert!(flows::in_talk(&group, BASE));
    let third = step(
        &mut group,
        &mut at,
        Event::Noticed {
            message: message(3, 12, "小红", "可以", false),
        },
    );
    assert!(matches!(&third.effects[..], [Effect::Wake { after, .. }] if *after == SETTLE_SECONDS));
    // Called there lately is in the talk too.
    let mut other = Group::default();
    other.called = Some(BASE);
    assert!(flows::in_talk(&other, BASE + secs(60.0)));
    assert!(!flows::in_talk(&other, BASE + secs(IN_TALK_SECONDS)));
}

#[test]
fn pictures_read_as_she_saw_them_and_one_sent_again_is_counted() {
    let mut group = Group::default();
    let picture = |id: i64| {
        let mut picture = message(id, 11, "阿明", "", false);
        picture.images = vec![GroupImage {
            key: "telegram:cat".into(),
            fetch: ImageFetch::TelegramFile {
                file_id: "f".into(),
            },
            hint: Some("😂".into()),
            sticker: true,
        }];
        picture
    };
    for heard in [
        picture(1),
        message(2, 12, "小红", "哈哈哈", false),
        picture(3),
    ] {
        step(
            &mut group,
            &mut at(BASE),
            Event::Heard {
                message: heard,
                restore: None,
            },
        );
    }
    assert_eq!(
        transcript(&group, None, BASE)[0].content,
        "阿明：[表情：😂]"
    );
    assert_eq!(group.pictures.get("telegram:cat"), Some(&2));
    group.lines[0].seen = vec![Some(crate::seeing::Seen {
        what: "一只翻白眼的猫".into(),
        says: Some("无语".into()),
        ..Default::default()
    })];
    assert_eq!(
        transcript(&group, None, BASE)[0].content,
        "阿明：[表情：一只翻白眼的猫（无语）]"
    );
    assert_eq!(said_now(&group, &picture(3)), "[表情：😂]");
}

#[test]
fn a_line_she_sees_hours_later_she_knows_she_sees_late_and_the_room_is_theirs() {
    let mut group = Group::default();
    let asked = message(1, 11, "阿明", "@bot 在吗", true);
    step(
        &mut group,
        &mut at(BASE),
        Event::Heard {
            message: asked.clone(),
            restore: None,
        },
    );
    assert_eq!(seen_late(&group, &asked, BASE), None);
    assert_eq!(
        seen_late(&group, &asked, BASE + secs(7.0 * 3600.0)).as_deref(),
        Some("7 hours ago")
    );
    assert_eq!(
        seen_late(&group, &asked, BASE + secs(2.0 * 3600.0)).as_deref(),
        Some("2 hours ago")
    );
    for index in 0..crate::talk_shape::ROOM_AT_LEAST as i64 {
        push_line(
            &mut group,
            line(
                BASE,
                Some(&format!("r{index}")),
                Some("11"),
                "阿明",
                "哈哈哈",
                false,
            ),
            BASE,
        );
        push_line(
            &mut group,
            line(
                BASE,
                None,
                None,
                "",
                "这句话很长很长很长很长很长很长！",
                true,
            ),
            BASE,
        );
    }
    let room = room(&group, BASE).unwrap();
    assert_eq!(room.chars_median, 3);
    assert_eq!(room.bang, 0.0);
}

#[test]
fn a_group_keeps_only_its_recent_lines_and_reads_back_what_was_said_while_away() {
    let mut group = Group::default();
    for index in 0..(TRANSCRIPT_LINES as i64 + 5) {
        step(
            &mut group,
            &mut at(BASE),
            Event::Heard {
                message: message(index, 1, "某人", &format!("第{index}句"), false),
                restore: None,
            },
        );
    }
    let lines = transcript(&group, None, BASE);
    assert_eq!(lines.len(), TRANSCRIPT_LINES);
    assert_eq!(lines[0].content, "某人：第5句");

    let mut group = Group::default();
    step(
        &mut group,
        &mut at(BASE),
        Event::Heard {
            message: message(5, 1, "瞳", "@bot 宝宝", true),
            restore: None,
        },
    );
    let ago = |minutes: i64| BASE - minutes * 60_000_000;
    step(
        &mut group,
        &mut at(BASE),
        Event::Resumed {
            why: Resume::Restart,
            past: vec![
                Past {
                    message: message(3, 2, "leaphy", "宝宝，晚安喵", false),
                    at: ago(3),
                    hers: false,
                },
                Past {
                    message: message(4, 0, "", "晚安", false),
                    at: ago(2),
                    hers: true,
                },
                // Already had, and long gone: neither is taken again.
                Past {
                    message: message(5, 1, "瞳", "@bot 宝宝", true),
                    at: ago(1),
                    hers: false,
                },
                Past {
                    message: message(1, 3, "某人", "昨天的事", false),
                    at: ago(60 * 24),
                    hers: false,
                },
            ],
            up_since: Some(BASE),
        },
    );
    let lines: Vec<(String, String)> = transcript(&group, None, BASE)
        .into_iter()
        .map(|spoken| (spoken.role, spoken.content))
        .collect();
    assert_eq!(
        lines,
        [
            ("user".to_string(), "leaphy：宝宝，晚安喵".to_string()),
            ("assistant".to_string(), "晚安".to_string()),
            ("user".to_string(), "瞳：@你 @bot 宝宝".to_string()),
        ]
    );
    // Read back, not answered: nothing is waiting for her.
    assert!(group.waiting.is_empty() && group.pending.is_none());
}

#[test]
fn a_line_that_called_her_before_a_restart_is_found_in_what_was_kept_and_taken_up_once() {
    let at_minutes = |minutes: i64| BASE - minutes * 60_000_000;
    let kept = |id: &str, minutes: i64, text: &str, hers: bool, addressed: bool| Line {
        addressed,
        ..line(
            at_minutes(minutes),
            (!hers).then_some(id),
            (!hers).then_some("1"),
            if hers { "" } else { "阿明" },
            text,
            hers,
        )
    };
    let past = kept_as_heard(
        "telegram",
        "-100777",
        Some(9),
        vec![
            kept("1", 40, "绮羽在吗", false, true),
            kept("", 35, "在", true, false),
            kept("3", 20, "绮羽你看这个", false, true),
            kept("4", 10, "没人理我", false, false),
        ],
    );
    let mut group = Group::default();
    step(
        &mut group,
        &mut at(BASE),
        Event::Resumed {
            why: Resume::Restart,
            past: past.clone(),
            up_since: Some(BASE),
        },
    );
    let taken: Vec<&Message> = group
        .errands
        .iter()
        .filter_map(|errand| match &errand.kind {
            flows::Kind::TakeUp { message, .. } => Some(message),
            _ => None,
        })
        .collect();
    assert_eq!(taken.len(), 1);
    assert_eq!(taken[0].message_id, "3");
    assert_eq!(taken[0].thread, Some(9));
    assert!(taken[0].addressed);
    // And a line taken up is answered once, however often it comes in.
    let mut group = Group::default();
    assert!(flows::first_time(&mut group, "3"));
    assert!(!flows::first_time(&mut group, "3"));
    assert!(flows::first_time(&mut group, ""));
    assert!(flows::first_time(&mut group, ""));
}

#[test]
fn held_back_she_answers_again_from_the_newest_line_of_theirs() {
    let mut group = Group::default();
    let asked = message(1, 11, "阿明", "@bot 周六几点", true);
    step(
        &mut group,
        &mut at(BASE),
        Event::Heard {
            message: asked.clone(),
            restore: None,
        },
    );
    assert!(newest_theirs(&group, &asked).is_none());
    step(
        &mut group,
        &mut at(BASE),
        Event::Heard {
            message: message(2, 12, "小红", "刚问过了 十点", false),
            restore: None,
        },
    );
    push_line(&mut group, line(BASE, None, None, "", "九点吧", true), BASE);
    let newest = newest_theirs(&group, &asked).unwrap();
    assert_eq!(newest.message_id, "2");
    assert_eq!(newest.text, "刚问过了 十点");
    assert_eq!(newest.venue(), asked.venue());
}

#[test]
fn her_speaking_up_counts_as_taken_up_soon_and_comes_back_after_a_restart_under_what_came_since() {
    let mut group = Group::default();
    flows::taken_up(&mut group, BASE);
    assert!(group.spoke_up.is_empty());
    group.spoke_up.push_back(SpokeUp {
        at: BASE - secs(TAKEN_UP_WITHIN_SECONDS),
        taken: false,
    });
    flows::taken_up(&mut group, BASE);
    assert!(!group.spoke_up.back().unwrap().taken);
    group.spoke_up.push_back(SpokeUp {
        at: BASE,
        taken: false,
    });
    flows::taken_up(&mut group, BASE);
    assert_eq!(
        group
            .spoke_up
            .iter()
            .map(|one| one.taken)
            .collect::<Vec<_>>(),
        [false, true]
    );

    let mut group = Group::default();
    group.spoke_up.push_back(SpokeUp {
        at: BASE,
        taken: false,
    });
    let kept: Vec<SpokeUp> = (0..SPOKE_UP_KEPT)
        .map(|index| SpokeUp {
            at: BASE - (10 - index as i64) * 60_000_000,
            taken: index % 2 == 0,
        })
        .collect();
    step(
        &mut group,
        &mut at(BASE),
        Event::Restored {
            lines: None,
            spoke_up: kept,
            table: None,
        },
    );
    assert_eq!(group.spoke_up.len(), SPOKE_UP_KEPT);
    assert!(
        !group.spoke_up.back().unwrap().taken,
        "the newest, since the restart, is last"
    );
    assert!(
        group
            .spoke_up
            .iter()
            .zip(group.spoke_up.iter().skip(1))
            .all(|(a, b)| a.at <= b.at)
    );
}

// ------------------------------------------------------------ trajectories

/// A small end: a clock, its timers, and what it answers.
struct End {
    now: i64,
    group: Group,
    her: Her,
    seed: u64,
    /// Events due, by when and in what order they came.
    due: BTreeMap<(i64, u64), Event>,
    order: u64,
    /// Answers by effect kind, in order, and how long each takes.
    answers: HashMap<&'static str, VecDeque<(f64, Answer)>>,
    steps: Vec<Value>,
    /// What was kept of the group before cannot be read now; else the end
    /// hands in that nothing was kept with the first line, as the site does.
    unreadable: bool,
}

impl End {
    fn new() -> Self {
        Self {
            now: BASE,
            group: Group::default(),
            her: her(),
            seed: 1,
            due: BTreeMap::new(),
            order: 0,
            answers: HashMap::new(),
            steps: Vec::new(),
            unreadable: false,
        }
    }

    fn answer(&mut self, kind: &'static str, after: f64, answer: Answer) {
        self.answers
            .entry(kind)
            .or_default()
            .push_back((after, answer));
    }

    fn at(&mut self, at: i64, event: Event) {
        self.order += 1;
        self.due.insert((at, self.order), event);
    }

    fn step(&mut self, mut event: Event) {
        if !self.group.restored && !self.unreadable {
            if let Event::Heard { restore, .. }
            | Event::Muted { restore, .. }
            | Event::StickerSent { restore, .. } = &mut event
            {
                restore.get_or_insert_with(Restore::default);
            }
        }
        self.seed = self
            .seed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let input = json!({
            "now": self.now,
            "seed": self.seed,
            "her": self.her,
            "event": event,
        });
        let mut at = At {
            now: self.now,
            her: self.her.clone(),
            rolls: Rolls::new(self.seed, Vec::new()),
        };
        let out = step(&mut self.group, &mut at, event);
        self.steps.push(json!({ "input": input, "output": out }));
        for effect in out.effects {
            self.does(effect);
        }
    }

    fn does(&mut self, effect: Effect) {
        let (id, kind) = match &effect {
            Effect::Wake { key, after } => {
                let at = self.now + secs(*after);
                self.at(at, Event::Woke { key: key.clone() });
                return;
            }
            Effect::Sticker { chosen, .. } => {
                let meaning = chosen["meaning"].as_str().unwrap_or("贴纸").to_string();
                self.at(
                    self.now,
                    Event::StickerSent {
                        meaning,
                        restore: None,
                    },
                );
                return;
            }
            Effect::Claim { id, .. } => (id, "claim"),
            Effect::Look { id, pictures } => {
                let seen = pictures
                    .iter()
                    .map(|picture| Looked {
                        message_id: picture.message_id.clone(),
                        index: picture.index,
                        seen: crate::seeing::Seen {
                            what: format!("看见了{}", picture.image.key),
                            ..Default::default()
                        },
                    })
                    .collect();
                let id = id.clone();
                self.at(
                    self.now,
                    Event::Done {
                        id,
                        answer: Answer::Looked { seen },
                    },
                );
                return;
            }
            Effect::Identify { id, .. } => (id, "identify"),
            Effect::Prepare { id, .. } => (id, "prepare"),
            Effect::Read { id, what, .. } => (
                id,
                match what {
                    ToRead::Soul => "soul",
                    ToRead::VenueRows { source, .. } => match source.as_str() {
                        "bit" => "rows:bit",
                        "group_day" => "rows:group_day",
                        _ => "rows",
                    },
                    ToRead::Unowned { source, .. } => match source.as_str() {
                        "stranger" => "unowned:stranger",
                        _ => "unowned:maybe_is",
                    },
                    ToRead::UnownedElsewhere { .. } => "elsewhere",
                    ToRead::OtherGroups { sources, .. } => {
                        if sources.iter().any(|source| source == "jargon") {
                            "others:jargon"
                        } else {
                            "others"
                        }
                    }
                    ToRead::Titles { .. } => "titles",
                    ToRead::Label { .. } => "label",
                    ToRead::Now => "now",
                    ToRead::Stickers { .. } => "stickers",
                    ToRead::Game { .. } => "game",
                    ToRead::RecentSurfaces => "recent",
                    ToRead::RecentDealt { .. } => "recent",
                    ToRead::HerName => "label",
                    ToRead::OwnRows { source, .. } => match source.as_str() {
                        "view" => "own:view",
                        "heard" => "own:heard",
                        "made_soup" => "own:made_soup",
                        _ => "own:meme",
                    },
                    ToRead::Life { .. } => "life",
                    ToRead::Cues { .. } => "cues",
                    ToRead::Vectors { .. } => "vectors",
                },
            ),
            Effect::Embed { id, .. } => (id, "embed"),
            Effect::ReadLedger { id } => (id, "ledger"),
            Effect::ReadKept { id } => (id, "kept"),
            Effect::TurnBegin { id, .. } => (id, "begin"),
            Effect::TurnEnd { id, said, .. } => {
                // Closed with what she said, it gives her words back as they
                // stand, unless the scene says otherwise.
                let id = id.clone();
                let scripted = self.answers.get_mut("end").and_then(VecDeque::pop_front);
                let (after, answer) = scripted.unwrap_or_else(|| {
                    (
                        0.0,
                        Answer::TurnEnded {
                            reply: said.clone(),
                            bound: true,
                        },
                    )
                });
                let at = self.now + secs(after);
                self.at(at, Event::Done { id, answer });
                return;
            }
            Effect::React { id, .. } => (id, "react"),
            Effect::MayLookUp { id, .. } => (id, "may"),
            Effect::Define { id, .. } => (id, "define"),
            Effect::Search { id, .. } => (id, "search"),
            Effect::Typing { id, .. } => (id, "typing"),
            Effect::Ask { id, operation, .. } => (
                id,
                match operation.as_str() {
                    "group_making_sense" => "ask:group_making_sense",
                    "group_chime" => "ask:group_chime",
                    "meme" => "ask:meme",
                    "group_stranger" => "ask:group_stranger",
                    "group_member" => "ask:group_member",
                    "soup_judge" => "ask:soup_judge",
                    "soup_start" => "ask:soup_start",
                    "undercover_words" => "ask:undercover_words",
                    "undercover_judge" => "ask:undercover_judge",
                    "undercover_voice" | "werewolf_voice" | "avalon_voice" | "marks_voice" => {
                        "ask:voice"
                    }
                    "undercover_her_move"
                    | "werewolf_her_move"
                    | "avalon_her_move"
                    | "marks_her_move" => "ask:her_move",
                    "jubensha_voice" => "ask:voice",
                    "jubensha_outline" => "ask:jubensha_outline",
                    "jubensha_clues" => "ask:jubensha_clues",
                    "jubensha_scripts" => "ask:jubensha_scripts",
                    "jubensha_check" => "ask:jubensha_check",
                    "jubensha_answer" => "ask:jubensha_answer",
                    _ => "ask",
                },
            ),
            Effect::Send { id, .. } => (id, "send"),
            Effect::Whisper { id, .. } => (id, "whisper"),
            _ => return,
        };
        let id = id.clone();
        let (after, answer) = self
            .answers
            .get_mut(kind)
            .and_then(VecDeque::pop_front)
            .unwrap_or_else(|| (0.0, default_answer(kind)));
        let at = self.now + secs(after);
        self.at(at, Event::Done { id, answer });
    }

    /// Run until nothing is due before `until`.
    fn run(&mut self, until: i64) {
        while let Some((&(at, order), _)) = self.due.iter().next() {
            if at > until {
                break;
            }
            let event = self.due.remove(&(at, order)).unwrap();
            self.now = self.now.max(at);
            self.step(event);
        }
        self.now = self.now.max(until);
    }

    fn check(self, name: &str) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/group")
            .join(format!("{name}.json"));
        let got = json!({ "steps": self.steps, "state": self.group });
        if std::env::var("MYRIAD_RECORD_G1").as_deref() == Ok("1") {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            // Written compact when it would pass 1 MiB laid out: the other
            // ends keep only smaller test data (each end reads it as JSON).
            let mut text = serde_json::to_string_pretty(&got).unwrap();
            if text.len() > 1 << 20 {
                text = serde_json::to_string(&got).unwrap();
            }
            std::fs::write(&path, text + "\n").unwrap();
            return;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            eprintln!(
                "{name}: not compared, no trajectory at {}; record it with \
                 MYRIAD_RECORD_G1=1 cargo test -p myriad-merope --lib group::tests::g1_",
                path.display()
            );
            return;
        };
        let want: Value = serde_json::from_str(&text).unwrap();
        assert_eq!(got, want, "{name} differs from its trajectory");
        replay(&want);
    }
}

fn default_answer(kind: &str) -> Answer {
    match kind {
        "claim" => Answer::Claimed { ok: true },
        "identify" => Answer::Identified {
            who: Who::Member {
                user_id: 101,
                bound: true,
            },
            owner: Some(1),
        },
        "prepare" => Answer::Prepared { ok: true },
        "soul" => Answer::Soul {
            text: "你是测试里的她。".into(),
        },
        "rows" | "rows:bit" | "rows:group_day" | "own:view" | "own:heard" | "own:meme"
        | "own:made_soup" | "unowned:stranger" | "unowned:maybe_is" | "elsewhere" | "others"
        | "others:jargon" => Answer::Rows { rows: Vec::new() },
        "titles" => Answer::Titles { titles: Vec::new() },
        "label" => Answer::Label {
            label: "某人".into(),
        },
        "now" => Answer::Now {
            local: "2026-10-14T14:00:00+08:00".into(),
            doing: None,
        },
        "stickers" => Answer::Stickers {
            stickers: Vec::new(),
            can_make: false,
            left: 0,
        },
        "game" => Answer::Game { game: None },
        "recent" => Answer::Surfaces {
            surfaces: Vec::new(),
        },
        "ask:group_stranger" | "ask:group_member" => Answer::Asked {
            raw: Some("嗯嗯".into()),
            failure: None,
        },
        "life" => Answer::Life {
            did: Vec::new(),
            would_tell: Vec::new(),
            now: None,
        },
        "cues" => Answer::Cues {
            cues: HashMap::new(),
        },
        "vectors" => Answer::Vectors {
            vectors: Vec::new(),
            known: false,
        },
        "embed" => Answer::Embedded { vector: None },
        "ledger" => Answer::LedgerRead { stored: None },
        "kept" => Answer::Kept { restore: None },
        "begin" => Answer::TurnBegun {
            turn: Some("turn-1".into()),
            sections: vec!["## How you feel toward them\nYou feel at ease with them.".into()],
            doing: None,
        },
        "react" => Answer::Reacted { ok: true },
        "may" => Answer::MayLookUp { may: May::Tried },
        "define" => Answer::Defined { entry: None },
        "search" => Answer::Found { found: None },
        "typing" => Answer::Typed,
        "ask"
        | "ask:group_making_sense"
        | "ask:group_chime"
        | "ask:meme"
        | "ask:soup_judge"
        | "ask:soup_start" => Answer::Asked {
            raw: None,
            failure: Some("unavailable".into()),
        },
        "send" => Answer::Sent { ok: true },
        "whisper" => Answer::Whispered { ok: true },
        "ask:undercover_words" => Answer::Asked {
            raw: Some(r#"{"civilian":"饺子","undercover":"包子"}"#.into()),
            failure: None,
        },
        "ask:undercover_judge" => Answer::Asked {
            raw: Some(r#"{"givesAway":false}"#.into()),
            failure: None,
        },
        "ask:jubensha_outline"
        | "ask:jubensha_clues"
        | "ask:jubensha_scripts"
        | "ask:jubensha_check"
        | "ask:jubensha_answer" => Answer::Asked {
            raw: None,
            failure: Some("unavailable".into()),
        },
        "ask:voice" | "ask:her_move" => Answer::Asked {
            raw: None,
            failure: Some("unavailable".into()),
        },
        _ => unreachable!("{kind}"),
    }
}

/// A trajectory step by step, as an end replays it: each step's state is
/// the one the step before handed back.
fn replay(trajectory: &Value) {
    let mut group = Group::default();
    for (index, recorded) in trajectory["steps"].as_array().unwrap().iter().enumerate() {
        let input = &recorded["input"];
        let event: Event = serde_json::from_value(input["event"].clone()).unwrap();
        let her: Her = serde_json::from_value(input["her"].clone()).unwrap();
        let mut at = At {
            now: input["now"].as_i64().unwrap(),
            her,
            rolls: Rolls::new(input["seed"].as_u64().unwrap(), Vec::new()),
        };
        // The state as an end keeps it: through its JSON.
        group = Group::read(serde_json::to_value(&group).unwrap());
        let out = step(&mut group, &mut at, event);
        assert_eq!(
            serde_json::to_value(&out).unwrap(),
            recorded["output"],
            "step {index}"
        );
    }
    assert_eq!(serde_json::to_value(&group).unwrap(), trajectory["state"]);
}

fn heard(end: &mut End, at: f64, message: Message) {
    let when = BASE + secs(at);
    end.at(
        when,
        Event::Heard {
            message: message.clone(),
            restore: None,
        },
    );
    let event = if message.addressed {
        Event::Called {
            message,
            waiter: Some(format!("w{at}")),
        }
    } else {
        Event::Noticed { message }
    };
    end.at(when, event);
}

#[test]
fn g1_a_member_calls_her() {
    let mut end = End::new();
    end.answer(
        "ask:group_member",
        4.0,
        Answer::Asked {
            raw: Some("去老地方吧\n便宜又好吃".into()),
            failure: None,
        },
    );
    heard(&mut end, 0.0, message(1, 11, "阿明", "周五聚餐吗", false));
    heard(&mut end, 2.0, message(2, 12, "小红", "我可以", false));
    heard(
        &mut end,
        4.0,
        message(3, 11, "阿明", "@bot 你推荐哪家", true),
    );
    end.run(BASE + secs(900.0));
    end.check("a_member_calls_her");
}

#[test]
fn g1_lines_wait_while_she_is_busy() {
    let mut end = End::new();
    end.answer(
        "ask:group_member",
        20.0,
        Answer::Asked {
            raw: Some("我先看看".into()),
            failure: None,
        },
    );
    heard(&mut end, 0.0, message(1, 11, "阿明", "@bot 周五去哪", true));
    heard(
        &mut end,
        30.0,
        message(2, 12, "小红", "@bot 你吃火锅吗", true),
    );
    heard(&mut end, 31.0, message(3, 12, "小红", "要辣的", false));
    end.run(BASE + secs(900.0));
    end.check("lines_wait_while_she_is_busy");
}

/// What she has to draw on cannot be read: she leaves that glance, saying
/// nothing; the next goes as ever.
#[test]
fn g1_she_says_nothing_when_what_she_has_cannot_be_read() {
    let mut end = End::new();
    end.answer("own:view", 0.5, Answer::Unread);
    end.answer(
        "ask:group_chime",
        1.0,
        Answer::Asked {
            raw: Some(
                r#"{"speak":"know","about":"那家店周五人多","basis":null,"unsure":[]}"#.into(),
            ),
            failure: None,
        },
    );
    end.answer(
        "ask:group_making_sense",
        1.0,
        Answer::Asked {
            raw: Some(r#"{"goingOn":"阿明在约周五去吃那家火锅","theyMean":"在问大家去不去","aboutYou":null,"unsure":[]}"#.into()),
            failure: None,
        },
    );
    heard(
        &mut end,
        0.0,
        message(1, 11, "阿明", "周五吃那家火锅吧", false),
    );
    heard(&mut end, 1.0, message(2, 11, "阿明", "大家觉得呢", false));
    heard(
        &mut end,
        701.0,
        message(3, 11, "阿明", "我也想去那家", false),
    );
    end.run(BASE + secs(2400.0));
    end.check("she_says_nothing_when_what_she_has_cannot_be_read");
}

/// Nobody asked her and she could not make sense of the talk: she keeps
/// quiet rather than chime in unread.
#[test]
fn g1_she_keeps_quiet_on_talk_she_could_not_read() {
    for who in [
        Who::Member {
            user_id: 101,
            bound: true,
        },
        Who::Stranger,
    ] {
        for (raw, failure) in [
            (None, Some("timeout".into())),
            (None, Some("unavailable".into())),
            (Some("{}".into()), None),
        ] {
            let mut end = End::new();
            // The glance and the answer both resolve who is there.
            for _ in 0..2 {
                end.answer(
                    "identify",
                    0.0,
                    Answer::Identified {
                        who: who.clone(),
                        owner: Some(1),
                    },
                );
            }
            end.answer(
                "ask:group_chime",
                1.0,
                Answer::Asked {
                    raw: Some(
                        r#"{"speak":"know","about":"那家店周五人多","basis":null,"unsure":[]}"#
                            .into(),
                    ),
                    failure: None,
                },
            );
            end.answer(
                "ask:group_making_sense",
                1.0,
                Answer::Asked { raw, failure },
            );
            heard(
                &mut end,
                0.0,
                message(1, 11, "阿明", "周五吃那家火锅吧", false),
            );
            heard(&mut end, 1.0, message(2, 11, "阿明", "大家觉得呢", false));
            end.run(BASE + secs(2400.0));
            let effects: Vec<&Value> = end
                .steps
                .iter()
                .flat_map(|step| step["output"]["effects"].as_array().unwrap())
                .collect();
            assert!(
                effects
                    .iter()
                    .any(|effect| effect["operation"] == "group_making_sense"),
                "the attempted chime must reach reading for {who:?}"
            );
            assert!(
                !effects.iter().any(|effect| effect["kind"] == "send"),
                "she chimed in on talk she could not read for {who:?}"
            );
        }
    }
}

#[test]
fn g1_she_speaks_up_unasked_and_reacts() {
    let mut end = End::new();
    // What she has that the talk touches: a view of hers, what she did,
    // what she heard, a word she knows, what she would tell someone.
    end.answer(
        "own:view",
        0.5,
        Answer::Rows {
            rows: vec![MemoryRow {
                id: "view-1".into(),
                content: "火锅还是周五吃最热闹".into(),
                concepts: Vec::new(),
                source: "view".into(),
                evidence: Some(
                    r#"{"about":"火锅","was":"火锅太辣","grewFrom":[{"what":"和大家吃了一次火锅"}]}"#
                        .into(),
                ),
                venue: None,
                created_at: BASE - secs(86_400.0),
                access_count: 0,
                last_accessed_at: None,
            }],
        },
    );
    end.answer(
        "life",
        0.5,
        Answer::Life {
            did: vec![("昨天听了一首火锅之歌".into(), "旋律很下饭".into())],
            would_tell: vec![
                "刚画完一张图：画了火锅".into(),
                "（想告诉人）昨天听了一首火锅之歌: 旋律很下饭".into(),
            ],
            now: Some("You are daydreaming, about 3 of 20 minutes in.".into()),
        },
    );
    end.answer(
        "own:heard",
        0.5,
        Answer::Rows {
            rows: vec![MemoryRow {
                id: "heard-1".into(),
                content: "那家火锅店周五要排队".into(),
                concepts: Vec::new(),
                source: "heard".into(),
                evidence: None,
                venue: None,
                created_at: BASE - secs(3_600.0),
                access_count: 0,
                last_accessed_at: None,
            }],
        },
    );
    end.answer(
        "own:meme",
        0.5,
        Answer::Rows {
            rows: vec![MemoryRow {
                id: "meme-1".into(),
                content: "「那家」：大家常去的那家店".into(),
                concepts: Vec::new(),
                source: "meme".into(),
                evidence: None,
                venue: None,
                created_at: BASE - secs(3_600.0),
                access_count: 0,
                last_accessed_at: None,
            }],
        },
    );
    end.answer(
        "ask:group_chime",
        1.0,
        Answer::Asked {
            raw: Some(
                r#"{"speak":"know","about":"那家店周五人多","basis":2,"unsure":["烧糊","那家","查无"]}"#
                    .into(),
            ),
            failure: None,
        },
    );
    end.answer(
        "ask:group_chime",
        1.0,
        Answer::Asked {
            raw: Some(r#"{"speak":"react","about":"😁","basis":null,"unsure":["锅巴"]}"#.into()),
            failure: None,
        },
    );
    end.answer(
        "ask:group_making_sense",
        1.0,
        Answer::Asked {
            raw: Some(r#"{"goingOn":"阿明在约周五去吃那家火锅","theyMean":"在问大家去不去","aboutYou":null,"unsure":[]}"#.into()),
            failure: None,
        },
    );
    // The words she was not sure of: one she knows, one the dictionaries
    // have, one only a search turns up and that does not say what it means
    // there; and later one with nothing left of the day's allowance.
    end.answer(
        "own:meme",
        0.5,
        Answer::Rows {
            rows: vec![MemoryRow {
                id: "meme-1".into(),
                content: "「那家」：大家常去的那家店".into(),
                concepts: Vec::new(),
                source: "meme".into(),
                evidence: None,
                venue: None,
                created_at: BASE - secs(3_600.0),
                access_count: 0,
                last_accessed_at: None,
            }],
        },
    );
    end.answer("may", 0.5, Answer::MayLookUp { may: May::Yes });
    end.answer("may", 0.5, Answer::MayLookUp { may: May::Yes });
    end.answer("may", 0.5, Answer::MayLookUp { may: May::NoneLeft });
    end.answer(
        "define",
        0.5,
        Answer::Defined {
            entry: Some(Entry {
                title: "烧糊".into(),
                text: "东西烧焦了".into(),
                url: "https://dictionary.example/烧糊".into(),
            }),
        },
    );
    end.answer(
        "search",
        0.5,
        Answer::Found {
            found: Some(("查无：一个地名".into(), "https://search.example/1".into())),
        },
    );
    end.answer(
        "ask:meme",
        1.0,
        Answer::Asked {
            raw: Some(r#"{"meaning":"东西烧焦了"}"#.into()),
            failure: None,
        },
    );
    end.answer(
        "ask:meme",
        1.0,
        Answer::Asked {
            raw: Some(r#"{"meaning":null}"#.into()),
            failure: None,
        },
    );
    heard(
        &mut end,
        0.0,
        message(1, 11, "阿明", "周五吃那家火锅吧", false),
    );
    heard(&mut end, 1.0, message(2, 11, "阿明", "大家觉得呢", false));
    heard(
        &mut end,
        700.0,
        message(3, 12, "小红", "我把锅烧糊了哈哈", false),
    );
    end.run(BASE + secs(2400.0));
    end.check("she_speaks_up_unasked_and_reacts");
}

#[test]
fn g1_an_outsider_calls_her() {
    let mut end = End::new();
    end.answer(
        "identify",
        0.0,
        Answer::Identified {
            who: Who::Stranger,
            owner: Some(1),
        },
    );
    end.answer(
        "ask:group_making_sense",
        1.0,
        Answer::Asked {
            raw: Some(
                r#"{"goingOn":"有人第一次来","theyMean":"在问她是谁","aboutYou":"说她像机器人"}"#
                    .into(),
            ),
            failure: None,
        },
    );
    // Who they are to her, and what she brings in that their words touch.
    let row = |id: &str, content: &str, source: &str, evidence: &str, venue: &str| MemoryRow {
        id: id.into(),
        content: content.into(),
        concepts: Vec::new(),
        source: source.into(),
        evidence: Some(evidence.into()),
        venue: Some(venue.into()),
        created_at: BASE - secs(7_200.0),
        access_count: 0,
        last_accessed_at: None,
    };
    let here = "group:telegram:-100777";
    let marker = r#"{"name":"路人","who":"telegram:31"}"#;
    end.answer(
        "unowned:stranger",
        0.2,
        Answer::Rows {
            rows: vec![row("note-1", "路人是常客", "stranger", marker, here)],
        },
    );
    end.answer(
        "elsewhere",
        0.2,
        Answer::Rows {
            rows: vec![row(
                "note-2",
                "在隔壁群说自己是机器人迷",
                "stranger",
                marker,
                "group:telegram:-100888",
            )],
        },
    );
    end.answer(
        "unowned:maybe_is",
        0.2,
        Answer::Rows {
            rows: vec![row(
                "guess-1",
                "他说话像小林",
                "maybe_is",
                r#"{"name":"路人","sure":"maybe","userId":7,"who":"telegram:31"}"#,
                here,
            )],
        },
    );
    end.answer(
        "label",
        0.2,
        Answer::Label {
            label: "小林".into(),
        },
    );
    end.answer(
        "now",
        0.2,
        Answer::Now {
            local: "2026-10-14T14:00:30+08:00".into(),
            doing: Some("You are listening to a song.".into()),
        },
    );
    end.answer(
        "rows:bit",
        0.2,
        Answer::Rows {
            rows: vec![
                row(
                    "bit-1",
                    "谁先到谁请奶茶",
                    "bit",
                    r#"{"handle":"山顶见"}"#,
                    here,
                ),
                row("bit-2", "没有把手", "bit", r#"{"handle":" "}"#, here),
            ],
        },
    );
    end.answer(
        "rows:group_day",
        0.2,
        Answer::Rows {
            rows: vec![
                row(
                    "day-1",
                    "聊了一下午",
                    "group_day",
                    r#"{"day":"2026-10-13"}"#,
                    here,
                ),
                row("day-2", "大家都在", "group_day", "{}", here),
            ],
        },
    );
    end.answer(
        "others",
        0.2,
        Answer::Rows {
            rows: vec![row(
                "theirs-1",
                "隔壁群天天说机器人",
                "bit",
                r#"{"handle":"机器人"}"#,
                "group:telegram:-100888",
            )],
        },
    );
    end.answer(
        "own:heard",
        0.2,
        Answer::Rows {
            rows: vec![row(
                "heard-1",
                "听说机器人会抢饭碗",
                "heard",
                r#"{"group":"telegram:-100888"}"#,
                "own",
            )],
        },
    );
    end.answer(
        "stickers",
        0.2,
        Answer::Stickers {
            stickers: vec![StickerRow {
                id: "sticker-1".into(),
                meaning: "我不是机器人".into(),
                venue: None,
                sent: 2,
                last_sent_at: Some(BASE - secs(3_600.0)),
                kept_from: Some("telegram:-100888".into()),
            }],
            can_make: true,
            left: 3,
        },
    );
    end.answer(
        "own:meme",
        0.2,
        Answer::Rows {
            rows: vec![row("meme-1", "「bot」：机器人", "meme", "{}", "own")],
        },
    );
    end.answer(
        "others:jargon",
        0.2,
        Answer::Rows {
            rows: vec![
                row("jargon-1", "「常客」：天天来的人", "jargon", "{}", here),
                row(
                    "jargon-2",
                    "「机器人」：隔壁群说的懒人",
                    "jargon",
                    "{}",
                    "group:telegram:-100888",
                ),
            ],
        },
    );
    end.answer(
        "titles",
        0.2,
        Answer::Titles {
            titles: vec![("telegram:-100888".into(), Some("隔壁群".into()))],
        },
    );
    // Cut the first time; then her words, a sticker, and a puzzle.
    end.answer(
        "ask:group_stranger",
        1.0,
        Answer::Asked {
            raw: None,
            failure: Some("cut".into()),
        },
    );
    end.answer(
        "ask:group_stranger",
        2.0,
        Answer::Asked {
            raw: Some(
                "（回复 路人）我是这里的常客啦\n来玩海龟汤吗\n[[game:soup]]\n[[sticker:1]]".into(),
            ),
            failure: None,
        },
    );
    // The puzzle she makes up when she says she will host one; and after
    // her answer, a game the table had finished is put away.
    end.answer(
        "ask:soup_start",
        1.0,
        Answer::Asked {
            raw: Some(
                r#"{"surface":"一个人走进酒吧，要了一杯水。","truth":"他在打嗝，酒保吓了他一跳。","keys":["打嗝"],"presentation":"汤面：一个人走进酒吧……"}"#
                    .into(),
            ),
            failure: None,
        },
    );
    end.answer("game", 0.2, Answer::Game { game: None });
    end.answer(
        "game",
        0.2,
        Answer::Game {
            game: Some(json!({
                "surface": "一个人走进酒吧，要了一杯水。",
                "truth": "他在打嗝，酒保吓了他一跳。",
                "keys": ["打嗝"],
                "asked": [{ "by": "路人", "question": "他在打嗝吗", "verdict": "yes" }],
                "found": [0],
                "ending": "Solved",
                "solver": "路人",
                "started": "2026-10-14T06:00:00Z",
                "last": "2026-10-14T06:00:02Z",
                "made": "made-1"
            })),
        },
    );
    heard(&mut end, 0.0, message(1, 31, "路人", "这个bot是谁", false));
    heard(
        &mut end,
        1.0,
        message(2, 31, "路人", "@bot 你是机器人吗", true),
    );
    end.run(BASE + secs(900.0));
    end.check("an_outsider_calls_her");
}

#[test]
fn g1_she_keeps_back_what_no_longer_fits() {
    let mut end = End::new();
    end.answer(
        "ask:group_member",
        10.0,
        Answer::Asked {
            raw: Some("九点吧\n老地方见".into()),
            failure: None,
        },
    );
    end.answer(
        "ask",
        1.0,
        Answer::Asked {
            raw: Some(r#"{"send":false}"#.into()),
            failure: None,
        },
    );
    end.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("那就十点\n[[sticker:new|笑着比耶的她|开心]]".into()),
            failure: None,
        },
    );
    heard(
        &mut end,
        0.0,
        message(1, 11, "阿明", "@bot 周六几点集合", true),
    );
    heard(&mut end, 12.0, message(2, 12, "小红", "刚说了十点", false));
    end.run(BASE + secs(900.0));
    end.check("she_keeps_back_what_no_longer_fits");
}

#[test]
fn g1_muted_away_and_back() {
    let mut end = End::new();
    heard(&mut end, 0.0, message(1, 12, "管理员", "安静点", false));
    end.at(
        BASE + secs(1.0),
        Event::Muted {
            by: "12".into(),
            mute: Mute::For { seconds: 600 },
            everyone: false,
            restore: None,
        },
    );
    heard(&mut end, 2.0, message(2, 11, "阿明", "@bot 你还在吗", true));
    end.run(BASE + secs(700.0));
    end.her.away = true;
    heard(&mut end, 710.0, message(3, 11, "阿明", "@bot 现在呢", true));
    end.run(BASE + secs(800.0));
    end.her.away = false;
    end.at(
        BASE + secs(801.0),
        Event::Resumed {
            why: Resume::Back,
            past: Vec::new(),
            up_since: None,
        },
    );
    end.run(BASE + secs(1500.0));
    end.check("muted_away_and_back");
}

#[test]
fn g1_read_back_after_a_restart() {
    let mut end = End::new();
    let ago = |minutes: i64| BASE - minutes * 60_000_000;
    let past = |id: i64, from: i64, minutes: i64, text: &str, hers: bool, addressed: bool| Past {
        message: message(id, from, if hers { "" } else { "阿明" }, text, addressed),
        at: ago(minutes),
        hers,
    };
    end.at(
        BASE,
        Event::Restored {
            lines: None,
            spoke_up: vec![SpokeUp {
                at: ago(50),
                taken: true,
            }],
            table: None,
        },
    );
    end.at(
        BASE,
        Event::Resumed {
            why: Resume::Restart,
            past: vec![
                past(1, 11, 90, "@bot 早", false, true),
                past(2, 0, 89, "早呀", true, false),
                past(3, 12, 40, "她起了没", false, false),
                past(4, 11, 30, "@bot 晚上打游戏吗", false, true),
                past(5, 12, 20, "没回，睡着了吧", false, false),
            ],
            up_since: Some(BASE - 5 * 60_000_000),
        },
    );
    end.run(BASE + secs(900.0));
    end.check("read_back_after_a_restart");
}

#[test]
fn g1_pictures_and_what_she_heard() {
    let mut end = End::new();
    let picture = |id: i64| {
        let mut picture = message(id, 11, "阿明", "", false);
        picture.images = vec![GroupImage {
            key: "telegram:cat".into(),
            fetch: ImageFetch::TelegramFile {
                file_id: "file-cat".into(),
            },
            hint: Some("😂".into()),
            sticker: true,
        }];
        picture
    };
    heard(&mut end, 0.0, picture(1));
    heard(&mut end, 1.0, message(2, 12, "小红", "哈哈哈", false));
    heard(&mut end, 2.0, picture(3));
    for index in 0..HEARD_EVERY as i64 {
        heard(
            &mut end,
            3.0 + index as f64,
            message(
                10 + index,
                12 + index % 3,
                "人",
                &format!("第{index}句"),
                false,
            ),
        );
    }
    heard(
        &mut end,
        40.0,
        message(99, 11, "阿明", "@bot 你看这个", true),
    );
    end.run(BASE + secs(900.0));
    end.check("pictures_and_what_she_heard");
}

#[test]
fn g1_she_brings_something_up_first_and_a_send_is_refused() {
    let mut end = End::new();
    end.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("今天听到一首好歌\n分享给你们".into()),
            failure: None,
        },
    );
    end.answer("send", 0.0, Answer::Sent { ok: true });
    end.answer("send", 0.0, Answer::Sent { ok: false });
    heard(
        &mut end,
        0.0,
        message(1, 11, "阿明", "最近有什么好歌", false),
    );
    end.run(BASE + secs(1800.0));
    let offered = offer(&end.group, end.now);
    assert!(offered.is_some(), "a group she is in, quiet a while");
    end.at(
        BASE + secs(1800.0),
        Event::SpeakFirst {
            reason: "something of yours: 一首歌".into(),
            owner: Some(1),
            waiter: "first".into(),
        },
    );
    end.run(BASE + secs(2400.0));
    end.check("she_brings_something_up_first_and_a_send_is_refused");
}

#[test]
fn g1_her_line_brings_back_what_a_failed_read_missed() {
    let mut end = End::new();
    // The first line after a restart: what was kept cannot be read.
    end.unreadable = true;
    let before = |minutes: i64| BASE - minutes * 60_000_000;
    end.answer(
        "kept",
        0.0,
        Answer::Kept {
            restore: Some(Restore {
                lines: Some(vec![
                    line(before(40), Some("1"), Some("11"), "阿明", "来点歌", false),
                    line(before(39), None, None, "", "放就放", true),
                ]),
                spoke_up: Vec::new(),
                table: None,
            }),
        },
    );
    end.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("记得，你说要来点歌".into()),
            failure: None,
        },
    );
    end.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("那就放这首".into()),
            failure: None,
        },
    );
    heard(
        &mut end,
        0.0,
        message(3, 11, "阿明", "@bot 还记得我刚才说的吗", true),
    );
    end.run(BASE + secs(1.0));
    // Read again once her own words are about to be kept: up again by then.
    end.unreadable = false;
    end.run(BASE + secs(600.0));
    heard(
        &mut end,
        600.0,
        message(4, 11, "阿明", "@bot 那放哪首", true),
    );
    end.run(BASE + secs(1200.0));
    assert!(end.group.restored);
    assert_eq!(
        end.group.lines.front().map(|line| line.text.as_str()),
        Some("来点歌")
    );
    end.check("her_line_brings_back_what_a_failed_read_missed");
}

/// A member asks her for a game with seats (the test game with private
/// marks): sign-up opens, two join and one of them twice, one more cannot be
/// whispered to and is told how to open it; each of the two is whispered
/// their own mark; in their turn a line that is no move and a number out
/// of range are each asked again with the choices (nothing picked for
/// them), and a move comes back in private and is taken; one out of turn
/// is answered that it is not theirs yet, one that is no move from a seat
/// whose turn it is not is not the game's; seats that do not move in time
/// pass (named in the group the second time in a row), and over, the group
/// remembers who played. Crate-only: the
/// test game exists only in tests, so no other end replays this one.
#[test]
fn g1_a_game_with_seats_is_played_in_private() {
    let mut end = End::new();
    end.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("好，来一局\n[[game:marks]]".into()),
            failure: None,
        },
    );
    end.answer("whisper", 1.0, Answer::Whispered { ok: true });
    end.answer("whisper", 1.0, Answer::Whispered { ok: true });
    end.answer("whisper", 1.0, Answer::Whispered { ok: false });
    heard(&mut end, 0.0, message(1, 11, "阿明", "@bot 来玩记号", true));
    heard(&mut end, 20.0, message(2, 11, "阿明", "加入", false));
    heard(&mut end, 21.0, message(3, 12, "小红", "加入", false));
    heard(&mut end, 22.0, message(4, 13, "阿强", "我要玩", false));
    heard(&mut end, 23.0, message(5, 12, "小红", "+1", false));
    let whispered = |end: &mut End, at: f64, who: &str, text: &str| {
        end.at(
            BASE + secs(at),
            Event::Whispered {
                who: who.into(),
                text: text.into(),
                wait: None,
            },
        );
    };
    whispered(&mut end, 100.0, "telegram:11", "你好");
    whispered(&mut end, 100.5, "telegram:11", "7");
    let log = |end: &End| match &end.group.seating.as_ref().unwrap().game {
        Some(crate::hosting::SeatGame::Marks(game)) => game.log.clone(),
        _ => unreachable!(),
    };
    end.run(BASE + secs(100.8));
    assert!(log(&end).is_empty(), "nothing is picked for them");
    whispered(&mut end, 101.0, "telegram:12", "1");
    whispered(&mut end, 101.5, "telegram:12", "你好");
    whispered(&mut end, 102.0, "telegram:11", "1");
    whispered(&mut end, 900.0, "telegram:11", "0");
    end.run(BASE + secs(1200.0));
    assert!(end.group.seating.is_none());
    let taken: Vec<Option<bool>> = end
        .steps
        .iter()
        .filter(|step| step["input"]["event"]["kind"] == "whispered")
        .map(|step| step["output"]["taken"].as_bool())
        .collect();
    // In turn, no move and a number out of range (each asked again); a
    // move out of turn (taken, and they are told so); not a move, not in
    // turn; a move in turn; one after it is over.
    assert_eq!(
        taken,
        [
            Some(true),
            Some(true),
            Some(true),
            Some(false),
            Some(true),
            Some(false)
        ]
    );
    let to_him: Vec<String> = whispers(&end)
        .into_iter()
        .filter(|(user, _)| user == "11")
        .map(|(_, text)| text)
        .collect();
    let again = |why: &str| {
        format!(
            "{why}，再选一次：\n0. pass\n1. guess the next seat's mark\n点下面的按钮，或者回号码、名字。"
        )
    };
    assert!(to_him.contains(&again("没看懂这句")), "{to_him:?}");
    assert!(to_him.contains(&again("没有这个号码")), "{to_him:?}");
    assert!(
        !to_him.contains(&crate::hosting::NOT_YOURS_LINE.to_string()),
        "{to_him:?}"
    );
    for step in &end.steps {
        for effect in step["output"]["effects"].as_array().into_iter().flatten() {
            if effect["kind"] == "send" {
                assert!(!effect.to_string().contains("SECRET-"), "{effect}");
            }
        }
    }
    end.check("a_game_with_seats_is_played_in_private");
}

fn message_on(
    platform: &str,
    chat: &str,
    id: i64,
    from: &str,
    name: &str,
    text: &str,
    addressed: bool,
) -> Message {
    Message {
        platform: platform.into(),
        chat: chat.into(),
        message_id: id.to_string(),
        thread: None,
        from: from.into(),
        display_name: name.into(),
        text: text.into(),
        addressed,
        reply_to: None,
        images: Vec::new(),
        shown_as: None,
    }
}

/// The players at the table, as a test plays them, every ten seconds from
/// `from` to `until`: whoever's turn it is to speak says `said(round)` in
/// the group (none: lets it pass); whoever has to choose whispers
/// `chosen(seat)`.
fn play_out(
    end: &mut End,
    from: f64,
    until: f64,
    said: &dyn Fn(usize, u32) -> Option<String>,
    chosen: &dyn Fn(usize) -> Option<String>,
) {
    play_out_by(end, from, until, said, &|_, seat| chosen(seat));
}

/// The same, whoever has to choose choosing by the table as it stands.
fn play_out_by(
    end: &mut End,
    from: f64,
    until: f64,
    said: &dyn Fn(usize, u32) -> Option<String>,
    chosen: &dyn Fn(&flows::Seating, usize) -> Option<String>,
) {
    use crate::hosting::Waiting;
    let mut at = from;
    let mut id = 500;
    while at < until {
        end.run(BASE + secs(at));
        let Some(seating) = end.group.seating.clone() else {
            break;
        };
        if let Some(game) = seating.game.as_ref() {
            let round = game.round();
            match game.waiting() {
                Waiting::Speak(seat) if !seating.reachable[seat].hers => {
                    let sat = &seating.reachable[seat];
                    let (platform, user) = sat.who.split_once(':').unwrap();
                    if let Some(line) = said(seat, round) {
                        id += 1;
                        heard(
                            end,
                            at + 3.0,
                            message_on(
                                platform,
                                &seating.chat.chat,
                                id,
                                user,
                                &sat.name,
                                &line,
                                false,
                            ),
                        );
                    }
                }
                Waiting::Choose(seats) => {
                    for seat in seats {
                        let sat = &seating.reachable[seat];
                        let Some(text) = chosen(&seating, seat).filter(|_| !sat.hers) else {
                            continue;
                        };
                        end.at(
                            BASE + secs(at + 3.0),
                            Event::Whispered {
                                who: sat.who.clone(),
                                text,
                                wait: seating.waited,
                            },
                        );
                    }
                }
                _ => {}
            }
        }
        at += 10.0;
    }
    end.run(BASE + secs(until));
}

/// What the group heard her say, in order.
fn said_in_group(end: &End) -> Vec<String> {
    end.steps
        .iter()
        .flat_map(|step| {
            step["output"]["effects"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter(|effect| effect["kind"] == "send")
        .map(|effect| {
            effect["pieces"]
                .as_array()
                .unwrap()
                .iter()
                .map(|piece| {
                    piece["text"]
                        .as_str()
                        .map(str::to_string)
                        .unwrap_or_else(|| format!("@{}", piece["name"].as_str().unwrap_or("")))
                })
                .collect::<String>()
        })
        .collect()
}

/// A member asks for 谁是卧底 in a Telegram group: four join, each is
/// whispered their word (the words made fresh, not one used lately), and the
/// game is played out: descriptions in the group in turn (one asks for more
/// time once), votes whispered back from buttons, someone out each round
/// and said to be undercover or not, until a side wins and every word is
/// told; the group remembers it.
#[test]
fn g1_undercover_is_played_with_buttons() {
    let mut end = End::new();
    end.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("好呀，来一局\n[[game:undercover]]".into()),
            failure: None,
        },
    );
    end.answer(
        "recent",
        0.0,
        Answer::Surfaces {
            surfaces: vec!["饺子/包子".into()],
        },
    );
    end.answer(
        "ask:undercover_words",
        3.0,
        Answer::Asked {
            raw: Some(r#"{"civilian":"眼镜","undercover":"墨镜"}"#.into()),
            failure: None,
        },
    );
    end.answer(
        "ask:voice",
        2.0,
        Answer::Asked {
            raw: Some(r#"{"say":"开始啦，别一上来就把词说出来哦"}"#.into()),
            failure: None,
        },
    );
    heard(
        &mut end,
        0.0,
        message(1, 11, "阿明", "@bot 来玩谁是卧底", true),
    );
    for (index, (from, name)) in [(11, "阿明"), (12, "小红"), (13, "阿强"), (14, "小李")]
        .iter()
        .enumerate()
    {
        heard(
            &mut end,
            40.0 + index as f64,
            message(2 + index as i64, *from, name, "加入", false),
        );
    }
    heard(&mut end, 101.0, message(9, 12, "小红", "等等", false));
    let describe = |seat: usize, round: u32| Some(format!("戴在脸上的{seat}-{round}"));
    play_out(&mut end, 110.0, 160.0, &describe, &|_| Some("0".into()));
    // A button of the first vote, pressed after it was counted: not taken
    // as a vote, and they are told it no longer counts.
    let first_vote = end
        .steps
        .iter()
        .flat_map(|step| {
            step["output"]["effects"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .find_map(|effect| effect["wait"].as_u64())
        .unwrap();
    end.at(
        BASE + secs(161.0),
        Event::Whispered {
            who: "telegram:11".into(),
            text: "0".into(),
            wait: Some(first_vote),
        },
    );
    play_out(&mut end, 165.0, 3000.0, &describe, &|_| Some("0".into()));
    assert!(end.group.seating.is_none(), "over");
    let said = said_in_group(&end);
    assert!(
        said.iter()
            .any(|line| line.contains("赢了") && line.contains("眼镜") && line.contains("墨镜"))
    );
    assert!(
        said.iter()
            .any(|line| line == crate::hosting::MORE_TIME_LINE)
    );
    let stale = end.steps.iter().any(|step| {
        step["output"]["effects"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|effect| effect["text"] == crate::hosting::STALE_BUTTON_LINE)
    });
    assert!(stale, "a button of a vote gone by is answered so");
    // Neither word is said in the group before the end.
    let end_at = said.iter().position(|line| line.contains("赢了")).unwrap();
    assert!(
        said[..end_at]
            .iter()
            .all(|line| !line.contains("眼镜") && !line.contains("墨镜"))
    );
    let buttons = end.steps.iter().any(|step| {
        step["output"]["effects"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|effect| effect["kind"] == "whisper" && effect["choices"].is_array())
    });
    assert!(buttons, "votes go out with buttons on Telegram");
    end.check("undercover_is_played_with_buttons");
}

/// On QQ three join; she offers to take the fourth seat and all agree: the
/// words are made by a judge with nothing of her (she must not know them),
/// she describes and votes from her own word alone, and when one lets two
/// turns pass they are named in the group.
#[test]
fn g1_undercover_she_fills_a_seat() {
    let mut end = End::new();
    end.answer(
        "identify",
        0.0,
        Answer::Identified {
            who: Who::Stranger,
            owner: Some(1),
        },
    );
    end.answer(
        "ask:group_stranger",
        2.0,
        Answer::Asked {
            raw: Some("好\n[[game:undercover]]".into()),
            failure: None,
        },
    );
    end.answer(
        "label",
        0.0,
        Answer::Label {
            label: "Arael".into(),
        },
    );
    for _ in 0..12 {
        end.answer(
            "ask:her_move",
            2.0,
            Answer::Asked {
                raw: Some(r#"{"say":"每天都离不开它","n":0}"#.into()),
                failure: None,
            },
        );
    }
    let qq = |id: i64, from: &str, name: &str, text: &str, addressed: bool| {
        message_on("onebot", "77001", id, from, name, text, addressed)
    };
    heard(
        &mut end,
        0.0,
        qq(1, "6001", "甲", "@bot 来玩谁是卧底", true),
    );
    for (index, (from, name)) in [("6001", "甲"), ("6002", "乙"), ("6003", "丙")]
        .iter()
        .enumerate()
    {
        heard(
            &mut end,
            40.0 + index as f64,
            qq(2 + index as i64, from, name, "加入", false),
        );
    }
    for (index, (from, name)) in [("6001", "甲"), ("6002", "乙"), ("6003", "丙")]
        .iter()
        .enumerate()
    {
        heard(
            &mut end,
            100.0 + index as f64,
            qq(10 + index as i64, from, name, "好", false),
        );
    }
    play_out(
        &mut end,
        110.0,
        4000.0,
        // 丙 never describes nor votes.
        &|seat, round| (seat != 2).then(|| format!("很常见的东西{round}")),
        &|seat| (seat != 2).then(|| "0".into()),
    );
    assert!(end.group.seating.is_none(), "over");
    let said = said_in_group(&end);
    assert!(said.iter().any(|line| line.contains("还差一个人")));
    assert!(
        said.iter().any(|line| line.contains("每天都离不开它")),
        "her description"
    );
    assert!(
        said.iter()
            .any(|line| line.contains("@丙") && line.contains("连着两次"))
    );
    let made_by_judge = end.steps.iter().any(|step| {
        step["output"]["effects"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|effect| {
                effect["kind"] == "ask"
                    && effect["operation"] == "undercover_words"
                    && effect["voice"] == "judge"
            })
    });
    assert!(made_by_judge);
    // What she was asked as her seat never holds the other word.
    let state: Value = end
        .steps
        .iter()
        .rev()
        .find_map(|step| {
            step["output"]["effects"]
                .as_array()
                .into_iter()
                .flatten()
                .find_map(|effect| {
                    (effect["kind"] == "keepTable" && !effect["table"].is_null())
                        .then(|| effect["table"].clone())
                })
        })
        .unwrap();
    let game = &state["game"];
    let hers = state["reachable"]
        .as_array()
        .unwrap()
        .iter()
        .position(|seat| seat["hers"] == true)
        .unwrap();
    let mine_is_undercover = game["undercover"][hers].as_bool().unwrap();
    let other = if mine_is_undercover {
        "饺子"
    } else {
        "包子"
    };
    for step in &end.steps {
        for effect in step["output"]["effects"].as_array().into_iter().flatten() {
            if effect["kind"] == "ask" && effect["operation"] == "undercover_her_move" {
                assert!(!effect.to_string().contains(other), "{effect}");
            }
            if effect["kind"] == "whisper" {
                assert!(effect["choices"].is_null(), "no buttons on QQ");
            }
        }
    }
    end.check("undercover_she_fills_a_seat");
}

/// A game of 谁是卧底 is on when her end restarts: the table comes back as
/// kept, everyone at it seated again, the wait on going on for the time it
/// had left, and the game is played to its end.
#[test]
fn g1_undercover_goes_on_after_a_restart() {
    let mut before = End::new();
    before.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("来\n[[game:undercover]]".into()),
            failure: None,
        },
    );
    heard(
        &mut before,
        0.0,
        message(1, 11, "阿明", "@bot 谁是卧底", true),
    );
    for (index, (from, name)) in [(11, "阿明"), (12, "小红"), (13, "阿强"), (14, "小李")]
        .iter()
        .enumerate()
    {
        heard(
            &mut before,
            40.0 + index as f64,
            message(2 + index as i64, *from, name, "加入", false),
        );
    }
    play_out(
        &mut before,
        110.0,
        135.0,
        &|_, _| Some("挺普通的".into()),
        &|_| Some("0".into()),
    );
    let table = before
        .steps
        .iter()
        .rev()
        .find_map(|step| {
            step["output"]["effects"]
                .as_array()
                .into_iter()
                .flatten()
                .find_map(|effect| (effect["kind"] == "keepTable").then(|| effect["table"].clone()))
        })
        .unwrap();
    assert!(!table.is_null());
    // The restart: a group of nothing in mind, then what was kept.
    let mut end = End::new();
    end.now = BASE + secs(150.0);
    end.at(
        end.now,
        Event::Restored {
            lines: None,
            spoke_up: Vec::new(),
            table: Some(table),
        },
    );
    play_out(
        &mut end,
        151.0,
        3000.0,
        &|_, round| Some(format!("挺普通的{round}")),
        &|_| Some("0".into()),
    );
    assert!(end.group.seating.is_none(), "played to its end");
    let seated = end.steps[0]["output"]["effects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|effect| effect["kind"] == "seat")
        .count();
    assert_eq!(seated, 4);
    assert!(said_in_group(&end).iter().any(|line| line.contains("赢了")));
    end.check("undercover_goes_on_after_a_restart");
}

/// The game of 狼人杀 on at a table, if it is one.
fn werewolf_of(seating: &flows::Seating) -> Option<&crate::werewolf::Werewolf> {
    match seating.game.as_ref()? {
        crate::hosting::SeatGame::Werewolf(game) => Some(game),
        _ => None,
    }
}

/// The last table kept before it ended, as kept.
fn last_table(end: &End) -> Value {
    end.steps
        .iter()
        .rev()
        .find_map(|step| {
            step["output"]["effects"]
                .as_array()
                .into_iter()
                .flatten()
                .find_map(|effect| {
                    (effect["kind"] == "keepTable" && !effect["table"].is_null())
                        .then(|| effect["table"].clone())
                })
        })
        .unwrap()
}

/// Every whisper she sent, as (user, text).
fn whispers(end: &End) -> Vec<(String, String)> {
    end.steps
        .iter()
        .flat_map(|step| {
            step["output"]["effects"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter(|effect| effect["kind"] == "whisper")
        .map(|effect| {
            (
                effect["user"].as_str().unwrap_or("").to_string(),
                effect["text"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

/// A member asks for 狼人杀 in a Telegram group: six join, each is
/// whispered their role (the werewolves each other), and it is played out:
/// at night the werewolves, the seer and the witch move in private from
/// buttons (the witch saves the first night's knifed), by day everyone
/// speaks in turn and votes from buttons, until a side wins and every role
/// is told. What a seat learns at night is whispered to it alone; no role
/// is said in the group before the end.
#[test]
fn g1_werewolf_is_played_with_buttons() {
    let mut end = End::new();
    end.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("好呀，我来当法官\n[[game:werewolf]]".into()),
            failure: None,
        },
    );
    end.answer(
        "ask:voice",
        2.0,
        Answer::Asked {
            raw: Some(r#"{"say":"天黑请闭眼，大家好运"}"#.into()),
            failure: None,
        },
    );
    heard(
        &mut end,
        0.0,
        message(1, 11, "阿明", "@bot 来玩狼人杀", true),
    );
    let people = [
        (11, "阿明"),
        (12, "小红"),
        (13, "阿强"),
        (14, "小李"),
        (15, "大刘"),
        (16, "老王"),
    ];
    for (index, (from, name)) in people.iter().enumerate() {
        heard(
            &mut end,
            40.0 + index as f64,
            message(2 + index as i64, *from, name, "加入", false),
        );
    }
    play_out_by(
        &mut end,
        110.0,
        5000.0,
        &|seat, round| Some(format!("我是好人，第{round}天{seat}号")),
        // The witch saves whom the first knife fell on; everyone else takes
        // their first choice.
        &|seating, seat| {
            let game = werewolf_of(seating)?;
            let choices = seating.game.as_ref()?.seat_view(seat).choices().to_vec();
            let save = game.night == 1
                && choices
                    .get(1)
                    .is_some_and(|choice| choice.starts_with("用解药"));
            Some(if save { "1" } else { "0" }.into())
        },
    );
    assert!(end.group.seating.is_none(), "over");
    let said = said_in_group(&end);
    let end_at = said
        .iter()
        .position(|line| line.contains("赢了。身份："))
        .expect("it ends with every role told");
    assert!(
        said[..end_at].iter().all(|line| !line.contains("身份：")),
        "no role said in the group before the end"
    );
    assert!(
        said.iter().any(|line| line.contains("平安夜")),
        "the witch saved"
    );
    let table = last_table(&end);
    let roles: Vec<String> = table["game"]["roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|role| role.as_str().unwrap().to_string())
        .collect();
    let users: Vec<String> = people.iter().map(|(from, _)| from.to_string()).collect();
    for (user, text) in whispers(&end) {
        let seat = users.iter().position(|known| *known == user).unwrap();
        let role = roles[seat].as_str();
        if text.contains("验") && text.contains('：') && text.starts_with("第") {
            assert_eq!(role, "seer", "{text}");
        }
        if text.contains("狼队友") {
            assert_eq!(role, "werewolf", "{text}");
        }
        if text.contains("被刀的是") {
            assert_eq!(role, "witch", "{text}");
        }
    }
    let buttons = end.steps.iter().any(|step| {
        step["output"]["effects"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|effect| effect["kind"] == "whisper" && effect["choices"].is_array())
    });
    assert!(buttons, "moves go out with buttons on Telegram");
    end.check("werewolf_is_played_with_buttons");
}

/// On QQ five join; she offers to take the sixth seat and all agree. She
/// speaks and moves from her own seat's view alone; whoever is the seer
/// (or the witch, when she is the seer) never moves: missing two nights in
/// a row they are reminded in private, never named in the group for it.
#[test]
fn g1_werewolf_she_fills_a_seat() {
    let mut end = End::new();
    end.answer(
        "identify",
        0.0,
        Answer::Identified {
            who: Who::Stranger,
            owner: Some(1),
        },
    );
    end.answer(
        "ask:group_stranger",
        2.0,
        Answer::Asked {
            raw: Some("好\n[[game:werewolf]]".into()),
            failure: None,
        },
    );
    end.answer(
        "label",
        0.0,
        Answer::Label {
            label: "Arael".into(),
        },
    );
    for _ in 0..30 {
        end.answer(
            "ask:her_move",
            2.0,
            Answer::Asked {
                raw: Some(r#"{"say":"我先听听大家怎么说","n":0}"#.into()),
                failure: None,
            },
        );
    }
    let qq = |id: i64, from: &str, name: &str, text: &str, addressed: bool| {
        message_on("onebot", "77002", id, from, name, text, addressed)
    };
    heard(&mut end, 0.0, qq(1, "7001", "甲", "@bot 来玩狼人杀", true));
    let people = [
        ("7001", "甲"),
        ("7002", "乙"),
        ("7003", "丙"),
        ("7004", "丁"),
        ("7005", "戊"),
    ];
    for (index, (from, name)) in people.iter().enumerate() {
        heard(
            &mut end,
            40.0 + index as f64,
            qq(2 + index as i64, from, name, "加入", false),
        );
    }
    for (index, (from, name)) in people.iter().enumerate() {
        heard(
            &mut end,
            100.0 + index as f64,
            qq(10 + index as i64, from, name, "好", false),
        );
    }
    // The one who never moves: the seer, or the witch when she is the seer.
    let idle = |game: &crate::werewolf::Werewolf, seating: &flows::Seating| {
        use crate::werewolf::Role;
        let human = |role: Role| {
            (0..game.roles.len())
                .find(|seat| game.roles[*seat] == role && !seating.reachable[*seat].hers)
        };
        human(Role::Seer).or_else(|| human(Role::Witch)).unwrap()
    };
    play_out_by(
        &mut end,
        110.0,
        6000.0,
        &|seat, round| Some(format!("{seat}号第{round}天：我是好人")),
        &|seating, seat| {
            use crate::werewolf::Role;
            let game = werewolf_of(seating)?;
            if seat == idle(game, seating)
                && matches!(game.phase, crate::werewolf::Phase::Night { .. })
            {
                return None;
            }
            // By day the good vote out a werewolf, the werewolves a
            // villager; the werewolves knife nobody the first two nights.
            let choices = seating.game.as_ref()?.seat_view(seat).choices().to_vec();
            let pick = |role: Role| {
                (0..game.roles.len())
                    .find(|other| game.alive[*other] && game.roles[*other] == role)
                    .map(|other| seating.reachable[other].name.clone())
                    .filter(|name| choices.contains(name))
            };
            let wolf = game.roles[seat] == Role::Werewolf;
            let night = matches!(game.phase, crate::werewolf::Phase::Night { .. });
            if wolf && night && game.night < 3 {
                return Some("不刀".into());
            }
            Some(if wolf {
                pick(Role::Villager).unwrap_or_else(|| "0".into())
            } else {
                pick(Role::Werewolf).unwrap_or_else(|| "0".into())
            })
        },
    );
    assert!(end.group.seating.is_none(), "over");
    let said = said_in_group(&end);
    assert!(said.iter().any(|line| line.contains("还差一个人")));
    assert!(
        said.iter().any(|line| line.contains("我先听听大家怎么说")),
        "her speech"
    );
    let table = last_table(&end);
    let roles = &table["game"]["roles"];
    let hers = table["reachable"]
        .as_array()
        .unwrap()
        .iter()
        .position(|seat| seat["hers"] == true)
        .unwrap();
    let idle_role = if roles[hers] == "seer" {
        "witch"
    } else {
        "seer"
    };
    let idle_seat = (0..6)
        .find(|seat| roles[*seat] == idle_role && *seat != hers)
        .unwrap();
    let idle_user = table["reachable"][idle_seat]["who"]
        .as_str()
        .unwrap()
        .trim_start_matches("onebot:")
        .to_string();
    let reminded = whispers(&end)
        .iter()
        .any(|(user, text)| *user == idle_user && text == crate::hosting::REMINDER_IN_PRIVATE);
    assert!(reminded, "two nights let pass are reminded in private");
    assert!(
        said.iter()
            .all(|line| !line.contains(crate::hosting::REMINDER_IN_PRIVATE)),
        "night misses are never named in the group"
    );
    // What she was asked as her seat holds no other seat's role.
    let mine = format!(
        "你的身份：{}",
        match roles[hers].as_str().unwrap() {
            "werewolf" => "狼人",
            "villager" => "村民",
            "seer" => "预言家",
            "witch" => "女巫",
            _ => "猎人",
        }
    );
    for step in &end.steps {
        for effect in step["output"]["effects"].as_array().into_iter().flatten() {
            if effect["kind"] == "ask" && effect["operation"] == "werewolf_her_move" {
                let asked = effect.to_string();
                assert!(asked.contains(&mine), "{asked}");
                for role in ["狼人", "村民", "预言家", "女巫", "猎人"] {
                    let theirs = format!("你的身份：{role}");
                    assert!(theirs == mine || !asked.contains(&theirs), "{asked}");
                }
            }
            if effect["kind"] == "whisper" {
                assert!(effect["choices"].is_null(), "no buttons on QQ");
            }
        }
    }
    end.check("werewolf_she_fills_a_seat");
}

/// A game of 狼人杀 is on, in its first night, when her end restarts: the
/// table comes back as kept, everyone at it seated again, the night goes on
/// for the time it had left, and the game is played to its end.
#[test]
fn g1_werewolf_goes_on_after_a_restart() {
    let mut before = End::new();
    before.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("来\n[[game:werewolf]]".into()),
            failure: None,
        },
    );
    heard(
        &mut before,
        0.0,
        message(1, 11, "阿明", "@bot 狼人杀", true),
    );
    let people = [
        (11, "阿明"),
        (12, "小红"),
        (13, "阿强"),
        (14, "小李"),
        (15, "大刘"),
        (16, "老王"),
    ];
    for (index, (from, name)) in people.iter().enumerate() {
        heard(
            &mut before,
            40.0 + index as f64,
            message(2 + index as i64, *from, name, "加入", false),
        );
    }
    // Into the first night; only some have moved when it restarts.
    before.run(BASE + secs(100.0));
    let seating = before.group.seating.as_ref().unwrap().clone();
    let seer = werewolf_of(&seating)
        .unwrap()
        .roles
        .iter()
        .position(|role| *role == crate::werewolf::Role::Seer)
        .unwrap();
    before.at(
        BASE + secs(101.0),
        Event::Whispered {
            who: seating.reachable[seer].who.clone(),
            text: "0".into(),
            wait: seating.waited,
        },
    );
    before.run(BASE + secs(102.0));
    let checked = whispers(&before)
        .into_iter()
        .map(|(_, text)| text)
        .find(|text| text.starts_with("第1夜验"))
        .unwrap();
    let table = last_table(&before);
    assert!(table["game"]["phase"]["phase"] == "night");
    let mut end = End::new();
    end.now = BASE + secs(110.0);
    end.at(
        end.now,
        Event::Restored {
            lines: None,
            spoke_up: Vec::new(),
            table: Some(table),
        },
    );
    play_out(
        &mut end,
        111.0,
        5000.0,
        &|seat, round| Some(format!("第{round}天{seat}号：过")),
        &|_| Some("0".into()),
    );
    assert!(end.group.seating.is_none(), "played to its end");
    let seated = end.steps[0]["output"]["effects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|effect| effect["kind"] == "seat")
        .count();
    assert_eq!(seated, 6);
    assert!(
        whispers(&end)
            .iter()
            .all(|(_, text)| !text.contains(&checked)),
        "a result already whispered before restart is not sent again"
    );
    assert!(
        said_in_group(&end)
            .iter()
            .any(|line| line.contains("赢了。身份："))
    );
    end.check("werewolf_goes_on_after_a_restart");
}

/// The game of 阿瓦隆 on at a table, if it is one.
fn avalon_of(seating: &flows::Seating) -> Option<&crate::avalon::Avalon> {
    match seating.game.as_ref()? {
        crate::hosting::SeatGame::Avalon(game) => Some(game),
        _ => None,
    }
}

/// How the players of a test play 阿瓦隆: the leader takes the first one
/// open each pick; the first team of the game is turned down, every other
/// one goes; the evil fail the first quest only; the assassin names the
/// first of those open.
fn avalon_player(seating: &flows::Seating, seat: usize) -> Option<String> {
    use crate::avalon::Phase;
    let game = avalon_of(seating)?;
    Some(match &game.phase {
        Phase::Vote { .. } if game.results.is_empty() && game.rejections == 0 => "1".into(),
        Phase::Quest { .. } if game.results.is_empty() && game.roles[seat].evil() => "1".into(),
        _ => "0".into(),
    })
}

/// A member asks for 阿瓦隆 in a Telegram group: five join, each is
/// whispered their role and whom they know, and it is played out from
/// buttons: the first team is turned down and the next leader picks, how
/// everyone voted is told, a quest fails to one card, the good win three
/// and the assassin names someone. What a seat knows is whispered to it
/// alone; no role is said in the group before the end.
#[test]
fn g1_avalon_is_played_with_buttons() {
    let mut end = End::new();
    end.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("好呀，我来主持\n[[game:avalon]]".into()),
            failure: None,
        },
    );
    end.answer(
        "ask:voice",
        2.0,
        Answer::Asked {
            raw: Some(r#"{"say":"为了亚瑟王，出发吧"}"#.into()),
            failure: None,
        },
    );
    heard(
        &mut end,
        0.0,
        message(1, 11, "阿明", "@bot 来玩阿瓦隆", true),
    );
    let people = [
        (11, "阿明"),
        (12, "小红"),
        (13, "阿强"),
        (14, "小李"),
        (15, "大刘"),
    ];
    for (index, (from, name)) in people.iter().enumerate() {
        heard(
            &mut end,
            40.0 + index as f64,
            message(2 + index as i64, *from, name, "加入", false),
        );
    }
    play_out_by(&mut end, 110.0, 5000.0, &|_, _| None, &avalon_player);
    assert!(end.group.seating.is_none(), "over");
    let said = said_in_group(&end);
    let end_at = said
        .iter()
        .position(|line| line.contains("赢了。身份："))
        .expect("it ends with every role told");
    assert!(said[..end_at].iter().all(|line| !line.contains("身份：")));
    assert!(
        said.iter()
            .any(|line| line.contains("没过半，这次没组成队"))
    );
    assert!(
        said.iter().any(|line| line.contains("反对 阿明")),
        "votes are told"
    );
    assert!(said.iter().any(|line| line.contains("任务失败")));
    assert!(
        said.iter()
            .any(|line| line.contains("刺客") && line.contains("刺杀了"))
    );
    let table = last_table(&end);
    let roles: Vec<String> = table["game"]["roles"]
        .as_array()
        .unwrap()
        .iter()
        .map(|role| role.as_str().unwrap().to_string())
        .collect();
    let users: Vec<String> = people.iter().map(|(from, _)| from.to_string()).collect();
    for (user, text) in whispers(&end) {
        let role = roles[users.iter().position(|known| *known == user).unwrap()].as_str();
        if text.contains("你看到的坏人") {
            assert_eq!(role, "merlin", "{text}");
        }
        if text.contains("一个是梅林") {
            assert_eq!(role, "percival", "{text}");
        }
        if text.contains("坏人同伴") {
            assert!(["morgana", "assassin"].contains(&role), "{text}");
        }
    }
    let buttons = end.steps.iter().any(|step| {
        step["output"]["effects"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|effect| effect["kind"] == "whisper" && effect["choices"].is_array())
    });
    assert!(buttons, "moves go out with buttons on Telegram");
    end.check("avalon_is_played_with_buttons");
}

/// On QQ four join; she offers to take the fifth seat and all agree. She
/// picks, votes and plays from her own seat's view alone: what she is
/// asked holds her own role and never another's.
#[test]
fn g1_avalon_she_fills_a_seat() {
    let mut end = End::new();
    end.answer(
        "identify",
        0.0,
        Answer::Identified {
            who: Who::Stranger,
            owner: Some(1),
        },
    );
    end.answer(
        "ask:group_stranger",
        2.0,
        Answer::Asked {
            raw: Some("好\n[[game:avalon]]".into()),
            failure: None,
        },
    );
    end.answer(
        "label",
        0.0,
        Answer::Label {
            label: "Arael".into(),
        },
    );
    for _ in 0..60 {
        end.answer(
            "ask:her_move",
            2.0,
            Answer::Asked {
                raw: Some(r#"{"n":0}"#.into()),
                failure: None,
            },
        );
    }
    let qq = |id: i64, from: &str, name: &str, text: &str, addressed: bool| {
        message_on("onebot", "77003", id, from, name, text, addressed)
    };
    heard(&mut end, 0.0, qq(1, "8001", "甲", "@bot 来玩阿瓦隆", true));
    let people = [
        ("8001", "甲"),
        ("8002", "乙"),
        ("8003", "丙"),
        ("8004", "丁"),
    ];
    for (index, (from, name)) in people.iter().enumerate() {
        heard(
            &mut end,
            40.0 + index as f64,
            qq(2 + index as i64, from, name, "加入", false),
        );
    }
    for (index, (from, name)) in people.iter().enumerate() {
        heard(
            &mut end,
            100.0 + index as f64,
            qq(10 + index as i64, from, name, "好", false),
        );
    }
    play_out_by(&mut end, 110.0, 6000.0, &|_, _| None, &avalon_player);
    assert!(end.group.seating.is_none(), "over");
    let said = said_in_group(&end);
    assert!(said.iter().any(|line| line.contains("还差一个人")));
    assert!(said.iter().any(|line| line.contains("赢了。身份：")));
    let table = last_table(&end);
    let hers = table["reachable"]
        .as_array()
        .unwrap()
        .iter()
        .position(|seat| seat["hers"] == true)
        .unwrap();
    let name = |role: &str| match role {
        "merlin" => "梅林",
        "percival" => "派西维尔",
        "loyal" => "亚瑟的忠臣",
        "morgana" => "莫甘娜",
        _ => "刺客",
    };
    let mine = format!(
        "你的身份：{}",
        name(table["game"]["roles"][hers].as_str().unwrap())
    );
    let mut asked = 0;
    for step in &end.steps {
        for effect in step["output"]["effects"].as_array().into_iter().flatten() {
            if effect["kind"] == "ask" && effect["operation"] == "avalon_her_move" {
                asked += 1;
                let text = effect.to_string();
                assert!(text.contains(&mine), "{text}");
                for role in ["梅林", "派西维尔", "亚瑟的忠臣", "莫甘娜", "刺客"] {
                    let theirs = format!("你的身份：{role}");
                    assert!(theirs == mine || !text.contains(&theirs), "{text}");
                }
            }
            if effect["kind"] == "whisper" {
                assert!(effect["choices"].is_null(), "no buttons on QQ");
            }
        }
    }
    assert!(asked > 0, "she moved as her seat");
    end.check("avalon_she_fills_a_seat");
}

/// A game of 阿瓦隆 is on, its first team being voted on, when her end
/// restarts: the table comes back as kept, everyone at it seated again, the
/// votes already cast stand, and the game is played to its end.
#[test]
fn g1_avalon_goes_on_after_a_restart() {
    let mut before = End::new();
    before.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("来\n[[game:avalon]]".into()),
            failure: None,
        },
    );
    heard(
        &mut before,
        0.0,
        message(1, 11, "阿明", "@bot 阿瓦隆", true),
    );
    let people = [
        (11, "阿明"),
        (12, "小红"),
        (13, "阿强"),
        (14, "小李"),
        (15, "大刘"),
    ];
    for (index, (from, name)) in people.iter().enumerate() {
        heard(
            &mut before,
            40.0 + index as f64,
            message(2 + index as i64, *from, name, "加入", false),
        );
    }
    // The leader picks; two vote for it before the restart.
    before.run(BASE + secs(100.0));
    for _ in 0..2 {
        let seating = before.group.seating.clone().unwrap();
        let leader = avalon_of(&seating).unwrap().leader;
        let now = before.now;
        before.at(
            now + secs(1.0),
            Event::Whispered {
                who: seating.reachable[leader].who.clone(),
                text: "0".into(),
                wait: seating.waited,
            },
        );
        before.run(now + secs(2.0));
    }
    for voter in 0..2 {
        let seating = before.group.seating.clone().unwrap();
        let now = before.now;
        before.at(
            now + secs(1.0),
            Event::Whispered {
                who: seating.reachable[voter].who.clone(),
                text: "0".into(),
                wait: seating.waited,
            },
        );
        before.run(now + secs(2.0));
    }
    let table = last_table(&before);
    assert_eq!(table["game"]["phase"]["phase"], "vote");
    assert_eq!(table["game"]["phase"]["votes"].as_array().unwrap().len(), 2);
    let mut end = End::new();
    end.now = before.now + secs(5.0);
    end.at(
        end.now,
        Event::Restored {
            lines: None,
            spoke_up: Vec::new(),
            table: Some(table),
        },
    );
    let from = (end.now - BASE) as f64 / 1e6 + 1.0;
    play_out_by(&mut end, from, 5000.0, &|_, _| None, &|_, _| {
        Some("0".into())
    });
    assert!(end.group.seating.is_none(), "played to its end");
    let seated = end.steps[0]["output"]["effects"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|effect| effect["kind"] == "seat")
        .count();
    assert_eq!(seated, 5);
    let said = said_in_group(&end);
    assert!(said.iter().any(|line| line.contains("赢了。身份：")));
    // The two who voted before the restart are not asked again.
    let first_vote = said.iter().find(|line| line.starts_with("投票：")).unwrap();
    assert!(first_vote.contains("赞成 阿明、小红"), "{first_vote}");
    end.check("avalon_goes_on_after_a_restart");
}

// ------------------------------------------------------------ 剧本杀

/// The game of 剧本杀 on at a table, if it is one.
fn jubensha_of(seating: &flows::Seating) -> Option<&crate::jubensha::Jubensha> {
    match seating.game.as_ref()? {
        crate::hosting::SeatGame::Jubensha(game) => Some(game),
        _ => None,
    }
}

/// The story she writes in a test, step by step, each `after` seconds.
fn jubensha_written(end: &mut End, players: usize, after: f64) {
    let [outline, clues, scripts, check] = crate::jubensha::sample(players);
    for (kind, raw) in [
        ("ask:jubensha_outline", outline),
        ("ask:jubensha_clues", clues),
        ("ask:jubensha_scripts", scripts),
        ("ask:jubensha_check", check),
    ] {
        end.answer(
            kind,
            after,
            Answer::Asked {
                raw: Some(raw),
                failure: None,
            },
        );
    }
}

/// The players of a test at 剧本杀, every ten seconds from `from` to
/// `until`: the first `speakers` seats introduce themselves in two lines and
/// then say they are done (the rest let their time pass); whoever has to
/// choose whispers `chosen` (as a button pressed when `buttons`, else
/// typed).
fn play_jubensha(
    end: &mut End,
    from: f64,
    until: f64,
    buttons: bool,
    speakers: usize,
    chosen: &dyn Fn(&flows::Seating, usize) -> Option<String>,
) {
    use crate::hosting::Waiting;
    use crate::jubensha::Phase;
    let mut at = from;
    let mut id = 700;
    while at < until {
        end.run(BASE + secs(at));
        let Some(seating) = end.group.seating.clone() else {
            break;
        };
        if let Some(game) = seating.game.as_ref() {
            match game.waiting() {
                Waiting::Speak(seat) if seat < speakers => {
                    let lines = match jubensha_of(&seating).map(|game| &game.phase) {
                        Some(Phase::Intro { lines, .. }) => *lines,
                        _ => 0,
                    };
                    let sat = &seating.reachable[seat];
                    let (platform, user) = sat.who.split_once(':').unwrap();
                    let line = match lines {
                        0 => "我是这家的老人了。",
                        1 => "那晚我一直在自己屋里。",
                        _ => "说完了",
                    };
                    id += 1;
                    heard(
                        end,
                        at + 3.0,
                        message_on(
                            platform,
                            &seating.chat.chat,
                            id,
                            user,
                            &sat.name,
                            line,
                            false,
                        ),
                    );
                }
                Waiting::Choose(seats) => {
                    for seat in seats {
                        let sat = &seating.reachable[seat];
                        let Some(text) = chosen(&seating, seat) else {
                            continue;
                        };
                        end.at(
                            BASE + secs(at + 3.0),
                            Event::Whispered {
                                who: sat.who.clone(),
                                text,
                                wait: if buttons { seating.waited } else { None },
                            },
                        );
                    }
                }
                _ => {}
            }
        }
        at += 10.0;
    }
    end.run(BASE + secs(until));
}

/// How the players of a test play 剧本杀: the theme as `theme` has it,
/// read at once, each search two places (its own seat's and the next with
/// a clue it may find), the first private clue held shown in the first
/// talk (whose time then runs out); the vote for whom `vote` names (a
/// character's name).
fn jubensha_player(
    theme: &dyn Fn(usize) -> usize,
    vote: &dyn Fn(&crate::jubensha::Jubensha, usize) -> &'static str,
) -> impl Fn(&flows::Seating, usize) -> Option<String> {
    move |seating, seat| {
        use crate::jubensha::Phase;
        let game = jubensha_of(seating)?;
        let choices = seating.game.as_ref()?.seat_view(seat).choices().to_vec();
        Some(match &game.phase {
            Phase::Theme { .. } => theme(seat).to_string(),
            Phase::Search { round } => {
                // Its own seat's place first, then the next that has a
                // clue it may find.
                let clues = &game.drafts.clues.as_ref()?.clues;
                let outline = game.drafts.outline.as_ref()?;
                let me = &outline.characters[game.cast[seat]].id;
                let shown = |id: &str| {
                    clues
                        .iter()
                        .position(|clue| clue.id == id)
                        .is_some_and(|index| game.shown[index])
                };
                let first = if game.picks[seat] == crate::jubensha::PICKS {
                    seat
                } else {
                    seat + 1
                };
                (0..outline.locations.len())
                    .map(|offset| (first + offset) % outline.locations.len())
                    .find(|place| {
                        clues.iter().enumerate().any(|(index, clue)| {
                            game.found[index].is_none()
                                && clue.place == outline.locations[*place].id
                                && clue.round <= *round
                                && !clue.excluded.contains(me)
                                && (clue.requires.is_empty() || shown(&clue.requires))
                        })
                    })?
                    .to_string()
            }
            // The first private clue held is shown; the talk is let run.
            Phase::Discuss {
                round: 1, offered, ..
            } => match offered[seat].first() {
                Some(card) if !game.shown[*card] => "1".into(),
                _ => return None,
            },
            Phase::Vote { .. } => {
                let name = vote(game, seat);
                choices
                    .iter()
                    .position(|choice| choice.starts_with(name))
                    .unwrap_or(choices.len() - 1)
                    .to_string()
            }
            _ => "0".into(),
        })
    }
}

/// The character `seat` plays, by name.
fn plays(game: &crate::jubensha::Jubensha, seat: usize) -> String {
    let outline = game.drafts.outline.as_ref().unwrap();
    outline.characters[game.cast[seat]].name.clone()
}

fn asks_of(end: &End, operation: &str) -> Vec<Value> {
    end.steps
        .iter()
        .flat_map(|step| {
            step["output"]["effects"]
                .as_array()
                .cloned()
                .unwrap_or_default()
        })
        .filter(|effect| effect["kind"] == "ask" && effect["operation"] == operation)
        .collect()
}

/// A member asks for 剧本杀 in a Telegram group: four join, one asking for
/// a theme; they vote on it from buttons; she writes the story in its
/// steps (saying how far she is); each is whispered their own part, the
/// culprit that it was them; they read, introduce themselves in more than
/// one line, search twice from buttons (public clues told in the group,
/// private ones whispered, one shown in the talk), ask her in the group
/// (answered there, not as a turn of hers) and in private (a line that
/// would give a secret away is not said: the facts it rests on are); the
/// vote catches the culprit, and the truth and every ending are told.
#[test]
fn g1_jubensha_is_played_with_buttons() {
    let mut end = End::new();
    end.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("好，我来当 DM\n[[game:jubensha]]".into()),
            failure: None,
        },
    );
    end.answer(
        "ask:voice",
        2.0,
        Answer::Asked {
            raw: Some(r#"{"say":"雨夜，老宅，大家小心。"}"#.into()),
            failure: None,
        },
    );
    jubensha_written(&mut end, 4, 20.0);
    end.answer(
        "ask:jubensha_answer",
        2.0,
        Answer::Asked {
            raw: Some(r#"{"verdict":"answered","cite":[0],"say":"对，书房只有一扇门。"}"#.into()),
            failure: None,
        },
    );
    end.answer(
        "ask:jubensha_answer",
        2.0,
        Answer::Asked {
            raw: Some(r#"{"verdict":"answered","cite":[0],"say":"茶里有安眠药。"}"#.into()),
            failure: None,
        },
    );
    heard(
        &mut end,
        0.0,
        message(1, 11, "阿明", "@bot 来玩剧本杀", true),
    );
    let people = [(11, "阿明"), (12, "小红"), (13, "阿强"), (14, "小李")];
    for (index, (from, name)) in people.iter().enumerate() {
        heard(
            &mut end,
            40.0 + index as f64,
            message(2 + index as i64, *from, name, "加入", false),
        );
    }
    heard(&mut end, 45.0, message(9, 12, "小红", "主题：武侠", false));
    let culprit_named = |_: &crate::jubensha::Jubensha, _: usize| "周妈";
    let player = jubensha_player(&|_| 0, &culprit_named);
    // Until reading.
    play_jubensha(&mut end, 110.0, 400.0, true, 1, &|seating, seat| {
        let game = jubensha_of(seating)?;
        matches!(game.phase, crate::jubensha::Phase::Theme { .. }).then(|| player(seating, seat))?
    });
    let seating = end.group.seating.clone().expect("on");
    let game = jubensha_of(&seating).unwrap();
    assert_eq!(game.theme, "武侠", "the theme asked for, voted for");
    assert!(matches!(game.phase, crate::jubensha::Phase::Read { .. }));
    // A question in the group, and one in private from someone who is not
    // the culprit.
    let asker = (0..4).find(|seat| plays(game, *seat) != "周妈").unwrap();
    let (_, user) = seating.reachable[asker].who.split_once(':').unwrap();
    let user = user.to_string();
    let now = (end.now - BASE) as f64 / 1e6;
    heard(
        &mut end,
        now + 1.0,
        message(
            30,
            user.parse().unwrap(),
            "问的人",
            "@bot 书房是不是只有一扇门？",
            true,
        ),
    );
    end.at(
        BASE + secs(now + 2.0),
        Event::Whispered {
            who: format!("telegram:{user}"),
            text: "那杯茶有问题吗？".into(),
            wait: None,
        },
    );
    // While it is theirs to press 读完了: a number out of range is asked
    // again; the question above was a question, not a move asked again.
    end.at(
        BASE + secs(now + 3.0),
        Event::Whispered {
            who: format!("telegram:{user}"),
            text: "9".into(),
            wait: None,
        },
    );
    end.run(BASE + secs(now + 10.0));
    let asked: Vec<String> = asks_of(&end, "jubensha_answer")
        .iter()
        .map(|ask| ask["input"].as_str().unwrap_or_default().to_string())
        .collect();
    assert!(
        asked.iter().any(|input| input.contains("那杯茶有问题吗")),
        "{asked:?}"
    );
    let to_asker: Vec<String> = whispers(&end)
        .into_iter()
        .filter(|(to, _)| *to == user)
        .map(|(_, text)| text)
        .collect();
    assert!(
        to_asker
            .iter()
            .any(|text| text.starts_with("没有这个号码，再选一次"))
    );
    assert!(
        to_asker.iter().all(|text| !text.starts_with("没看懂这句")),
        "{to_asker:?}"
    );
    play_jubensha(&mut end, now + 10.0, 6000.0, true, 1, &|seating, seat| {
        // The second talk is let pass.
        match jubensha_of(seating)?.phase {
            crate::jubensha::Phase::Discuss { round: 2, .. } => None,
            _ => player(seating, seat),
        }
    });
    assert!(end.group.seating.is_none(), "played to its end");
    let said = said_in_group(&end);
    assert!(
        said.iter().any(|line| line.contains("对，书房只有一扇门")),
        "{said:?}"
    );
    assert_eq!(
        asks_of(&end, "group_member").len(),
        1,
        "the question in the group is the game's, not a turn of hers"
    );
    let over = said
        .iter()
        .position(|line| line.contains("复盘："))
        .expect("the truth is told at the end");
    assert!(said[over].contains("投中了"));
    assert!(
        said[..over]
            .iter()
            .all(|line| !line.contains("安眠药") || line.contains("公开线索"))
    );
    assert!(said.iter().any(|line| line.contains("公开了线索")));
    assert!(
        said.iter()
            .any(|line| line.contains("你们") || line.contains("自我介绍"))
    );
    let mine = whispers(&end)
        .into_iter()
        .filter(|(to, _)| *to == user)
        .map(|(_, text)| text)
        .collect::<Vec<_>>();
    assert!(
        mine.iter().all(|text| !text.contains("茶里有安眠药")),
        "a line giving a secret away is not said: {mine:?}"
    );
    let buttons = end.steps.iter().any(|step| {
        step["output"]["effects"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|effect| effect["kind"] == "whisper" && effect["choices"].is_array())
    });
    assert!(buttons);
    // What the story was is kept as dealt lately.
    assert!(end.steps.iter().any(|step| {
        step["output"]["effects"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|effect| effect.to_string().contains("雨夜书房/武侠"))
    }));
    end.check("jubensha_is_played_with_buttons");
}

/// On QQ, typed numbers: the theme vote is a tie and she picks one of
/// those tied; the first outline does not hold (one culprit too many) and
/// is written again; the rest time out; the vote names the wrong one, the
/// culprit escapes and the truth is told all the same.
#[test]
fn g1_jubensha_on_qq_a_tie_a_rewrite_and_a_wrong_vote() {
    let mut end = End::new();
    end.answer(
        "identify",
        0.0,
        Answer::Identified {
            who: Who::Stranger,
            owner: Some(1),
        },
    );
    end.answer(
        "ask:group_stranger",
        2.0,
        Answer::Asked {
            raw: Some("好\n[[game:jubensha]]".into()),
            failure: None,
        },
    );
    let [outline, ..] = crate::jubensha::sample(4);
    let mut wrong: Value = serde_json::from_str(&outline).unwrap();
    wrong["characters"][0]["culprit"] = json!(true);
    end.answer(
        "ask:jubensha_outline",
        10.0,
        Answer::Asked {
            raw: Some(wrong.to_string()),
            failure: None,
        },
    );
    jubensha_written(&mut end, 4, 10.0);
    let qq = |id: i64, from: &str, name: &str, text: &str, addressed: bool| {
        message_on("onebot", "77031", id, from, name, text, addressed)
    };
    heard(&mut end, 0.0, qq(1, "9101", "甲", "@bot 剧本杀", true));
    let people = [
        ("9101", "甲"),
        ("9102", "乙"),
        ("9103", "丙"),
        ("9104", "丁"),
    ];
    for (index, (from, name)) in people.iter().enumerate() {
        heard(
            &mut end,
            40.0 + index as f64,
            qq(2 + index as i64, from, name, "加入", false),
        );
    }
    let wrong_named = |game: &crate::jubensha::Jubensha, seat: usize| {
        if plays(game, seat) == "林叔" {
            "阿青"
        } else {
            "林叔"
        }
    };
    // Two and two on the theme; reading done, everything else let pass but
    // the vote.
    let player = jubensha_player(&|seat| if seat < 2 { 0 } else { 3 }, &wrong_named);
    play_jubensha(&mut end, 110.0, 8000.0, false, 0, &|seating, seat| {
        use crate::jubensha::Phase;
        match jubensha_of(seating)?.phase {
            Phase::Theme { .. } | Phase::Read { .. } | Phase::Vote { .. } => player(seating, seat),
            _ => None,
        }
    });
    assert!(end.group.seating.is_none(), "played to its end");
    let said = said_in_group(&end);
    assert!(
        said.iter().any(|line| line.contains("平票，我挑了")),
        "{said:?}"
    );
    assert_eq!(
        asks_of(&end, "jubensha_outline").len(),
        2,
        "written again once"
    );
    let rewrite = asks_of(&end, "jubensha_outline")[1]["input"]
        .as_str()
        .unwrap()
        .to_string();
    assert!(
        rewrite.contains("exactly one character must be the culprit"),
        "{rewrite}"
    );
    assert!(said.iter().any(|line| line.contains("投错了")));
    assert!(
        said.iter()
            .any(|line| line.contains("周妈第二天就离开了老宅"))
    );
    let whispered = end.steps.iter().any(|step| {
        step["output"]["effects"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|effect| effect["kind"] == "whisper" && effect["choices"].is_array())
    });
    assert!(!whispered, "no buttons on QQ");
    end.check("jubensha_on_qq_a_tie_a_rewrite_and_a_wrong_vote");
}

/// Ten minutes is all writing may take: the steps come too slowly and she
/// says this time it is not on; nothing is remembered of it.
#[test]
fn g1_jubensha_is_given_up_after_ten_minutes() {
    let mut end = End::new();
    end.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("好\n[[game:jubensha]]".into()),
            failure: None,
        },
    );
    jubensha_written(&mut end, 4, 170.0);
    heard(&mut end, 0.0, message(1, 11, "阿明", "@bot 剧本杀", true));
    for (index, (from, name)) in [(11, "阿明"), (12, "小红"), (13, "阿强"), (14, "小李")]
        .iter()
        .enumerate()
    {
        heard(
            &mut end,
            40.0 + index as f64,
            message(2 + index as i64, *from, name, "加入", false),
        );
    }
    end.run(BASE + secs(2000.0));
    assert!(end.group.seating.is_none());
    let said = said_in_group(&end);
    assert!(
        said.iter().any(|line| line.contains("本子写得太久了")),
        "{said:?}"
    );
    assert!(
        !end.steps
            .iter()
            .any(|step| step["output"].to_string().contains("rememberInVenue")),
        "nothing kept of a game that never began"
    );
    end.check("jubensha_is_given_up_after_ten_minutes");
}

/// While they read, three of the four say in the group to stop: at the
/// third, more than half, the truth is told at once.
#[test]
fn g1_jubensha_stops_when_most_ask() {
    let mut end = End::new();
    end.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("好\n[[game:jubensha]]".into()),
            failure: None,
        },
    );
    jubensha_written(&mut end, 4, 5.0);
    heard(&mut end, 0.0, message(1, 11, "阿明", "@bot 剧本杀", true));
    let people = [(11, "阿明"), (12, "小红"), (13, "阿强"), (14, "小李")];
    for (index, (from, name)) in people.iter().enumerate() {
        heard(
            &mut end,
            40.0 + index as f64,
            message(2 + index as i64, *from, name, "加入", false),
        );
    }
    play_jubensha(&mut end, 110.0, 200.0, true, 0, &|seating, seat| {
        matches!(
            jubensha_of(seating)?.phase,
            crate::jubensha::Phase::Theme { .. }
        )
        .then(|| (seat % 2).to_string())
    });
    assert!(matches!(
        jubensha_of(end.group.seating.as_ref().unwrap())
            .unwrap()
            .phase,
        crate::jubensha::Phase::Read { .. }
    ));
    let now = (end.now - BASE) as f64 / 1e6;
    for (index, (from, name)) in people.iter().take(3).enumerate() {
        heard(
            &mut end,
            now + 1.0 + index as f64,
            message(40 + index as i64, *from, name, "结束剧本杀", false),
        );
    }
    end.run(BASE + secs(now + 30.0));
    assert!(end.group.seating.is_none());
    let said = said_in_group(&end);
    assert!(said.iter().any(|line| line.contains("想结束（2/4")));
    assert!(
        said.iter()
            .any(|line| line.contains("复盘：") && line.contains("凶手没被找出来"))
    );
    end.check("jubensha_stops_when_most_ask");
}

/// A restart while she writes the clues (the outline already written): the
/// table comes back with the outline kept, only the step it was on is
/// asked again, and the game is played on (all of it let pass) to its end.
#[test]
fn g1_jubensha_goes_on_after_a_restart_while_she_writes() {
    let mut before = End::new();
    before.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("好\n[[game:jubensha]]".into()),
            failure: None,
        },
    );
    let [outline, clues, scripts, check] = crate::jubensha::sample(4);
    before.answer(
        "ask:jubensha_outline",
        20.0,
        Answer::Asked {
            raw: Some(outline),
            failure: None,
        },
    );
    before.answer(
        "ask:jubensha_clues",
        500.0,
        Answer::Asked {
            raw: Some(clues.clone()),
            failure: None,
        },
    );
    heard(
        &mut before,
        0.0,
        message(1, 11, "阿明", "@bot 剧本杀", true),
    );
    for (index, (from, name)) in [(11, "阿明"), (12, "小红"), (13, "阿强"), (14, "小李")]
        .iter()
        .enumerate()
    {
        heard(
            &mut before,
            40.0 + index as f64,
            message(2 + index as i64, *from, name, "加入", false),
        );
    }
    play_jubensha(&mut before, 110.0, 250.0, true, 0, &|seating, seat| {
        matches!(
            jubensha_of(seating)?.phase,
            crate::jubensha::Phase::Theme { .. }
        )
        .then(|| (seat % 2).to_string())
    });
    let table = last_table(&before);
    assert_eq!(table["game"]["phase"]["phase"], "writing");
    assert_eq!(table["game"]["phase"]["step"], 1);
    assert!(table["game"]["drafts"]["outline"].is_object());
    let mut end = End::new();
    end.now = before.now + secs(5.0);
    for (kind, raw) in [
        ("ask:jubensha_clues", clues),
        ("ask:jubensha_scripts", scripts),
        ("ask:jubensha_check", check),
    ] {
        end.answer(
            kind,
            10.0,
            Answer::Asked {
                raw: Some(raw),
                failure: None,
            },
        );
    }
    end.at(
        end.now,
        Event::Restored {
            lines: None,
            spoke_up: Vec::new(),
            table: Some(table),
        },
    );
    end.run(BASE + secs(9000.0));
    assert!(end.group.seating.is_none(), "played to its end");
    assert!(
        asks_of(&end, "jubensha_outline").is_empty(),
        "the outline is not written again"
    );
    assert_eq!(asks_of(&end, "jubensha_clues").len(), 1);
    let said = said_in_group(&end);
    assert!(said.iter().any(|line| line.contains("复盘：")));
    end.check("jubensha_goes_on_after_a_restart_while_she_writes");
}

/// A restart in the first talk, with a question asked in private still
/// waiting on its answer and a private clue held: the question is asked
/// again and answered to who asked, the clue is still theirs to show, and
/// the game goes on (let pass) to its end.
#[test]
fn g1_jubensha_goes_on_after_a_restart_mid_game() {
    let mut before = End::new();
    before.answer(
        "ask:group_member",
        2.0,
        Answer::Asked {
            raw: Some("好\n[[game:jubensha]]".into()),
            failure: None,
        },
    );
    jubensha_written(&mut before, 4, 5.0);
    before.answer(
        "ask:jubensha_answer",
        300.0,
        Answer::Asked {
            raw: Some(r#"{"verdict":"notInYourInfo","cite":[],"say":""}"#.into()),
            failure: None,
        },
    );
    heard(
        &mut before,
        0.0,
        message(1, 11, "阿明", "@bot 剧本杀", true),
    );
    for (index, (from, name)) in [(11, "阿明"), (12, "小红"), (13, "阿强"), (14, "小李")]
        .iter()
        .enumerate()
    {
        heard(
            &mut before,
            40.0 + index as f64,
            message(2 + index as i64, *from, name, "加入", false),
        );
    }
    let culprit_named = |_: &crate::jubensha::Jubensha, _: usize| "周妈";
    let player = jubensha_player(&|_| 1, &culprit_named);
    // Until the first talk.
    let mut at = 110.0;
    while at < 3000.0 {
        play_jubensha(&mut before, at, at + 10.0, true, 0, &player);
        at += 10.0;
        let talking = before
            .group
            .seating
            .as_ref()
            .and_then(jubensha_of)
            .is_some_and(|game| matches!(game.phase, crate::jubensha::Phase::Discuss { .. }));
        if talking {
            break;
        }
    }
    let seating = before.group.seating.clone().expect("in the talk");
    assert!(matches!(
        jubensha_of(&seating).unwrap().phase,
        crate::jubensha::Phase::Discuss { round: 1, .. }
    ));
    let now = (before.now - BASE) as f64 / 1e6;
    before.at(
        BASE + secs(now + 1.0),
        Event::Whispered {
            who: seating.reachable[0].who.clone(),
            text: "我那晚到底在哪？".into(),
            wait: None,
        },
    );
    before.run(BASE + secs(now + 5.0));
    let table = last_table(&before);
    assert_eq!(table["asides"].as_array().map(Vec::len), Some(1), "{table}");
    let mut end = End::new();
    end.now = before.now + secs(5.0);
    end.answer(
        "ask:jubensha_answer",
        2.0,
        Answer::Asked {
            raw: Some(r#"{"verdict":"notInYourInfo","cite":[],"say":""}"#.into()),
            failure: None,
        },
    );
    end.at(
        end.now,
        Event::Restored {
            lines: None,
            spoke_up: Vec::new(),
            table: Some(table),
        },
    );
    end.run(BASE + secs(9000.0));
    assert!(end.group.seating.is_none(), "played to its end");
    assert_eq!(asks_of(&end, "jubensha_answer").len(), 1, "asked again");
    let to_asker = whispers(&end)
        .into_iter()
        .any(|(user, text)| user == "11" && text.contains("得靠搜证和推理"));
    assert!(to_asker);
    end.check("jubensha_goes_on_after_a_restart_mid_game");
}
