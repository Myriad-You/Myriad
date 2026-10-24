//! The site owner's devices that sync her: add one (its token is shown this
//! once), list them, remove one; and her host lease: where she lives, and a
//! move of her started, confirmed or called off. Session routes, owner only:
//! a move that writes to the site's her is confirmed here, logged in, never
//! by a device token alone.

use super::*;
use crate::error::HttpError;
use crate::services::persona_sync::devices::{self, AddError, Device, RevokeReason};
use crate::services::persona_sync::lease;
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
        "createdAt": device.created_at.to_rfc3339(),
        "lastSeenAt": device.last_seen_at.map(|at| at.to_rfc3339()),
        "revokedAt": device.revoked_at.map(|at| at.to_rfc3339()),
        "revokedReason": device.revoked_reason,
        "stale": device.stale(now),
        // Its link to her is open now (lastSeenAt is when it last was).
        "connected": device.revoked_at.is_none()
            && crate::services::agent::link::is_open(&device.id),
    })
}

/// The owner's devices, where she lives, and the limit.
async fn device_list(db: &DatabaseConnection, owner: i32) -> Result<Value, HttpError> {
    let list = devices::list(db, owner).await.map_err(store_failed)?;
    let host = lease::shown(db).await.map_err(store_failed)?;
    let now = crate::clock::now();
    // What came by chat app while she was away, held until she is back (D3).
    let held = crate::services::channel_work::held::count(db)
        .await
        .map_err(store_failed)?;
    // Her on her way into this site, which has none (H3c).
    let import = crate::services::persona_sync::import::shown(db, owner)
        .await
        .map_err(store_failed)?;
    // Whether the site could take her in from a file now.
    let empty = crate::services::persona_sync::import::not_empty(db, owner)
        .await
        .map_err(store_failed)?
        .is_none();
    // Her daily backups (D4): what the site keeps and cannot open.
    let backups = crate::services::persona_sync::backups::listed(db)
        .await
        .map_err(store_failed)?;
    // What is left of others here while she is not (Q2), to let go of.
    let sealed = if lease::current().is_some_and(|lease| {
        matches!(
            lease.place(),
            lease::Where::Elsewhere(_) | lease::Where::Vacant
        )
    }) {
        Some(sealed_count(db).await.map_err(store_failed)?)
    } else {
        None
    };
    Ok(json!({
        "devices": list.iter().map(|device| device_json(device, now)).collect::<Vec<_>>(),
        "host": host,
        "import": import,
        "backups": backups,
        "sealed": sealed,
        // Her living on a device opens with H4 (Q8); the acceptance entry only.
        "deviceHostOpen": lease::device_host_open(),
        // What the site sealed of its her before a merge (H3d-12).
        "retired": crate::services::persona_sync::merge::retired().await,
        "empty": empty,
        "held": held,
        // The site's last check of its own clock (null while off or not
        // run yet), for the warning when it is wrong.
        "serverClock": crate::services::server_clock::shown(),
        "limit": devices::MAX_DEVICES,
    }))
}

/// What is left of others on the site while she is away (sealed, Q2): their
/// memories, as a count.
async fn sealed_count(db: &DatabaseConnection) -> Result<i64, sea_orm::DbErr> {
    use sea_orm::ConnectionTrait;
    let sources = crate::services::agent::merope::api::store::PERSONA_MEMORY_SOURCES
        .iter()
        .map(|source| format!("'{source}'"))
        .collect::<Vec<_>>()
        .join(", ");
    Ok(db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            format!(
                "SELECT COUNT(*)::BIGINT AS n FROM agent_memories WHERE source IN ({sources}) \
                 OR venue = 'own'"
            ),
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "n"))
        .transpose()?
        .unwrap_or(0))
}

/// GET /api/agent/devices
pub async fn list_devices(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    Ok(Json(device_list(&db, owner).await?))
}

/// GET /api/agent/host: where she lives, as the devices page shows it.
pub async fn get_host(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    super::persona::require_site_owner(&claims, &db).await?;
    Ok(Json(
        json!({ "host": lease::shown(&db).await.map_err(store_failed)? }),
    ))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartMoveRequest {
    kind: String,
    device: String,
}

/// POST /api/agent/host/moves: the owner, logged in, starts a move of her
/// with one of their live devices: `move` (选主机, she goes to live on it:
/// it waits for the device to agree, then for the owner's confirmation with
/// a passphrase; Q8: the acceptance entry only until H4), `back` (she comes
/// back from the device she lives on: confirmed already, she is frozen
/// there at once) or `merge`. Answers the lease.
pub async fn start_move(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<StartMoveRequest>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let back = request.kind == "back";
    let kind = if back {
        lease::Kind::Move
    } else {
        match lease::Kind::parse(&request.kind) {
            Some(kind @ (lease::Kind::Move | lease::Kind::Merge)) => kind,
            _ => {
                return Err(HttpError::from((
                    StatusCode::BAD_REQUEST,
                    Json(
                        json!({ "error": "kind is move, back or merge", "code": "invalid_move_kind" }),
                    ),
                )));
            }
        }
    };
    if kind == lease::Kind::Move && !back && !lease::device_host_open() {
        return Err(super::sync::move_refused(lease::MoveError::Later));
    }
    let live = devices::list(&db, owner)
        .await
        .map_err(store_failed)?
        .into_iter()
        .any(|device| device.id == request.device && device.revoked_at.is_none());
    if !live {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "No such device", "code": "device_not_found" })),
        )));
    }
    if back {
        lease::start_back(&db, &request.device, lease::Starter::Owner)
            .await
            .map_err(super::sync::move_refused)?;
    } else {
        lease::start(&db, kind, &request.device, lease::Starter::Owner)
            .await
            .map_err(super::sync::move_refused)?;
    }
    lease::reload(&db).await;
    Ok(Json(
        json!({ "host": lease::shown(&db).await.map_err(store_failed)? }),
    ))
}

/// POST /api/agent/host/moves/{move_id}/confirm: the owner, logged in,
/// confirms a move a device asked for. She is frozen where she is. A move
/// to a device is confirmed with `{passphrase}` once the device agreed: the
/// site seals the copy it keeps with it (used once, kept nowhere, D4).
pub async fn confirm_move(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(move_id): Path<String>,
    body: axum::body::Bytes,
) -> Result<Json<Value>, HttpError> {
    #[derive(Debug, Default, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ConfirmRequest {
        #[serde(default)]
        passphrase: Option<String>,
    }
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let request: ConfirmRequest = if body.is_empty() {
        ConfirmRequest::default()
    } else {
        serde_json::from_slice(&body).map_err(|_| {
            HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "The request does not read", "code": "invalid_request" })),
            ))
        })?
    };
    let to_device = lease::read(&db)
        .await
        .map_err(store_failed)?
        .and_then(|lease| lease.moving)
        .is_some_and(|moving| moving.id == move_id && moving.to_device());
    if to_device {
        if !lease::device_host_open() {
            return Err(super::sync::move_refused(lease::MoveError::Later));
        }
        let Some(passphrase) = request
            .passphrase
            .filter(|passphrase| !passphrase.is_empty())
        else {
            return Err(HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(
                    json!({ "error": "The copy the site keeps is sealed with a passphrase", "code": "passphrase_required" }),
                ),
            )));
        };
        crate::services::persona_sync::moves::confirm_to_device(&db, owner, &move_id, passphrase)
            .await
            .map_err(super::sync::move_refused)?;
    } else {
        lease::confirm(&db, &move_id)
            .await
            .map_err(super::sync::move_refused)?;
    }
    lease::reload(&db).await;
    Ok(Json(
        json!({ "host": lease::shown(&db).await.map_err(store_failed)? }),
    ))
}

/// POST /api/agent/host/moves/{move_id}/cancel: the owner calls a move off
/// before its commit, or turns down one a device asked for: she is where she
/// was, as she was.
pub async fn cancel_move(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(move_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    super::persona::require_site_owner(&claims, &db).await?;
    lease::cancel(&db, &move_id, None)
        .await
        .map_err(super::sync::move_refused)?;
    lease::reload(&db).await;
    Ok(Json(
        json!({ "host": lease::shown(&db).await.map_err(store_failed)? }),
    ))
}

/// POST /api/agent/host/imports/{import_id}/confirm: the owner, logged in,
/// confirms that the her on one of their devices comes to live on this
/// site, which has none. The site's first lease names that device, frozen,
/// until she lands.
///
/// A merge (H3d) is confirmed with `{passphrase}`: the site seals the her it
/// has with it first (used once for the key, kept nowhere).
pub async fn confirm_import(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(import_id): Path<String>,
    body: axum::body::Bytes,
) -> Result<Json<Value>, HttpError> {
    #[derive(Debug, Default, Deserialize)]
    #[serde(deny_unknown_fields)]
    struct ConfirmRequest {
        #[serde(default)]
        passphrase: Option<String>,
        /// The site she left keeps what was left of another her: let it go.
        #[serde(default)]
        replace: bool,
    }
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let request: ConfirmRequest = if body.is_empty() {
        ConfirmRequest::default()
    } else {
        serde_json::from_slice(&body).map_err(|_| {
            super::sync::import_refused(
                crate::services::persona_sync::import::ImportError::Invalid("body"),
            )
        })?
    };
    crate::services::persona_sync::import::confirm(
        &db,
        owner,
        &import_id,
        request.passphrase,
        request.replace,
    )
    .await
    .map_err(super::sync::import_refused)?;
    Ok(Json(device_list(&db, owner).await?))
}

/// GET /api/agent/host/retired/{name}: one copy the site sealed of its her
/// before a merge, as it was written (the owner's passphrase opens it).
pub async fn download_retired(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(name): Path<String>,
) -> Result<axum::response::Response, HttpError> {
    use axum::http::header;
    super::persona::require_site_owner(&claims, &db).await?;
    let path = crate::services::persona_sync::merge::retired_path(&name).ok_or_else(|| {
        HttpError(AppError::not_found("No such file").with_code("retired_unknown"))
    })?;
    let bytes = tokio::fs::read(&path).await.map_err(store_failed)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        bytes,
    )
        .into_response())
}

/// DELETE /api/agent/host/retired/{name}: the owner lets one such copy go.
pub async fn delete_retired(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(name): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let path = crate::services::persona_sync::merge::retired_path(&name).ok_or_else(|| {
        HttpError(AppError::not_found("No such file").with_code("retired_unknown"))
    })?;
    tokio::fs::remove_file(&path).await.map_err(store_failed)?;
    tracing::info!(file = %name, "[merge] the owner let a copy sealed before a merge go");
    Ok(Json(device_list(&db, owner).await?))
}

/// POST /api/agent/host/imports/{import_id}/cancel: the owner turns down or
/// calls off an import; the site is as it was.
pub async fn cancel_import(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(import_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    crate::services::persona_sync::import::cancel(&db, &import_id, None)
        .await
        .map_err(super::sync::import_refused)?;
    Ok(Json(device_list(&db, owner).await?))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BeginFileRequest {
    total: u64,
}

/// POST /api/agent/host/imports/file: the owner starts bringing her in from
/// an export file of `total` bytes, sent in pieces next.
pub async fn begin_file_import(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<BeginFileRequest>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let session = crate::services::persona_sync::import::begin_file(&db, owner, request.total)
        .await
        .map_err(super::sync::import_refused)?;
    Ok(Json(json!({
        "import": session.json(None),
        "pieceBytes": crate::services::persona_sync::import::PIECE_MAX_BYTES,
    })))
}

/// GET /api/agent/host/imports/{import_id}: where a file's upload stands.
pub async fn file_import_status(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(import_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    super::persona::require_site_owner(&claims, &db).await?;
    let session = crate::services::persona_sync::import::file_status(&db, &import_id)
        .await
        .map_err(super::sync::import_refused)?;
    Ok(Json(json!({ "import": session.json(None) })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PieceQuery {
    offset: u64,
}

/// PUT /api/agent/host/imports/{import_id}/file?offset=N: one piece of the
/// file, at where the upload stands.
pub async fn file_import_piece(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(import_id): Path<String>,
    Query(query): Query<PieceQuery>,
    body: axum::body::Bytes,
) -> Result<Json<Value>, HttpError> {
    super::persona::require_site_owner(&claims, &db).await?;
    let session = crate::services::persona_sync::import::take_piece(
        &db,
        &import_id,
        query.offset,
        body.to_vec(),
    )
    .await
    .map_err(super::sync::import_refused)?;
    Ok(Json(json!({ "import": session.json(None) })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenFileRequest {
    passphrase: String,
    /// The site she left keeps what was left of another her: let it go.
    #[serde(default)]
    replace: bool,
}

/// POST /api/agent/host/imports/{import_id}/open: the whole file is here;
/// the owner's passphrase opens it, once, and she lands. The passphrase is
/// used for the key and kept nowhere: this site is the one taking her in.
pub async fn open_file_import(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(import_id): Path<String>,
    Json(request): Json<OpenFileRequest>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let landed = crate::services::persona_sync::import::open_file(
        &db,
        owner,
        &import_id,
        request.passphrase,
        request.replace,
    )
    .await
    .map_err(super::sync::import_refused)?;
    let host = lease::shown(&db).await.map_err(store_failed)?;
    Ok(Json(super::sync::landed_json(&landed, host)))
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
        Json(json!({ "device": device_json(&device, crate::clock::now()), "token": token })),
    )
        .into_response())
}

/// DELETE /api/agent/host/held: the owner lets go of what came by chat app
/// while she was away; she will not read it (D3-4).
pub async fn clear_held(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    super::persona::require_site_owner(&claims, &db).await?;
    let cleared = crate::services::channel_work::held::clear(&db)
        .await
        .map_err(store_failed)?;
    Ok(Json(json!({ "cleared": cleared, "held": 0 })))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportRequest {
    passphrase: String,
}

/// POST /api/agent/host/export: 「导出她」. The owner, logged in, gets
/// everything the site has of her as one `myriad.her-archive` file sealed
/// with the passphrase they typed (always sealed, H3-6). The passphrase is
/// used once for the key and kept nowhere. Only while she lives here or is
/// moving.
pub async fn export_her(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<ExportRequest>,
) -> Result<axum::response::Response, HttpError> {
    use crate::services::persona_sync::export::{self, ExportError};
    use axum::http::header;
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let refused = |status: StatusCode, error: &str, code: &str, extra: Option<(&str, Value)>| {
        let mut body = json!({ "error": error, "code": code });
        if let Some((key, value)) = extra {
            body[key] = value;
        }
        HttpError::from((status, Json(body)))
    };
    match export::export(&db, owner, request.passphrase).await {
        Ok(file) => Ok((
            [
                (header::CONTENT_TYPE, "application/octet-stream".to_string()),
                (
                    header::CONTENT_DISPOSITION,
                    format!("attachment; filename=\"{}\"", file.name),
                ),
                (header::CACHE_CONTROL, "no-store".to_string()),
            ],
            file.bytes,
        )
            .into_response()),
        Err(ExportError::Passphrase) => Err(refused(
            StatusCode::BAD_REQUEST,
            "An export is sealed with a passphrase",
            "passphrase_required",
            None,
        )),
        Err(ExportError::Elsewhere(device)) => {
            let name = match device {
                Some(id) => lease::device_name(&db, &id).await.ok().flatten(),
                None => None,
            };
            Err(refused(
                StatusCode::CONFLICT,
                "She does not live on this site now",
                "host_moved",
                Some((
                    "host",
                    json!(name.map(|name| json!({ "kind": "app", "name": name }))),
                )),
            ))
        }
        Err(ExportError::Empty) => Err(refused(
            StatusCode::NOT_FOUND,
            "The site has no her to export",
            "her_absent",
            None,
        )),
        Err(ExportError::Failed(what)) => {
            tracing::error!(what, "[sync] she could not be exported");
            Err(refused(
                StatusCode::INTERNAL_SERVER_ERROR,
                "She could not be exported",
                "export_failed",
                None,
            ))
        }
    }
}

fn sealed_file(name: String, bytes: Vec<u8>) -> axum::response::Response {
    use axum::http::header;
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream".to_string()),
            (
                header::CONTENT_DISPOSITION,
                format!("attachment; filename=\"{name}\""),
            ),
            (header::CACHE_CONTROL, "no-store".to_string()),
        ],
        bytes,
    )
        .into_response()
}

/// POST /api/agent/host/move-out: 「导出并搬走」 on the site (H3c-5, Q7). The
/// owner, logged in, gets her as one sealed file, and she moves out: stopped
/// here (no reply, no life of her own) until the owner lets this copy go
/// (`/host/move-out/let-go`) or undoes it. Nothing registers where the file
/// goes, so the same file brought in twice cannot be told apart; the page
/// says so.
pub async fn move_out(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<ExportRequest>,
) -> Result<axum::response::Response, HttpError> {
    use crate::services::persona_sync::export;
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    if request.passphrase.is_empty() {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(
                json!({ "error": "An export is sealed with a passphrase", "code": "passphrase_required" }),
            ),
        )));
    }
    lease::move_out(&db)
        .await
        .map_err(super::sync::move_refused)?;
    lease::reload(&db).await;
    match export::export(&db, owner, request.passphrase).await {
        Ok(file) => Ok(sealed_file(file.name, file.bytes)),
        Err(error) => {
            // No file: she did not move out.
            let _ = lease::undo_move_out(&db).await;
            lease::reload(&db).await;
            tracing::error!(
                ?error,
                "[host] she could not be exported to move out; undone"
            );
            Err(HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "She could not be exported", "code": "export_failed" })),
            )))
        }
    }
}

/// POST /api/agent/host/move-out/undo: she lives here again, as she was.
pub async fn undo_move_out(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    lease::undo_move_out(&db)
        .await
        .map_err(super::sync::move_refused)?;
    lease::reload(&db).await;
    Ok(Json(device_list(&db, owner).await?))
}

/// POST /api/agent/host/move-out/let-go: 「已经搬好，删掉这里的她」. Her own
/// parts go; what was left of others stays, sealed with her lineage (Q2),
/// and the site has no her now (vacant).
pub async fn let_her_go(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    use sea_orm::TransactionTrait;
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let txn = db.begin().await.map_err(store_failed)?;
    lease::let_go_on(&txn)
        .await
        .map_err(super::sync::move_refused)?;
    crate::services::agent::merope::retire_her_on(&txn, owner)
        .await
        .map_err(store_failed)?;
    let cleared = crate::services::merope_rig::persist_active_asset(&txn, None)
        .await
        .map_err(store_failed)?;
    crate::services::media::bind_persona(
        &txn,
        None,
        None,
        None,
        &crate::services::media::configured_origins().await,
    )
    .await
    .map_err(|error| HttpError(error.into()))?;
    txn.commit().await.map_err(store_failed)?;
    crate::services::merope_rig::mirror_active_asset(cleared).await;
    crate::services::agent::merope::forget_in_memory();
    lease::reload(&db).await;
    tracing::info!("[host] she moved out and the site let its copy of her go");
    Ok(Json(device_list(&db, owner).await?))
}

/// DELETE /api/agent/host/sealed: the owner lets go of what was left of
/// others on the site while she is away (Q2). On a site she left (vacant),
/// her lease goes too: the next her here is another birth (H3e-15).
pub async fn clear_sealed(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    use sea_orm::TransactionTrait;
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let place = lease::read(&db)
        .await
        .map_err(store_failed)?
        .map(|lease| lease.place());
    if !matches!(
        place,
        Some(lease::Where::Elsewhere(_) | lease::Where::Vacant)
    ) {
        return Err(HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({ "error": "She is not away from the site", "code": "her_not_away" })),
        )));
    }
    let txn = db.begin().await.map_err(store_failed)?;
    crate::services::agent::merope::clear_sealed_on(&txn)
        .await
        .map_err(store_failed)?;
    if place == Some(lease::Where::Vacant) {
        lease::forget_vacant_on(&txn).await.map_err(store_failed)?;
    }
    txn.commit().await.map_err(store_failed)?;
    crate::services::agent::merope::forget_in_memory();
    lease::reload(&db).await;
    tracing::info!("[host] the owner let what was left of others here go");
    Ok(Json(device_list(&db, owner).await?))
}

/// GET /api/agent/host/backups/{backup_id}: one daily backup as it was
/// pushed. The site cannot open it; the app that made it can, with its
/// passphrase (Q5).
pub async fn download_backup(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(backup_id): Path<String>,
) -> Result<axum::response::Response, HttpError> {
    super::persona::require_site_owner(&claims, &db).await?;
    let (path, _) = crate::services::persona_sync::backups::file(&db, &backup_id)
        .await
        .map_err(super::sync::backup_refused)?;
    let bytes = tokio::fs::read(&path).await.map_err(store_failed)?;
    let name = path
        .file_name()
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "her.her-archive".into());
    Ok(sealed_file(format!("backup-{name}"), bytes))
}

/// DELETE /api/agent/host/backups/{backup_id}: the owner deletes a backup.
pub async fn delete_backup(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(backup_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    crate::services::persona_sync::backups::forget(&db, &backup_id)
        .await
        .map_err(super::sync::backup_refused)?;
    Ok(Json(device_list(&db, owner).await?))
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
    lease::reload(&db).await;
    tracing::info!(device = %removed.id, "[sync] device removed by the owner");
    Ok(Json(
        json!({ "device": device_json(&removed, crate::clock::now()) }),
    ))
}

/// DELETE /api/agent/devices/{device_id}/record: the owner forgets a device
/// already removed, so it leaves the list.
pub async fn forget_device(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(device_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    if !devices::forget(&db, owner, &device_id)
        .await
        .map_err(store_failed)?
    {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "No such removed device", "code": "device_not_found" })),
        )));
    }
    tracing::info!(device = %device_id, "[sync] removed device forgotten by the owner");
    Ok(Json(device_list(&db, owner).await?))
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

    async fn answer(result: Result<Json<Value>, HttpError>) -> Result<Value, (StatusCode, Value)> {
        match result {
            Ok(Json(value)) => Ok(value),
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

    async fn start(
        db: &DatabaseConnection,
        user: i32,
        kind: &str,
        device: &str,
    ) -> Result<Value, (StatusCode, Value)> {
        let request: StartMoveRequest =
            serde_json::from_value(json!({ "kind": kind, "device": device })).unwrap();
        answer(start_move(State(db.clone()), Extension(claims(user)), Json(request)).await).await
    }

    /// Where she lives, and a move of her, from the owner's session (an
    /// isolated schema on a real database; skipped without a test database):
    /// the device list says where she lives; a move the owner starts freezes
    /// her and can be called off; one a device asked for waits for the
    /// owner's confirmation, which nobody else can give.
    #[tokio::test]
    async fn the_owner_moves_her_from_the_site() {
        use sea_orm::ConnectionTrait;
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = fixture.db.clone();
        db.execute_unprepared(
            "INSERT INTO users (id, username, is_admin, is_owner) VALUES \
             (41, 'owner41', true, true), (42, 'guest42', false, false); \
             INSERT INTO agent_persona (id, name, updated_at) VALUES ('site', 'Merope', NOW())",
        )
        .await
        .unwrap();
        crate::services::principal::invalidate_site_owner_cache();
        let (mac, _) = devices::add(&db, 41, "Mac").await.unwrap();

        let list = answer(list_devices(State(db.clone()), Extension(claims(41))).await)
            .await
            .unwrap();
        assert_eq!(list["host"]["holder"], "site");
        assert_eq!(list["host"]["epoch"], 1);
        assert_eq!(list["host"]["state"], "live");
        assert!(list.get("parent").is_none());
        assert!(list["devices"][0].get("role").is_none());

        // Her living on a device opens with H4 (Q8): only the acceptance entry.
        let (status, body) = start(&db, 41, "move", &mac.id).await.unwrap_err();
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::CONFLICT, Some("device_host_later"))
        );
        let (status, body) = start(&db, 41, "merge", "dev_nope").await.unwrap_err();
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::NOT_FOUND, Some("device_not_found"))
        );
        let (status, _) = start(&db, 42, "merge", &mac.id).await.unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN);
        let frozen = start(&db, 41, "merge", &mac.id).await.unwrap();
        assert_eq!(frozen["host"]["state"], "frozen");
        assert_eq!(frozen["host"]["move"]["phase"], "frozen");
        assert_eq!(frozen["host"]["move"]["fromName"], "Mac");
        let id = frozen["host"]["move"]["id"].as_str().unwrap().to_string();
        let (status, body) = start(&db, 41, "merge", &mac.id).await.unwrap_err();
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::CONFLICT, Some("move_in_progress"))
        );
        let back = answer(cancel_move(State(db.clone()), Extension(claims(41)), Path(id)).await)
            .await
            .unwrap();
        assert_eq!(back["host"]["state"], "live");
        assert_eq!(back["host"]["move"], Value::Null);
        assert_eq!(back["host"]["epoch"], 1);

        // Asked by the device: only the owner confirms it.
        let proposed = lease::start(&db, lease::Kind::Merge, &mac.id, lease::Starter::Device)
            .await
            .unwrap();
        let id = proposed.moving.unwrap().id;
        let (status, _) = answer(
            confirm_move(
                State(db.clone()),
                Extension(claims(42)),
                Path(id.clone()),
                Default::default(),
            )
            .await,
        )
        .await
        .unwrap_err();
        assert_eq!(status, StatusCode::FORBIDDEN);
        assert_eq!(
            lease::read(&db).await.unwrap().unwrap().state,
            lease::State::Live
        );
        let confirmed = answer(
            confirm_move(
                State(db.clone()),
                Extension(claims(41)),
                Path(id.clone()),
                Default::default(),
            )
            .await,
        )
        .await
        .unwrap();
        assert_eq!(confirmed["host"]["state"], "frozen");
        assert_eq!(confirmed["host"]["move"]["by"], "device");
        let (status, body) = answer(
            confirm_move(
                State(db.clone()),
                Extension(claims(41)),
                Path(id.clone()),
                Default::default(),
            )
            .await,
        )
        .await
        .unwrap_err();
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::CONFLICT, Some("move_phase"))
        );
        answer(cancel_move(State(db.clone()), Extension(claims(41)), Path(id)).await)
            .await
            .unwrap();

        fixture.close().await;
    }
}
