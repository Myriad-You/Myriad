//! Unit tests for the group-chat module.

use super::*;

fn line(chat_id: i64, message_id: i64, name: &str, text: &str) -> GroupLine {
    GroupLine::from(TelegramGroupMessage {
        update_id: message_id,
        message_id,
        chat_id,
        message_thread_id: None,
        from_id: 1,
        display_name: name.into(),
        text: text.into(),
        addressed: false,
        reply_to: None,
        images: Vec::new(),
    })
}

fn venue(chat_id: i64) -> String {
    format!("telegram:{chat_id}")
}

/// After a restart a chat app sends its last batch again: a line heard
/// already is neither kept twice nor answered twice.
#[tokio::test]
async fn a_line_delivered_again_is_heard_once() {
    let chat = -9_017;
    assert!(record(&line(chat, 1, "阿明", "@bot 在吗")).await);
    assert!(!record(&line(chat, 1, "阿明", "@bot 在吗")).await);
    assert_eq!(transcript(&venue(chat), None).len(), 1);
}

#[tokio::test]
async fn the_group_transcript_is_the_lines_before_the_one_she_answers() {
    let chat = -9_001;
    record(&line(chat, 1, "阿明", "周五聚餐吗")).await;
    record(&line(chat, 2, "小红", "我可以")).await;
    record_hers(&venue(chat), "我在屏幕里，就不去了，你们吃好").await;
    record(&line(chat, 3, "阿明", "@bot 你推荐哪家")).await;
    let lines: Vec<(String, String)> = transcript(&venue(chat), Some("3"))
        .into_iter()
        .map(|message| (message.role, message.content))
        .collect();
    assert_eq!(
        lines,
        vec![
            ("user".into(), "阿明：周五聚餐吗".into()),
            ("user".into(), "小红：我可以".into()),
            ("assistant".into(), "我在屏幕里，就不去了，你们吃好".into()),
        ]
    );
}

#[tokio::test]
async fn she_can_mention_whoever_spoke_there_lately_by_their_last_name() {
    let chat = -9_471;
    let said = |id: i64, from: i64, name: &str| {
        GroupLine::from(TelegramGroupMessage {
            update_id: id,
            message_id: id,
            chat_id: chat,
            message_thread_id: None,
            from_id: from,
            display_name: name.into(),
            text: "嗯嗯".into(),
            addressed: false,
            reply_to: None,
            images: Vec::new(),
        })
    };
    record(&said(1, 11, "阿明")).await;
    record(&said(2, 12, "小红")).await;
    record_hers(&venue(chat), "@阿明 你好").await;
    record(&said(3, 11, "阿明同学")).await;
    assert_eq!(
        people(&venue(chat)),
        vec![
            ("阿明同学".to_string(), "11".to_string()),
            ("小红".to_string(), "12".to_string())
        ]
    );
}

#[test]
fn a_group_gets_one_turn_at_a_time_and_a_pause_after_replying() {
    let chat = -9_002;
    assert!(matches!(begin_turn(&venue(chat)), Turn::Began));
    assert!(
        matches!(begin_turn(&venue(chat)), Turn::Busy),
        "one turn at a time"
    );
    end_turn(&venue(chat), true);
    assert!(
        matches!(begin_turn(&venue(chat)), Turn::Resting(_)),
        "a short pause after a reply"
    );
    let other = -9_003;
    assert!(matches!(begin_turn(&venue(other)), Turn::Began));
    end_turn(&venue(other), false);
    assert!(
        matches!(begin_turn(&venue(other)), Turn::Began),
        "no reply, no pause"
    );
    end_turn(&venue(other), false);
}

#[tokio::test]
async fn she_looks_once_the_talk_settles_on_its_latest_line() {
    let chat = -9_472;
    let later = line(chat, 2, "阿明", "你们说呢");
    notice(
        line(chat, 1, "阿明", "有人听过 amazarashi 吗"),
        String::new(),
    );
    notice(later, String::new());
    with_group(&venue(chat), |group| {
        assert_eq!(
            group.pending.as_ref().map(|line| line.message_id.as_str()),
            Some("2")
        );
        assert!(group.unjudged_since.is_some());
    });
}

#[tokio::test]
async fn she_follows_talk_she_is_in_and_glances_at_the_rest_now_and_then() {
    let chat = -9_474;
    record(&line(chat, 1, "阿明", "周五聚餐吗")).await;
    with_group(&venue(chat), |group| assert!(!in_talk(group)));
    // Not in the talk: one glance is planned, not one per line.
    notice(line(chat, 1, "阿明", "周五聚餐吗"), String::new());
    let planned = with_group(&venue(chat), |group| group.glance_at).flatten();
    // Busy at most so long first, or asleep until she is up.
    let first = match crate::services::agent::merope::group::timing::where_she_is(false, true) {
        myriad_merope::timing::Where::Asleep { wakes_in } => {
            Duration::from_secs_f64(wakes_in + 60.0)
        }
        _ => LONGEST_BUSY,
    };
    assert!(planned.is_some_and(|at| {
        let wait = at - Instant::now();
        wait <= Duration::from_secs(*GLANCE_AFTER_SECONDS.end()) + first
    }));
    notice(line(chat, 2, "小红", "可以"), String::new());
    assert_eq!(
        with_group(&venue(chat), |group| group.glance_at).flatten(),
        planned
    );
    // She said something there: she is in the talk now.
    record_hers(&venue(chat), "我在屏幕里，你们吃好").await;
    with_group(&venue(chat), |group| assert!(in_talk(group)));
    let other = -9_475;
    with_group(&venue(other), |group| group.called = Some(Instant::now()));
    with_group(&venue(other), |group| assert!(in_talk(group)));
}

#[tokio::test]
async fn pictures_read_as_she_saw_them_and_one_sent_again_is_counted() {
    let chat = -9_476;
    let picture = |id: i64| {
        let mut line = line(chat, id, "阿明", "");
        line.images = vec![GroupImage {
            key: "telegram:cat".into(),
            fetch: ImageFetch::TelegramFile {
                file_id: "f".into(),
            },
            hint: Some("😂".into()),
            sticker: true,
        }];
        line
    };
    record(&picture(1)).await;
    record(&line(chat, 2, "小红", "哈哈哈")).await;
    record(&picture(3)).await;
    let lines: Vec<String> = transcript(&venue(chat), None)
        .into_iter()
        .map(|line| line.content)
        .collect();
    assert_eq!(
        lines[0], "阿明：[表情：😂]",
        "not looked at yet: what the app calls it"
    );
    with_group(&venue(chat), |group| {
        assert_eq!(group.pictures.get("telegram:cat"), Some(&2));
        group.lines[0].seen = vec![Some(myriad_merope::seeing::Seen {
            what: "一只翻白眼的猫".into(),
            says: Some("无语".into()),
        })];
    });
    assert_eq!(
        transcript(&venue(chat), None)[0].content,
        "阿明：[表情：一只翻白眼的猫（无语）]"
    );
    assert_eq!(said_now(&picture(3)), "[表情：😂]");
}

/// Replayed from the QQ group's own traffic on 2026-09-28 (NapCat's log of
/// it): a member @-ing another, by number only and with a name.
#[tokio::test]
async fn a_real_qq_line_at_someone_else_reads_as_who() {
    let wire = |at: &str| {
        format!(
            r#"{{"post_type":"message","message_type":"group","group_id":1076198,"user_id":3059342645,"self_id":3264977935,"message_id":7,
                "sender":{{"card":"梦想成为猪侯王的leaphy"}},
                "message":[{{"type":"text","data":{{"text":"你去看ave mujika "}}}},{at}]}}"#
        )
    };
    let spoke = GroupLine::from(
            myriad_agent_rules::onebot::decode::decode_group_inbound(
                r#"{"post_type":"message","message_type":"group","group_id":1076198,"user_id":798494815,"self_id":3264977935,"message_id":6,
                "sender":{"card":"染川 瞳"},"message":[{"type":"text","data":{"text":"所以乐奈是什么意思"}}]}"#,
                3264977935,
            )
            .unwrap(),
        );
    let venue = spoke.venue();
    record(&spoke).await;
    for at in [
        r#"{"type":"at","data":{"qq":"798494815"}}"#,
        r#"{"type":"at","data":{"qq":"798494815","name":"染川 瞳"}}"#,
    ] {
        let line = GroupLine::from(
            myriad_agent_rules::onebot::decode::decode_group_inbound(&wire(at), 3264977935)
                .unwrap(),
        );
        assert!(!line.addressed, "someone else @-ed, not her");
        record(&line).await;
        let last = transcript(&venue, None).last().unwrap().content.clone();
        assert_eq!(last, "梦想成为猪侯王的leaphy：你去看ave mujika @染川 瞳");
    }
}

#[test]
fn someone_at_by_their_id_reads_as_their_name_when_known() {
    let people = vec![("小红".to_string(), "111".to_string())];
    assert_eq!(
        by_name("@111 你看 @1112 @222", &people),
        "@小红 你看 @1112 @222"
    );
    assert_eq!(by_name("邮箱 a@b.c", &people), "邮箱 a@b.c");
}

#[test]
fn her_speaking_up_counts_as_taken_up_when_someone_turns_to_her_soon() {
    let mut group = Group::default();
    taken_up(&mut group);
    assert!(group.spoke_up.is_empty());
    group
        .spoke_up
        .push_back((Instant::now() - TAKEN_UP_WITHIN, false));
    taken_up(&mut group);
    assert_eq!(group.spoke_up.back().map(|(_, taken)| *taken), Some(false));
    group.spoke_up.push_back((Instant::now(), false));
    taken_up(&mut group);
    assert_eq!(
        group
            .spoke_up
            .iter()
            .map(|(_, taken)| *taken)
            .collect::<Vec<_>>(),
        [false, true]
    );
}

#[test]
fn how_speaking_up_went_comes_back_after_a_restart_under_what_came_since() {
    let now = chrono::Utc::now();
    let mut group = Group::default();
    group.spoke_up.push_back((Instant::now(), false));
    let kept: Vec<_> = (0..SPOKE_UP_KEPT)
        .map(|index| {
            (
                now - chrono::Duration::minutes(10 - index as i64),
                index % 2 == 0,
            )
        })
        .collect();
    restore_spoke_up(&mut group, &kept, now);
    assert_eq!(group.spoke_up.len(), SPOKE_UP_KEPT);
    // The newest, said since the restart, is still last and not yet taken up.
    assert_eq!(group.spoke_up.back().map(|(_, taken)| *taken), Some(false));
    let order: Vec<Instant> = group.spoke_up.iter().map(|(at, _)| *at).collect();
    assert!(order.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn she_stops_answering_one_who_never_stops_but_not_a_person() {
    let chat = -9_473;
    let from = |id: i64, from: i64, name: &str| {
        GroupLine::from(TelegramGroupMessage {
            update_id: id,
            message_id: id,
            chat_id: chat,
            message_thread_id: None,
            from_id: from,
            display_name: name.into(),
            text: "在吗".into(),
            addressed: false,
            reply_to: None,
            images: Vec::new(),
        })
    };
    let bot = from(1, 77, "复读机");
    for _ in 0..LOOP_ROUNDS - 1 {
        answered(&bot);
    }
    assert!(!stopped_answering(&bot));
    answered(&from(2, 11, "阿明"));
    for _ in 0..LOOP_ROUNDS - 1 {
        answered(&bot);
    }
    assert!(!stopped_answering(&bot), "someone else came between");
    answered(&bot);
    assert!(stopped_answering(&bot));
}

#[test]
fn a_line_that_comes_while_she_is_busy_waits_for_her() {
    let chat = -9_007;
    assert!(matches!(begin_turn(&venue(chat)), Turn::Began));
    assert!(matches!(begin_turn(&venue(chat)), Turn::Busy));
    with_group(&venue(chat), |group| {
        for id in 1..=7 {
            park(group, line(chat, id, "阿明", &format!("第{id}问")));
        }
    });
    end_turn(&venue(chat), true);
    assert!(matches!(begin_turn(&venue(chat)), Turn::Resting(_)));
    // Questions that came while she was busy are answered in order, all of
    // them; only past what she could still read the conversation of do the
    // oldest go.
    let waiting: Vec<String> = with_group(&venue(chat), |group| {
        group.waiting.iter().map(|line| line.text.clone()).collect()
    })
    .unwrap();
    assert_eq!(waiting.len(), 7);
    assert_eq!(waiting.first().map(String::as_str), Some("第1问"));
    with_group(&venue(chat), |group| {
        for id in 8..=(WAITING_LINES as i64 + 3) {
            park(group, line(chat, id, "阿明", &format!("第{id}问")));
        }
    });
    let waiting: Vec<String> = with_group(&venue(chat), |group| {
        group.waiting.iter().map(|line| line.text.clone()).collect()
    })
    .unwrap();
    assert_eq!(waiting.len(), WAITING_LINES);
    assert_eq!(waiting.first().map(String::as_str), Some("第4问"));
    let mut today = None;
    for _ in 0..3 {
        assert!(count_today(&mut today, 3));
    }
    assert!(!count_today(&mut today, 3));
}

#[test]
fn a_group_is_its_venue_on_every_platform() {
    let telegram = line(-100123, 7, "阿明", "在吗");
    assert_eq!(telegram.venue(), "telegram:-100123");
    assert_eq!(telegram.message_id, "7");
    let discord = GroupLine::from(DiscordGroupMessage {
        message_id: "11".into(),
        channel_id: "22".into(),
        guild_id: "33".into(),
        author_id: "44".into(),
        display_name: "阿明".into(),
        text: "说到一半怎么没了".into(),
        addressed: true,
        reply_to: Some(QuotedLine {
            name: "若泉".into(),
            text: "听完要是".into(),
            hers: true,
        }),
        images: Vec::new(),
    });
    assert_eq!(discord.said(), "（回复你说的：听完要是）说到一半怎么没了");
    assert_eq!(discord.venue(), "discord:22");
    assert_eq!(text_limit(ChannelPlatform::Discord), 2000);
}

/// A restart does not wipe the group from her mind: what was kept comes
/// back from the runtime registry with the first line after it.
#[tokio::test]
async fn a_restart_keeps_what_the_group_said() {
    let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
        return;
    };
    let schema = crate::db::IsolatedSchema::migrated(&url, "group_lines").await;
    let db = &schema.db;
    let chat = -9_300;
    let venue = venue(chat);
    let said = |id: i64, text: &str| Line {
        at: chrono::Utc::now(),
        message_id: Some(id.to_string()),
        name: "阿明".into(),
        from: Some("1".into()),
        text: text.into(),
        hers: false,
        addressed: false,
        images: Vec::new(),
        seen: Vec::new(),
    };
    remember_line_on(Some(db), &venue, said(1, "来点歌")).await;
    remember_line_on(Some(db), &venue, said(2, "放一首")).await;
    // The process restarts: nothing of the group is left in memory.
    if let Ok(mut groups) = GROUPS.lock() {
        groups.remove(&venue);
    }
    remember_line_on(Some(db), &venue, said(3, "说到一半怎么没了")).await;
    let lines: Vec<String> = transcript(&venue, None)
        .into_iter()
        .map(|line| line.content)
        .collect();
    assert_eq!(
        lines,
        ["阿明：来点歌", "阿明：放一首", "阿明：说到一半怎么没了"]
    );
    schema.drop().await;
}

/// After a restart the group comes back as it was, under what was heard
/// since, each line once.
#[test]
fn lines_kept_before_a_restart_come_back_under_newer_ones() {
    let at = |minutes: i64| chrono::Utc::now() - chrono::Duration::minutes(minutes);
    let kept = |id: &str, minutes: i64, text: &str| Line {
        at: at(minutes),
        message_id: Some(id.into()),
        name: "阿明".into(),
        from: Some("1".into()),
        text: text.into(),
        hers: false,
        addressed: false,
        images: Vec::new(),
        seen: Vec::new(),
    };
    let mut group = Group::default();
    push_line(&mut group, kept("9", 1, "说到一半怎么没了"));
    merge_restored(
        &mut group,
        vec![
            kept("7", 30, "来点歌"),
            Line {
                at: at(29),
                message_id: None,
                name: String::new(),
                from: None,
                text: "放就放，听完要是".into(),
                hers: true,
                addressed: false,
                images: Vec::new(),
                seen: Vec::new(),
            },
            kept("9", 1, "说到一半怎么没了"),
            kept("1", 7 * 60, "太久以前的话"),
        ],
    );
    let texts: Vec<&str> = group.lines.iter().map(|line| line.text.as_str()).collect();
    assert_eq!(texts, ["来点歌", "放就放，听完要是", "说到一半怎么没了"]);
    let stored = serde_json::to_string(&StoredLines {
        lines: group.lines.iter().cloned().collect(),
    })
    .unwrap();
    let back: StoredLines = serde_json::from_str(&stored).unwrap();
    assert_eq!(back.lines.len(), 3);
}

#[test]
fn she_does_not_echo_the_reply_mark() {
    assert_eq!(
        without_reply_mark("（回复 阿明）谁是你宝宝！"),
        "谁是你宝宝！"
    );
    assert_eq!(without_reply_mark("（回复你说的：听完要是）断了"), "断了");
    assert_eq!(
        without_reply_mark("谁暴躁了（回复一下）"),
        "谁暴躁了（回复一下）"
    );
}

#[tokio::test]
async fn a_line_she_sees_hours_later_she_knows_she_sees_late() {
    let chat = -9_005;
    let asked = line(chat, 1, "阿明", "@bot 在吗");
    record(&asked).await;
    assert_eq!(seen_late(&asked), None);
    with_group(&venue(chat), |group| {
        group.lines[0].at = chrono::Utc::now() - chrono::Duration::hours(7);
    });
    assert_eq!(seen_late(&asked).as_deref(), Some("7 hours ago"));
}

#[tokio::test]
async fn the_room_is_how_the_others_type_not_her() {
    let chat = -9_006;
    for index in 0..myriad_merope::talk_shape::ROOM_AT_LEAST as i64 {
        record(&line(chat, index, "阿明", "哈哈哈")).await;
        record_hers(&venue(chat), "这句话很长很长很长很长很长很长！").await;
    }
    let room = room(&venue(chat)).unwrap();
    assert_eq!(room.messages, myriad_merope::talk_shape::ROOM_AT_LEAST);
    assert_eq!(room.chars_median, 3);
    assert_eq!(room.bang, 0.0);
}

#[tokio::test]
async fn the_ledger_keeps_numbers_not_words_or_ids() {
    let chat = -9_007;
    let venue = venue(chat);
    for index in 0..(LEDGER_EVERY as i64 - 1) {
        record(&line(chat, index, "阿明", "周五聚餐吗")).await;
    }
    record(&line(chat, 99, "阿明", "[图片]")).await;
    let kept = with_group(&venue, |group| group.ledger.clone()).unwrap();
    assert_eq!(kept.len(), LEDGER_EVERY - 1);
    let first = &kept[0];
    assert_eq!(first.chars, 5);
    assert!(!first.mark && !first.bang);
    // The member is a token, the same each time, and not their id.
    assert_eq!(first.by, ledger_who("1"));
    assert_ne!(first.by, "1");
    assert!(kept.iter().all(|typed| typed.by == first.by));
    let json = serde_json::to_string(&kept).unwrap();
    assert!(!json.contains("聚餐"));
    // Her first words to someone who called her are due a write at once.
    assert!(note_said(
        &venue,
        myriad_merope::talk_shape::typed_by(HER, 0, "在"),
        Some(12.0),
        Some("在")
    ));
    let hers = with_group(&venue, |group| group.ledger_hers.clone()).unwrap();
    let theirs = with_group(&venue, |group| group.ledger_theirs.clone()).unwrap();
    assert_eq!(
        (hers.messages, theirs.messages),
        (1, LEDGER_EVERY as u32 - 1)
    );
    assert!(theirs.pieces.contains_key("end:餐吗"));
}

/// How she talks in each group against its members, from the ledgers in
/// the site database: `MEROPE_TALK_REPORT=1 DATABASE_URL=… cargo test
/// -p myriad-backend --bin myriad-backend -- --ignored
/// how_she_talks_against_the_members --nocapture`.
#[tokio::test]
#[ignore = "reads the site database; MEROPE_TALK_REPORT=1 with DATABASE_URL"]
async fn how_she_talks_against_the_members() {
    use myriad_merope::talk_shape::{out_of_line, shape_of, waits};
    assert_eq!(std::env::var("MEROPE_TALK_REPORT").as_deref(), Ok("1"));
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL");
    let db = sea_orm::Database::connect(url).await.expect("database");
    let reference: serde_json::Value =
        serde_json::from_str(include_str!("../../../../tests/merope/talk-reference.json")).unwrap();
    let rows = crate::services::runtime_registry::list(&db, LEDGER_NAMESPACE, None, None)
        .await
        .expect("ledgers");
    println!("ledgers: {}", rows.len());
    for row in rows {
        let Ok(ledger) = serde_json::from_value::<Ledger>(row.payload) else {
            continue;
        };
        let (hers, members): (Vec<_>, Vec<_>) = ledger
            .messages
            .into_iter()
            .partition(|typed| typed.by == HER);
        let members = shape_of(&members);
        let hers = shape_of(&hers);
        println!("\n{}", row.record_id);
        println!("  members: {members:?}");
        println!("  hers:    {hers:?}");
        if let (Some(hers), Some(members)) = (&hers, &members) {
            println!("  out of line: {:?}", out_of_line(hers, members));
        }
        println!(
            "  she answered a call after (s): {:?}; members in the reference group: {}",
            waits(&ledger.her_waits),
            reference["chatApp"]["answerToAt"]
        );
    }
}

#[tokio::test]
async fn what_was_said_while_she_was_away_is_read_back_in_its_place() {
    let chat = -9_008;
    let venue = venue(chat);
    record(&line(chat, 5, "瞳", "@bot 宝宝")).await;
    let now = chrono::Utc::now();
    let ago = |minutes: i64| now - chrono::Duration::minutes(minutes);
    catch_up(
        &venue,
        vec![
            (line(chat, 3, "leaphy", "宝宝，晚安喵"), ago(3), false),
            (line(chat, 4, "", "晚安"), ago(2), true),
            // Already had, and long gone: neither is taken again.
            (line(chat, 5, "瞳", "@bot 宝宝"), ago(1), false),
            (line(chat, 1, "某人", "昨天的事"), ago(60 * 24), false),
        ],
    )
    .await;
    let lines: Vec<(String, String)> = transcript(&venue, None)
        .into_iter()
        .map(|message| (message.role, message.content))
        .collect();
    assert_eq!(
        lines,
        vec![
            ("user".into(), "leaphy：宝宝，晚安喵".into()),
            ("assistant".into(), "晚安".into()),
            ("user".into(), "瞳：@bot 宝宝".into()),
        ]
    );
    // Read back, not answered: nothing is waiting for her.
    assert!(
        with_group(&venue, |group| group.waiting.is_empty()
            && group.pending.is_none())
        .unwrap()
    );
}

#[tokio::test]
async fn a_group_keeps_only_its_recent_lines() {
    let chat = -9_004;
    for index in 0..(TRANSCRIPT_LINES as i64 + 5) {
        record(&line(chat, index, "某人", &format!("第{index}句"))).await;
    }
    let lines = transcript(&venue(chat), None);
    assert_eq!(lines.len(), TRANSCRIPT_LINES);
    assert_eq!(lines[0].content, "某人：第5句");
}

#[test]
fn a_line_left_waiting_by_a_restart_is_taken_back_up_once() {
    let venue = venue(-9_777);
    let calling = |message_id: i64, text: &str| GroupLine {
        addressed: true,
        ..line(-9_777, message_id, "阿明", text)
    };
    let up = *turns::UP_SINCE;
    let ago = |minutes: i64| up - chrono::Duration::minutes(minutes);
    // She answered the first call; the second came while she slept.
    let past = vec![
        (calling(1, "@bot 早"), ago(90), false),
        (line(-9_777, 2, "", "早呀"), ago(89), true),
        (line(-9_777, 3, "小红", "她起了没"), ago(40), false),
        (calling(4, "@bot 晚上打游戏吗"), ago(30), false),
        (line(-9_777, 5, "小红", "没回，睡着了吧"), ago(20), false),
    ];
    let taken = turns::left_waiting(&venue, &past).expect("the call she never got to");
    assert_eq!(taken.message_id, "4");
    assert!(turns::left_waiting(&venue, &past).is_none(), "once");
    // Answered since, or only heard after this process was up: not hers to
    // take back.
    let answered = vec![
        (calling(6, "@bot 在吗"), ago(30), false),
        (line(-9_777, 7, "", "在"), ago(29), true),
    ];
    assert!(turns::left_waiting(&venue, &answered).is_none());
    let live = vec![(
        calling(8, "@bot 在吗"),
        up + chrono::Duration::seconds(30),
        false,
    )];
    assert!(turns::left_waiting(&venue, &live).is_none());
    let stale = vec![(calling(9, "@bot 在吗"), ago(60 * 7), false)];
    assert!(turns::left_waiting(&venue, &stale).is_none());
}

#[test]
fn a_mute_is_told_as_it_happened_and_holds_until_it_ends() {
    use myriad_agent_rules::onebot::decode::Muted;
    assert_eq!(
        muting::told(Muted::For(600), false),
        "（把你禁言了 10 分钟）"
    );
    assert_eq!(
        muting::told(Muted::For(3 * 3600), false),
        "（把你禁言了 3 小时）"
    );
    assert_eq!(
        muting::told(Muted::For(2 * 86400), true),
        "（全员禁言 2 天）"
    );
    assert_eq!(muting::told(Muted::UntilLifted, true), "（开了全员禁言）");
    assert_eq!(muting::told(Muted::Lifted, false), "（解除了你的禁言）");

    let now = chrono::Utc::now();
    let mut group = Group::default();
    assert!(!muting::muted_now(&group, now));
    group.muted_until = Some(now + chrono::Duration::minutes(10));
    assert!(muting::muted_now(&group, now));
    assert!(!muting::muted_now(
        &group,
        now + chrono::Duration::minutes(11)
    ));
    // Everyone's mute lifted does not lift hers, and the other way round.
    group.everyone_muted_until = Some(now + chrono::Duration::days(1));
    group.muted_until = None;
    assert!(muting::muted_now(&group, now));
}

/// When she gets to a line that calls her, she reads the group as it is
/// then: what came after it is in the talk, and what the same person added
/// is part of what she answers, not a line for another turn.
#[tokio::test]
async fn she_answers_what_was_said_up_to_when_she_gets_to_it() {
    let chat = -9_031;
    let from = |id: i64, from_id: i64, name: &str, text: &str| {
        let mut line = line(chat, id, name, text);
        line.from = from_id.to_string();
        line
    };
    record(&from(1, 11, "阿明", "周五聚餐吗")).await;
    let mut called = from(2, 11, "阿明", "你推荐哪家");
    called.addressed = true;
    record(&called).await;
    record(&from(3, 12, "小红", "我想吃火锅")).await;
    let mut added = from(4, 11, "阿明", "最好便宜点");
    added.addressed = true;
    record(&added).await;
    with_group(&venue(chat), |group| park(group, added.clone()));

    let read = reading(&called);
    assert_eq!(read.said, "你推荐哪家\n最好便宜点");
    let lines: Vec<String> = read
        .transcript
        .into_iter()
        .map(|line| line.content)
        .collect();
    assert_eq!(
        lines,
        [
            "阿明：周五聚餐吗",
            "阿明：你推荐哪家",
            "小红：我想吃火锅",
            "阿明：最好便宜点"
        ]
    );
    // Answered with the line before it: not again on its own.
    assert!(with_group(&venue(chat), |group| group.waiting.is_empty()).unwrap());
    assert!(read_already(&added));
    assert!(!read_already(&called));
}

#[test]
fn after_a_restart_she_says_something_only_where_she_still_may() {
    use super::reaching_out::token_for;
    let mut config = crate::config::DynamicConfig::default();
    // A platform that is off: nothing.
    assert_eq!(token_for(ChannelPlatform::OneBot, "123", &config), None);
    config.onebot_bot_enabled = true;
    assert_eq!(
        token_for(ChannelPlatform::OneBot, "123", &config).as_deref(),
        Some("")
    );
    // A group taken off the allowlist since: nothing.
    config.onebot_bot_group_ids = "456".into();
    assert_eq!(token_for(ChannelPlatform::OneBot, "123", &config), None);
    assert!(token_for(ChannelPlatform::OneBot, "456", &config).is_some());
    // Telegram sends with the configured token, never with none.
    config.telegram_bot_enabled = true;
    assert_eq!(token_for(ChannelPlatform::Telegram, "-100", &config), None);
    config.telegram_bot_token = Some(" tg-token ".into());
    assert_eq!(
        token_for(ChannelPlatform::Telegram, "-100", &config).as_deref(),
        Some("tg-token")
    );
    config.qq_bot_enabled = true;
    assert_eq!(token_for(ChannelPlatform::Qq, "1", &config), None);
    let kept = serde_json::to_value(StoredReach {
        thread: Some(7),
        seen_at: chrono::Utc::now(),
    })
    .unwrap();
    assert!(kept.get("token").is_none());
    let back: StoredReach = serde_json::from_value(kept).unwrap();
    assert_eq!(back.thread, Some(7));
}

/// Telegram and Discord cannot read a group back: after a restart, the line
/// that called her and that she never got to is found in the lines kept.
#[test]
fn a_line_that_called_her_before_a_restart_is_found_in_what_was_kept() {
    heard_since_up();
    let at = |minutes: i64| chrono::Utc::now() - chrono::Duration::minutes(minutes);
    let line = |id: &str, minutes: i64, text: &str, hers: bool, addressed: bool| Line {
        at: at(minutes),
        message_id: (!hers).then(|| id.to_string()),
        name: if hers { String::new() } else { "阿明".into() },
        from: (!hers).then(|| "1".to_string()),
        text: text.into(),
        hers,
        addressed,
        images: Vec::new(),
        seen: Vec::new(),
    };
    let kept = vec![
        line("1", 40, "绮羽在吗", false, true),
        line("", 35, "在", true, false),
        line("3", 20, "绮羽你看这个", false, true),
        line("4", 10, "没人理我", false, false),
    ];
    // Kept before the flag was: read back as not calling her.
    let mut old = serde_json::to_value(&kept[0]).unwrap();
    old.as_object_mut().unwrap().remove("addressed");
    assert!(!serde_json::from_value::<Line>(old).unwrap().addressed);
    let venue = "telegram:-100777";
    let past = kept_as_heard(ChannelPlatform::Telegram, "-100777", Some(9), kept);
    let waiting = left_waiting(venue, &past).expect("the line she never got to");
    assert_eq!(waiting.message_id, "3");
    assert_eq!(waiting.thread, Some(9));
    assert!(waiting.addressed);
    // Once.
    assert!(left_waiting(venue, &past).is_none());
    // And a line taken up is answered once, however often it comes in.
    assert!(super::turns::first_time(venue, "3"));
    assert!(!super::turns::first_time(venue, "3"));
    assert!(super::turns::first_time(venue, ""));
    assert!(super::turns::first_time(venue, ""));
}
