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
