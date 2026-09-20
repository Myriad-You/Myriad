//! Run with AI_QUOTA_TEST_DATABASE_URL pointing to a disposable PostgreSQL database:
//! cargo test -p myriad-backend services::ai_quota::database_tests -- --ignored
//! Each test creates and removes its own schema; no application tables are used.
use super::*;
use sea_orm::{ConnectOptions, Database};
use std::sync::{Arc, Mutex};

struct TestDb {
    db: DatabaseConnection,
    schema: String,
    statements: Arc<Mutex<Vec<String>>>,
}

impl TestDb {
    async fn new() -> Self {
        let url = std::env::var("AI_QUOTA_TEST_DATABASE_URL")
            .expect("set AI_QUOTA_TEST_DATABASE_URL to a disposable PostgreSQL database");
        let schema = format!("quota_test_{}", uuid::Uuid::new_v4().simple());
        let admin = Database::connect(url.clone()).await.unwrap();
        admin
            .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
            .await
            .unwrap();
        admin.close().await.unwrap();
        let mut options = ConnectOptions::new(url);
        options
            .set_schema_search_path(&schema)
            .max_connections(12)
            .sqlx_logging(false);
        let mut db = Database::connect(options).await.unwrap();
        db.execute_unprepared(
            r#"
            CREATE TABLE tapp_quota_usage (
                user_id integer NOT NULL, tapp_id text NOT NULL, quota_type text NOT NULL,
                used integer NOT NULL, "limit" integer NOT NULL,
                period_start timestamptz NOT NULL, period_end timestamptz NOT NULL,
                updated_at timestamptz NOT NULL,
                UNIQUE(user_id, tapp_id, quota_type, period_start)
            )
        "#,
        )
        .await
        .unwrap();
        let statements = Arc::new(Mutex::new(Vec::new()));
        let captured = statements.clone();
        db.set_metric_callback(move |info| {
            captured.lock().unwrap().push(info.statement.sql.clone())
        });
        Self {
            db,
            schema,
            statements,
        }
    }

    fn reset_count(&self) {
        self.statements.lock().unwrap().clear();
    }
    fn assert_count(&self, expected: usize) {
        let statements = self.statements.lock().unwrap();
        assert_eq!(statements.len(), expected, "{statements:#?}");
    }
    async fn exec(&self, sql: &str) {
        self.db.execute_unprepared(sql).await.unwrap();
    }
    async fn used(&self) -> Vec<i32> {
        self.db.query_all_raw(Statement::from_string(DbBackend::Postgres,
            "SELECT used FROM tapp_quota_usage ORDER BY user_id, tapp_id, quota_type, period_start"))
            .await.unwrap().iter().map(|row| row.try_get("", "used").unwrap()).collect()
    }
    async fn finish(self) {
        self.exec(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .await;
        self.db.close().await.unwrap();
    }
}

fn buckets(subject: i32, ip: &str, calls: i32, tokens: i32) -> Vec<AiQuotaBucketLimits> {
    guest_quota_buckets(
        subject,
        1,
        "test.app",
        AiQuotaLimits {
            calls,
            tokens,
            cooldown_seconds: 0,
            unlimited: false,
        },
        Some(ip),
    )
}

async fn reserve(
    db: &DatabaseConnection,
    buckets: Vec<AiQuotaBucketLimits>,
    tokens: i32,
) -> Result<AiQuotaReservation, AiQuotaError> {
    reserve_buckets(db, buckets, tokens, 0, AiQuotaReserveOptions::default()).await
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL via AI_QUOTA_TEST_DATABASE_URL"]
async fn batch_counts_and_lifecycle() {
    let test = TestDb::new().await;
    // Exercise the public wrapper for the unlimited path as well.
    test.reset_count();
    let admin = reserve_ai_quota(&test.db, UserRole::Admin, 1, 1, "app", 50, None)
        .await
        .unwrap();
    settle_ai_quota(&test.db, &admin, 50).await.unwrap();
    release_ai_token_reservation(&test.db, &admin)
        .await
        .unwrap();
    rollback_ai_quota_reservation(&test.db, &admin)
        .await
        .unwrap();
    test.assert_count(0);
    for count in [1, 3] {
        test.exec("TRUNCATE tapp_quota_usage").await;
        let mut input = buckets(-1, "ip", 10, 1000);
        input.truncate(count);
        test.reset_count();
        let reservation = reserve(&test.db, input, 100).await.unwrap();
        test.assert_count(3);
        assert_eq!(test.used().await, [1, 100].repeat(count));
        test.reset_count();
        settle_ai_quota(&test.db, &reservation, 70).await.unwrap();
        test.assert_count(1);
        assert_eq!(test.used().await, [1, 70].repeat(count));
        // Release a fresh reservation, leaving its call charged.
        let mut input = buckets(-1, "ip", 10, 1000);
        input.truncate(count);
        let second = reserve(&test.db, input, 100).await.unwrap();
        test.reset_count();
        release_ai_token_reservation(&test.db, &second)
            .await
            .unwrap();
        test.assert_count(1);
        assert_eq!(test.used().await, [2, 70].repeat(count));
        let mut input = buckets(-1, "ip", 10, 1000);
        input.truncate(count);
        let third = reserve(&test.db, input, 100).await.unwrap();
        test.reset_count();
        rollback_ai_quota_reservation(&test.db, &third)
            .await
            .unwrap();
        test.assert_count(1);
        assert_eq!(test.used().await, [2, 70].repeat(count));
    }
    test.exec("TRUNCATE tapp_quota_usage").await;
    let zero = reserve(&test.db, buckets(-1, "ip", 10, 1000), 0)
        .await
        .unwrap();
    test.reset_count();
    release_ai_token_reservation(&test.db, &zero).await.unwrap();
    test.assert_count(0);
    rollback_ai_quota_reservation(&test.db, &zero)
        .await
        .unwrap();
    test.assert_count(1);
    assert_eq!(test.used().await, vec![0; 6]);
    test.finish().await;
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL via AI_QUOTA_TEST_DATABASE_URL"]
async fn rejections_and_sql_failures_are_atomic() {
    let test = TestDb::new().await;
    let first = reserve(&test.db, buckets(-1, "ip", 10, 1000), 100)
        .await
        .unwrap();
    let before = test.used().await;
    let error = reserve_buckets(
        &test.db,
        buckets(-1, "ip", 10, 1000),
        100,
        3600,
        AiQuotaReserveOptions::default(),
    )
    .await
    .unwrap_err();
    assert!(matches!(error, AiQuotaError::Cooldown { .. }));
    let error = reserve(&test.db, buckets(-1, "ip", 1, 1000), 100)
        .await
        .unwrap_err();
    assert_eq!(error, AiQuotaError::DailyCallLimit { anonymous: false });
    let error = reserve(&test.db, buckets(-1, "ip", 10, 100), 1)
        .await
        .unwrap_err();
    assert_eq!(error, AiQuotaError::DailyTokenLimit { anonymous: false });
    // Last business bucket rejects; all earlier limit changes must roll back too.
    let mut input = buckets(-1, "ip", 20, 2000);
    input[2].calls = 1;
    assert_eq!(
        reserve(&test.db, input, 10).await.unwrap_err(),
        AiQuotaError::DailyCallLimit { anonymous: true }
    );
    assert_eq!(test.used().await, before);
    let limit: i32 = test.db.query_one_raw(Statement::from_string(DbBackend::Postgres,
        "SELECT \"limit\" FROM tapp_quota_usage WHERE user_id = -1 AND quota_type LIKE 'ai_calls%'"))
        .await.unwrap().unwrap().try_get("", "limit").unwrap();
    assert_eq!(limit, 10);
    // Force the last statement to fail after ensure has changed the limits.
    test.exec("ALTER TABLE tapp_quota_usage ADD CONSTRAINT injected_failure CHECK (used <= 100)")
        .await;
    assert!(
        reserve(&test.db, buckets(-1, "ip", 20, 2000), 100)
            .await
            .is_err()
    );
    assert_eq!(test.used().await, before);
    test.exec("ALTER TABLE tapp_quota_usage DROP CONSTRAINT injected_failure")
        .await;
    // A failed ensure must not leave earlier rows or refreshed limits behind.
    test.exec("CREATE FUNCTION reject_ensure() RETURNS trigger LANGUAGE plpgsql AS $$ BEGIN IF NEW.user_id = 0 THEN RAISE EXCEPTION 'injected ensure failure'; END IF; RETURN NEW; END $$").await;
    test.exec("CREATE TRIGGER reject_ensure BEFORE INSERT ON tapp_quota_usage FOR EACH ROW EXECUTE FUNCTION reject_ensure()").await;
    assert!(
        reserve(&test.db, buckets(-2, "new-ip", 20, 2000), 10)
            .await
            .is_err()
    );
    assert_eq!(test.used().await, before);
    test.exec("DROP TRIGGER reject_ensure ON tapp_quota_usage")
        .await;
    // Corrupt a lock-read scalar: strict decoding must reject and undo ensure.
    test.exec("ALTER TABLE tapp_quota_usage ALTER COLUMN updated_at DROP NOT NULL")
        .await;
    test.exec("UPDATE tapp_quota_usage SET updated_at = NULL WHERE user_id = -1")
        .await;
    assert!(
        reserve(&test.db, buckets(-1, "ip", 20, 2000), 10)
            .await
            .is_err()
    );
    assert_eq!(test.used().await, before);
    test.exec("UPDATE tapp_quota_usage SET updated_at = NOW() WHERE updated_at IS NULL")
        .await;
    let limit: i32 = test.db.query_one_raw(Statement::from_string(DbBackend::Postgres,
        "SELECT \"limit\" FROM tapp_quota_usage WHERE user_id = -1 AND quota_type LIKE 'ai_calls%'"))
        .await.unwrap().unwrap().try_get("", "limit").unwrap();
    assert_eq!(limit, 10);
    // Continuations bypass cooldown but still charge and advance only the subject clock.
    test.exec("UPDATE tapp_quota_usage SET updated_at = NOW() - interval '10 seconds'")
        .await;
    let continuation = reserve_buckets(
        &test.db,
        buckets(-1, "ip", 10, 1000),
        10,
        3600,
        AiQuotaReserveOptions {
            skip_cooldown: true,
        },
    )
    .await
    .unwrap();
    let touched: i64 = test.db.query_one_raw(Statement::from_string(DbBackend::Postgres,
        "SELECT count(*) AS count FROM tapp_quota_usage WHERE updated_at > NOW() - interval '5 seconds'"))
        .await.unwrap().unwrap().try_get("", "count").unwrap();
    assert_eq!(touched, 1);
    rollback_ai_quota_reservation(&test.db, &continuation)
        .await
        .unwrap();
    // Missing one target must undo updates to the other targets on every finalizer.
    test.exec("DELETE FROM tapp_quota_usage WHERE tapp_id = '__anonymous_ai_site__' AND quota_type LIKE 'ai_tokens%'").await;
    let before = test.used().await;
    assert!(settle_ai_quota(&test.db, &first, 50).await.is_err());
    assert_eq!(test.used().await, before);
    assert!(
        release_ai_token_reservation(&test.db, &first)
            .await
            .is_err()
    );
    assert_eq!(test.used().await, before);
    assert!(
        rollback_ai_quota_reservation(&test.db, &first)
            .await
            .is_err()
    );
    assert_eq!(test.used().await, before);
    test.finish().await;
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL via AI_QUOTA_TEST_DATABASE_URL"]
async fn finalizers_keep_the_reserved_period_and_reject_underflow() {
    let test = TestDb::new().await;
    let mut old = reserve(&test.db, buckets(-1, "ip", 10, 1000), 100)
        .await
        .unwrap();
    test.exec("UPDATE tapp_quota_usage SET period_start = period_start - interval '1 day', period_end = period_end - interval '1 day'").await;
    for bucket in &mut old.buckets {
        bucket.period_start -= chrono::Duration::days(1);
    }
    let today = reserve(&test.db, buckets(-1, "ip", 10, 1000), 100)
        .await
        .unwrap();
    settle_ai_quota(&test.db, &old, 70).await.unwrap();
    assert_eq!(test.used().await, [1, 1, 70, 100].repeat(3));
    release_ai_token_reservation(&test.db, &today)
        .await
        .unwrap();
    assert_eq!(test.used().await, [1, 1, 70, 0].repeat(3));
    assert!(
        release_ai_token_reservation(&test.db, &today)
            .await
            .is_err()
    );
    assert_eq!(test.used().await, [1, 1, 70, 0].repeat(3));
    // The old reservation still addresses yesterday on rollback.
    old.reserved_tokens = 70;
    rollback_ai_quota_reservation(&test.db, &old).await.unwrap();
    assert_eq!(test.used().await, [0, 1, 0, 0].repeat(3));
    test.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires disposable PostgreSQL via AI_QUOTA_TEST_DATABASE_URL"]
async fn concurrent_shared_buckets_never_overspend_or_deadlock() {
    let test = TestDb::new().await;
    // Contend on the same subject, same IP across subjects, and site across IPs.
    for (scope, token_limited) in (0..3).flat_map(|scope| [(scope, false), (scope, true)]) {
        test.exec("TRUNCATE tapp_quota_usage").await;
        let mut tasks = tokio::task::JoinSet::new();
        for i in 0..12 {
            let db = test.db.clone();
            tasks.spawn(async move {
                let subject = if scope == 0 { -1 } else { -i - 1 };
                let ip = if scope < 2 {
                    "shared".to_owned()
                } else {
                    format!("ip-{i}")
                };
                let mut input = buckets(subject, &ip, 100, 10000);
                if token_limited {
                    input[scope].tokens = 40;
                } else {
                    input[scope].calls = 4;
                }
                reserve(&db, input, 10).await
            });
        }
        let mut successes = Vec::new();
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            while let Some(result) = tasks.join_next().await {
                match result.unwrap() {
                    Ok(reservation) => successes.push(reservation),
                    Err(error) => assert_eq!(
                        error,
                        if token_limited {
                            AiQuotaError::DailyTokenLimit {
                                anonymous: scope != 0,
                            }
                        } else {
                            AiQuotaError::DailyCallLimit {
                                anonymous: scope != 0,
                            }
                        }
                    ),
                }
            }
        })
        .await
        .expect("quota operations deadlocked");
        assert_eq!(successes.len(), 4);
        // Finalizers share rows with new reservations and each other.
        let mut tasks = tokio::task::JoinSet::new();
        for (i, reservation) in successes.into_iter().enumerate() {
            let db = test.db.clone();
            tasks.spawn(async move {
                match i % 3 {
                    0 => settle_ai_quota(&db, &reservation, 5).await,
                    1 => release_ai_token_reservation(&db, &reservation).await,
                    _ => rollback_ai_quota_reservation(&db, &reservation).await,
                }
            });
        }
        let db = test.db.clone();
        tasks.spawn(async move {
            let r = reserve(&db, buckets(-100, "new", 100, 10000), 10).await?;
            rollback_ai_quota_reservation(&db, &r).await
        });
        tokio::time::timeout(std::time::Duration::from_secs(20), async {
            while let Some(result) = tasks.join_next().await {
                result.unwrap().unwrap();
            }
        })
        .await
        .expect("mixed quota operations deadlocked");
        let rows = test.db.query_all_raw(Statement::from_string(DbBackend::Postgres,
            "SELECT used FROM tapp_quota_usage WHERE tapp_id = '__anonymous_ai_site__' ORDER BY quota_type"))
            .await.unwrap();
        assert_eq!(rows[0].try_get::<i32>("", "used").unwrap(), 3);
        assert_eq!(rows[1].try_get::<i32>("", "used").unwrap(), 10);
    }
    test.finish().await;
}
