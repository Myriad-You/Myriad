//! What a device calls to sync her (`/api/agent/sync/*`), behind
//! `sync_device_auth`: who it is and what the site has (`hello`), reading
//! her (`pull`, `assets`), leaving, and which end her own life happens at.
//! `hello` and `role` need the device's clock (`X-Sync-Clock`); `leave`
//! does not, so a device with a wrong clock can still give its token back.

use super::*;
use crate::error::HttpError;
use crate::middleware::sync_device::{SyncClock, SyncDevice};
use crate::services::persona_sync::{self, devices};
use myriad_error::AppError;

pub(crate) fn sync_failed(error: impl std::fmt::Display) -> HttpError {
    tracing::error!(%error, "[sync] request failed");
    HttpError(AppError::internal("Sync failed"))
}

/// The site's owner, who every device belongs to (the gate already checked).
pub(crate) fn owner_of(device: &SyncDevice) -> i32 {
    device.0.user_id
}

/// The device's clock, which this route needs (checked by the gate).
fn require_clock(clock: &SyncClock) -> Result<(), HttpError> {
    match clock.0 {
        Some(_) => Ok(()),
        None => Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({
                "error": "X-Sync-Clock is required: the revision this device's next write would get",
                "code": "sync_clock_required",
            })),
        ))),
    }
}

/// GET /api/agent/sync/hello
pub async fn sync_hello(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Extension(clock): Extension<SyncClock>,
) -> Result<Json<Value>, HttpError> {
    require_clock(&clock)?;
    let owner = owner_of(&device);
    let holdings = persona_sync::holdings(&db, owner)
        .await
        .map_err(sync_failed)?;
    let name = persona_sync::persona_name(&db).await.map_err(sync_failed)?;
    let embedding_model = persona_sync::embedding_model(&db)
        .await
        .map_err(sync_failed)?;
    let parent = devices::parent(&db, owner).await.map_err(sync_failed)?;
    Ok(Json(json!({
        "formatVersion": myriad_persona_sync::FORMAT_VERSION,
        "deviceId": device.0.id,
        "lineage": device.0.lineage,
        "empty": myriad_persona_sync::is_empty(&holdings),
        "persona": name.map(|name| json!({ "name": name })),
        "embeddingModel": embedding_model,
        "role": device.0.role.as_str(),
        "parent": parent.unwrap_or_else(|| "site".to_string()),
        "limits": {
            "pullMaxEntities": persona_sync::PULL_MAX_ENTITIES,
            "pullMaxBytes": persona_sync::PULL_MAX_BYTES,
            "pushMaxEntities": persona_sync::PUSH_MAX_ENTITIES,
            "pushMaxBytes": persona_sync::PUSH_MAX_BYTES,
            "assetMaxBytes": persona_sync::ASSET_MAX_BYTES,
            "clockMaxAheadMs": myriad_persona_sync::hlc::MAX_AHEAD_MS,
        },
        "serverTime": Utc::now().timestamp_micros(),
    })))
}

/// POST /api/agent/sync/leave: the device gives its token back.
pub async fn sync_leave(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
) -> Result<Json<Value>, HttpError> {
    devices::revoke(
        &db,
        owner_of(&device),
        &device.0.id,
        devices::RevokeReason::DeviceLeft,
    )
    .await
    .map_err(sync_failed)?;
    tracing::info!(device = %device.0.id, "[sync] device left");
    Ok(Json(json!({ "left": true })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleRequest {
    role: String,
}

/// PUT /api/agent/sync/role: chosen when the device connects. `parent`
/// makes this device the parent end (another parent device becomes a
/// child); `child` leaves the site as the parent unless another device is.
pub async fn sync_role(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Extension(clock): Extension<SyncClock>,
    Json(request): Json<RoleRequest>,
) -> Result<Json<Value>, HttpError> {
    require_clock(&clock)?;
    let Some(role) = devices::Role::parse(&request.role) else {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "role is parent or child", "code": "invalid_role" })),
        )));
    };
    let owner = owner_of(&device);
    let updated = devices::set_role(&db, owner, &device.0.id, role)
        .await
        .map_err(sync_failed)?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "This device was removed from the site", "code": "device_revoked" })),
            ))
        })?;
    let parent = devices::parent(&db, owner).await.map_err(sync_failed)?;
    tracing::info!(device = %updated.id, role = role.as_str(), "[sync] device role set");
    Ok(Json(json!({
        "role": updated.role.as_str(),
        "parent": parent.unwrap_or_else(|| "site".to_string()),
    })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PullQuery {
    #[serde(default)]
    cursor: Option<String>,
    #[serde(default)]
    limit: Option<usize>,
    /// The device's embedding model, as JSON `{model, dims}`.
    #[serde(default)]
    embedding_model: Option<String>,
}

fn bad_request(error: &str, code: &str) -> HttpError {
    HttpError::from((
        StatusCode::BAD_REQUEST,
        Json(json!({ "error": error, "code": code })),
    ))
}

/// GET /api/agent/sync/pull: one page of everything the site has of her,
/// in `(kind, id)` order. Read only. No cursor is the start; `next` is
/// where the following page starts.
pub async fn sync_pull(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Query(query): Query<PullQuery>,
) -> Result<Json<Value>, HttpError> {
    use crate::services::persona_sync::snapshot::{Cursor, Limits, page};
    let after = match query.cursor.as_deref().filter(|raw| !raw.is_empty()) {
        Some(raw) => Some(
            Cursor::decode(raw).ok_or_else(|| bad_request("Unknown cursor", "invalid_cursor"))?,
        ),
        None => None,
    };
    let theirs = match query.embedding_model.as_deref() {
        Some(raw) => Some(
            serde_json::from_str::<myriad_persona_sync::entity::EmbeddingModel>(raw).map_err(
                |_| bad_request("embeddingModel is {model, dims}", "invalid_embedding_model"),
            )?,
        ),
        None => None,
    };
    let ours = persona_sync::embedding_model(&db)
        .await
        .map_err(sync_failed)?;
    let shared = match (theirs, ours) {
        (Some(mut theirs), Some(ours)) => {
            theirs.model = theirs.model.trim().to_string();
            (theirs == ours).then_some(ours)
        }
        _ => None,
    };
    let limits = Limits {
        entities: query
            .limit
            .unwrap_or(persona_sync::PULL_DEFAULT_ENTITIES)
            .clamp(1, persona_sync::PULL_MAX_ENTITIES),
        bytes: persona_sync::PULL_MAX_BYTES,
    };
    let page = page(
        &db,
        owner_of(&device),
        after.as_ref(),
        &limits,
        shared.as_ref(),
    )
    .await
    .map_err(sync_failed)?;
    Ok(Json(json!({
        "lineage": device.0.lineage,
        "entities": page.entities,
        "embeddings": page.embeddings,
        "assets": page.assets,
        "next": page.next.as_ref().map(Cursor::encode),
        "more": page.more,
    })))
}

/// GET /api/agent/sync/assets/{asset_id}: a picture as itself, a rig as
/// `multipart/mixed` (`manifest.json`, then `atlas.png`). Only what her
/// persona refers to now.
pub async fn sync_asset(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Path(asset_id): Path<String>,
) -> Result<axum::response::Response, HttpError> {
    use crate::services::persona_sync::assets::{self, Pictures, RIG_MEDIA_TYPE, Source};
    let pictures = Pictures::new(&db, owner_of(&device));
    use axum::http::header;
    use axum::response::IntoResponse;
    let not_found = || {
        HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "No such asset", "code": "asset_not_found" })),
        ))
    };
    let source = assets::find(&pictures, &asset_id)
        .await
        .map_err(sync_failed)?
        .ok_or_else(not_found)?;
    const IMMUTABLE: &str = "private, max-age=31536000, immutable";
    match source {
        Source::Image { url, media_type } => {
            let id = asset_id.trim().to_ascii_lowercase();
            let bytes = assets::image_bytes(&pictures, &url, &id)
                .await
                .ok_or_else(not_found)?;
            Ok((
                [
                    (header::CONTENT_TYPE, media_type),
                    (header::CACHE_CONTROL, IMMUTABLE.to_string()),
                ],
                bytes,
            )
                .into_response())
        }
        Source::Rig { id } => {
            let manifest = crate::services::merope_rig::read_manifest_bytes(&id)
                .await
                .map_err(|_| not_found())?;
            let atlas = crate::services::merope_rig::read_atlas_bytes(&id)
                .await
                .map_err(|_| not_found())?;
            let boundary = format!("myriad-rig-{}", &id[..16]);
            let mut body = Vec::with_capacity(manifest.len() + atlas.len() + 512);
            for (media_type, name, bytes) in [
                ("application/json", "manifest.json", &manifest),
                ("image/png", "atlas.png", &atlas),
            ] {
                body.extend_from_slice(
                    format!(
                        "--{boundary}\r\nContent-Type: {media_type}\r\n\
                         Content-Disposition: attachment; filename=\"{name}\"\r\n\
                         Content-Length: {}\r\n\r\n",
                        bytes.len()
                    )
                    .as_bytes(),
                );
                body.extend_from_slice(bytes);
                body.extend_from_slice(b"\r\n");
            }
            body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
            Ok((
                [
                    (
                        header::CONTENT_TYPE,
                        format!("{RIG_MEDIA_TYPE}; boundary={boundary}"),
                    ),
                    (header::CACHE_CONTROL, IMMUTABLE.to_string()),
                ],
                body,
            )
                .into_response())
        }
    }
}

#[cfg(test)]
mod tests {
    use axum::{
        body::{Body, to_bytes},
        http::{Request, StatusCode},
    };
    use sea_orm::ConnectionTrait;
    use serde_json::Value;
    use tower::ServiceExt;

    async fn call(
        app: &axum::Router,
        method: &str,
        path: &str,
        token: Option<&str>,
    ) -> (StatusCode, Value) {
        call_with(app, method, path, token, &[], None).await
    }

    async fn call_with(
        app: &axum::Router,
        method: &str,
        path: &str,
        token: Option<&str>,
        headers: &[(&str, String)],
        json_body: Option<Value>,
    ) -> (StatusCode, Value) {
        let mut request = Request::builder().method(method).uri(path);
        if let Some(token) = token {
            request = request.header("authorization", format!("Bearer {token}"));
        }
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let body = match json_body {
            Some(json_body) => {
                request = request.header("content-type", "application/json");
                Body::from(json_body.to_string())
            }
            None => Body::empty(),
        };
        let response = app
            .clone()
            .oneshot(request.body(body).unwrap())
            .await
            .unwrap();
        let status = response.status();
        let bytes = to_bytes(response.into_body(), 1 << 22).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes).unwrap_or(Value::Null),
        )
    }

    /// The device routes over HTTP on a real database (an isolated schema;
    /// skipped without a test database): only a live device token gets in,
    /// a device token is no session, and hello, pull and leave answer.
    #[tokio::test]
    async fn device_routes_take_only_a_live_device_token() {
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = fixture.db.clone();
        db.execute_unprepared(
            "INSERT INTO users (id, username, is_admin, is_owner) VALUES (41, 'owner41', true, true)",
        )
        .await
        .unwrap();
        crate::services::principal::invalidate_site_owner_cache();
        let state = crate::state::AppState::new(
            db.clone(),
            crate::config::AppConfig::default(),
            crate::config::DynamicConfig::default(),
        );
        let app = axum::Router::new()
            .nest(
                "/api/agent",
                super::super::create_agent_routes(state.clone()),
            )
            .with_state(state);
        let (_, token) = crate::services::persona_sync::devices::add(&db, 41, "Mac")
            .await
            .unwrap();

        let (status, body) = call(&app, "GET", "/api/agent/sync/hello", None).await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::UNAUTHORIZED, Some("device_token_required"))
        );
        let forged = format!("mys_{}", "A".repeat(43));
        let (status, body) = call(&app, "GET", "/api/agent/sync/hello", Some(&forged)).await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::UNAUTHORIZED, Some("device_token_invalid"))
        );
        // A device token is not a session. (Without a JWT secret configured
        // the session gate answers 500 here instead of 401; either way, no.)
        let (status, body) = call(&app, "GET", "/api/agent/devices", Some(&token)).await;
        assert!(
            status == StatusCode::UNAUTHORIZED || status == StatusCode::INTERNAL_SERVER_ERROR,
            "{status} {body}"
        );
        assert!(!body.to_string().contains(&token));

        crate::services::principal::invalidate_site_owner_cache();
        // The device's clock: required on hello, refused when far ahead.
        let clock_at = |offset_ms: i64| {
            let now_ms = chrono::Utc::now().timestamp_millis() + offset_ms;
            let rev = myriad_persona_sync::Rev::new(now_ms as u64, 0, "dev_test").unwrap();
            vec![("x-sync-clock", rev.to_string())]
        };
        let (status, body) = call(&app, "GET", "/api/agent/sync/hello", Some(&token)).await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("sync_clock_required"))
        );
        let (status, body) = call_with(
            &app,
            "GET",
            "/api/agent/sync/hello",
            Some(&token),
            &[("x-sync-clock", "tomorrow".to_string())],
            None,
        )
        .await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("invalid_sync_clock"))
        );
        let (status, body) = call_with(
            &app,
            "GET",
            "/api/agent/sync/hello",
            Some(&token),
            &clock_at(60 * 60 * 1000),
            None,
        )
        .await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::CONFLICT, Some("clock_ahead")),
            "{body}"
        );
        assert!(body["aheadMs"].as_u64().unwrap() > 50 * 60 * 1000);
        assert_eq!(body["maxAheadMs"], 300_000);
        assert!(body["serverTime"].as_i64().unwrap() > 0);
        // A pull with a clock far ahead is refused too; without one it is not.
        let (status, body) = call_with(
            &app,
            "GET",
            "/api/agent/sync/pull",
            Some(&token),
            &clock_at(10 * 60 * 1000),
            None,
        )
        .await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::CONFLICT, Some("clock_ahead"))
        );
        let (status, hello) = call_with(
            &app,
            "GET",
            "/api/agent/sync/hello",
            Some(&token),
            &clock_at(4 * 60 * 1000),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{hello}");
        assert_eq!(hello["limits"]["clockMaxAheadMs"], 300_000);
        assert_eq!(hello["formatVersion"], 1);
        assert_eq!(hello["empty"], true);
        assert_eq!(hello["persona"], Value::Null);
        assert_eq!(hello["parent"], "site");
        assert_eq!(hello["role"], "child");
        assert!(hello["deviceId"].as_str().unwrap().starts_with("dev_"));

        let (status, page) = call(&app, "GET", "/api/agent/sync/pull", Some(&token)).await;
        assert_eq!(status, StatusCode::OK, "{page}");
        assert_eq!(page["entities"], serde_json::json!([]));
        assert_eq!(page["more"], false);
        let (status, body) = call(
            &app,
            "GET",
            "/api/agent/sync/pull?cursor=nope",
            Some(&token),
        )
        .await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("invalid_cursor"))
        );
        let (status, body) = call(
            &app,
            "GET",
            &format!("/api/agent/sync/assets/{}", "ab".repeat(32)),
            Some(&token),
        )
        .await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::NOT_FOUND, Some("asset_not_found"))
        );

        // Role needs the clock too.
        let role = Some(serde_json::json!({ "role": "parent" }));
        let (status, body) = call_with(
            &app,
            "PUT",
            "/api/agent/sync/role",
            Some(&token),
            &[],
            role.clone(),
        )
        .await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::BAD_REQUEST, Some("sync_clock_required"))
        );
        let (status, body) = call_with(
            &app,
            "PUT",
            "/api/agent/sync/role",
            Some(&token),
            &clock_at(0),
            role,
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        assert_eq!(body["role"], "parent");

        // A wrong clock does not keep a device from leaving.
        let (status, _) = call_with(
            &app,
            "POST",
            "/api/agent/sync/leave",
            Some(&token),
            &clock_at(60 * 60 * 1000),
            None,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let (status, body) = call_with(
            &app,
            "GET",
            "/api/agent/sync/hello",
            Some(&token),
            &clock_at(0),
            None,
        )
        .await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::UNAUTHORIZED, Some("device_revoked"))
        );

        fixture.close().await;
    }
}
