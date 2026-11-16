//! The background written once into a schema of its own (the snapshot),
//! and copies of it, row for row, to write each question onto.

use super::*;

/// Her nodes kept up after this many sessions, as a day's upkeep would.
const UPKEEP_EVERY: usize = 10;
/// Beside the snapshot's own tables: what each background session left
/// her, never copied into a question's copy.
const LEFT: &str = "bench_background_left";
/// Beside them too: the last background session written in full.
const DONE: &str = "bench_background_done";

/// What is written is kept apart to go on from after this many sessions
/// (`MEROPE_MEMORY_BENCH_KEEP_EVERY`, as often as her nodes are kept up
/// when not said).
fn keep_every() -> usize {
    std::env::var("MEROPE_MEMORY_BENCH_KEEP_EVERY")
        .ok()
        .and_then(|every| every.parse().ok())
        .filter(|every| *every > 0)
        .unwrap_or(UPKEEP_EVERY)
}

/// The background written into a schema of its own, the settle way; from
/// `resume` (a copy kept while writing) on, when given.
pub(super) async fn write_snapshot(
    url: &str,
    judge: &crate::services::analyzer::AiAnalyzer,
    background: &Background,
    resume: Option<&str>,
) -> crate::db::IsolatedSchema {
    let (snapshot, user_id, start) = match resume {
        Some(kept) => {
            let snapshot = copy_whole(url, kept).await;
            let (user_id, _) = read_snapshot(&snapshot.db).await;
            let done = done_upto(&snapshot.db).await;
            println!("background resumed from {kept} after session {done:?}");
            (snapshot, user_id, done.map_or(0, |done| done + 1))
        }
        None => {
            let snapshot = crate::db::IsolatedSchema::migrated(url, "memory_background").await;
            let user_id = new_user(&snapshot.db, PERSON).await;
            snapshot
                .db
                .execute_unprepared(&format!(
                    "CREATE TABLE {LEFT} (session INTEGER NOT NULL, fact TEXT NOT NULL); \
                     CREATE TABLE {DONE} (upto INTEGER NOT NULL)"
                ))
                .await
                .unwrap();
            (snapshot, user_id, 0)
        }
    };
    let db = &snapshot.db;
    let mut checkpoint: Option<String> = resume.map(str::to_string);
    // Short notes still fresh at the end stay, as on the day after.
    let end = &background.sessions.last().expect("a background").date;
    for (index, day) in background.sessions.iter().enumerate().skip(start) {
        if let Some(why) = crate::services::ai_cost_ledger::spent::stop_starting() {
            panic!(
                "background stopped before session {index}: {why}; \
                 MEROPE_MEMORY_BENCH_RESUME={}",
                checkpoint.as_deref().unwrap_or("(none yet)")
            );
        }
        let at = date_of(&day.date);
        let here = audience_at(day.venue(), user_id, PERSON);
        let turns = day.marked();
        keep_chat(db, &format!("{PERSON}-{index}"), user_id, at, &turns, &here).await;
        let (mut left, mut kept) = (Vec::new(), 0);
        settle::go_over(
            db,
            judge,
            user_id,
            &[(at, &turns, here)],
            end,
            &mut left,
            &mut kept,
            &mut super::super::note_trail::Trail::off(),
        )
        .await;
        for fact in &left {
            db.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                &format!("INSERT INTO {LEFT} (session, fact) VALUES ($1, $2)"),
                [(index as i32).into(), fact.clone().into()],
            ))
            .await
            .unwrap();
        }
        println!(
            "background session {index}: {} left, charged ${:.4}",
            left.len(),
            crate::services::ai_cost_ledger::spent::charged_so_far()
        );
        if (index + 1) % UPKEEP_EVERY == 0 {
            settle::keep_up_nodes(db, user_id).await;
        }
        if (index + 1) % keep_every() == 0 {
            mark_done(db, index).await;
            // What is written so far kept apart, to go on from; the copy
            // before it is no longer needed.
            let kept = copy_whole(url, snapshot.name()).await.keep().await;
            println!("background kept after session {index}: MEROPE_MEMORY_BENCH_RESUME={kept}");
            if let Some(before) = checkpoint.replace(kept) {
                crate::db::IsolatedSchema::existing(url, &before)
                    .await
                    .drop()
                    .await;
            }
        }
        // To try going on from a kept copy without paying for a whole run.
        if std::env::var("MEROPE_MEMORY_BENCH_STOP_AFTER")
            .ok()
            .and_then(|after| after.parse::<usize>().ok())
            == Some(index)
        {
            panic!(
                "background stopped after session {index} as asked; \
                 MEROPE_MEMORY_BENCH_RESUME={}",
                checkpoint.as_deref().unwrap_or("(none yet)")
            );
        }
    }
    settle::keep_up_nodes(db, user_id).await;
    mark_done(db, background.sessions.len() - 1).await;
    if let Some(before) = checkpoint {
        crate::db::IsolatedSchema::existing(url, &before)
            .await
            .drop()
            .await;
    }
    snapshot
}

/// The last background session written in full, as the snapshot says.
async fn done_upto(db: &sea_orm::DatabaseConnection) -> Option<usize> {
    db.query_one_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        format!("SELECT MAX(upto) AS upto FROM {DONE}"),
    ))
    .await
    .unwrap()
    .and_then(|row| row.try_get::<Option<i32>>("", "upto").unwrap())
    .map(|upto| upto as usize)
}

async fn mark_done(db: &sea_orm::DatabaseConnection, index: usize) {
    db.execute_unprepared(&format!("DELETE FROM {DONE}"))
        .await
        .unwrap();
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        &format!("INSERT INTO {DONE} (upto) VALUES ($1)"),
        [(index as i32).into()],
    ))
    .await
    .unwrap();
}

/// A copy of `snapshot` with what is kept beside it: to write on from.
async fn copy_whole(url: &str, snapshot: &str) -> crate::db::IsolatedSchema {
    let copy = restore(url, snapshot).await;
    let to = copy.name().to_string();
    for side in [LEFT, DONE] {
        copy.db
            .execute_unprepared(&format!(
                "CREATE TABLE \"{to}\".{side} AS SELECT * FROM \"{snapshot}\".{side}"
            ))
            .await
            .unwrap();
    }
    copy
}

/// What the snapshot holds of the person: their id, and what each
/// background session left her.
pub(super) async fn read_snapshot(
    db: &sea_orm::DatabaseConnection,
) -> (i32, BTreeMap<usize, Vec<String>>) {
    let user_id = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE username = $1",
            [PERSON.into()],
        ))
        .await
        .unwrap()
        .expect("the person")
        .try_get::<i32>("", "id")
        .unwrap();
    let mut left: BTreeMap<usize, Vec<String>> = BTreeMap::new();
    for row in db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            format!("SELECT session, fact FROM {LEFT} ORDER BY session"),
        ))
        .await
        .unwrap()
    {
        left.entry(row.try_get::<i32>("", "session").unwrap() as usize)
            .or_default()
            .push(row.try_get("", "fact").unwrap());
    }
    (user_id, left)
}

/// The snapshot's tables, as copies take them.
pub(super) async fn tables(db: &sea_orm::DatabaseConnection, schema: &str) -> Vec<String> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT table_name::text AS name FROM information_schema.tables \
         WHERE table_schema = $1 AND table_type = 'BASE TABLE' \
           AND table_name NOT IN ('seaql_migrations', $2, $3) ORDER BY 1",
        [schema.into(), LEFT.into(), DONE.into()],
    ))
    .await
    .unwrap()
    .iter()
    .map(|row| row.try_get("", "name").unwrap())
    .collect()
}

/// A fresh schema holding what the snapshot holds, row for row, its
/// sequences going on from there.
pub(in super::super) async fn restore(url: &str, snapshot: &str) -> crate::db::IsolatedSchema {
    use sea_orm::TransactionTrait;
    let copy = crate::db::IsolatedSchema::migrated(url, "memory_long").await;
    let db = &copy.db;
    let to = copy.name().to_string();
    let tables = tables(db, snapshot).await;
    let transaction = db.begin().await.unwrap();
    transaction
        .execute_unprepared("SET LOCAL session_replication_role = replica")
        .await
        .unwrap();
    // What the migrations seeded makes way for what the snapshot has; a
    // table newer than the snapshot that rests on one of its tables (each
    // message's vector on the message) goes with it, empty here as yet.
    let all: Vec<String> = tables
        .iter()
        .map(|table| format!("\"{to}\".\"{table}\""))
        .collect();
    transaction
        .execute_unprepared(&format!("TRUNCATE {} CASCADE", all.join(", ")))
        .await
        .unwrap();
    for table in &tables {
        let columns: Vec<String> = transaction
            .query_all_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT column_name::text AS name FROM information_schema.columns \
                 WHERE table_schema = $1 AND table_name = $2 AND is_generated = 'NEVER' \
                 ORDER BY ordinal_position",
                [snapshot.into(), table.clone().into()],
            ))
            .await
            .unwrap()
            .iter()
            .map(|row| format!("\"{}\"", row.try_get::<String>("", "name").unwrap()))
            .collect();
        let columns = columns.join(", ");
        transaction
            .execute_unprepared(&format!(
                "INSERT INTO \"{to}\".\"{table}\" ({columns}) OVERRIDING SYSTEM VALUE \
                 SELECT {columns} FROM \"{snapshot}\".\"{table}\""
            ))
            .await
            .unwrap();
    }
    transaction.commit().await.unwrap();
    let sequences = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT table_name::text AS t, column_name::text AS c, \
               pg_get_serial_sequence(format('%I.%I', table_schema, table_name), column_name) AS s \
             FROM information_schema.columns WHERE table_schema = $1 \
               AND pg_get_serial_sequence(format('%I.%I', table_schema, table_name), column_name) \
                 IS NOT NULL",
            [to.clone().into()],
        ))
        .await
        .unwrap();
    for row in &sequences {
        let (table, column, sequence): (String, String, String) = (
            row.try_get("", "t").unwrap(),
            row.try_get("", "c").unwrap(),
            row.try_get("", "s").unwrap(),
        );
        db.execute_unprepared(&format!(
            "SELECT setval('{sequence}', COALESCE((SELECT MAX(\"{column}\") FROM \
             \"{to}\".\"{table}\"), 0) + 1, false)"
        ))
        .await
        .unwrap();
    }
    copy
}

/// A copy of a snapshot is the snapshot, row for row, and copies are apart:
/// what is written in one is in no other, each goes on numbering from where
/// the snapshot stopped, and recalling in two at once, for the same person,
/// finds in each only what that one holds. On the test database
/// (`MYRIAD_MEDIA_TEST_DATABASE_URL`), skipped without it.
#[tokio::test]
async fn a_restored_snapshot_is_the_snapshot_and_copies_are_apart() {
    let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
        return;
    };
    let snapshot = crate::db::IsolatedSchema::migrated(&url, "bench_snapshot").await;
    let user_id = new_user(&snapshot.db, PERSON).await;
    let at = date_of("2026/10/01 (Thu) 21:00");
    let turns: Vec<Turn> = serde_json::from_value(json!([
        {"role": "user", "content": "我家孔雀鱼又生了一窝 二十多条"},
        {"role": "assistant", "content": "鱼缸要挤爆了"}
    ]))
    .unwrap();
    keep_chat(
        &snapshot.db,
        "background-0",
        user_id,
        at,
        &turns,
        &Audience::private(user_id),
    )
    .await;
    remember(
        &snapshot.db,
        user_id,
        at,
        "养了一缸孔雀鱼，又生了二十多条小鱼",
    )
    .await;
    snapshot
        .db
        .execute_unprepared(&format!(
            "CREATE TABLE {LEFT} (session INTEGER NOT NULL, fact TEXT NOT NULL)"
        ))
        .await
        .unwrap();
    let (a, b) = futures::join!(
        restore(&url, snapshot.name()),
        restore(&url, snapshot.name())
    );
    // Row for row; what is beside the snapshot is not copied.
    for table in tables(&snapshot.db, snapshot.name()).await {
        let rows = |schema: String| {
            let (db, table) = (&a.db, table.clone());
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
        assert_eq!(
            rows(snapshot.name().to_string()).await,
            rows(a.name().to_string()).await,
            "{table}"
        );
    }
    assert!(
        tables(&a.db, a.name())
            .await
            .iter()
            .all(|table| table != LEFT)
    );
    let copied =
        a.db.query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            format!(
                "SELECT COUNT(*) AS n FROM information_schema.tables \
                 WHERE table_schema = '{}' AND table_name = '{LEFT}'",
                a.name()
            ),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap();
    assert_eq!(copied, 0);
    // Written in one, in that one only; both number on from the snapshot.
    remember(&a.db, user_id, at, "家里那只乌龟叫阿慢，十岁了").await;
    let (in_a, in_b) = (new_user(&a.db, "next").await, new_user(&b.db, "next").await);
    assert_eq!(in_a, in_b);
    assert!(in_a > user_id);
    let question: Question = serde_json::from_value(json!({
        "question_id": "apart", "question_type": "single-session-user",
        "question": "我家乌龟阿慢多大了", "answer": "十岁",
        "question_date": "2026/10/02 (Fri) 21:00",
        "haystack_dates": [], "haystack_sessions": []
    }))
    .unwrap();
    let present = Audience::private(user_id);
    let ((from_a, _), (from_b, _)) = futures::join!(
        recalled_for(&a.db, &question, user_id, &present, None, THOROUGH),
        recalled_for(&b.db, &question, user_id, &present, None, THOROUGH)
    );
    assert!(
        from_a.iter().any(|line| line.contains("阿慢")),
        "{from_a:?}"
    );
    assert!(
        !from_b.iter().any(|line| line.contains("阿慢")),
        "{from_b:?}"
    );
    for schema in [a, b, snapshot] {
        schema.drop().await;
    }
}
