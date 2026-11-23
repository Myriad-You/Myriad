//! Kept payloads opened together ([`store::bodies`]): byte for byte what
//! opening them one at a time gave, in three queries however many there are.

use std::sync::atomic::{AtomicUsize, Ordering};

use sea_orm::{DbBackend, DbErr, ExecResult, QueryResult, Statement, TransactionTrait};

use super::*;
use crate::services::her_log::keys;

/// A connection that counts what is asked of it.
struct Counting<'a> {
    db: &'a DatabaseConnection,
    asked: AtomicUsize,
}

impl<'a> Counting<'a> {
    fn new(db: &'a DatabaseConnection) -> Self {
        Self {
            db,
            asked: AtomicUsize::new(0),
        }
    }

    fn one(&self) -> &'a DatabaseConnection {
        self.asked.fetch_add(1, Ordering::Relaxed);
        self.db
    }
}

#[async_trait::async_trait]
impl ConnectionTrait for Counting<'_> {
    fn get_database_backend(&self) -> DbBackend {
        self.db.get_database_backend()
    }

    async fn execute_raw(&self, stmt: Statement) -> Result<ExecResult, DbErr> {
        self.one().execute_raw(stmt).await
    }

    async fn execute_unprepared(&self, sql: &str) -> Result<ExecResult, DbErr> {
        self.one().execute_unprepared(sql).await
    }

    async fn query_one_raw(&self, stmt: Statement) -> Result<Option<QueryResult>, DbErr> {
        self.one().query_one_raw(stmt).await
    }

    async fn query_all_raw(&self, stmt: Statement) -> Result<Vec<QueryResult>, DbErr> {
        self.one().query_all_raw(stmt).await
    }
}

/// [`store::bodies`] as it was: each payload's keys looked up for itself.
async fn one_at_a_time(
    db: &DatabaseConnection,
    root: &TestRoot,
    rows: &[&Row],
) -> (BTreeMap<String, Value>, BTreeMap<String, keys::Opened>) {
    let mut bodies = BTreeMap::new();
    let mut raw = BTreeMap::new();
    for row in rows {
        let Some(sealed) = sealed_of(db, &row.id).await else {
            continue;
        };
        let opened =
            keys::open_payload(db, root, OWNER, PERSONAL, &row.object, &row.id, &sealed).await;
        let opened = match opened {
            Ok(Some(data)) => keys::Opened::Says(data),
            Ok(None) => keys::Opened::Gone,
            Err(super::super::Error::Key) => {
                tracing::warn!(row = %row.id, "[her-log] a kept body does not open; its row goes without it");
                keys::Opened::Not
            }
            Err(error) => panic!("{error}"),
        };
        if let keys::Opened::Says(data) = &opened
            && let Ok(body) = serde_json::from_slice::<Value>(data)
        {
            bodies.insert(row.id.clone(), body);
        }
        raw.insert(row.id.clone(), opened);
    }
    (bodies, raw)
}

async fn sealed_of(db: &DatabaseConnection, row: &str) -> Option<Vec<u8>> {
    db.query_one_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        "SELECT sealed FROM agent_log_payloads WHERE user_id = $1 AND row_id = $2",
        [OWNER.into(), row.into()],
    ))
    .await
    .unwrap()
    .map(|row| row.try_get("", "sealed").unwrap())
}

async fn run<const N: usize>(db: &DatabaseConnection, sql: &str, values: [sea_orm::Value; N]) {
    let done = db
        .execute_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            sql,
            values,
        ))
        .await
        .unwrap();
    assert_eq!(done.rows_affected(), 1, "{sql}");
}

/// Two rows of each of `objects`.
async fn written(db: &DatabaseConnection, objects: usize) -> Vec<Row> {
    let owner = owner(db).await;
    let txn = db.begin().await.unwrap();
    let mut rows = Vec::new();
    for n in 0..objects * 2 {
        let row = store::write_row_on(
            &txn,
            &root(),
            &owner,
            PERSONAL,
            None,
            &format!("speech:s{}", n / 2),
            "speech.said",
            &said(&format!("第 {n} 句，“引号” 和 \u{2028}"), n as i64),
            (Author::Her, Some(1)),
        )
        .await
        .unwrap();
        rows.push(row);
    }
    txn.commit().await.unwrap();
    rows
}

/// Both ways over `rows`: the same bodies and the same bytes, with the same
/// said of them in the log. How many came.
async fn same(db: &DatabaseConnection, rows: &[&Row]) -> usize {
    same_once(db, rows, async || ()).await
}

/// [`same`], `before` each way.
async fn same_once(db: &DatabaseConnection, rows: &[&Row], before: impl AsyncFn()) -> usize {
    const SAID: &str = "a kept body does not open";
    let without_time = |lines: Vec<String>| -> Vec<String> {
        lines
            .into_iter()
            .map(|line| line[line.find(SAID).unwrap()..].to_string())
            .collect()
    };
    before().await;
    let (old, raw, old_said) = {
        let logged = Logged::start();
        let (old, raw) = one_at_a_time(db, &root(), rows).await;
        (old, raw, without_time(logged.lines(SAID)))
    };
    before().await;
    let (new, new_said) = {
        let logged = Logged::start();
        let new = store::bodies(db, &root(), OWNER, PERSONAL, rows)
            .await
            .unwrap();
        (new, without_time(logged.lines(SAID)))
    };
    assert_eq!(
        serde_json::to_vec(&new).unwrap(),
        serde_json::to_vec(&old).unwrap()
    );
    assert_eq!(new_said, old_said);
    // And the opened bytes, those that do not read as JSON too.
    before().await;
    let mut sealed = Vec::new();
    for row in rows {
        if let Some(kept) = sealed_of(db, &row.id).await {
            sealed.push((*row, kept));
        }
    }
    let kept: Vec<keys::Kept> = sealed
        .iter()
        .map(|(row, sealed)| keys::Kept {
            object: &row.object,
            row_id: &row.id,
            sealed,
        })
        .collect();
    let opened = keys::open_payloads(db, &root(), OWNER, PERSONAL, &kept)
        .await
        .unwrap();
    assert_eq!(opened.len(), kept.len());
    for ((row, _), opened) in sealed.iter().zip(&opened) {
        assert!(opened == &raw[&row.id], "{}", row.object);
    }
    new.len()
}

#[tokio::test]
async fn payloads_opened_together_are_what_one_at_a_time_gave() {
    let Some(schema) = schema("her_log_bodies").await else {
        return;
    };
    let db = &schema.db;
    let rows = written(db, 12).await;
    let all: Vec<&Row> = rows.iter().collect();
    assert_eq!(same(db, &all).await, 24);
    assert_eq!(same(db, &[]).await, 0);

    // Forgotten: its object's key is gone.
    run(
        db,
        "DELETE FROM agent_log_object_keys WHERE user_id = $1 AND object = $2",
        [OWNER.into(), rows[0].object.clone().into()],
    )
    .await;
    // A payload that is not one, another row's, none, and one not JSON.
    let set_payload =
        "UPDATE agent_log_payloads SET sealed = $3 WHERE user_id = $1 AND row_id = $2";
    let set = async |row: &Row, sealed: Vec<u8>| {
        run(
            db,
            set_payload,
            [OWNER.into(), row.id.clone().into(), sealed.into()],
        )
        .await;
    };
    set(&rows[2], vec![0]).await;
    set(&rows[4], sealed_of(db, &rows[5].id).await.unwrap()).await;
    run(
        db,
        "DELETE FROM agent_log_payloads WHERE user_id = $1 AND row_id = $2",
        [OWNER.into(), rows[6].id.clone().into()],
    )
    .await;
    let prose = keys::seal_payload(
        db,
        &root(),
        OWNER,
        PERSONAL,
        &rows[8].object,
        None,
        &rows[8].id,
        b"not json",
    )
    .await
    .unwrap();
    set(&rows[8], prose).await;
    // An object's key that is not one, and another object's.
    let set_key = "UPDATE agent_log_object_keys SET sealed = $3 WHERE user_id = $1 AND object = $2";
    run(
        db,
        set_key,
        [
            OWNER.into(),
            rows[10].object.clone().into(),
            vec![0u8].into(),
        ],
    )
    .await;
    let other = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            "SELECT sealed FROM agent_log_object_keys WHERE user_id = $1 AND object = $2",
            [OWNER.into(), rows[14].object.clone().into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<Vec<u8>>("", "sealed")
        .unwrap();
    run(
        db,
        set_key,
        [OWNER.into(), rows[12].object.clone().into(), other.into()],
    )
    .await;
    // One asked for twice, and one not kept at all.
    let unknown = Row {
        id: "site_none".into(),
        ..rows[20].clone()
    };
    let mut asked = all.clone();
    asked.extend([&rows[22], &unknown]);
    // 24 less two forgotten, one broken, one moved, one gone, one not JSON,
    // and two under each of two keys that do not open.
    assert_eq!(same(db, &asked).await, 14);
    // All of them forgotten.
    assert_eq!(same(db, &all[..2]).await, 0);

    // The audience's key does not open, or is not there (made again, as it
    // was: nothing sealed before opens with it).
    run(
        db,
        "UPDATE agent_log_audience_keys SET sealed = $2 WHERE user_id = $1",
        [OWNER.into(), "not a key".into()],
    )
    .await;
    assert_eq!(same(db, &all).await, 0);
    assert_eq!(
        same_once(db, &all, async || {
            run(
                db,
                "DELETE FROM agent_log_audience_keys WHERE user_id = $1",
                [OWNER.into()],
            )
            .await;
        })
        .await,
        0
    );
    schema.drop().await;
}

#[tokio::test]
async fn payloads_are_opened_in_three_queries_however_many() {
    let Some(schema) = schema("her_log_bodies_queries").await else {
        return;
    };
    let db = &schema.db;
    let rows = written(db, 60).await;
    let all: Vec<&Row> = rows.iter().collect();
    for many in [1, 2, 120] {
        let counting = Counting::new(db);
        let bodies = store::bodies(&counting, &root(), OWNER, PERSONAL, &all[..many])
            .await
            .unwrap();
        assert_eq!(bodies.len(), many);
        assert_eq!(counting.asked.load(Ordering::Relaxed), 3, "{many} rows");
    }
    // No row, no query; no payload kept, no key read.
    let counting = Counting::new(db);
    store::bodies(&counting, &root(), OWNER, PERSONAL, &[])
        .await
        .unwrap();
    assert_eq!(counting.asked.load(Ordering::Relaxed), 0);
    db.execute_unprepared("DELETE FROM agent_log_payloads")
        .await
        .unwrap();
    store::bodies(&counting, &root(), OWNER, PERSONAL, &all)
        .await
        .unwrap();
    assert_eq!(counting.asked.load(Ordering::Relaxed), 1);
    schema.drop().await;
}
