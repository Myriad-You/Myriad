//! Asking again from what a run kept of each question's writing, rather
//! than writing it again: what was kept, one more session written onto it,
//! and what to look for asked again.

use super::snapshot::tables;
use super::*;

/// `MEROPE_MEMORY_BENCH_KEEP_WRITTEN=1`: what each long question wrote of
/// its own sessions is kept as a schema of its own, before it is asked, and
/// named in its row (`keptWritten`): a later run that changes only how she
/// recalls or answers asks from these instead of writing again.
pub(in super::super) fn keep_written() -> bool {
    std::env::var("MEROPE_MEMORY_BENCH_KEEP_WRITTEN").is_ok_and(|value| value.trim() == "1")
}

/// What a question wrote, as kept: its schema, what of it answers the
/// question, and the calls that failed while writing it (a run asking from
/// it is void where the writing was).
pub(in super::super) struct KeptWritten {
    pub(in super::super) schema: String,
    pub(in super::super) written: Written,
    pub(in super::super) failed: Vec<String>,
    /// What the judge answered then when asked what to look for.
    pub(in super::super) asked: Option<String>,
}

pub(in super::super) fn kept_written_row(
    schema: &str,
    written: &Written,
    failed: &[String],
) -> Value {
    json!({"schema": schema, "answering": written.answering, "kept": written.kept,
        "failed": failed})
}

/// `MEROPE_MEMORY_BENCH_FROM_WRITTEN=<.partial.jsonl>`: each long question
/// asked from what a run with `MEROPE_MEMORY_BENCH_KEEP_WRITTEN=1` kept of
/// its writing, by question and round; nothing is written again.
pub(in super::super) fn from_written()
-> Option<std::collections::HashMap<(String, usize), KeptWritten>> {
    let path = std::env::var("MEROPE_MEMORY_BENCH_FROM_WRITTEN").ok()?;
    let text = std::fs::read_to_string(&path).expect("the run whose writing was kept");
    Some(read_kept_written(&text))
}

fn read_kept_written(text: &str) -> std::collections::HashMap<(String, usize), KeptWritten> {
    let strings = |value: &Value| -> Vec<String> {
        value
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|one| one.as_str().map(str::to_string))
            .collect()
    };
    text.lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter_map(|row| {
            let kept = &row["keptWritten"];
            Some((
                (
                    row["id"].as_str()?.to_string(),
                    row["round"].as_u64()? as usize,
                ),
                KeptWritten {
                    schema: kept["schema"].as_str()?.to_string(),
                    written: Written {
                        answering: strings(&kept["answering"]),
                        kept: kept["kept"].as_u64().unwrap_or(0) as usize,
                    },
                    failed: strings(&kept["failed"]),
                    asked: row["asked"].as_str().map(str::to_string),
                },
            ))
        })
        .collect()
}

/// `MEROPE_MEMORY_BENCH_THEN=<file>`, with `MEROPE_MEMORY_BENCH_FROM_WRITTEN`:
/// one more session for each question it names (question id to `{date,
/// venue, turns}`), written onto what was kept as its own sessions were,
/// before it is asked. With `MEROPE_MEMORY_BENCH_KEEP_WRITTEN=1` the copy
/// with it is kept apart from the one it went on from; a question it does
/// not name keeps pointing at its copy as it was.
pub(super) fn then_sessions() -> std::collections::HashMap<String, Then> {
    std::env::var("MEROPE_MEMORY_BENCH_THEN")
        .map(|path| read_then(&std::fs::read_to_string(&path).expect("what is written next")))
        .unwrap_or_default()
}

/// A session written after what a question kept.
#[derive(Deserialize, Clone)]
pub(super) struct Then {
    date: String,
    /// `private` when not given.
    #[serde(default)]
    venue: Option<String>,
    turns: Vec<Turn>,
}

fn read_then(text: &str) -> std::collections::HashMap<String, Then> {
    serde_json::from_str(text).expect("question ids, each to the session written next")
}

/// `question` with only `then` left to write: its date, place and turns,
/// and nothing she already keeps (that went in with the rest).
pub(super) fn then_question(question: &Question, then: &Then) -> Question {
    assert!(
        date_of(&then.date) < date_of(&question.question_date),
        "{} is asked before {}",
        question.question_id,
        then.date
    );
    Question {
        haystack_dates: vec![then.date.clone()],
        haystack_sessions: vec![then.turns.clone()],
        haystack_venues: vec![then.venue.clone().unwrap_or_else(|| "private".into())],
        memories: Vec::new(),
        ..question.clone()
    }
}

/// `MEROPE_MEMORY_BENCH_ASK_AGAIN=plain|topics`, with
/// `MEROPE_MEMORY_BENCH_FROM_WRITTEN`: what to look for is asked again
/// rather than taken as it was answered then. `plain` asks as production
/// does (the control: asking again moves the cues by itself); `topics` also
/// shows the names of every topic she keeps of them, a trial production
/// does not run.
pub(super) fn ask_again() -> Option<AskAgain> {
    match std::env::var("MEROPE_MEMORY_BENCH_ASK_AGAIN").ok()?.trim() {
        "plain" => Some(AskAgain::Plain),
        "topics" => Some(AskAgain::Topics),
        other => panic!("MEROPE_MEMORY_BENCH_ASK_AGAIN={other}: plain or topics"),
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(super) enum AskAgain {
    Plain,
    Topics,
}

/// The names of the topics she keeps of them in private, still kept (not
/// gone into another or dropped), in order.
pub(super) async fn topic_names(db: &sea_orm::DatabaseConnection, user_id: i32) -> Vec<String> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT name FROM agent_node_heads WHERE kind = 'topic' AND audience = $1 \
         AND status NOT IN ('absorbed', 'dropped', 'gone') ORDER BY name",
        [format!("personal:{user_id}").into()],
    ))
    .await
    .unwrap_or_default()
    .into_iter()
    .filter_map(|row| row.try_get::<String>("", "name").ok())
    .collect()
}

/// What to look for, asked with `topics` beside the message: production's
/// question and one more line on them.
pub(super) fn with_topics(question: &Question, topics: &[String]) -> (String, String) {
    let system = format!(
        "{} topics: the names of the topics your memories of them are kept under; a cue may be \
         one of these names as it is, when the answer may rest on what is under it.",
        myriad_merope::remembering::system()
    );
    let mut input: Value = serde_json::from_str(&myriad_merope::remembering::input(
        &question.question,
        None,
        &question.question_date,
    ))
    .expect("the question as asked");
    input["topics"] = json!(topics);
    (system, input.to_string())
}

/// What a question wrote, kept and copied back to ask from, is what it
/// wrote, row for row; and what one question wrote is not in another's.
#[tokio::test]
async fn what_a_question_wrote_comes_back_row_for_row_and_apart() {
    let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
        return;
    };
    let snapshot = crate::db::IsolatedSchema::migrated(&url, "bench_snapshot").await;
    let user_id = new_user(&snapshot.db, PERSON).await;
    let at = date_of("2026/10/01 (Thu) 21:00");
    remember(&snapshot.db, user_id, at, "每天骑电动车上班").await;
    let (a, b) = futures::join!(
        restore(&url, snapshot.name()),
        restore(&url, snapshot.name())
    );
    let later = date_of("2026/10/19 (Mon) 21:00");
    remember(&a.db, user_id, later, "电动车卖了，改坐地铁上班").await;
    remember(&b.db, user_id, later, "膝盖半月板磨损，三个月内不能跑步").await;
    let kept_a = restore(&url, a.name()).await.keep().await;
    let kept_b = restore(&url, b.name()).await.keep().await;
    // As a later run reads them back: by question and round.
    let rows = [
        json!({"id": "a", "round": 0, "asked": "{\"cues\":[]}", "keptWritten": kept_written_row(
            &kept_a, &Written { answering: vec!["电动车卖了".into()], kept: 1 }, &[])}),
        json!({"id": "b", "round": 0, "keptWritten": kept_written_row(
            &kept_b, &Written { answering: vec!["半月板".into()], kept: 1 },
            &["model: timeout".to_string()])}),
        json!({"id": "c", "round": 0}),
    ];
    let text: String = rows.iter().map(|row| format!("{row}\n")).collect();
    let read = read_kept_written(&text);
    assert_eq!(read.len(), 2, "a row with nothing kept is not read as kept");
    let (from_a, from_b) = (&read[&("a".to_string(), 0)], &read[&("b".to_string(), 0)]);
    assert_eq!(from_a.written.answering, ["电动车卖了"]);
    assert_eq!(from_a.written.kept, 1);
    assert!(from_a.failed.is_empty());
    // What to look for is asked again only where nothing was kept of it.
    assert_eq!(from_a.asked.as_deref(), Some("{\"cues\":[]}"));
    assert_eq!(from_b.asked, None);
    assert_eq!(
        from_b.failed,
        ["model: timeout"],
        "a failed writing stays void"
    );
    let (back_a, back_b) =
        futures::join!(restore(&url, &from_a.schema), restore(&url, &from_b.schema));
    let hash = |db: &sea_orm::DatabaseConnection, schema: String, table: String| {
        let db = db.clone();
        async move {
            db.query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                format!(
                    "SELECT md5(COALESCE(string_agg(x::text, '|' ORDER BY x::text), '')) AS h \
                     FROM \"{schema}\".\"{table}\" x"
                ),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<String>("", "h")
            .unwrap()
        }
    };
    for table in tables(&a.db, a.name()).await {
        for (written, back) in [(&a, &back_a), (&b, &back_b)] {
            assert_eq!(
                hash(&a.db, written.name().to_string(), table.clone()).await,
                hash(&a.db, back.name().to_string(), table.clone()).await,
                "{table}"
            );
        }
    }
    let holds = |db: &sea_orm::DatabaseConnection, fact: &'static str| {
        let db = db.clone();
        async move {
            db.query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT count(*) AS n FROM agent_memories WHERE content = $1",
                [fact.into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i64>("", "n")
            .unwrap()
        }
    };
    assert_eq!(holds(&back_a.db, "电动车卖了，改坐地铁上班").await, 1);
    assert_eq!(
        holds(&back_a.db, "膝盖半月板磨损，三个月内不能跑步").await,
        0
    );
    assert_eq!(
        holds(&back_b.db, "膝盖半月板磨损，三个月内不能跑步").await,
        1
    );
    assert_eq!(holds(&back_b.db, "电动车卖了，改坐地铁上班").await, 0);
    assert_eq!(holds(&back_a.db, "每天骑电动车上班").await, 1);
    // Asking from a copy leaves what was kept as it was.
    remember(&back_a.db, user_id, later, "问的时候又写了一条").await;
    let kept_again = crate::db::IsolatedSchema::existing(&url, &kept_a).await;
    assert_eq!(holds(&kept_again.db, "问的时候又写了一条").await, 0);
    for schema in [back_a, back_b, a, b, kept_again, snapshot] {
        schema.drop().await;
    }
    crate::db::IsolatedSchema::existing(&url, &kept_b)
        .await
        .drop()
        .await;
}

/// What is written next onto a kept question is that one session alone, at
/// its own date and place, and must come before the question is asked.
#[test]
fn what_comes_next_is_written_alone_and_before_asking() {
    let question: Question = serde_json::from_value(json!({
        "question_id": "zl-chg-commute-use",
        "question_type": "implicit-use",
        "question": "明早九点有个会 我几点出门比较稳",
        "answer": "按坐地铁四十分钟算",
        "question_date": "2026/10/27 (Tue) 22:10",
        "haystack_dates": ["2026/10/19 (Mon) 21:00"],
        "haystack_sessions": [[{"role": "user", "content": "今天开始坐地铁上班了", "has_answer": true}]],
        "haystack_venues": ["group:g1"],
        "memories": ["已经记着的一条"],
        "needs": [["地铁"]],
    }))
    .unwrap();
    let thens = read_then(
        &json!({
            "zl-chg-commute-use": {"date": "2026/10/23 (Fri) 21:00", "turns": [
                {"role": "user", "content": "今天上班路上碰到隔壁组的小周"},
                {"role": "assistant", "content": "这么巧"},
            ]},
            "zl-still-run": {"date": "2026/10/24 (Sat) 21:00", "venue": "group:g2", "turns": []},
        })
        .to_string(),
    );
    assert_eq!(thens.len(), 2);
    assert_eq!(thens["zl-still-run"].venue.as_deref(), Some("group:g2"));
    let next = then_question(&question, &thens["zl-chg-commute-use"]);
    assert_eq!(next.haystack_dates, ["2026/10/23 (Fri) 21:00"]);
    assert_eq!(next.haystack_venues, ["private"], "private when not given");
    let turns: Vec<(&str, &str, bool)> = next.haystack_sessions[0]
        .iter()
        .map(|turn| (turn.role.as_str(), turn.content.as_str(), turn.has_answer))
        .collect();
    assert_eq!(
        turns,
        [
            ("user", "今天上班路上碰到隔壁组的小周", false),
            ("assistant", "这么巧", false)
        ]
    );
    assert_eq!(
        next.haystack_sessions.len(),
        1,
        "its own sessions are not written again"
    );
    assert!(
        next.memories.is_empty(),
        "what she kept went in with the rest"
    );
    assert_eq!(
        (
            next.question_id.as_str(),
            next.question_date.as_str(),
            &next.needs
        ),
        (
            "zl-chg-commute-use",
            "2026/10/27 (Tue) 22:10",
            &question.needs
        )
    );
    let late = Then {
        date: "2026/10/28 (Wed) 21:00".into(),
        venue: None,
        turns: Vec::new(),
    };
    assert!(
        std::panic::catch_unwind(|| then_question(&question, &late)).is_err(),
        "nothing is written after the question is asked"
    );
}

/// Asked again with her topics, the question is production's own with the
/// names beside the message, every one and as they are.
#[test]
fn asked_with_topics_is_the_question_with_every_name_beside_it() {
    let question: Question = serde_json::from_value(json!({
        "question_id": "zl-chg-lens-use",
        "question_type": "implicit-use",
        "question": "周末想去游泳 要准备点啥",
        "answer": "",
        "question_date": "2026/10/26 (Mon) 22:20",
        "haystack_dates": [],
        "haystack_sessions": [],
    }))
    .unwrap();
    let topics = ["全飞秒近视手术".to_string(), "游泳".to_string()];
    let (system, input) = with_topics(&question, &topics);
    assert!(system.starts_with(&myriad_merope::remembering::system()));
    assert!(system.ends_with("when the answer may rest on what is under it."));
    let input: Value = serde_json::from_str(&input).unwrap();
    let plain: Value = serde_json::from_str(&myriad_merope::remembering::input(
        &question.question,
        None,
        &question.question_date,
    ))
    .unwrap();
    for key in ["message", "before", "today"] {
        assert_eq!(input[key], plain[key], "{key}");
    }
    assert_eq!(input["topics"], json!(["全飞秒近视手术", "游泳"]));
}
