//! Site owner resolution for public/dashboard surfaces.
//!
//! Prefer durable `users.is_owner`; fall back to lowest admin id.
//! Lives in services; profile re-exports it for reports/config.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

/// True when an administrator or durable owner already exists.
pub async fn installation_has_owner(db: &DatabaseConnection) -> Result<bool, String> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT EXISTS (
                SELECT 1 FROM users
                WHERE is_admin = true OR COALESCE(is_owner, false) = true
            ) AS claimed"
                .to_string(),
        ))
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to read installation claim");
            "Failed to read installation claim".to_string()
        })?
        .ok_or_else(|| "installation claim query returned no row".to_string())?;
    row.try_get::<bool>("", "claimed").map_err(|error| {
        tracing::error!(%error, "failed to decode installation claim state");
        "Failed to read installation claim".to_string()
    })
}

/// Durable owner first (lowest id), otherwise the lowest admin id, in one query.
/// `CASE` keeps a NULL `is_owner` out of the owner tier.
const SITE_OWNER_SQL: &str = "SELECT id FROM users \
     WHERE is_owner = true OR is_admin = true \
     ORDER BY CASE WHEN is_owner = true THEN 0 ELSE 1 END, id ASC \
     LIMIT 1";

/// Query / decode errors are failures, never "no owner" and never a different identity.
pub async fn site_owner_user_id(db: &DatabaseConnection) -> Result<i32, String> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            SITE_OWNER_SQL.to_string(),
        ))
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to resolve site owner");
            "Failed to resolve site owner".to_string()
        })?
        .ok_or_else(|| "No administrator is configured as the site owner".to_string())?;
    row.try_get::<i32>("", "id").map_err(|error| {
        tracing::error!(%error, "failed to decode site owner id");
        "Failed to resolve site owner".to_string()
    })
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    /// Runs in the default suite; the PostgreSQL test below covers execution.
    #[test]
    fn owner_query_prefers_owner_tier_then_lowest_id() {
        let sql = super::SITE_OWNER_SQL;
        assert!(sql.contains("WHERE is_owner = true OR is_admin = true"));
        assert!(sql.contains("ORDER BY CASE WHEN is_owner = true THEN 0 ELSE 1 END, id ASC"));
        assert!(sql.ends_with("LIMIT 1"));
    }

    #[tokio::test]
    #[ignore = "requires SITE_OWNER_TEST_DATABASE_URL pointing to a disposable PostgreSQL database"]
    async fn owner_tier_wins_then_lowest_admin_then_unconfigured() {
        let url = std::env::var("SITE_OWNER_TEST_DATABASE_URL").expect("disposable test DB");
        let mut options = sea_orm::ConnectOptions::new(url);
        options.max_connections(1);
        let db = sea_orm::Database::connect(options).await.unwrap();
        let exec = |sql: &str| {
            let db = db.clone();
            let sql = sql.to_string();
            async move {
                db.execute_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
                    .await
                    .unwrap();
            }
        };
        // A session-local temp table shadows any real `users` table.
        exec("CREATE TEMP TABLE users (id INT PRIMARY KEY, is_admin BOOLEAN, is_owner BOOLEAN)")
            .await;

        let error = super::site_owner_user_id(&db).await.unwrap_err();
        assert!(error.contains("No administrator"));

        exec("INSERT INTO users VALUES (1, false, NULL), (3, true, NULL), (5, true, false)").await;
        assert_eq!(super::site_owner_user_id(&db).await.unwrap(), 3);

        exec("INSERT INTO users VALUES (7, false, true), (9, true, true)").await;
        assert_eq!(super::site_owner_user_id(&db).await.unwrap(), 7);

        exec("DROP TABLE users").await;
        exec("CREATE TEMP TABLE users (id TEXT, is_admin BOOLEAN, is_owner BOOLEAN)").await;
        exec("INSERT INTO users VALUES ('x', true, true)").await;
        let error = super::site_owner_user_id(&db).await.unwrap_err();
        assert_eq!(error, "Failed to resolve site owner");
    }
}
