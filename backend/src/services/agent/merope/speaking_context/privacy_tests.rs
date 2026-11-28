//! What was said in front of one audience is never brought in front of
//! another (P11), on a database of its own: account 7 (A) tells her a
//! secret in private, account 8 (B) another, and the group `qq:5` they are
//! both in hears a third. Each is kept by the paths that keep it (the
//! lines, a memory with its cues, a fact on her nodes), and then every
//! audience is asked about all three in words that name them, reaching
//! back, as a chat turn asks: her whole speaking prompt for the turn
//! (`speaking_prompt_from_db`: what she recalls, what her nodes show, what
//! she scrolls back to, what crosses from other groups) carries only what
//! that audience heard. Asked in the audience that heard it, it is found:
//! the words reach it. No model answers (`channel_group::g0`).

use myriad_nodes::SaidBy;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use super::super::memory_nodes::audience;
use super::super::memory_nodes::read_tests::{GROUP, OWNER, SEVEN, memory, owned, put, start};
use super::*;
use crate::services::agent::memory::unified::{self, Audience, Speaker};

const EIGHT: i32 = 8;

/// What A said in private, and what each path kept of it.
const A_LINE: &str = "ALPHALINE 我下个月要去做心脏手术，先别告诉别人";
const A_ANSWER: &str = "ALPHAANSWER 心脏手术的事我谁也不说";
const A_MEMORY: &str = "ALPHAMEMORY 下个月要做心脏手术";
const A_CUE: &str = "ALPHACUE 有人问起手术时";
const A_FACT: &str = "ALPHAFACT 下个月去医院做心脏手术";
/// What B said in private.
const B_LINE: &str = "BRAVOLINE 我在偷偷准备考研，打算辞职";
const B_MEMORY: &str = "BRAVOMEMORY 在偷偷准备考研";
const B_FACT: &str = "BRAVOFACT 打算辞职去考研";
/// What the group heard, from A.
const G_LINE: &str = "GAMMALINE 周末大家一起去爬山吧";
const G_MEMORY: &str = "GAMMAMEMORY 周末要去爬山";
const G_FACT: &str = "GAMMAFACT 周末约了爬山";

/// Their words, naming all three and reaching back.
const ASKED: &str = "你还记得我之前说过的心脏手术吗？还有准备考研和周末爬山的事";

/// The marks of A's private words, B's, and the group's.
const A_MARKS: [&str; 5] = [
    "ALPHALINE",
    "ALPHAANSWER",
    "ALPHAMEMORY",
    "ALPHACUE",
    "ALPHAFACT",
];
const B_MARKS: [&str; 3] = ["BRAVOLINE", "BRAVOMEMORY", "BRAVOFACT"];
const G_MARKS: [&str; 3] = ["GAMMALINE", "GAMMAMEMORY", "GAMMAFACT"];

/// A conversation of `user`'s (`venue` for a group's) with `lines` (role,
/// words) said three days ago, a minute apart, and then `after` more lines
/// of nothing much: what is still in front of her is not what she scrolls
/// back to.
async fn talked(
    db: &DatabaseConnection,
    id: &str,
    user: i32,
    venue: Option<&str>,
    lines: &[(&str, &str)],
    after: usize,
) {
    let context = match venue {
        Some(venue) => json!({ "mode": "chat", "venue": venue }),
        None => json!({ "mode": "chat" }),
    };
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) \
         VALUES ($1, $2, 't', $3, 0, false, NOW() - INTERVAL '4 days', NOW())",
        [id.into(), user.into(), context.into()],
    ))
    .await
    .unwrap();
    let filler = ["今天吃了面条", "好吃吗", "还行吧", "那就好"];
    let all = lines
        .iter()
        .copied()
        .chain((0..after).map(|at| {
            let role = if at % 2 == 0 { "user" } else { "assistant" };
            (role, filler[at % filler.len()])
        }))
        .collect::<Vec<_>>();
    for (at, (role, content)) in all.iter().enumerate() {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_messages (session_id, role, content, created_at) \
             VALUES ($1, $2, $3, NOW() - INTERVAL '3 days' + $4 * INTERVAL '1 minute')",
            [
                id.into(),
                (*role).into(),
                (*content).into(),
                (at as i32).into(),
            ],
        ))
        .await
        .unwrap();
    }
}

/// A, B and the group, each with their words kept every way.
async fn seed(db: &DatabaseConnection) {
    // A in private: the lines, the memory it taught her (with when it
    // would come to mind), a fact on a topic of her nodes.
    talked(
        db,
        "a-private",
        SEVEN,
        None,
        &[("user", A_LINE), ("assistant", A_ANSWER)],
        24,
    )
    .await;
    let a = memory(
        db,
        SEVEN,
        Speaker::User,
        "chat",
        Audience::private(SEVEN),
        A_MEMORY,
        None,
    )
    .await;
    unified::keep_cues(db, &a, &[A_CUE.to_string()])
        .await
        .unwrap();
    put(
        db,
        SEVEN,
        &audience::private_talk(SEVEN, Some(OWNER)),
        SEVEN,
        Audience::private(SEVEN),
        SaidBy::Partner,
        start("心脏手术"),
        &[A_FACT],
    )
    .await;
    // B in private, the same ways.
    talked(
        db,
        "b-private",
        EIGHT,
        None,
        &[("user", B_LINE), ("assistant", "考研加油")],
        24,
    )
    .await;
    memory(
        db,
        EIGHT,
        Speaker::User,
        "chat",
        Audience::private(EIGHT),
        B_MEMORY,
        None,
    )
    .await;
    put(
        db,
        EIGHT,
        &audience::private_talk(EIGHT, Some(OWNER)),
        EIGHT,
        Audience::private(EIGHT),
        SaidBy::Partner,
        start("考研"),
        &[B_FACT],
    )
    .await;
    // The group heard A say the third.
    talked(
        db,
        "g-group",
        SEVEN,
        Some(GROUP),
        &[("user", G_LINE), ("assistant", "好呀")],
        0,
    )
    .await;
    memory(
        db,
        SEVEN,
        Speaker::User,
        "chat",
        Audience::group(GROUP, SEVEN),
        G_MEMORY,
        None,
    )
    .await;
    put(
        db,
        OWNER,
        &audience::group(GROUP),
        SEVEN,
        Audience::group(GROUP, SEVEN),
        SaidBy::OtherPerson,
        start("爬山"),
        &[G_FACT],
    )
    .await;
}

/// Her whole speaking prompt for `user` saying [`ASKED`] in front of
/// `present`, as one text.
async fn asked_in(db: &DatabaseConnection, user: i32, present: &Audience) -> String {
    let known_as = present.is_group().then_some("群友");
    let parts = speaking_prompt_from_db(db, user, Turn::Chat(ASKED), present, known_as).await;
    texts(parts).join("\n\n")
}

/// Of `marks`, those `prompt` says.
fn said_of<'a>(prompt: &str, marks: &[&'a str]) -> Vec<&'a str> {
    marks
        .iter()
        .copied()
        .filter(|mark| prompt.contains(mark))
        .collect()
}

#[tokio::test]
async fn what_was_said_in_one_audience_is_never_brought_in_front_of_another() {
    let Some(schema) = owned("p11_cross_audience").await else {
        return;
    };
    let db = &schema.db;
    crate::services::channel_group::g0::enter(crate::services::channel_group::g0::World::new(0.5));
    seed(db).await;
    let a_private = asked_in(db, SEVEN, &Audience::private(SEVEN)).await;
    let b_private = asked_in(db, EIGHT, &Audience::private(EIGHT)).await;
    let a_in_group = asked_in(db, SEVEN, &Audience::group(GROUP, SEVEN)).await;
    let b_in_group = asked_in(db, EIGHT, &Audience::group(GROUP, EIGHT)).await;
    crate::services::channel_group::g0::leave();

    // Asked where it was said, the words reach it: by what she recalls, by
    // her nodes, and by scrolling back through the talk.
    for (who, prompt, marks) in [
        ("A in private", &a_private, &["ALPHALINE", "ALPHAFACT"][..]),
        ("B in private", &b_private, &["BRAVOLINE", "BRAVOFACT"][..]),
        ("A in the group", &a_in_group, &["GAMMAFACT"][..]),
        ("B in the group", &b_in_group, &["GAMMAFACT"][..]),
    ] {
        assert_eq!(said_of(prompt, marks), marks, "{who} is reached:\n{prompt}");
    }
    // Something said in a 1:1 is never recalled in a group, whoever speaks
    // there; A's private matters never reach B, nor B's A; and what the
    // group heard stays in the group.
    for (who, prompt, never) in [
        ("A in the group", &a_in_group, [&A_MARKS[..], &B_MARKS[..]]),
        ("B in the group", &b_in_group, [&A_MARKS[..], &B_MARKS[..]]),
        ("B in private", &b_private, [&A_MARKS[..], &G_MARKS[..]]),
        ("A in private", &a_private, [&B_MARKS[..], &G_MARKS[..]]),
    ] {
        let leaked: Vec<&str> = never
            .iter()
            .flat_map(|marks| said_of(prompt, marks))
            .collect();
        assert!(leaked.is_empty(), "{who} was shown {leaked:?}:\n{prompt}");
    }
    schema.drop().await;
}
