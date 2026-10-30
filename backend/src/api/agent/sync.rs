//! What a device calls to sync her (`/api/agent/sync/*`), behind
//! `sync_device_auth`: who it is, what the site has and where she lives
//! (`hello`), reading her (`pull`, `assets`), asking for a move of her that
//! the owner then confirms on the site (`moves`), and leaving. `hello` needs
//! the device's clock (`X-Sync-Clock`); `leave` does not, so a device with a
//! wrong clock can still give its token back.

use super::*;
use crate::error::HttpError;
use crate::middleware::sync_device::{SyncClock, SyncDevice};
use crate::services::persona_sync::{self, devices, lease};
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
    let host = lease::shown(&db).await.map_err(sync_failed)?;
    let role = if host["holder"].as_str() == Some(device.0.id.as_str()) {
        "host"
    } else {
        "child"
    };
    let server_time = crate::clock::now_micros();
    let server_time_source = crate::services::server_clock::source();
    Ok(Json(json!({
        "formatVersion": myriad_persona_sync::FORMAT_VERSION,
        "deviceId": device.0.id,
        "lineage": host["lineage"],
        "empty": myriad_persona_sync::is_empty(&holdings),
        "persona": name.map(|name| json!({ "name": name })),
        "embeddingModel": embedding_model,
        // Where she lives (her host lease), null while the site has no her.
        "host": host,
        // This device's import of her into an empty site, if it asked (H3c).
        "import": persona_sync::import::shown_for_device(&db, &device.0.id)
            .await
            .map_err(sync_failed)?,
        // A Mac from before host leases reads this and needs it: a child,
        // unless this is the device she lives on (H3e-19).
        "role": role,
        "limits": {
            "pullMaxEntities": persona_sync::PULL_MAX_ENTITIES,
            "pullMaxBytes": persona_sync::PULL_MAX_BYTES,
            "assetMaxBytes": persona_sync::ASSET_MAX_BYTES,
            "clockMaxAheadMs": myriad_persona_sync::hlc::MAX_AHEAD_MS,
        },
        // The time a device's clock is compared with (microseconds), and
        // where it came from: `clock` (this host's), or `sntp`/`https` (real
        // time, by the site's trusted measurement of its own clock).
        "serverTime": server_time,
        "serverTimeSource": server_time_source,
        // The site's last check of its own clock, null while the check is
        // off or has not run yet (`services::server_clock`).
        "serverClock": crate::services::server_clock::shown(),
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
    lease::reload(&db).await;
    tracing::info!(device = %device.0.id, "[sync] device left");
    Ok(Json(json!({ "left": true })))
}

/// A move refused, with its code.
pub(crate) fn move_refused(error: lease::MoveError) -> HttpError {
    if let lease::MoveError::Db(error) = &error {
        tracing::error!(%error, "[host] could not change her host lease");
    }
    let (status, code, message) = error.refusal();
    let mut body = json!({ "error": message, "code": code });
    // Where the move stands, for a refusal about its step.
    if let lease::MoveError::Phase(phase) = &error {
        body["phase"] = json!(phase);
    }
    HttpError::from((status, Json(body)))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposeMoveRequest {
    kind: String,
}

/// POST /api/agent/sync/moves: the device asks for a move with itself:
/// `move` (she comes to live on it) or `merge` (its own her joins the
/// site's). Nothing happens to her until the site owner confirms it on the
/// site, logged in: a device token alone never moves her. Answers the lease.
pub async fn sync_propose_move(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Json(request): Json<ProposeMoveRequest>,
) -> Result<Json<Value>, HttpError> {
    // `back`: the device she lives on asks to bring her back to the site.
    if request.kind == "back" {
        lease::start_back(&db, &device.0.id, lease::Starter::Device)
            .await
            .map_err(move_refused)?;
    } else {
        let kind = match lease::Kind::parse(&request.kind) {
            Some(kind @ (lease::Kind::Move | lease::Kind::Merge)) => kind,
            _ => {
                return Err(bad_request(
                    "kind is move, back or merge",
                    "invalid_move_kind",
                ));
            }
        };
        // Her living on a device opens with H4 (Q8): the acceptance entry only.
        if kind == lease::Kind::Move && !lease::device_host_open() {
            return Err(move_refused(lease::MoveError::Later));
        }
        lease::start(&db, kind, &device.0.id, lease::Starter::Device)
            .await
            .map_err(move_refused)?;
    }
    lease::reload(&db).await;
    tracing::info!(device = %device.0.id, "[host] a device asked for a move; the owner confirms it on the site");
    Ok(Json(
        json!({ "host": lease::shown(&db).await.map_err(sync_failed)? }),
    ))
}

/// POST /api/agent/sync/moves/{move_id}/agree: the device a move to it is
/// with says yes (its owner agreed there, H3e-2). She is not frozen until
/// the owner confirms on the site.
pub async fn sync_agree_move(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Path(move_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    if !lease::device_host_open() {
        return Err(move_refused(lease::MoveError::Later));
    }
    lease::agree(&db, &move_id, &device.0.id)
        .await
        .map_err(move_refused)?;
    lease::reload(&db).await;
    Ok(Json(
        json!({ "host": lease::shown(&db).await.map_err(sync_failed)? }),
    ))
}

/// GET /api/agent/sync/moves/{move_id}: the move to this device as it
/// reads it: where it stands, and once she is sealed for it the file's size,
/// digest and the key of this move (sent to this device alone, kept in no
/// log; it goes with the move).
pub async fn sync_move_status(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Path(move_id): Path<String>,
) -> Result<axum::response::Response, HttpError> {
    use axum::http::header;
    use axum::response::IntoResponse;
    let answer = persona_sync::moves::shown_to_device(&db, &move_id, &device.0.id)
        .await
        .map_err(move_refused)?;
    Ok(([(header::CACHE_CONTROL, "no-store")], Json(answer)).into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PieceAt {
    offset: u64,
    #[serde(default)]
    length: Option<usize>,
}

/// GET /api/agent/sync/moves/{move_id}/archive?offset=&length=: a piece of
/// the sealed file of her this device takes.
pub async fn sync_move_archive(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Path(move_id): Path<String>,
    Query(at): Query<PieceAt>,
) -> Result<axum::response::Response, HttpError> {
    use axum::http::header;
    use axum::response::IntoResponse;
    let bytes = persona_sync::moves::piece(
        &db,
        &move_id,
        &device.0.id,
        at.offset,
        at.length.unwrap_or(persona_sync::moves::PIECE_MAX_BYTES),
    )
    .await
    .map_err(move_refused)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DigestRequest {
    digest: String,
}

/// POST /api/agent/sync/moves/{move_id}/applied: this device has all of her,
/// checked against the digest and kept apart; it pushes its first backup next.
pub async fn sync_move_applied(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Path(move_id): Path<String>,
    Json(request): Json<DigestRequest>,
) -> Result<Json<Value>, HttpError> {
    persona_sync::moves::applied(&db, &move_id, &device.0.id, &request.digest)
        .await
        .map_err(move_refused)?;
    lease::reload(&db).await;
    Ok(Json(
        json!({ "host": lease::shown(&db).await.map_err(sync_failed)? }),
    ))
}

/// POST /api/agent/sync/moves/{move_id}/commit: she lives on this device from
/// now; the site lets its copy go in the same transaction. Asked again, it
/// answers the lease. The device starts her only once this answered.
pub async fn sync_move_commit(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Path(move_id): Path<String>,
    Json(request): Json<DigestRequest>,
) -> Result<Json<Value>, HttpError> {
    persona_sync::moves::commit_to_device(
        &db,
        owner_of(&device),
        &move_id,
        &device.0.id,
        &request.digest,
    )
    .await
    .map_err(move_refused)?;
    Ok(Json(
        json!({ "host": lease::shown(&db).await.map_err(sync_failed)? }),
    ))
}

/// POST /api/agent/sync/moves/{move_id}/frozen: the device she lives on is
/// frozen for her move back and says what it froze; she comes as an import
/// it sends next (pages, files, commit).
pub async fn sync_move_frozen(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Path(move_id): Path<String>,
    Json(frozen): Json<persona_sync::moves::Frozen>,
) -> Result<Json<Value>, HttpError> {
    let session = persona_sync::moves::report_frozen(&db, &move_id, &device.0.id, frozen)
        .await
        .map_err(import_refused)?;
    Ok(Json(json!({
        "import": session.json(None),
        "host": lease::shown(&db).await.map_err(sync_failed)?,
    })))
}

/// A backup refused, with its code.
pub(crate) fn backup_refused(error: persona_sync::backups::BackupError) -> HttpError {
    match &error {
        persona_sync::backups::BackupError::Failed(what) => {
            tracing::error!(what, "[backup] failed")
        }
        persona_sync::backups::BackupError::Invalid(what) => {
            tracing::warn!(what, "[backup] refused: a piece that does not read")
        }
        _ => {}
    }
    let (status, code, message) = error.refusal();
    HttpError::from((status, Json(json!({ "error": message, "code": code }))))
}

/// PUT /api/agent/sync/backups/{day}?offset=&total=&lineage=&epoch=&digest=:
/// one piece of today's sealed backup from the device she lives on (D4).
pub async fn sync_put_backup(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Path(day): Path<String>,
    Query(query): Query<persona_sync::backups::PieceQuery>,
    body: axum::body::Bytes,
) -> Result<Json<Value>, HttpError> {
    let taken = persona_sync::backups::take_piece(&db, &device.0.id, &day, query, body.to_vec())
        .await
        .map_err(backup_refused)?;
    Ok(Json(taken))
}

/// GET /api/agent/sync/backups: her lineage's daily backups the site keeps,
/// for a device to take her back from one (opened in the app, Q5).
pub async fn sync_backups(
    State(db): State<DatabaseConnection>,
    Extension(_device): Extension<SyncDevice>,
) -> Result<Json<Value>, HttpError> {
    Ok(Json(json!({
        "backups": persona_sync::backups::listed(&db).await.map_err(sync_failed)?,
        "host": lease::shown(&db).await.map_err(sync_failed)?,
    })))
}

/// GET /api/agent/sync/backups/{backup_id}/file?offset=&length=: a piece of
/// one of them.
pub async fn sync_backup_piece(
    State(db): State<DatabaseConnection>,
    Extension(_device): Extension<SyncDevice>,
    Path(backup_id): Path<String>,
    Query(at): Query<PieceAt>,
) -> Result<axum::response::Response, HttpError> {
    use axum::http::header;
    use axum::response::IntoResponse;
    let bytes = persona_sync::backups::piece(
        &db,
        &backup_id,
        at.offset,
        at.length.unwrap_or(persona_sync::backups::PIECE_MAX_BYTES),
    )
    .await
    .map_err(backup_refused)?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CACHE_CONTROL, "no-store"),
        ],
        bytes,
    )
        .into_response())
}

/// POST /api/agent/sync/moves/{move_id}/cancel: the device calls off a move
/// with itself before its commit; she is where she was, as she was.
pub async fn sync_cancel_move(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Path(move_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    lease::cancel(&db, &move_id, Some(&device.0.id))
        .await
        .map_err(move_refused)?;
    lease::reload(&db).await;
    Ok(Json(
        json!({ "host": lease::shown(&db).await.map_err(sync_failed)? }),
    ))
}

/// An import refused, with its code (and her name when the site has her).
pub(crate) fn import_refused(error: persona_sync::import::ImportError) -> HttpError {
    use persona_sync::import::ImportError;
    match &error {
        ImportError::Failed(what) => tracing::error!(what, "[import] failed"),
        ImportError::Mismatch(what) => {
            tracing::warn!(what, "[import] refused: not what was confirmed")
        }
        ImportError::Invalid(what) => {
            tracing::warn!(what, "[import] refused: a request that does not read")
        }
        _ => {}
    }
    let (status, code, message) = error.refusal();
    let mut body = json!({ "error": message, "code": code });
    match &error {
        ImportError::NotEmpty(name) => body["name"] = json!(name),
        ImportError::Phase(phase) => body["phase"] = json!(phase),
        _ => {}
    }
    HttpError::from((status, Json(body)))
}

/// POST /api/agent/sync/imports: the device asks to bring the her it holds
/// into this site, which has none (H3c). The owner confirms it on the site,
/// logged in; nothing happens before.
pub async fn sync_propose_import(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Json(proposal): Json<persona_sync::import::Proposal>,
) -> Result<Json<Value>, HttpError> {
    let session = persona_sync::import::propose(&db, owner_of(&device), &device.0.id, proposal)
        .await
        .map_err(import_refused)?;
    Ok(Json(json!({ "import": session.json(None) })))
}

/// GET /api/agent/sync/imports/{import_id}: where this device's import
/// stands, and what has arrived (to go on after an interruption).
pub async fn sync_import_status(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Path(import_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let mut answer = persona_sync::import::received(&db, &import_id, &device.0.id)
        .await
        .map_err(import_refused)?;
    answer["host"] = lease::shown(&db).await.map_err(sync_failed)?;
    Ok(Json(answer))
}

/// POST /api/agent/sync/imports/{import_id}/pages: one page of her.
pub async fn sync_import_page(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Path(import_id): Path<String>,
    Json(envelope): Json<myriad_persona_sync::Envelope>,
) -> Result<Json<Value>, HttpError> {
    let staged = persona_sync::import::take_page(&db, &import_id, &device.0.id, envelope)
        .await
        .map_err(import_refused)?;
    Ok(Json(json!({ "staged": staged })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportAssetQuery {
    #[serde(default)]
    file: Option<String>,
}

/// PUT /api/agent/sync/imports/{import_id}/assets/{asset_id}: one file of
/// her, as itself (`?file=manifest.json|atlas.png` for a rig's two).
pub async fn sync_import_asset(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Path((import_id, asset_id)): Path<(String, String)>,
    Query(query): Query<ImportAssetQuery>,
    body: axum::body::Bytes,
) -> Result<Json<Value>, HttpError> {
    persona_sync::import::take_asset(
        &db,
        &import_id,
        &device.0.id,
        &asset_id,
        query.file.as_deref(),
        body.to_vec(),
    )
    .await
    .map_err(import_refused)?;
    Ok(Json(json!({ "kept": true })))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CommitImportRequest {
    digest: String,
}

/// The answer once she landed: where she lives now, what came, and her
/// switch (which the import leaves as it was).
pub(crate) fn landed_json(landed: &persona_sync::import::Landed, host: Value) -> Value {
    json!({
        "host": host,
        "counts": landed.counts,
        "vectors": landed.vectors,
        "herOn": landed.her_on,
    })
}

/// POST /api/agent/sync/imports/{import_id}/commit: all of her is sent;
/// checked against what the owner confirmed, she lands in one transaction.
pub async fn sync_commit_import(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Path(import_id): Path<String>,
    Json(request): Json<CommitImportRequest>,
) -> Result<Json<Value>, HttpError> {
    let landed = persona_sync::import::commit(
        &db,
        owner_of(&device),
        &import_id,
        &device.0.id,
        &request.digest,
    )
    .await
    .map_err(import_refused)?;
    let host = lease::shown(&db).await.map_err(sync_failed)?;
    Ok(Json(landed_json(&landed, host)))
}

/// POST /api/agent/sync/imports/{import_id}/cancel: the device calls its
/// import off before she landed; the site is as it was.
pub async fn sync_cancel_import(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Path(import_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    persona_sync::import::cancel(&db, &import_id, Some(&device.0.id))
        .await
        .map_err(import_refused)?;
    Ok(Json(json!({ "cancelled": true })))
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
    // The first comparison reads only what two sides compare; the rest of
    // her crosses when she moves.
    let page = page(
        &db,
        owner_of(&device),
        &myriad_persona_sync::Kind::COMPARED,
        after.as_ref(),
        &limits,
        shared.as_ref(),
    )
    .await
    .map_err(sync_failed)?;
    Ok(Json(json!({
        "lineage": lease::read(&db).await.map_err(sync_failed)?.map(|lease| lease.lineage),
        "entities": page.entities,
        "embeddings": page.embeddings,
        "assets": page.assets,
        "next": page.next.as_ref().map(Cursor::encode),
        "more": page.more,
    })))
}

/// GET /api/agent/sync/log/full/{link}: a new host core's first full sync
/// of one audience of her log, from the single-use link `log.full` gave it
/// (`services::her_log::full`). Not found while her log on the server is
/// off, and for a link that is not this device's, used or expired.
pub async fn sync_log_full(
    State(db): State<DatabaseConnection>,
    Extension(device): Extension<SyncDevice>,
    Path(link): Path<String>,
) -> Result<Json<Value>, HttpError> {
    use crate::services::her_log;
    let not_found = || {
        HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Not found", "code": "not_found" })),
        ))
    };
    if !her_log::enabled()
        || !her_log::store::account(&db, owner_of(&device))
            .await
            .map_err(sync_failed)?
            .is_some_and(|account| account.on())
    {
        return Err(not_found());
    }
    let now = her_log::now();
    her_log::full::redeem(
        &db,
        her_log::keys::root(),
        owner_of(&device),
        &device.0.id,
        &link,
        now,
    )
    .await
    .map_err(sync_failed)?
    .map(Json)
    .ok_or_else(not_found)
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
        // Nor does it confirm a move of her: that takes the owner, logged in.
        for path in [
            "/api/agent/host/moves/mv_x/confirm",
            "/api/agent/host/moves",
            "/api/agent/host/moves/mv_x/cancel",
        ] {
            let (status, _) = call_with(
                &app,
                "POST",
                path,
                Some(&token),
                &[],
                Some(serde_json::json!({ "kind": "move", "device": "dev_x" })),
            )
            .await;
            assert!(
                status == StatusCode::UNAUTHORIZED || status == StatusCode::INTERNAL_SERVER_ERROR,
                "{path}: {status}"
            );
        }

        crate::services::principal::invalidate_site_owner_cache();
        // The device's clock: required on hello, refused when far ahead.
        let clock_at = |offset_ms: i64| {
            let now_ms = crate::clock::now().timestamp_millis() + offset_ms;
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
        // No clock check has run in this test: the host's clock, and no
        // measurement.
        assert_eq!(hello["serverTimeSource"], "clock");
        assert!(hello["serverClock"].is_null(), "{hello}");
        assert_eq!(hello["formatVersion"], myriad_persona_sync::FORMAT_VERSION);
        assert_eq!(hello["empty"], true);
        assert_eq!(hello["persona"], Value::Null);
        // No her yet: no lease. A Mac from before leases still reads its role.
        assert_eq!(hello["host"], Value::Null);
        assert_eq!(hello["role"], "child");
        assert!(hello.get("parent").is_none());
        assert!(hello["limits"].get("pushMaxEntities").is_none());
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

        // The parent/child role is gone.
        let (status, _) = call_with(
            &app,
            "PUT",
            "/api/agent/sync/role",
            Some(&token),
            &clock_at(0),
            Some(serde_json::json!({ "role": "parent" })),
        )
        .await;
        assert!(
            matches!(
                status,
                StatusCode::NOT_FOUND | StatusCode::METHOD_NOT_ALLOWED
            ),
            "{status}"
        );
        // A move asks for a her to move: there is none yet.
        let (status, body) = call_with(
            &app,
            "POST",
            "/api/agent/sync/moves",
            Some(&token),
            &[],
            Some(serde_json::json!({ "kind": "merge" })),
        )
        .await;
        assert_eq!(
            (status, body["code"].as_str()),
            (StatusCode::CONFLICT, Some("move_no_her"))
        );

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
