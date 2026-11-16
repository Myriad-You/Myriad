//! Moving what she kept before her nodes onto them, for the site owner
//! (`merope::memory_nodes::migrate`, step S4). Site owner only.
//!
//! - `GET  /api/agent/memory/nodes/migration`: the latest runs, newest
//!   first: how each ended, what it counted and wrote, what asking the
//!   model was estimated to cost and cost, why it stopped.
//! - `POST /api/agent/memory/nodes/migration` `{dryRun?, replay?}`: a dry
//!   run (the default) counts and estimates at once and answers with its
//!   record; a real one (`dryRun: false`) moves, in the background, what
//!   needs no model, and with `replay: true` what does too, within the
//!   cap; it answers with its record's id. One at a time.
//!
//! Errors say what was refused by a code, never with what.

use axum::{Extension, Json, Router, extract::State, http::StatusCode, routing::get};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::services::agent::merope::memory_nodes::migrate::{self, Options};

/// Runs listed, at most.
const RUNS: u64 = 10;

fn refused(status: StatusCode, code: &str) -> HttpError {
    HttpError::from((status, Json(json!({ "error": code, "code": code }))))
}

fn failed(error: impl std::fmt::Display) -> HttpError {
    tracing::error!(%error, "[memory-nodes] moving her memory could not be started or read");
    refused(StatusCode::INTERNAL_SERVER_ERROR, "memory_migration_failed")
}

/// GET /api/agent/memory/nodes/migration
async fn runs(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    super::persona::require_site_owner(&claims, &db).await?;
    let runs = migrate::runs(&db, RUNS).await.map_err(failed)?;
    Ok(Json(json!({ "runs": runs })))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Start {
    #[serde(default)]
    dry_run: Option<bool>,
    #[serde(default)]
    replay: bool,
}

/// POST /api/agent/memory/nodes/migration
async fn start(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(start): Json<Start>,
) -> Result<(StatusCode, Json<Value>), HttpError> {
    super::persona::require_site_owner(&claims, &db).await?;
    let options = Options {
        dry_run: start.dry_run.unwrap_or(true),
        replay: start.replay,
    };
    if options.dry_run {
        let Some(report) = migrate::run(&db, options).await.map_err(failed)? else {
            return Err(refused(StatusCode::CONFLICT, "memory_migration_running"));
        };
        return Ok((StatusCode::OK, Json(json!({ "run": report }))));
    }
    crate::services::persona_sync::lease::refuse_edit_while_away()?;
    match migrate::start(&db, options).await.map_err(failed)? {
        Some(id) => Ok((StatusCode::ACCEPTED, Json(json!({ "id": id })))),
        None => Err(refused(StatusCode::CONFLICT, "memory_migration_running")),
    }
}

pub(super) fn routes(app_state: crate::state::AppState) -> Router<crate::state::AppState> {
    use axum::middleware::from_fn_with_state;
    let auth = || from_fn_with_state(app_state.clone(), crate::middleware::auth::auth_middleware);
    Router::<crate::state::AppState>::new().route(
        "/memory/nodes/migration",
        get(runs).post(start).route_layer(auth()),
    )
}
