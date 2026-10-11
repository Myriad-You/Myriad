//! The site owner's devices that sync her: add one (its token is shown this
//! once), list them, remove one, and say which end is her parent. Session
//! routes, owner only.

use super::*;
use crate::error::HttpError;
use crate::services::persona_sync::devices::{self, AddError, Device, RevokeReason};
use axum::response::IntoResponse;
use myriad_error::AppError;

fn store_failed(error: impl std::fmt::Display) -> HttpError {
    tracing::error!(%error, "[sync] device store failed");
    HttpError(AppError::internal("Failed to update devices"))
}

/// A device as the owner's settings page shows it.
pub(crate) fn device_json(device: &Device, now: chrono::DateTime<Utc>) -> Value {
    json!({
        "id": device.id,
        "name": device.name,
        "tokenHint": device.token_hint,
        "role": device.role.as_str(),
        "createdAt": device.created_at.to_rfc3339(),
        "lastSeenAt": device.last_seen_at.map(|at| at.to_rfc3339()),
        "revokedAt": device.revoked_at.map(|at| at.to_rfc3339()),
        "revokedReason": device.revoked_reason,
        "stale": device.stale(now),
    })
}

/// The owner's devices, which end is her parent, and the limit.
async fn device_list(db: &DatabaseConnection, owner: i32) -> Result<Value, HttpError> {
    let list = devices::list(db, owner).await.map_err(store_failed)?;
    let parent = devices::parent(db, owner).await.map_err(store_failed)?;
    let now = Utc::now();
    Ok(json!({
        "devices": list.iter().map(|device| device_json(device, now)).collect::<Vec<_>>(),
        "parent": parent.unwrap_or_else(|| "site".to_string()),
        "limit": devices::MAX_DEVICES,
    }))
}

/// GET /api/agent/devices
pub async fn list_devices(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    Ok(Json(device_list(&db, owner).await?))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeviceRoleRequest {
    role: String,
}

/// PUT /api/agent/devices/{device_id}/role: the owner says which end is
/// her parent from the site. `parent` makes this device it (a parent
/// device before it becomes a child); `child` on the parent device gives
/// it back to the site. The device learns it at its next `hello`. Answers
/// the list, as `GET /devices` does.
pub async fn set_device_role(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(device_id): Path<String>,
    Json(request): Json<DeviceRoleRequest>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let Some(role) = devices::Role::parse(&request.role) else {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "role is parent or child", "code": "invalid_role" })),
        )));
    };
    let updated = devices::set_role(&db, owner, &device_id, role)
        .await
        .map_err(store_failed)?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "No such device", "code": "device_not_found" })),
            ))
        })?;
    tracing::info!(device = %updated.id, role = role.as_str(), "[sync] device role set by the owner");
    Ok(Json(device_list(&db, owner).await?))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddDeviceRequest {
    name: String,
}

/// POST /api/agent/devices
pub async fn add_device(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<AddDeviceRequest>,
) -> Result<axum::response::Response, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let name = request.name.trim();
    if name.is_empty() || name.chars().any(char::is_control) {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "A device needs a name", "code": "device_name_required" })),
        )));
    }
    let (device, token) = match devices::add(&db, owner, name).await {
        Ok(added) => added,
        Err(AddError::Limit) => {
            return Err(HttpError::from((
                StatusCode::CONFLICT,
                Json(json!({
                    "error": format!("A site can have up to {} devices", devices::MAX_DEVICES),
                    "code": "device_limit",
                    "limit": devices::MAX_DEVICES,
                })),
            )));
        }
        Err(AddError::Db(error)) => return Err(store_failed(error)),
    };
    tracing::info!(device = %device.id, "[sync] device added");
    Ok((
        StatusCode::CREATED,
        Json(json!({ "device": device_json(&device, Utc::now()), "token": token })),
    )
        .into_response())
}

/// DELETE /api/agent/devices/{device_id}
pub async fn remove_device(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(device_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let removed = devices::revoke(&db, owner, &device_id, RevokeReason::ByOwner)
        .await
        .map_err(store_failed)?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "No such device", "code": "device_not_found" })),
            ))
        })?;
    tracing::info!(device = %removed.id, "[sync] device removed by the owner");
    Ok(Json(json!({ "device": device_json(&removed, Utc::now()) })))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(user_id: i32) -> Claims {
        Claims {
            sub: user_id.to_string(),
            username: format!("user-{user_id}"),
            is_admin: user_id == 41,
            is_owner: user_id == 41,
            exp: i64::MAX,
            iat: 0,
            tv: 0,
            subject: crate::middleware::auth::AuthSubject::from_test_sub(&user_id.to_string()),
        }
    }

    async fn role(
        db: &DatabaseConnection,
        user: i32,
        device: &str,
        role: &str,
    ) -> Result<Value, (StatusCode, Value)> {
        let request: DeviceRoleRequest = serde_json::from_value(json!({ "role": role })).unwrap();
        match set_device_role(
            State(db.clone()),
            Extension(claims(user)),
            Path(device.to_string()),
            Json(request),
        )
        .await
        {
            Ok(Json(list)) => Ok(list),
            Err(error) => {
                let response = error.into_response();
                let status = response.status();
                let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
                    .await
                    .unwrap();
                Err((
                    status,
                    serde_json::from_slice(&bytes).unwrap_or(Value::Null),
                ))
            }
        }
    }

    /// The owner picks her parent from the site (an isolated schema on a
    /// real database; skipped without a test database): a device, then the
    /// site again; nobody else can.
    #[tokio::test]
    async fn the_owner_sets_her_parent_from_the_site() {
        use sea_orm::ConnectionTrait;
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = fixture.db.clone();
        db.execute_unprepared(
            "INSERT INTO users (id, username, is_admin, is_owner) VALUES \
             (41, 'owner41', true, true), (42, 'guest42', false, false)",
        )
        .await
        .unwrap();
        crate::services::principal::invalidate_site_owner_cache();
        let (mac, _) = devices::add(&db, 41, "Mac").await.unwrap();
        let (imac, _) = devices::add(&db, 41, "iMac").await.unwrap();

        let list = role(&db, 41, &mac.id, "parent").await.unwrap();
        assert_eq!(list["parent"], mac.id.as_str());
        let list = role(&db, 41, &imac.id, "parent").await.unwrap();
        assert_eq!(list["parent"], imac.id.as_str());
        let parents = list["devices"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|device| device["role"] == "parent")
            .count();
        assert_eq!(parents, 1);
        // The parent made a child gives her back to the site.
        let list = role(&db, 41, &imac.id, "child").await.unwrap();
        assert_eq!(list["parent"], "site");

        let (status, body) = role(&db, 41, &mac.id, "boss").await.unwrap_err();
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("invalid_role"))
        );
        let (status, body) = role(&db, 41, "dev_nope", "parent").await.unwrap_err();
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::NOT_FOUND, Some("device_not_found"))
        );
        devices::revoke(&db, 41, &mac.id, RevokeReason::ByOwner)
            .await
            .unwrap();
        let (status, _) = role(&db, 41, &mac.id, "parent").await.unwrap_err();
        assert_eq!(status, StatusCode::NOT_FOUND);
        let (status, _) = role(&db, 42, &imac.id, "parent").await.unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            devices::parent(&db, 41).await.unwrap(),
            None,
            "nothing changed"
        );

        fixture.close().await;
    }
}
