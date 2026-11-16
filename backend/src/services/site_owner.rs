//! Site owner resolution for public/dashboard surfaces.
//!
//! Prefer durable `users.is_owner`; fall back to lowest admin id. The answer
//! comes from [`crate::services::principal`]; this adapter keeps the `String`
//! errors its callers use. Profile re-exports it for reports/config.

use sea_orm::DatabaseConnection;

/// Query / decode errors are failures, never "no owner" and never a different identity.
pub async fn site_owner_user_id(db: &DatabaseConnection) -> Result<i32, String> {
    crate::services::principal::site_owner_id(db)
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to resolve site owner");
            "Failed to resolve site owner".to_string()
        })?
        .ok_or_else(|| "No administrator is configured as the site owner".to_string())
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    #[tokio::test]
    async fn owner_tier_wins_then_lowest_admin_then_unconfigured() {
        let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
            return;
        };
        let mut options = sea_orm::ConnectOptions::new(url);
        options.max_connections(1).min_connections(1);
        let db = sea_orm::Database::connect(options).await.unwrap();
        let exec = |sql: &str| {
            let db = db.clone();
            let sql = sql.to_string();
            async move {
                db.execute_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
                    .await
                    .unwrap();
                // Fixture writes skip admin_users, so invalidate like it does.
                crate::services::principal::invalidate_site_owner_cache();
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
        db.close().await.unwrap();
    }
}
