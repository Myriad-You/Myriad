//! Where her one mind is, for the site owner (the mind lease,
//! `her_log::pen`): here, on another of their devices, or being taken back
//! here; and whether the owner syncs her (`her_log::store::start`/`stop`),
//! which only they turn on or off.

use axum::{Extension, Json, extract::State, http::StatusCode};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::services::persona_sync::lease::{self, SITE, Where};

/// GET /api/agent/mind/where: whether her mind is here, and when not, the
/// device it is on (as the owner named it). `catchingUp` while the site is
/// taking her back and reading what that device wrote first. `leaves` while
/// what the owner says to her here reaches her there (her log synced, D6-b).
pub async fn get_mind(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    Ok(Json(match lease::place() {
        Where::MindOn(holder) if holder == SITE => json!({ "here": false, "catchingUp": true }),
        Where::MindOn(holder) => {
            let name = lease::device_name(&db, &holder).await.ok().flatten();
            let leaves = crate::services::her_log::pen::in_play(
                &db,
                crate::services::her_log::settings(),
                Some(owner),
            )
            .await
            .unwrap_or(false);
            json!({ "here": false, "holder": holder, "holderName": name, "leaves": leaves })
        }
        _ => json!({ "here": true }),
    }))
}

fn sync_failed(error: impl std::fmt::Display) -> HttpError {
    tracing::error!(%error, "[her-log] syncing her could not be read or changed");
    HttpError(myriad_error::AppError::internal("Failed to change syncing"))
}

/// Whether the owner syncs her, as the settings page shows it: `available`
/// while her log on the server is on (`HER_LOG_ENABLED`), `on` while their
/// account syncs, `stoppedAt` when they last turned it off, and how many
/// days the site keeps her log after that.
async fn sync_shown(db: &DatabaseConnection, owner: i32) -> Result<Value, HttpError> {
    let settings = crate::services::her_log::current_settings();
    let account = crate::services::her_log::store::account(db, owner)
        .await
        .map_err(sync_failed)?;
    let available = crate::services::her_log::currently_enabled();
    Ok(json!({
        "available": available,
        "on": available && account.as_ref().is_some_and(|account| account.on()),
        "stoppedAt": account
            .as_ref()
            .and_then(|account| account.stopped_at)
            .map(|at| at.to_rfc3339()),
        "retentionDays": settings.retention_days,
    }))
}

/// GET /api/agent/mind/sync: whether the owner syncs her.
pub async fn get_sync(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    Ok(Json(sync_shown(&db, owner).await?))
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncRequest {
    on: bool,
}

/// PUT /api/agent/mind/sync `{on}`: the owner turns syncing her on or off.
/// Off, her mind comes back here from the device it is on (`lease::sync_off`)
/// and the site keeps her log for the retention period. Either way every
/// link says hello again, and its welcome tells it which.
pub async fn put_sync(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(request): Json<SyncRequest>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    let settings = crate::services::her_log::current_settings();
    if !crate::services::her_log::currently_enabled() {
        return Err(HttpError::from((
            StatusCode::CONFLICT,
            Json(json!({
                "error": "Syncing her is not open on this site",
                "code": "her_log_off",
            })),
        )));
    }
    if request.on {
        crate::services::her_log::store::start(&db, owner)
            .await
            .map_err(sync_failed)?;
        tracing::info!("[her-log] the owner turned syncing her on");
    } else {
        let now = crate::clock::now().timestamp_micros();
        let moved = crate::services::her_log::lease::sync_off(&db, &settings, owner, now)
            .await
            .map_err(sync_failed)?;
        // Where her mind is, read again here (the other processes hear the
        // grant from the database).
        crate::services::her_log::pen::settle_for(&db, &settings, Some(owner))
            .await
            .map_err(sync_failed)?;
        tracing::info!(
            took_back = moved.is_some(),
            "[her-log] the owner turned syncing her off"
        );
    }
    let devices = crate::services::persona_sync::devices::list(&db, owner)
        .await
        .map_err(sync_failed)?;
    let ids: Vec<String> = devices.into_iter().map(|device| device.id).collect();
    crate::services::agent::link::sync_changed(&ids);
    Ok(Json(sync_shown(&db, owner).await?))
}
