//! Opt-in PostgreSQL fixture for federation tests: one fresh schema per test.
//!
//! Set `FEDERATION_TEST_DATABASE_URL` (or the CI `MYRIAD_SCHEMA_DRIFT_DB`) to a
//! disposable database. Each fixture migrates its own `federation_test_<uuid>`
//! schema via `search_path` and drops it on [`SchemaDb::close`], so no rows are
//! left in the shared schema. Unset → tests skip.
use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseConnection};
use sea_orm_migration::MigratorTrait;

pub(crate) struct SchemaDb {
    pub db: DatabaseConnection,
    schema: String,
}

impl SchemaDb {
    pub async fn new() -> Option<Self> {
        let url = std::env::var("FEDERATION_TEST_DATABASE_URL")
            .or_else(|_| std::env::var("MYRIAD_SCHEMA_DRIFT_DB"))
            .ok()?;
        let schema = format!("federation_test_{}", uuid::Uuid::new_v4().simple());
        let admin = Database::connect(&url).await.expect("connect test db");
        admin
            .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
            .await
            .expect("create test schema");
        admin.close().await.expect("close admin connection");
        let mut options = ConnectOptions::new(url);
        options
            .set_schema_search_path(&schema)
            .max_connections(4)
            .sqlx_logging(false);
        let db = Database::connect(options).await.expect("connect test schema");
        migration::Migrator::up(&db, None)
            .await
            .expect("migrator up");
        Some(Self { db, schema })
    }

    pub async fn close(self) {
        self.db
            .execute_unprepared(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .await
            .expect("drop test schema");
        self.db.close().await.expect("close test db");
    }
}
