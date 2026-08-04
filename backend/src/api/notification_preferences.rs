//! 当前用户的通知策略 API。

use crate::error::HttpError;
use axum::{http::StatusCode, Extension, Json};
use serde_json::{json, Value};

use crate::middleware::auth::Claims;
use crate::services::agent::notification_preferences::{
    NotificationPreferences, EVENT_DEFINITIONS, SOURCE_KEYS,
};
use crate::services::agent::notifications::get_notification_manager;

fn user_id(claims: &Claims) -> Result<i32, HttpError> {
    claims.sub.parse::<i32>().map_err(|_| {
        HttpError::from((
            StatusCode::UNAUTHORIZED,
            Json(json!({"error": "Invalid authenticated user"})),
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
            Json(json!({"error": "Notification system not initialized"})),
        ))
    })?;
    let preferences = manager.notification_preferences(user_id).await;
    Ok(Json(json!({
        "success": true,
        "preferences": preferences,
        "catalog": {
            "sources": SOURCE_KEYS,
            "events": EVENT_DEFINITIONS,
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
            Json(json!({"error": "Notification system not initialized"})),
        ))
    })?;
    let preferences = manager
        .update_notification_preferences(user_id, payload)
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": error})),
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
            .query_one(Statement::from_sql_and_values(
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
        first_preferences.sources.insert("brew".to_string(), false);
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
        assert_eq!(first["preferences"]["sources"]["brew"], false);
        assert_eq!(second["preferences"]["sources"]["brew"], true);
        assert_eq!(first["catalog"]["sources"].as_array().unwrap().len(), 8);
        assert_eq!(first["catalog"]["events"].as_array().unwrap().len(), 28);

        db.execute(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM users WHERE id IN ($1, $2)",
            [first_user.into(), second_user.into()],
        ))
        .await
        .unwrap();
    }
}
