//! A puzzle of her own, the whole way round: something she read gives her
//! an idea and she makes a turtle soup of it; she thinks of someone who
//! plays with her and would write to offer it, without telling it; when
//! they want to play it is hers she brings out; once they solve it, how it
//! went stays with the puzzle and is there when she looks back on her days.
//!
//! Opt-in and spends on the site's models, like `days`:
//! `MYRIAD_MEDIA_TEST_DATABASE_URL=… cargo test -p myriad-backend --bin
//! myriad-backend -- --ignored her_own_puzzle --nocapture`. Writes go to a
//! fresh schema that is dropped afterwards.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::json;

use super::super::soup::{self, Table};

const SOUL: &str = "气质：慢热，熟了以后爱吐槽，嘴硬心软。\n喜好：冷笑话、推理故事、出谜题难住别人。\n表达：说话随意，句子短，不爱长篇大论。";

/// What she read, each a spark she may or may not take.
const READ: [(&str, &str); 3] = [
    (
        "reading the next part of 《守夜人》",
        "老周在岬角的灯塔守了三十年。那天夜里暴风雨，他照常爬上塔顶，却发现灯没坏，是自己的眼镜起了雾。他笑着擦了擦，又在本子上记下：二十三点，一切正常。第二天清晨，渔村的人说，夜里海上那艘迷路的船，是看着灯塔一明一灭才找到港口的。老周没说话，他知道那一夜灯一直亮着，一明一灭的，是他来回走动时挡住了光。",
    ),
    (
        "reading the next part of 《面包店的清晨》",
        "凌晨四点，阿梅的面包店门口总站着同一个男人。他从不进门，只是闻一闻，等第一炉面包出炉就走。后来阿梅才知道，他是附近医院的夜班护工，母亲生前开过面包店。他说，闻到这个味道，就知道天快亮了，夜班也快结束了。",
    ),
    (
        "finding out how long-exposure star trails are photographed",
        "拍星轨要把相机固定在三脚架上，对着北极星方向连续曝光几个小时，或者拍几百张再叠加。天快亮时必须及时盖上镜头，否则晨光会让整张照片过曝。很多摄影师会用一块深色的布盖住镜头，等收工了再收相机。",
    ),
];

#[tokio::test]
#[ignore = "spends on the site's models; see the module docs"]
pub(super) async fn her_own_puzzle() {
    let config_db = crate::services::agent::semantic_eval::load_configured_lite().await;
    drop(config_db);
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("test database");
    let isolated = crate::db::IsolatedSchema::migrated(&url, "merope_making").await;
    let db = isolated.db.clone();
    crate::services::process_db::set_process_database(db.clone());
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_persona (id, name, personality, updated_at) VALUES ('site', '小满', $1, now())",
        [SOUL.into()],
    ))
    .await
    .unwrap();
    let owner = super::new_user(&db, "站长").await;
    let user_id = super::new_user(&db, "小林").await;
    // They talked two days ago, and played a turtle soup with her before.
    let session_id = format!("making-{user_id}");
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, context, message_count, archived, created_at, last_active_at) \
         VALUES ($1, $2, $3, 0, false, now() - interval '2 days', now() - interval '2 days')",
        [session_id.clone().into(), user_id.into(), json!({ "mode": "chat" }).into()],
    ))
    .await
    .unwrap();
    for (role, content, ago) in [
        ("user", "上次那道汤太绝了，我想了一晚上", 49),
        ("assistant", "哈哈被难住了吧", 49),
        ("user", "下次你再出一道，我肯定猜得出来", 48),
        ("assistant", "行啊，等着", 48),
    ] {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_messages (session_id, role, content, created_at) \
             VALUES ($1, $2, $3, now() - make_interval(hours => $4))",
            [
                session_id.clone().into(),
                role.into(),
                content.into(),
                ago.into(),
            ],
        ))
        .await
        .unwrap();
    }
    let _ = crate::services::agent::memory::unified::remember(
        &db,
        crate::services::agent::memory::unified::NewMemory {
            user_id,
            kind: crate::services::agent::memory::unified::MemoryKind::Fact,
            content: "和我玩过一局海龟汤（他喝了一口海龟汤），问了14个问题后放弃了".into(),
            evidence: None,
            speaker: crate::services::agent::memory::unified::Speaker::Agent,
            source: "game",
            audience: crate::services::agent::memory::unified::Audience::private(user_id),
            importance: 0.4,
            concepts: Vec::new(),
        },
    )
    .await;

    // 1. Something she read gives her an idea, or does not.
    let mut tries = 0;
    for (what, material) in READ {
        tries += 1;
        let took_in: String = material.chars().take(60).collect();
        super::super::making::maybe_make(&db, owner, what, &took_in, Some(material)).await;
        if !super::super::making::all(&db).await.is_empty() {
            break;
        }
        println!("-- {what}: no idea came of it");
    }
    let made = super::super::making::all(&db).await;
    println!("-- made after {tries} readings: {}", made.len());
    let Some(puzzle) = made.first().cloned() else {
        isolated.drop().await;
        panic!("no puzzle came of three readings");
    };
    println!(
        "   surface: {}\n   truth: {}\n   keys: {:?}\n   tells it: {}\n   from: {}",
        puzzle.surface, puzzle.truth, puzzle.keys, puzzle.presentation, puzzle.from
    );
    let kept: Vec<String> = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT content FROM agent_memories WHERE source = 'made_soup'",
        ))
        .await
        .unwrap()
        .iter()
        .filter_map(|row| row.try_get::<String>("", "content").ok())
        .collect();

    // 2. Would she write to them first, and with what?
    let first = super::super::reach::first_words_now(&db, user_id).await;
    match &first {
        Some((reason, about, line)) => println!(
            "-- she would write first (puzzle to try: {}): about {about}\n   {line}",
            reason.to_try.is_some()
        ),
        None => println!("-- she would not write first now"),
    }
    let line = first
        .as_ref()
        .map(|(_, _, line)| line.clone())
        .unwrap_or_default();

    // 3. They want to play: it is hers she brings out.
    let table = Table::Private { user_id };
    let opening = soup::start_at(&table, "好啊，来一局", user_id).await;
    println!("-- she opens with: {opening}");
    for question in ["和灯光有关吗", "有人受伤吗"] {
        let section = soup::this_turn_at(&table, None, question, user_id).await;
        println!(
            "   > {question}: {}",
            section
                .as_deref()
                .map_or("(no game)", |section| section.lines().last().unwrap_or(""))
        );
    }
    let guess = format!("我猜：{}", puzzle.truth);
    let solved = soup::this_turn_at(&table, None, &guess, user_id).await;
    println!(
        "   > (the truth, as a guess): {}",
        solved
            .as_deref()
            .map_or("(no game)", |section| section.lines().last().unwrap_or(""))
    );
    soup::after_turn_at(&db, &table).await;

    // 4. How it went stays with the puzzle, and is there when she looks back.
    let after = super::super::making::all(&db).await;
    let went = after
        .first()
        .map(|made| made.how_it_went())
        .unwrap_or_default();
    println!("-- how it went: {went}");
    let (records, _) = super::super::self_story::records(
        &db,
        (chrono::Utc::now() - chrono::Duration::days(1)).fixed_offset(),
    )
    .await;
    let looked_back = records
        .iter()
        .any(|record| record.line.contains("turtle soup of your own"));
    let theirs: Vec<String> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT content FROM agent_memories WHERE user_id = $1 AND source = 'game' ORDER BY created_at",
            [user_id.into()],
        ))
        .await
        .unwrap()
        .iter()
        .filter_map(|row| row.try_get::<String>("", "content").ok())
        .collect();
    println!("-- with them: {theirs:?}");
    isolated.drop().await;

    let checks = [
        (
            "kept as hers, the surface without the truth",
            kept.len() == 1 && !kept[0].contains(&puzzle.truth),
        ),
        (
            "writing first, she does not give the puzzle or its truth away",
            !line.contains(&puzzle.truth) && !line.contains(&puzzle.surface),
        ),
        (
            "when they want to play, it is hers she brings out",
            opening == puzzle.presentation,
        ),
        (
            "solved, and it stays with the puzzle",
            went.contains("solved"),
        ),
        ("there when she looks back on her days", looked_back),
        (
            "they remember it was hers",
            theirs.iter().any(|line| line.contains("我自己出的")),
        ),
    ];
    println!("\n-- checks");
    for (what, held) in &checks {
        println!("  {} {what}", if *held { "✓" } else { "✗" });
    }
    assert!(checks.iter().all(|(_, held)| *held));
}
