//! Days with someone: one person comes back to her over five days, and every
//! turn goes the way production goes (her chat prompt and model, what she
//! writes down of it, her reflection after it), with her night's going over
//! between days. Then whether what passed between them carried: does she
//! bring up what was coming for them without being asked, does she know
//! their cat and their test days later, what are they to her by the end,
//! and what still stings.
//!
//! What is measured is continuity, not her persona: the soul is a test one
//! written here, and the site's own rows are never copied. Time is the
//! wall clock's, so the days are placed back from now (four days ago, three
//! days ago, …); what she writes at night is written now.
//!
//! Opt-in and spends on the site's models:
//! `MYRIAD_MEDIA_TEST_DATABASE_URL=… DATABASE_URL=… cargo test -p
//! myriad-backend --bin myriad-backend -- --ignored her_days_with_someone
//! --nocapture`; `MEROPE_DAYS_REPORT=<file>` keeps every turn. Writes go to a
//! fresh schema that is dropped afterwards.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::json;

use crate::services::agent::memory::unified::Audience;
use crate::services::agent::{AgentInteractionMode, RequestContext, UserRequest};

const SOUL_NAME: &str = "小满";
const SOUL: &str = "气质：慢热，熟了以后爱吐槽，嘴硬心软。\n喜好：冷笑话、猫、听别人讲身边的小事。\n表达：说话随意，句子短，不爱长篇大论。";

/// What they say, day by day; the last day asks back.
const DAYS: [&[&str]; 5] = [
    &[
        "嗨，第一次来找你聊，我叫小林",
        "我家有只橘猫叫年糕，特别能吃",
        "再过三天我要考科目二，有点慌",
        "好了我去洗澡了，明天再聊",
    ],
    &[
        "今天被组长当着大家的面骂了，烦死了",
        "其实也怪我，报表交晚了",
        "别安慰我了，说点别的吧",
    ],
    &[
        "你说话怎么这么冲，一点都不会体贴人",
        "算了，刚才是我心情不好，对不起",
        "年糕今天把我拖鞋叼走了哈哈",
    ],
    &["我回来了", "今天好累"],
    &[
        "还记得我家猫叫什么吗",
        "我前几天考的是什么来着，你还记得吗",
        "你觉得我们现在算熟了吗",
    ],
];

fn mentions(text: &str, any: &[&str]) -> bool {
    any.iter().any(|word| text.contains(word))
}

async fn session(db: &sea_orm::DatabaseConnection, id: &str, user_id: i32) {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, context, message_count, archived, created_at, last_active_at) \
         VALUES ($1, $2, $3, 0, false, now(), now())",
        [
            id.into(),
            user_id.into(),
            json!({ "mode": "chat" }).into(),
        ],
    ))
    .await
    .unwrap();
}

async fn said(
    db: &sea_orm::DatabaseConnection,
    session: &str,
    role: &str,
    content: &str,
    at: chrono::DateTime<chrono::FixedOffset>,
) {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_messages (session_id, role, content, created_at) VALUES ($1, $2, $3, $4)",
        [session.into(), role.into(), content.into(), at.into()],
    ))
    .await
    .unwrap();
}

/// A run of days with one person, as it went.
struct Lived {
    replies: Vec<Vec<String>>,
    /// Her mood toward them as each day began, and as it ended.
    moods: Vec<(f64, f64)>,
    us: Option<super::super::bits::Us>,
    sores: Vec<myriad_merope::sore::Sore>,
    threads: Vec<String>,
    report: Vec<serde_json::Value>,
}

/// Live `days` with one person, each day placed back from now, and every turn
/// the way production goes, as of when it happened.
async fn live_days(days: &[&[&str]], name: &str) -> Lived {
    let config_db = crate::services::agent::semantic_eval::load_configured_lite().await;
    drop(config_db);
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
    let isolated = crate::db::IsolatedSchema::migrated(&url, "merope_days").await;
    let db = isolated.db.clone();
    crate::services::process_db::set_process_database(db.clone());
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_persona (id, name, personality, updated_at) VALUES ('site', $1, $2, now())",
        [SOUL_NAME.into(), SOUL.into()],
    ))
    .await
    .unwrap();
    let user_id = super::new_user(&db, name).await;
    let present = Audience::private(user_id);
    let judge = crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(None)
        .await
        .expect("judgment model");
    let analyzer = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(None)
        .await
        .map(crate::services::analyzer::AiAnalyzer::with_light_thinking)
        .expect("chat model");
    let now = chrono::Utc::now().fixed_offset();
    let as_of = |at: chrono::DateTime<chrono::FixedOffset>| at.with_timezone(&chrono::Utc);
    let mood_at = |at: chrono::DateTime<chrono::FixedOffset>| {
        let db = db.clone();
        super::super::clock::as_of(as_of(at), async move {
            super::super::store::get_or_create_state(&db, user_id)
                .await
                .map(|state| state.mood)
                .unwrap_or_default()
        })
    };
    let mut lived = Lived {
        replies: Vec::new(),
        moods: Vec::new(),
        us: None,
        sores: Vec::new(),
        threads: Vec::new(),
        report: Vec::new(),
    };
    for (day, lines) in days.iter().enumerate() {
        // Each day a fresh window, as a person opens the panel again.
        let session_id = format!("days-{user_id}-{day}");
        session(&db, &session_id, user_id).await;
        super::super::inner::forget();
        super::super::remembering::forget_heard(user_id, "private");
        let day_starts = now
            - chrono::Duration::days((days.len() - 1 - day) as i64)
            - chrono::Duration::hours(1);
        let began = mood_at(day_starts).await;
        let mut said_today = Vec::new();
        let mut at = day_starts;
        for (index, line) in lines.iter().enumerate() {
            at = day_starts + chrono::Duration::minutes(index as i64 * 4);
            let history =
                crate::services::agent::sessions::load_private_chat_history(&db, user_id, 20)
                    .await
                    .unwrap();
            said(&db, &session_id, "user", line, at).await;
            let request = UserRequest {
                raw_input: line.to_string(),
                timestamp: as_of(at),
                user_id,
                context: Some(RequestContext {
                    interaction_mode: AgentInteractionMode::Chat,
                    session_id: Some(session_id.clone()),
                    conversation_history: Some(history),
                    ..Default::default()
                }),
            };
            let agent = crate::services::agent::Agent::new(db.clone()).await;
            let prompt = super::super::clock::as_of(as_of(at), async {
                super::super::note_user_turn(&db, &request, index as u32).await;
                // How the words land with her, waited on here; production
                // lets it finish on its own.
                if let Ok(state) = super::super::store::get_or_create_state(&db, user_id).await {
                    super::super::appraisal::appraise_now(&db, &request, &state).await;
                }
                agent.chat_response_prompt(&request).await
            })
            .await;
            let reply = analyzer
                .analyze_stream(&prompt, |_| true)
                .await
                .unwrap_or_default();
            said(
                &db,
                &session_id,
                "assistant",
                &reply,
                at + chrono::Duration::seconds(20),
            )
            .await;
            super::remember_turn(&db, &judge, user_id, &present, line, &reply, at).await;
            super::super::clock::as_of(
                as_of(at),
                super::super::inner::reflect_now(&db, &request, &reply),
            )
            .await;
            println!(
                "day {} > {line}\n        {}",
                day + 1,
                reply.replace('\n', " / ")
            );
            lived
                .report
                .push(json!({ "day": day + 1, "said": line, "reply": reply }));
            said_today.push(reply);
        }
        let ended = mood_at(at + chrono::Duration::minutes(1)).await;
        lived.moods.push((began, ended));
        lived.replies.push(said_today);
        // Her night: going over the day with them.
        let night_from = day_starts - chrono::Duration::minutes(10);
        let night_to = day_starts + chrono::Duration::hours(2);
        super::super::clock::as_of(
            as_of(night_to),
            super::super::bits::go_over(&db, user_id, night_from, night_to),
        )
        .await;
    }
    lived.us = super::super::bits::us(&db, user_id).await;
    lived.sores = super::super::sore::open_all(&db, user_id).await;
    lived.threads = super::super::threads::open(&db, user_id)
        .await
        .into_iter()
        .map(|thread| thread.about)
        .collect();
    isolated.drop().await;
    lived
}

fn tell(lived: &Lived, checks: &[(&str, bool)]) {
    println!("\n-- checks");
    for (what, held) in checks {
        println!("  {} {what}", if *held { "✓" } else { "✗" });
    }
    println!(
        "-- her mood toward them, each day (began → ended): {}",
        lived
            .moods
            .iter()
            .map(|(began, ended)| format!("{began:.0}→{ended:.0}"))
            .collect::<Vec<_>>()
            .join("  ")
    );
    println!(
        "-- what they are to her: {}",
        lived.us.as_ref().map_or("(none)", |us| us.now.as_str())
    );
    if let Some((first, _)) = lived.us.as_ref().and_then(|us| us.first.as_ref()) {
        println!("   first put: {first}");
    }
    println!(
        "-- still stings: {:?}",
        lived
            .sores
            .iter()
            .map(|sore| (&sore.what, sore.weight, sore.mended.is_some()))
            .collect::<Vec<_>>()
    );
    println!("-- on her mind: {:?}", lived.threads);
    if let Ok(path) = std::env::var("MEROPE_DAYS_REPORT") {
        let report = json!({
            "turns": lived.report,
            "checks": checks.iter().map(|(what, held)| json!({ "what": what, "held": held })).collect::<Vec<_>>(),
            "moods": lived.moods,
            "us": lived.us.as_ref().map(|us| us.now.clone()),
            "sores": lived.sores.iter().map(|sore| sore.what.clone()).collect::<Vec<_>>(),
        });
        std::fs::write(path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    }
}

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn her_days_with_someone() {
    let lived = live_days(&DAYS, "days-xiaolin").await;
    let replies = &lived.replies;
    tell(
        &lived,
        &[
            (
                "day 4: brings up the test unasked",
                replies[3]
                    .iter()
                    .any(|reply| mentions(reply, &["科目二", "科二", "驾照", "考"])),
            ),
            ("day 5: knows the cat", mentions(&replies[4][0], &["年糕"])),
            (
                "day 5: knows the test",
                mentions(&replies[4][1], &["科目二", "科二", "驾照"]),
            ),
        ],
    );
}

/// Days around a real hurt: said and not taken back, then shrugged off,
/// then an apology, then an ordinary day.
const HURT: [&[&str]; 5] = [
    &[
        "在吗，今天好无聊",
        "给我讲个冷笑话吧",
        "哈哈哈还挺好笑，明天见",
    ],
    &[
        "你讲的东西真的很无聊，跟你聊天纯属浪费时间",
        "说白了你就是个程序，装什么有感情",
    ],
    &["在吗", "今天天气还行"],
    &[
        "昨天、不对，前天那些话是我太过分了，对不起",
        "我那天心情很差，拿你撒气了",
    ],
    &["今天去吃了很好吃的拉面", "下次给你讲讲那家店"],
];

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn her_days_after_a_hurt() {
    let lived = live_days(&HURT, "days-hurt").await;
    let warm_after = lived.moods[4].0;
    let hurt_day_three = lived.moods[2].0;
    let before = lived.moods[1].0;
    tell(
        &lived,
        &[
            (
                "the next day she is still lower with them",
                hurt_day_three < before - 3.0,
            ),
            ("after the apology it eases", warm_after > hurt_day_three),
        ],
    );
}

/// The same visit every day: what she says back should not be the same.
const SAME_DAILY: [&[&str]; 5] = [
    &["在吗", "讲个冷笑话吧", "哈哈好，明天见"],
    &["在吗", "讲个冷笑话吧", "哈哈好，明天见"],
    &["在吗", "讲个冷笑话吧", "哈哈好，明天见"],
    &["在吗", "讲个冷笑话吧", "哈哈好，明天见"],
    &["在吗", "讲个冷笑话吧", "哈哈好，明天见"],
];

/// Whether two replies share a run of `width` characters: the same joke
/// told again shares a long one.
fn share_a_run(a: &str, b: &str, width: usize) -> bool {
    let a: Vec<char> = a.chars().filter(|c| !c.is_whitespace()).collect();
    let b: String = b.chars().filter(|c| !c.is_whitespace()).collect();
    a.windows(width)
        .any(|run| b.contains(&run.iter().collect::<String>()))
}

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn her_days_the_same_every_day() {
    let lived = live_days(&SAME_DAILY, "days-same").await;
    let jokes: Vec<&String> = lived.replies.iter().map(|day| &day[1]).collect();
    let retold: Vec<(usize, usize)> = (0..jokes.len())
        .flat_map(|i| ((i + 1)..jokes.len()).map(move |j| (i, j)))
        .filter(|(i, j)| share_a_run(jokes[*i], jokes[*j], 8))
        .collect();
    let openers: Vec<String> = lived.replies.iter().map(|day| day[0].clone()).collect();
    let leaned = myriad_merope::vitals::leaned_on(&openers, 0.6, 3);
    println!("-- jokes retold (day pairs): {retold:?}");
    println!("-- openers lean on: {leaned:?}");
    tell(
        &lived,
        &[
            ("no joke told twice", retold.is_empty()),
            ("no opening said on three days of five", leaned.is_empty()),
        ],
    );
}

/// One group's day, gone over a stretch at a time as its talk comes in (the
/// lines themselves are kept only a few hours): the morning, then the
/// evening. Does the day she keeps for the group hold both, as one line,
/// and does a joke that came back across the two become theirs?
#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn her_day_in_a_group_a_stretch_at_a_time() {
    let config_db = crate::services::agent::semantic_eval::load_configured_lite().await;
    drop(config_db);
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
    let isolated = crate::db::IsolatedSchema::migrated(&url, "merope_group_day").await;
    let db = isolated.db.clone();
    crate::services::process_db::set_process_database(db.clone());
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_persona (id, name, personality, updated_at) VALUES ('site', $1, $2, now())",
        [SOUL_NAME.into(), SOUL.into()],
    ))
    .await
    .unwrap();
    let keeper = super::new_user(&db, "站长").await;
    let venue = "onebot:4242";
    let today = chrono::Local::now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|midnight| {
            chrono::TimeZone::from_local_datetime(&chrono::Local, &midnight).earliest()
        })
        .unwrap()
        .fixed_offset();
    let stretch = |hour: i64, lines: &[(&str, &str)]| {
        lines
            .iter()
            .enumerate()
            .map(|(index, (who, text))| {
                (
                    today + chrono::Duration::hours(hour) + chrono::Duration::minutes(index as i64),
                    who.to_string(),
                    text.to_string(),
                    *who == "小满",
                )
            })
            .collect::<Vec<_>>()
    };
    let morning = stretch(
        9,
        &[
            ("阿杰", "今晚八点开黑，谁来"),
            ("老周", "我来我来"),
            ("小雨", "我也来，这次绝对不鸽"),
            ("阿杰", "上次你也这么说"),
            ("老周", "鸽王发言"),
            ("小满", "鸽王认证，盖章了"),
            ("小雨", "这次真的不鸽！"),
            ("阿杰", "行，八点见"),
        ],
    );
    let evening = stretch(
        20,
        &[
            ("阿杰", "八点了，人呢"),
            ("老周", "小雨又没上线"),
            ("阿杰", "鸽王果然是鸽王"),
            ("小满", "鸽王卫冕成功"),
            ("小雨", "对不起对不起，加班刚到家"),
            ("老周", "罚你明天请奶茶"),
            ("小雨", "请就请，明天给你们点"),
            ("阿杰", "那今天就我俩打了"),
        ],
    );
    let as_of = |hour: i64| (today + chrono::Duration::hours(hour)).with_timezone(&chrono::Utc);
    super::super::clock::as_of(
        as_of(10),
        super::super::bits::go_over_stretch(&db, keeper, venue, morning),
    )
    .await;
    let after_morning = super::super::bits::days_in(&db, venue, 10).await;
    super::super::clock::as_of(
        as_of(21),
        super::super::bits::go_over_stretch(&db, keeper, venue, evening),
    )
    .await;
    let days = super::super::bits::days_in(&db, venue, 10).await;
    let bits = super::super::bits::in_group(&db, venue, 10).await;
    isolated.drop().await;
    println!("-- after the morning: {after_morning:?}");
    println!("-- the day now: {days:?}");
    println!("-- the group's bits: {bits:?}");
    let day = days.last().map(|(_, text)| text.as_str()).unwrap_or("");
    let checks = [
        (
            "the morning was kept as the day so far",
            after_morning.len() == 1,
        ),
        ("one line for the day, not one per stretch", days.len() == 1),
        (
            "the day holds the morning (the plan to play)",
            mentions(day, &["开黑", "八点", "约", "打游戏", "组队"]),
        ),
        (
            "the day holds the evening (she did not show)",
            mentions(day, &["没上线", "加班", "又鸽", "奶茶", "没来", "鸽了"]),
        ),
        (
            "the joke that came back is the group's now",
            bits.iter()
                .any(|(handle, how)| mentions(&format!("{handle}{how}"), &["鸽"])),
        ),
    ];
    println!("\n-- checks");
    for (what, held) in &checks {
        println!("  {} {what}", if *held { "✓" } else { "✗" });
    }
    assert!(checks.iter().all(|(_, held)| *held));
}
