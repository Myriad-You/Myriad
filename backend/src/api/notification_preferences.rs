//! 当前用户的通知策略 API。

use crate::error::HttpError;
use axum::{Extension, Json, http::StatusCode};
use serde_json::{Value, json};

use crate::middleware::auth::Claims;
use crate::services::agent::notification_preferences::{
    EVENT_DEFINITIONS, NotificationPreferences, SOURCE_KEYS,
};
use crate::services::agent::notifications::get_notification_manager;

fn user_id(claims: &Claims) -> Result<i32, HttpError> {
    crate::services::tapp_ownership::positive_user_id(&claims.sub).ok_or_else(|| {
        HttpError::from((
            StatusCode::UNAUTHORIZED,
            Json(AppError::public_json("Invalid authenticated user")),
        ))
    })
}

pub async fn get_notification_preferences(
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let user_id = user_id(&claims)?;
    let manager = get_notification_manager().ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Notification system not initialized",
                "code": "notification_unavailable",
            })),
        ))
    })?;
    let preferences = manager
        .notification_preferences(user_id)
        .await
        .map_err(|error| {
            tracing::error!("Failed to load notification preferences: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Notification action failed",
                    "code": "notification_failed",
                })),
            )
        })?;
    Ok(Json(json!({
        "success": true,
        "preferences": preferences,
        "catalog": {
            "sources": SOURCE_KEYS,
            "events": EVENT_DEFINITIONS.as_slice(),
        }
    })))
}

pub async fn update_notification_preferences(
    Extension(claims): Extension<Claims>,
    Json(payload): Json<NotificationPreferences>,
) -> Result<Json<Value>, HttpError> {
    let user_id = user_id(&claims)?;
    let manager = get_notification_manager().ok_or_else(|| {
        HttpError::from((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Notification system not initialized",
                "code": "notification_unavailable",
            })),
        ))
    })?;
    let preferences = manager
        .update_notification_preferences(user_id, payload)
        .await
        .map_err(|error| {
            tracing::error!("Failed to update notification preferences: {error}");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "error": "Notification action failed",
                    "code": "notification_failed",
                })),
            )
        })?;
    Ok(Json(json!({"success": true, "preferences": preferences})))
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::{ConnectionTrait, Database, DatabaseBackend, Statement};
    use sea_orm_migration::MigratorTrait;

    fn claims(user_id: i32) -> Claims {
        Claims {
            sub: user_id.to_string(),
            username: format!("user-{user_id}"),
            is_admin: false,
            is_owner: false,
            exp: i64::MAX,
            iat: 0,
            tv: 0,
        }
    }

    async fn insert_user(db: &sea_orm::DatabaseConnection, suffix: &str) -> i32 {
        let row = db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO users (username) VALUES ($1) RETURNING id",
                [format!(
                    "notification-api-{suffix}-{}",
                    uuid::Uuid::new_v4().simple()
                )
                .into()],
            ))
            .await
            .unwrap()
            .unwrap();
        row.try_get::<i32>("", "id").unwrap()
    }

    #[tokio::test]
    async fn api_round_trip_is_scoped_to_authenticated_user_when_database_is_provided() {
        let Ok(database_url) = std::env::var("NOTIFICATION_TEST_DATABASE_URL") else {
            return;
        };
        let db = Database::connect(&database_url).await.unwrap();
        migration::Migrator::up(&db, None).await.unwrap();
        let first_user = insert_user(&db, "first").await;
        let second_user = insert_user(&db, "second").await;
        crate::services::agent::notifications::init_notifications(db.clone()).await;

        let mut first_preferences = NotificationPreferences::default();
        first_preferences
            .sources
            .insert("phantasi".to_string(), false);
        let _updated =
            update_notification_preferences(Extension(claims(first_user)), Json(first_preferences))
                .await
                .unwrap();

        let first = get_notification_preferences(Extension(claims(first_user)))
            .await
            .unwrap()
            .0;
        let second = get_notification_preferences(Extension(claims(second_user)))
            .await
            .unwrap()
            .0;
        assert_eq!(first["preferences"]["sources"]["phantasi"], false);
        assert_eq!(second["preferences"]["sources"]["phantasi"], true);
        // The API must expose the whole catalog, in order; the catalog itself grows
        // whenever a producer gains a new event, so compare against it directly.
        assert_eq!(first["catalog"]["sources"], json!(SOURCE_KEYS));
        // Compare whole definitions, so a changed `source` cannot slip through.
        assert_eq!(first["catalog"]["events"], json!(EVENT_DEFINITIONS.as_slice()));
        let catalog_keys: Vec<&str> = EVENT_DEFINITIONS.iter().map(|event| event.key).collect();
        assert_eq!(
            catalog_keys
                .iter()
                .collect::<std::collections::BTreeSet<_>>()
                .len(),
            catalog_keys.len(),
            "event keys must be unique"
        );
        assert!(
            EVENT_DEFINITIONS
                .iter()
                .all(|event| SOURCE_KEYS.contains(&event.source)),
            "every event must belong to a declared source"
        );

        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM users WHERE id IN ($1, $2)",
            [first_user.into(), second_user.into()],
        ))
        .await
        .unwrap();
    }
}
use myriad_error::AppError;
