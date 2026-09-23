pub mod connection;
pub mod health;
pub mod schema_check;
pub mod worker_policy;

// Re-export the Migrator from migrations
pub use migration::Migrator;

/// Test-only: a fresh schema on a caller-supplied database, fully migrated.
/// Tests never touch the database's default schema, so pointing the env var at a
/// shared database cannot truncate or leak rows; `drop` removes the schema.
#[cfg(test)]
pub(crate) struct IsolatedSchema {
    pub db: sea_orm::DatabaseConnection,
    url: String,
    schema: String,
}

#[cfg(test)]
impl IsolatedSchema {
    pub(crate) async fn migrated(url: &str, prefix: &str) -> Self {
        use sea_orm::ConnectionTrait;
        use sea_orm_migration::MigratorTrait;
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
        self.db.close().await.unwrap();
        let admin = sea_orm::Database::connect(&self.url).await.unwrap();
        admin
            .execute_unprepared(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .await
            .unwrap();
        admin.close().await.unwrap();
    }
}
