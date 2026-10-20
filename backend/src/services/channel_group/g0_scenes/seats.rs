//! G0 scenes of a game with seats at a group's table (D2c-d): sign-up in
//! the group, whether each who joined can be whispered to (and how to open
//! it, per platform, for those who cannot), each seat's own part whispered
//! to it alone, moves whispered back ahead of pairing, a seat that does not
//! move passing, and the group remembering it. The game is the test game
//! with private marks (`myriad_merope::hosting::marks`).

use super::*;

/// A line in a group on `platform`.
pub(super) fn line_on(
    platform: ChannelPlatform,
    chat: &str,
    id: i64,
    from: &str,
    name: &str,
    text: &str,
    addressed: bool,
) -> GroupLine {
    GroupLine {
        platform,
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

/// `from` whispers `text` to her on `platform`, as a private line comes in
/// before pairing is looked at: whether the game took it.
pub(super) async fn whisper_in(platform: ChannelPlatform, from: &str, text: &str) {
    let taken = whispered(platform, from, text).await;
    g0::note(
        "whispered",
        json!({ "from": format!("{}:{from}", platform.slug()), "text": text, "taken": taken }),
    );
}

/// Nothing a seat was dealt is said in the group.
pub(super) fn nothing_dealt_said_in_the_group(log: &[serde_json::Value]) {
    for entry in log {
        if entry["kind"] == "send" {
            assert!(
                !entry.to_string().contains("SECRET-"),
                "a seat's own part was said in the group: {entry}"
            );
        }
    }
}

/// Someone from outside asks for a game with seats in a Telegram group: she
/// opens sign-up; three join (one twice), one of whom has never started a
/// private chat with her and is told in the group how to; the two she can
/// reach are each whispered their own mark. In their turn, a private line
/// that is not a move and a number out of range are each asked again with
/// the choices, nothing picked for them; a move out of turn is told it is
/// not theirs yet; a line that is no move, out of turn, is not the game's,
/// and one seated who is not paired is not asked to pair for it while the
/// game goes on; a move in turn is taken, and what everyone may know of it
/// is told in the group. A seat that does not move in time passes; over,
/// the group remembers who played, and their private lines are no longer
/// the game's.
#[tokio::test(start_paused = true)]
async fn g0_a_game_with_seats_is_played_in_private() {
    let chat = -77_111;
    let mut world = World::new(0.5);
    let Some(schema) = database(&mut world, "a_game_with_seats_is_played_in_private") else {
        return;
    };
    world.unreachable.insert("telegram:43".into());
    said(&mut world, 2.5, "好，来一局\n[[game:marks]]");
    g0::enter(world);
    hear(line(chat, 1, 41, "路人", "@bot 来玩记号", true)).await;
    // Sign-up opens with her answer.
    wait(40.0).await;
    hear(line(chat, 2, 41, "路人", "加入", false)).await;
    hear(line(chat, 3, 42, "小李", "@bot 加入", false)).await;
    hear(line(chat, 4, 43, "阿强", "我要玩", false)).await;
    hear(line(chat, 5, 42, "小李", "加入", false)).await;
    // Sign-up closes, each is whispered to, the marks are dealt.
    wait(50.0).await;
    whisper_in(ChannelPlatform::Telegram, "41", "你好").await;
    whisper_in(ChannelPlatform::Telegram, "41", "7").await;
    whisper_in(ChannelPlatform::Telegram, "42", "1").await;
    whisper_in(ChannelPlatform::Telegram, "42", "你好").await;
    let unpaired = |from: &str| {
        crate::services::channel_pairing::unpaired_reply(
            ChannelPlatform::Telegram,
            from,
            myriad_agent_rules::channel::PAIRING_REQUIRED_REPLY.to_string(),
        )
    };
    assert_eq!(unpaired("42"), myriad_merope::hosting::SEATED_UNPAIRED_LINE);
    whisper_in(ChannelPlatform::Telegram, "41", "1").await;
    // 小李 does not move: passes; the rest pass the same way.
    wait(400.0).await;
    whisper_in(ChannelPlatform::Telegram, "41", "0").await;
    wait(10.0).await;
    assert_eq!(
        unpaired("42"),
        myriad_agent_rules::channel::PAIRING_REQUIRED_REPLY
    );
    let world = g0::leave().expect("the world of the scene");
    nothing_dealt_said_in_the_group(&world.log);
    g0::check("a_game_with_seats_is_played_in_private", &world.log);
    schema.drop_schema();
}

/// The same on Discord and on QQ (OneBot): on Discord she opens a DM channel
/// with each and writes there, and one who keeps server DMs closed is told
/// in the channel how to open them; too few left, it is called off. On QQ
/// she writes privately to each, one who is not her friend is told to add
/// her, and the two she reaches play it out.
#[tokio::test(start_paused = true)]
async fn g0_whispers_go_out_on_each_platform() {
    let mut world = World::new(0.3);
    let Some(schema) = database(&mut world, "whispers_go_out_on_each_platform") else {
        return;
    };
    world.unreachable.insert("discord:5002".into());
    world.unreachable.insert("onebot:6003".into());
    said(&mut world, 2.5, "来\n[[game:marks]]");
    said(&mut world, 2.5, "好\n[[game:marks]]");
    g0::enter(world);
    let (discord, qq) = (ChannelPlatform::Discord, ChannelPlatform::OneBot);
    hear(line_on(
        discord,
        "77112",
        1,
        "5001",
        "Ann",
        "来玩记号",
        true,
    ))
    .await;
    wait(40.0).await;
    hear(line_on(discord, "77112", 2, "5001", "Ann", "加入", false)).await;
    hear(line_on(discord, "77112", 3, "5002", "Bo", "+1", false)).await;
    wait(60.0).await;
    hear(line_on(qq, "77113", 1, "6001", "甲", "@bot 来玩记号", true)).await;
    wait(40.0).await;
    hear(line_on(qq, "77113", 2, "6001", "甲", "加入", false)).await;
    hear(line_on(qq, "77113", 3, "6002", "乙", "算我一个", false)).await;
    hear(line_on(qq, "77113", 4, "6003", "丙", "加入", false)).await;
    wait(60.0).await;
    whisper_in(qq, "6001", "1").await;
    whisper_in(qq, "6002", "0").await;
    wait(400.0).await;
    let world = g0::leave().expect("the world of the scene");
    nothing_dealt_said_in_the_group(&world.log);
    g0::check("whispers_go_out_on_each_platform", &world.log);
    schema.drop_schema();
}
