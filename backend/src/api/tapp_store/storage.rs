//! Manifest-declared host settings and subject-private Tapp storage.

use super::{
    authorize_runtime_storage, can_write_installation_settings, current_is_admin,
    optional_authenticated_user_id, tapp_setting_value_is_valid, validate_tapp_id, ApiResponse,
    TappSettingDef, TappStorageAccess,
};
use crate::api::tapp_runtime::{common as tapp_common, RuntimeGrantContext};
use crate::{middleware::auth::Claims, models::entities::tapp_storage};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::{
    ColumnTrait, ConnectionTrait, DatabaseBackend, DatabaseConnection, EntityTrait,
    FromQueryResult, QueryFilter, Statement, TransactionTrait,
};
use serde::Serialize;
use std::collections::{BTreeMap, HashSet};

pub(crate) fn validate_storage_key(key: &str) -> Result<(), &'static str> {
    if key.is_empty() {
        return Err("Key cannot be empty");
    }
    if key.len() > 256 {
        return Err("Key too long (max 256 characters)");
    }
    if key.starts_with('.') || key.ends_with('.') {
        return Err("Key cannot start or end with a dot");
    }
    if key.contains("..") {
        return Err("Key cannot contain consecutive dots");
    }
    if !key
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | ':'))
    {
        return Err("Key contains invalid characters (only alphanumeric, underscore, hyphen, dot, colon allowed)");
    }
    Ok(())
}

const HOST_STORAGE_KEY_PREFIXES: [&str; 4] =
    ["_settings.", "_component:", "_shortcut:", "_report:"];

fn is_host_storage_key(key: &str) -> bool {
    key == "_settings"
        || HOST_STORAGE_KEY_PREFIXES
            .iter()
            .any(|prefix| key.starts_with(prefix))
}

pub(crate) fn validate_sandbox_storage_key(key: &str) -> Result<(), &'static str> {
    validate_storage_key(key)?;
    if is_host_storage_key(key) {
        return Err("Key prefix is reserved for host-managed Tapp data");
    }
    Ok(())
}

pub(crate) fn validate_storage_value_size(value: &serde_json::Value) -> Result<(), StatusCode> {
    let size = serde_json::to_vec(value)
        .map_err(|_| StatusCode::BAD_REQUEST)?
        .len();
    if size > 1024 * 1024 {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    Ok(())
}

const TAPP_STORAGE_QUOTA_BYTES: i64 = 5 * 1024 * 1024;

#[derive(FromQueryResult)]
struct StorageBytesRow {
    bytes: i64,
}

async fn storage_bytes(
    db: &impl ConnectionTrait,
    user_id: i32,
    tapp_id: &str,
) -> Result<i64, StatusCode> {
    StorageBytesRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"SELECT COALESCE(SUM(octet_length(key) + octet_length(value::text)), 0)::BIGINT AS bytes
           FROM tapp_storage WHERE user_id = $1 AND tapp_id = $2"#,
        vec![user_id.into(), tapp_id.into()],
    ))
    .one(db)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
    .map(|row| row.map_or(0, |row| row.bytes))
}

async fn authorize_tapp_settings(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
) -> Result<(TappStorageAccess, Vec<TappSettingDef>), StatusCode> {
    validate_tapp_id(tapp_id).map_err(|_| StatusCode::BAD_REQUEST)?;
    let subject_id =
        optional_authenticated_user_id(Some(claims)).ok_or(StatusCode::UNAUTHORIZED)?;
    let tapp = tapp_common::resolve_accessible_tapp(db, subject_id, tapp_id)
        .await
        .map_err(|(status, _)| status)?;
    let settings = tapp
        .manifest
        .get("settings")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default()
        .into_iter()
        .map(|setting| {
            serde_json::from_value(setting).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
        })
        .collect::<Result<Vec<TappSettingDef>, StatusCode>>()?;
    Ok((
        TappStorageAccess::from_owner_and_subject(tapp.user_id, subject_id),
        settings,
    ))
}

async fn authorize_tapp_setting(
    db: &DatabaseConnection,
    claims: &Claims,
    tapp_id: &str,
    key: &str,
) -> Result<(TappStorageAccess, String, TappSettingDef), StatusCode> {
    validate_storage_key(key).map_err(|_| StatusCode::BAD_REQUEST)?;
    let (access, settings) = authorize_tapp_settings(db, claims, tapp_id).await?;
    let setting = settings
        .into_iter()
        .find(|setting| setting.key == key)
        .ok_or(StatusCode::NOT_FOUND)?;
    Ok((access, format!("_settings.{key}"), setting))
}

pub(crate) async fn read_storage_value(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    key: &str,
) -> Result<serde_json::Value, StatusCode> {
    let item = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(tapp_id))
        .filter(tapp_storage::Column::Key.eq(key))
        .one(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(item.map_or(serde_json::Value::Null, |item| item.value))
}

pub(crate) async fn write_storage_value(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    key: &str,
    value: serde_json::Value,
) -> Result<(), StatusCode> {
    let txn = db
        .begin()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        vec![format!("tapp-storage:{user_id}:{tapp_id}").into()],
    ))
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    #[derive(FromQueryResult)]
    struct ProjectedBytesRow {
        bytes: i64,
    }
    let projected = ProjectedBytesRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
SELECT (
    COALESCE(SUM(octet_length(key) + octet_length(value::text))
        FILTER (WHERE key <> $3), 0)
    + octet_length($3)
    + octet_length($4::jsonb::text)
)::BIGINT AS bytes
FROM tapp_storage
WHERE user_id = $1 AND tapp_id = $2
"#,
        vec![
            user_id.into(),
            tapp_id.into(),
            key.into(),
            value.clone().into(),
        ],
    ))
    .one(&txn)
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_or(i64::MAX, |row| row.bytes);
    if projected > TAPP_STORAGE_QUOTA_BYTES {
        txn.rollback().await.ok();
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    txn.execute(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
INSERT INTO tapp_storage (tapp_id, user_id, key, value, created_at, updated_at)
VALUES ($1, $2, $3, $4, NOW(), NOW())
ON CONFLICT (user_id, tapp_id, key) DO UPDATE SET
    value = EXCLUDED.value,
    updated_at = NOW()
"#,
        vec![tapp_id.into(), user_id.into(), key.into(), value.into()],
    ))
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    txn.commit()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(())
}

pub(super) async fn get_tapp_settings(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<BTreeMap<String, serde_json::Value>>>, StatusCode> {
    let (access, settings) = authorize_tapp_settings(&db, &claims, &tapp_id).await?;
    let declared_keys: HashSet<String> = settings.into_iter().map(|setting| setting.key).collect();
    let values = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(access.installation_namespace()))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.starts_with("_settings."))
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .filter_map(|item| {
            let key = item.key.strip_prefix("_settings.")?.to_string();
            declared_keys.contains(&key).then_some((key, item.value))
        })
        .collect();
    Ok(Json(ApiResponse::success(values)))
}

pub(super) async fn get_tapp_setting(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<serde_json::Value>>, StatusCode> {
    let (access, storage_key, _) = authorize_tapp_setting(&db, &claims, &tapp_id, &key).await?;
    let value =
        read_storage_value(&db, access.installation_namespace(), &tapp_id, &storage_key).await?;
    Ok(Json(ApiResponse::success(value)))
}

pub(super) async fn set_tapp_setting(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path((tapp_id, key)): Path<(String, String)>,
    Json(value): Json<serde_json::Value>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    validate_storage_value_size(&value)?;
    let (access, storage_key, setting) =
        authorize_tapp_setting(&db, &claims, &tapp_id, &key).await?;
    if !can_write_installation_settings(access, current_is_admin(&claims).await) {
        return Err(StatusCode::FORBIDDEN);
    }
    if !tapp_setting_value_is_valid(&setting, &value) {
        return Err(StatusCode::BAD_REQUEST);
    }
    write_storage_value(
        &db,
        access.installation_namespace(),
        &tapp_id,
        &storage_key,
        value,
    )
    .await?;
    Ok(Json(ApiResponse::success(())))
}

pub(super) async fn list_storage_keys(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<Vec<String>>>, StatusCode> {
    let access = authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id).await?;
    let items = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(access.private_storage_namespace()))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let keys = items
        .into_iter()
        .filter(|item| !is_host_storage_key(&item.key))
        .map(|item| item.key)
        .collect();
    Ok(Json(ApiResponse::success(keys)))
}

pub(super) async fn list_storage_entries(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<BTreeMap<String, serde_json::Value>>>, StatusCode> {
    let access = authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id).await?;
    let entries = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(access.private_storage_namespace()))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .filter(|item| !is_host_storage_key(&item.key))
        .map(|item| (item.key, item.value))
        .collect();
    Ok(Json(ApiResponse::success(entries)))
}

#[derive(Debug, Serialize)]
pub(super) struct TappStorageUsage {
    used: usize,
    quota: usize,
}

pub(super) async fn get_storage_usage(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<TappStorageUsage>>, StatusCode> {
    let access = authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id).await?;
    let used = storage_bytes(&db, access.private_storage_namespace(), &tapp_id).await? as usize;
    Ok(Json(ApiResponse::success(TappStorageUsage {
        used,
        quota: TAPP_STORAGE_QUOTA_BYTES as usize,
    })))
}

pub(super) async fn get_storage(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<serde_json::Value>>, StatusCode> {
    validate_sandbox_storage_key(&key).map_err(|_| StatusCode::BAD_REQUEST)?;
    let access = authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id).await?;
    let value = read_storage_value(&db, access.private_storage_namespace(), &tapp_id, &key).await?;
    Ok(Json(ApiResponse::success(value)))
}

pub(super) async fn set_storage(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, key)): Path<(String, String)>,
    Json(value): Json<serde_json::Value>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    validate_sandbox_storage_key(&key).map_err(|_| StatusCode::BAD_REQUEST)?;
    validate_storage_value_size(&value)?;
    let access = authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id).await?;
    write_storage_value(
        &db,
        access.private_storage_namespace(),
        &tapp_id,
        &key,
        value,
    )
    .await?;
    Ok(Json(ApiResponse::success(())))
}

pub(super) async fn delete_storage(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path((tapp_id, key)): Path<(String, String)>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    validate_sandbox_storage_key(&key).map_err(|_| StatusCode::BAD_REQUEST)?;
    let access = authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id).await?;
    tapp_storage::Entity::delete_many()
        .filter(tapp_storage::Column::UserId.eq(access.private_storage_namespace()))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .filter(tapp_storage::Column::Key.eq(&key))
        .exec(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Json(ApiResponse::success(())))
}

pub(super) async fn clear_storage(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Path(tapp_id): Path<String>,
) -> Result<Json<ApiResponse<()>>, StatusCode> {
    let access = authorize_runtime_storage(&db, &claims, &runtime_grant, &tapp_id).await?;
    let clearable_ids: Vec<i32> = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(access.private_storage_namespace()))
        .filter(tapp_storage::Column::TappId.eq(&tapp_id))
        .all(&db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .into_iter()
        .filter(|item| !is_host_storage_key(&item.key))
        .map(|item| item.id)
        .collect();
    if !clearable_ids.is_empty() {
        tapp_storage::Entity::delete_many()
            .filter(tapp_storage::Column::Id.is_in(clearable_ids))
            .exec(&db)
            .await
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }
    Ok(Json(ApiResponse::success(())))
}
