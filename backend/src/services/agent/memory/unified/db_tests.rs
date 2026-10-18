use super::*;
use sea_orm::{ConnectionTrait, DatabaseConnection};

async fn temp_db() -> Option<DatabaseConnection> {
    let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").ok()?;
    let mut options = sea_orm::ConnectOptions::new(url);
    options.max_connections(1).sqlx_logging(false);
    let db = sea_orm::Database::connect(options).await.unwrap();
    let ddl = crate::db::schema_check::AGENT_MEMORIES_DDL
        .replace("CREATE TABLE IF NOT EXISTS", "CREATE TEMP TABLE")
        .replace("REFERENCES users(id) ON DELETE CASCADE", "");
    db.execute_unprepared(&ddl).await.unwrap();
    Some(db)
}

fn fact(user_id: i32, content: &str) -> NewMemory {
    NewMemory {
        user_id,
        kind: MemoryKind::Fact,
        content: content.into(),
        evidence: Some(content.into()),
        speaker: Speaker::User,
        source: "chat",
        audience: Audience::private(user_id),
        importance: 0.5,
        concepts: Vec::new(),
    }
}

#[tokio::test]
async fn remember_recall_and_supersede_stay_within_the_person() {
    let Some(db) = temp_db().await else {
        return;
    };
    let first = remember(&db, fact(7, "prefers saffron tea")).await.unwrap();
    assert!(first.is_some());
    assert!(
        remember(&db, fact(7, "  prefers   saffron tea "))
            .await
            .unwrap()
            .is_none(),
        "the same fact is stored once"
    );
    remember(&db, fact(7, "works night shifts")).await.unwrap();
    remember(&db, fact(8, "prefers saffron tea")).await.unwrap();

    let tea = recall(
        &db,
        7,
        &Audience::private(7),
        Some("tea"),
        &MemoryKind::ABOUT_PERSON,
        8,
    )
    .await
    .unwrap();
    // Learned moments apart, the night shifts come along by association;
    // user 8's identical fact is never in the graph at all.
    assert_eq!(
        tea.iter()
            .map(|memory| memory.content.as_str())
            .collect::<Vec<_>>(),
        vec!["prefers saffron tea", "works night shifts"]
    );
    assert!(tea.iter().all(|memory| memory.user_id == Some(7)));

    assert!(
        recall(&db, 7, &Audience::private(8), None, &[], 8)
            .await
            .unwrap()
            .is_empty(),
        "user 8 is not in the audience of user 7's memories"
    );

    let saffron: Vec<String> = active(&db, 7, &[])
        .await
        .unwrap()
        .into_iter()
        .filter(|row| row.content == "prefers saffron tea")
        .map(|row| row.id)
        .collect();
    assert_eq!(retire(&db, 7, &saffron, "superseded").await.unwrap(), 1);
    let left = recall(&db, 7, &Audience::private(7), None, &[], 8)
        .await
        .unwrap();
    assert_eq!(left.len(), 1);
    assert_eq!(left[0].content, "works night shifts");
    assert!(
        remember(&db, fact(7, "prefers saffron tea"))
            .await
            .unwrap()
            .is_some(),
        "a retired fact may be learned again"
    );
}

#[tokio::test]
async fn a_group_and_a_private_chat_never_share_what_they_heard() {
    let Some(db) = temp_db().await else {
        return;
    };
    let group = Audience::group("telegram:-100123", 7);
    remember(&db, fact(7, "私下说过在准备跳槽")).await.unwrap();
    let mut said_in_group = fact(7, "周五想去吃火锅");
    said_in_group.audience = group.clone();
    assert!(remember(&db, said_in_group).await.unwrap().is_some());
    let mut someone_else = fact(8, "周五要加班");
    someone_else.audience = Audience::group("telegram:-100123", 8);
    remember(&db, someone_else).await.unwrap();
    let mut same_words = fact(7, "私下说过在准备跳槽");
    same_words.audience = group.clone();
    assert!(
        remember(&db, same_words).await.unwrap().is_some(),
        "said again in front of the group, the group now shares it"
    );

    let in_group: Vec<String> = recall(&db, 7, &group, None, &[], 8)
        .await
        .unwrap()
        .into_iter()
        .map(|memory| memory.content)
        .collect();
    assert!(in_group.contains(&"周五想去吃火锅".to_string()));
    assert!(
        in_group.contains(&"周五要加班".to_string()),
        "whoever said it in the group"
    );
    assert_eq!(
        in_group
            .iter()
            .filter(|content| content.contains("跳槽"))
            .count(),
        1,
        "only the copy the group heard"
    );
    let in_private: Vec<String> = recall(&db, 7, &Audience::private(7), None, &[], 8)
        .await
        .unwrap()
        .into_iter()
        .map(|memory| memory.content)
        .collect();
    assert_eq!(
        in_private,
        vec!["私下说过在准备跳槽"],
        "the group's memories stay there"
    );
    let other_group = Audience::group("telegram:-100999", 7);
    assert!(
        recall(&db, 7, &other_group, None, &[], 8)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn her_own_days_belong_to_no_one_and_are_kept_once() {
    let Some(db) = temp_db().await else {
        return;
    };
    let day = chrono::NaiveDate::from_ymd_opt(2026, 9, 24).unwrap();
    assert!(
        write_own_day(&db, day, "今天陪了好几个人聊天，有点累。")
            .await
            .unwrap()
    );
    assert!(
        !write_own_day(&db, day, "另一个版本").await.unwrap(),
        "one entry per day"
    );
    let days = own_days(&db, 3).await.unwrap();
    assert_eq!(days.len(), 1);
    assert_eq!(days[0].content, "今天陪了好几个人聊天，有点累。");
    assert_eq!(days[0].user_id, None);
    remember(&db, fact(7, "养了一只猫")).await.unwrap();
    assert!(
        recall(&db, 7, &Audience::private(7), None, &[], 8)
            .await
            .unwrap()
            .iter()
            .all(|memory| memory.user_id == Some(7)),
        "her days never come back as a memory about someone"
    );
}

#[tokio::test]
async fn old_memories_get_concepts_only_once_and_only_for_their_person() {
    let Some(db) = temp_db().await else {
        return;
    };
    let id = remember(&db, fact(7, "养了一只猫叫年糕"))
        .await
        .unwrap()
        .unwrap();
    remember(&db, fact(8, "喜欢茶")).await.unwrap();
    let mut people = people_without_concepts(&db, 10).await.unwrap();
    people.sort();
    assert_eq!(people, vec![7, 8]);
    let cat = || {
        vec![Concept {
            name: "猫".into(),
            aliases: vec!["喵".into()],
        }]
    };
    assert!(
        !fill_concepts(&db, 8, &id, cat()).await.unwrap(),
        "not 8's memory"
    );
    assert!(fill_concepts(&db, 7, &id, cat()).await.unwrap());
    assert!(
        !fill_concepts(&db, 7, &id, cat()).await.unwrap(),
        "already filled"
    );
    assert!(without_concepts(&db, 7, 10).await.unwrap().is_empty());
    let found = recall(&db, 7, &Audience::private(7), Some("喵呢"), &[], 8)
        .await
        .unwrap();
    assert_eq!(found[0].id, id);
}

#[tokio::test]
async fn only_what_comes_to_her_mind_counts_as_recalled_not_what_is_read_behind_it() {
    let Some(db) = temp_db().await else {
        return;
    };
    let id = remember(&db, fact(7, "has a cat called Niangao"))
        .await
        .unwrap()
        .unwrap();
    let uses = |db: DatabaseConnection, id: String| async move {
        agent_memories::Entity::find_by_id(id)
            .one(&db)
            .await
            .unwrap()
            .unwrap()
            .access_count
    };
    let present = Audience::private(7);
    let ask = || recall(&db, 7, &present, Some("cat"), &MemoryKind::ABOUT_PERSON, 8);
    assert_eq!(ask().await.unwrap().len(), 1);
    assert_eq!(uses(db.clone(), id.clone()).await, 1);
    // Read behind the scenes (keeping facts, reflecting, deciding): found,
    // and not made any readier for it.
    assert_eq!(quietly(ask()).await.unwrap().len(), 1);
    assert_eq!(uses(db.clone(), id).await, 1);
}

#[tokio::test]
async fn a_group_keeps_its_freshest_in_view_and_the_talk_brings_back_an_older_one() {
    let Some(db) = temp_db().await else {
        return;
    };
    let venue = "group:onebot:42";
    for content in [
        "周五晚上一起打麻将，输的人唱歌",
        "群主的咖啡永远是冰美式",
        "复读机比赛",
        "早安接龙",
    ] {
        remember_in_venue(&db, venue, content, "{}", "bit")
            .await
            .unwrap();
    }
    remember_in_venue(&db, "group:onebot:7", "麻将三缺一", "{}", "bit")
        .await
        .unwrap();
    let kept = |rows: Vec<agent_memories::Model>| {
        rows.into_iter().map(|row| row.content).collect::<Vec<_>>()
    };
    assert_eq!(
        kept(fresh_and_brought(&db, venue, "bit", 2, Some("今晚谁来打麻将"), 2).await),
        ["早安接龙", "复读机比赛", "周五晚上一起打麻将，输的人唱歌"]
    );
    assert_eq!(
        kept(fresh_and_brought(&db, venue, "bit", 2, Some("今天天气不错"), 2).await),
        ["早安接龙", "复读机比赛"]
    );
    assert_eq!(
        kept(fresh_and_brought(&db, venue, "bit", 2, None, 2).await),
        ["早安接龙", "复读机比赛"]
    );
}

#[tokio::test]
async fn from_other_groups_comes_only_what_is_theirs_and_never_the_private() {
    let Some(db) = temp_db().await else {
        return;
    };
    let in_group = |venue: &str, content: &str| NewMemory {
        audience: Audience::group(venue, 7),
        ..fact(7, content)
    };
    remember(&db, in_group("onebot:1", "下个月要去大阪"))
        .await
        .unwrap();
    remember(&db, in_group("onebot:2", "在学吉他"))
        .await
        .unwrap();
    remember(&db, fact(7, "私下说的事")).await.unwrap();
    remember(
        &db,
        NewMemory {
            audience: Audience::group("onebot:1", 8),
            ..fact(8, "别人的事")
        },
    )
    .await
    .unwrap();
    let theirs = in_other_groups(&db, Some(7), "group:onebot:2", &["chat"], 10).await;
    let contents: Vec<&str> = theirs.iter().map(|row| row.content.as_str()).collect();
    assert_eq!(contents, ["下个月要去大阪"]);

    let marker = "\"who\":\"onebot:42\"";
    for venue in ["group:onebot:1", "group:onebot:2", "group:telegram:3"] {
        remember_in_venue(
            &db,
            venue,
            &format!("{venue} 的笔记"),
            &format!("{{{marker},\"name\":\"阿木\"}}"),
            "stranger",
        )
        .await
        .unwrap();
    }
    let noted = unowned_elsewhere(&db, "group:onebot:2", "stranger", marker, 10)
        .await
        .unwrap();
    let mut venues: Vec<&str> = noted.iter().map(|row| row.venue.as_str()).collect();
    venues.sort();
    assert_eq!(venues, ["group:onebot:1", "group:telegram:3"]);
}

#[tokio::test]
async fn a_memory_comes_to_mind_by_when_it_was_kept_for_and_is_said_as_it_was() {
    let Some(db) = temp_db().await else {
        return;
    };
    let wants = remember(&db, fact(7, "想找人一起练日语口语"))
        .await
        .unwrap()
        .unwrap();
    remember(&db, fact(7, "养了一只叫年糕的猫")).await.unwrap();
    assert!(
        keep_cues(&db, &wants, &["有人问有什么文化活动推荐时".into()])
            .await
            .unwrap()
    );
    assert!(!keep_cues(&db, &wants, &["再写一次".into()]).await.unwrap());
    let recalled = recall(
        &db,
        7,
        &Audience::private(7),
        Some("最近有什么文化活动推荐吗"),
        &MemoryKind::ABOUT_PERSON,
        1,
    )
    .await
    .unwrap();
    assert_eq!(recalled.len(), 1);
    assert_eq!(recalled[0].content, "想找人一起练日语口语");
    let missing: Vec<String> = without_cues(&db, 7, &["chat"], 10)
        .await
        .into_iter()
        .map(|row| row.content)
        .collect();
    assert_eq!(missing, ["养了一只叫年糕的猫"]);
}

#[tokio::test]
async fn what_was_done_comes_back_only_when_what_is_asked_names_it() {
    let Some(db) = temp_db().await else {
        return;
    };
    let done = |content: &str| NewMemory {
        user_id: 7,
        kind: MemoryKind::Done,
        content: content.into(),
        evidence: Some(r#"{"asked":"x"}"#.into()),
        speaker: Speaker::Agent,
        source: "work",
        audience: Audience::private(7),
        importance: 0.4,
        concepts: Vec::new(),
    };
    remember(
        &db,
        done("2026-10-01 Done for them: 把周报按日期整理成表格"),
    )
    .await
    .unwrap();
    remember(
        &db,
        done("2026-10-02 Done for them: 订了下周去大阪的新干线"),
    )
    .await
    .unwrap();
    remember(&db, fact(7, "喜欢靠窗的座位")).await.unwrap();
    let present = Audience::private(7);
    let named: Vec<String> = named_done(&db, 7, &present, "上次那份周报", 2)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.content)
        .collect();
    assert_eq!(named, ["2026-10-01 Done for them: 把周报按日期整理成表格"]);
    assert!(
        named_done(&db, 7, &present, "今天天气怎么样", 2)
            .await
            .unwrap()
            .is_empty(),
        "nothing done is brought up when nothing names it"
    );
    // Not someone else's, and not with what she knows of them.
    assert!(
        named_done(&db, 8, &Audience::private(8), "周报", 2)
            .await
            .unwrap()
            .is_empty()
    );
    let about = recall(&db, 7, &present, None, &MemoryKind::ABOUT_PERSON, 8)
        .await
        .unwrap();
    assert!(about.iter().all(|row| row.kind != "done"));
}

#[tokio::test]
async fn what_she_knows_most_about_them_stays_where_it_was_heard() {
    let Some(db) = temp_db().await else {
        return;
    };
    let about = |user_id: i32, content: &str, subject: &str, audience: &Audience| NewMemory {
        audience: audience.clone(),
        concepts: vec![Concept {
            name: subject.into(),
            aliases: Vec::new(),
        }],
        ..fact(user_id, content)
    };
    let private = Audience::private(7);
    let group = Audience::group("telegram:-100123", 7);
    for line in ["养了一只猫叫年糕", "年糕怕吸尘器", "年糕上周打了疫苗"] {
        remember(&db, about(7, line, "年糕", &private))
            .await
            .unwrap();
    }
    for line in ["在群里说在学吉他", "吉他换了新弦", "吉他想换琴"] {
        remember(&db, about(7, line, "吉他", &group)).await.unwrap();
    }
    // Someone else in the group, much talked of, is not who they are.
    let other = Audience::group("telegram:-100123", 8);
    for line in ["他在跑马拉松", "马拉松跑进四小时", "马拉松下月比赛"] {
        remember(&db, about(8, line, "马拉松", &other))
            .await
            .unwrap();
    }

    let mine: Vec<String> = always_known(&db, 7, &private)
        .await
        .unwrap()
        .into_iter()
        .map(|memory| memory.content)
        .collect();
    assert_eq!(mine.len(), 1);
    assert!(mine[0].contains("年糕"));
    let there: Vec<String> = always_known(&db, 7, &group)
        .await
        .unwrap()
        .into_iter()
        .map(|memory| memory.content)
        .collect();
    assert_eq!(
        there.len(),
        1,
        "the group's own, not the private nor the other member's"
    );
    assert!(there[0].contains("吉他"));
    assert!(always_known(&db, 0, &private).await.unwrap().is_empty());
}
