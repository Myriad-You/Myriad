//! HTTP handlers for module visibility preferences.
//!
//! Types, constants, and load live in `myriad_module_visibility` so Agent does
//! not import this HTTP module for flags or preference structs.
use axum::{http::StatusCode, Json};
use serde_json::{json, Value};

pub use myriad_module_visibility::{
    AgentUsagePreferences, ModuleVisibilityPreferences, MODULE_VISIBILITY_KEYS,
    MODULE_VISIBILITY_LEVELS, MODULE_VISIBILITY_PREFERENCES_KEY,
};

pub async fn get_module_visibility_preferences(
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    let preferences = myriad_module_visibility::load_module_visibility_preferences(&db).await;
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "preferences": preferences
        })),
    )
}

pub async fn update_module_visibility_preferences(
    crate::extract::Db(db): crate::extract::Db,
    Json(payload): Json<ModuleVisibilityPreferences>,
) -> (StatusCode, Json<Value>) {
    let preferences = payload.normalized();
    let config_service = crate::services::config_service::ConfigService::new(db);

    match config_service
        .update_config(
            MODULE_VISIBILITY_PREFERENCES_KEY,
            serde_json::to_value(&preferences).unwrap_or_else(|_| json!({})),
        )
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "preferences": preferences
            })),
        ),
        Err(e) => {
            tracing::error!("Failed to save module visibility preferences: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Failed to save module visibility preferences"
                })),
            )
        }
    }
}
