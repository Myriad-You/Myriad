//! Unit tests for the site's side of group chat: the platforms' lines as
//! her group mind takes them, and what the site keeps. Her group mind
//! itself is tested in `myriad_merope::group` and held to the site's
//! recorded behaviour by `g0_scenes`.

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

fn transcript(venue: &str) -> Vec<String> {
    let now = now().timestamp_micros();
    with_group(venue, |group| {
        myriad_merope::group::transcript(group, None, now)
            .into_iter()
            .map(|spoken| spoken.content)
            .collect()
    })
    .unwrap_or_default()
}

/// After a restart a chat app sends its last batch again: a line heard
/// already is neither kept twice nor answered twice.
#[tokio::test]
async fn a_line_delivered_again_is_heard_once() {
    let chat = -9_017;
    assert!(record(&line(chat, 1, "阿明", "@bot 在吗")).await);
    assert!(!record(&line(chat, 1, "阿明", "@bot 在吗")).await);
    assert_eq!(transcript(&format!("telegram:{chat}")).len(), 1);
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
        let last = transcript(&venue).last().unwrap().clone();
        assert_eq!(last, "梦想成为猪侯王的leaphy：你去看ave mujika @染川 瞳");
    }
}

#[test]
fn a_group_is_its_venue_on_every_platform() {
    let telegram = line(-100123, 7, "阿明", "在吗");
    assert_eq!(telegram.venue(), "telegram:-100123");
    assert_eq!(telegram.message_id, "7");
    assert_eq!(telegram.to_message().venue(), telegram.venue());
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
    assert_eq!(
        discord.to_message().said(),
        "（回复你之前说的「听完要是」）说到一半怎么没了"
    );
    assert_eq!(discord.venue(), "discord:22");
    assert_eq!(discord.to_message().venue(), "discord:22");
}

#[test]
fn after_a_restart_she_says_something_only_where_she_still_may() {
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
        seen_at: crate::clock::now(),
    })
    .unwrap();
    assert!(kept.get("token").is_none());
    let back: StoredReach = serde_json::from_value(kept).unwrap();
    assert_eq!(back.thread, Some(7));
}

/// What she keeps of a group across a restart is in the shape it has always
/// been kept in: lines kept before her mind moved into the core read back.
#[test]
fn lines_kept_before_read_back_in_their_shape() {
    let kept = serde_json::json!({ "lines": [
        { "at": "2026-10-14T06:00:00Z", "message_id": "1", "name": "阿明", "from": "11",
          "text": "来点歌", "hers": false, "addressed": true },
        { "at": "2026-10-14T06:00:05Z", "message_id": null, "name": "", "from": null, "text": "放就放",
          "hers": true, "addressed": false },
    ]});
    let stored: StoredLines = serde_json::from_value(kept.clone()).unwrap();
    assert_eq!(stored.lines.len(), 2);
    assert!(stored.lines[0].addressed);
    assert_eq!(serde_json::to_value(&stored).unwrap(), kept);
}

/// A restart does not wipe the group from her mind: what was kept comes
/// back from the runtime registry with the first line after it.
#[tokio::test]
async fn a_restart_keeps_what_the_group_said() {
    let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
        return;
    };
    let schema = crate::db::IsolatedSchema::migrated(&url, "group_lines").await;
    crate::services::process_db::set_process_database(schema.db.clone());
    let chat = -9_300;
    let venue = format!("telegram:{chat}");
    record(&line(chat, 1, "阿明", "来点歌")).await;
    record(&line(chat, 2, "阿明", "放一首")).await;
    // The process restarts: nothing of the group is left in memory.
    forget(&venue);
    record(&line(chat, 3, "阿明", "说到一半怎么没了")).await;
    assert_eq!(
        transcript(&venue),
        ["阿明：来点歌", "阿明：放一首", "阿明：说到一半怎么没了"]
    );
    schema.drop().await;
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
        let Ok(ledger) = serde_json::from_value::<myriad_merope::group::Ledger>(row.payload) else {
            continue;
        };
        let (hers, members): (Vec<_>, Vec<_>) = ledger
            .messages
            .into_iter()
            .partition(|typed| typed.by == myriad_merope::group::HER);
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

/// A game's choices go out as buttons whose press comes back as the number
/// of the choice: one a row on Telegram, five a row on Discord; nothing else
/// reads as a game's button.
#[test]
fn a_games_buttons_come_back_as_their_number() {
    let choices: Vec<String> = (0..7).map(|n| format!("玩家{n}")).collect();
    let telegram = platform::telegram_buttons(&choices, 12).unwrap();
    assert_eq!(telegram["inline_keyboard"].as_array().unwrap().len(), 7);
    assert_eq!(telegram["inline_keyboard"][3][0]["callback_data"], "seat:12:3");
    let discord = platform::discord_buttons(&choices, 12).unwrap();
    let rows = discord.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[1]["components"][1]["custom_id"], "seat:12:6");
    assert!(platform::telegram_buttons(&[], 12).is_none());
    assert_eq!(platform::seat_button("seat:12:3"), Some((Some(12), "3".into())));
    assert_eq!(platform::seat_button("seat:12:x"), None);
    assert_eq!(platform::seat_button("seat:3"), Some((None, "3".into())));
    assert_eq!(platform::seat_button("approve:1"), None);
}

/// A pressed button of a game is the game's before anything looks at
/// pairing, on both platforms with buttons.
#[test]
fn a_games_button_is_the_games_before_pairing() {
    for (name, source, entry, lookup) in [
        (
            "telegram",
            include_str!("../telegram_pairing.rs"),
            "pub async fn handle_callback",
            "lookup_openid(",
        ),
        (
            "discord",
            include_str!("../discord_pairing.rs"),
            "pub async fn handle_component",
            "lookup_openid(",
        ),
    ] {
        let body = source.split(entry).nth(1).expect(name);
        let game = body.find("channel_group::pressed(").expect(name);
        assert!(game < body.find(lookup).expect(name), "{name}");
    }
}
