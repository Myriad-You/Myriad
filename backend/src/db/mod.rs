pub mod connection;
pub mod health;
pub mod schema_check;
pub mod worker_policy;

// Re-export the Migrator from migrations
pub use migration::Migrator;

/// Test-only: the test database's address, from the first of `vars` that is
/// set. None set, the test does not run, and says so where it is seen (straight
/// to stderr, past the test harness's capture), numbered: a run shows how many
/// were skipped rather than counting them as passed. With
/// `MYRIAD_TEST_DB=required` a missing address fails the test instead.
#[cfg(test)]
pub(crate) fn test_url(vars: &[&str]) -> Option<String> {
    if let Some(url) = vars.iter().find_map(|var| std::env::var(var).ok()) {
        return Some(url);
    }
    let wanted = vars.join(" / ");
    assert!(
        std::env::var("MYRIAD_TEST_DB").as_deref() != Ok("required"),
        "{wanted} is not set, and MYRIAD_TEST_DB=required"
    );
    static SKIPPED: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let number = SKIPPED.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
    let test = std::thread::current()
        .name()
        .unwrap_or("a test")
        .to_string();
    use std::io::Write;
    let _ = writeln!(
        std::io::stderr(),
        "db test skipped #{number}: {test} (no {wanted})"
    );
    None
}

/// Test-only: a fresh schema on a caller-supplied database, fully migrated.
/// Tests never touch the database's default schema, so pointing the env var at a
/// shared database cannot truncate or leak rows; `drop` removes the schema.
#[cfg(test)]
pub(crate) struct IsolatedSchema {
    pub db: sea_orm::DatabaseConnection,
    url: String,
    schema: String,
}

/// Making or dropping a whole schema takes a lock on every object in it, and
/// Postgres has room for only so many at once (`max_locks_per_transaction`
/// across its connections): tests run side by side make and drop their
/// schemas a few at a time, or they run out ("out of shared memory").
/// Every test fixture that makes a schema takes its turn here
/// (`federation::test_db` too).
#[cfg(test)]
pub(crate) static SCHEMA_WORK: std::sync::LazyLock<tokio::sync::Semaphore> =
    std::sync::LazyLock::new(|| tokio::sync::Semaphore::new(2));

#[cfg(test)]
impl IsolatedSchema {
    pub(crate) async fn migrated(url: &str, prefix: &str) -> Self {
        use sea_orm::ConnectionTrait;

        let _turn = SCHEMA_WORK.acquire().await.expect("never closed");

        let schema = format!("{prefix}_{}", uuid::Uuid::new_v4().simple());
        let admin = sea_orm::Database::connect(url).await.unwrap();
        admin
            .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
            .await
            .unwrap();
        admin.close().await.unwrap();
        let mut options = sea_orm::ConnectOptions::new(url.to_string());
        options
            .set_schema_search_path(&schema)
            .max_connections(4)
            .sqlx_logging(false);
        let db = sea_orm::Database::connect(options).await.unwrap();
        Migrator::up(&db, None).await.unwrap();
        Self {
            db,
            url: url.to_string(),
            schema,
        }
    }

    pub(crate) async fn drop(self) {
        use sea_orm::ConnectionTrait;
        let _turn = SCHEMA_WORK.acquire().await.expect("never closed");
        self.db.close().await.unwrap();
        let admin = sea_orm::Database::connect(&self.url).await.unwrap();
        admin
            .execute_unprepared(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .await
            .unwrap();
        admin.close().await.unwrap();
    }

    /// The schema's name, to copy out of it by name.
    pub(crate) fn name(&self) -> &str {
        &self.schema
    }

    /// Left in place for a later run to open again ([`Self::existing`]);
    /// the connection closes. Its name.
    pub(crate) async fn keep(self) -> String {
        self.db.close().await.unwrap();
        self.schema
    }

    /// A schema kept earlier, by name: to read, or to drop when done.
    pub(crate) async fn existing(url: &str, schema: &str) -> Self {
        let mut options = sea_orm::ConnectOptions::new(url.to_string());
        options
            .set_schema_search_path(schema)
            .max_connections(4)
            .sqlx_logging(false);
        Self {
            db: sea_orm::Database::connect(options).await.unwrap(),
            url: url.to_string(),
            schema: schema.to_string(),
        }
    }
}
