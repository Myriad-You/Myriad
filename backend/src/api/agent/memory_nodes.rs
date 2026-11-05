//! The site owner's page of her nodes (decision N9): what she keeps in
//! their private talk, in her own and in each group she is in, and the few
//! things they may change there. Site owner only; another account's
//! private talk is never read here (`merope::memory_nodes::page`).
//!
//! - `GET  /api/agent/memory/nodes`: the switch, her name, each audience
//!   with its nodes.
//! - `GET  /api/agent/memory/nodes/one?audience=&node=`: one node.
//! - `POST /api/agent/memory/nodes/rename` `{audience, node, name}`
//! - `POST /api/agent/memory/nodes/forget` `{audience, node, row?}`: a node,
//!   or one entry on it.
//! - `POST /api/agent/memory/nodes/cancel` `{audience, node, intent}`
//! - `POST /api/agent/memory/nodes/heard` `{audience, node, row, answer}`
//!
//! Errors say what was refused by a code, never with what.

use std::time::Duration;

use axum::{
    Extension, Json, Router,
    extract::{Query, State},
    http::StatusCode,
    routing::{get, post},
};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::services::agent::merope::memory_nodes::{
    self,
    page::{self, AudienceLine},
    page_edit::{self, Answer, Refusal},
};

/// How long a group's name is waited for before its id stands in.
const GROUP_NAME_WAIT: Duration = Duration::from_secs(3);

fn refused(status: StatusCode, code: &str) -> HttpError {
    HttpError::from((status, Json(json!({ "error": code, "code": code }))))
}

fn failed(error: impl std::fmt::Display) -> HttpError {
    tracing::error!(%error, "[memory-nodes] the page could not be read or changed");
    refused(StatusCode::INTERNAL_SERVER_ERROR, "memory_nodes_failed")
}

fn refusal(refusal: Refusal) -> HttpError {
    match refusal {
        Refusal::NotFound => refused(StatusCode::NOT_FOUND, "node_not_found"),
        Refusal::No(code) => refused(StatusCode::CONFLICT, code),
        Refusal::Failed(error) => failed(error),
    }
}

/// The site owner, or refused (`site_owner_required`).
async fn owner(claims: &Claims, db: &DatabaseConnection) -> Result<i32, HttpError> {
    super::persona::require_site_owner(claims, db).await
}

/// The owner, for a change: refused while she is not on the site.
async fn editor(claims: &Claims, db: &DatabaseConnection) -> Result<i32, HttpError> {
    let owner = owner(claims, db).await?;
    crate::services::persona_sync::lease::refuse_edit_while_away()?;
    Ok(owner)
}

/// A group's name as its platform gives it, or none in time.
async fn group_name(id: &str) -> Option<String> {
    tokio::time::timeout(GROUP_NAME_WAIT, crate::services::channel_group::title(id))
        .await
        .ok()
        .flatten()
}

/// GET /api/agent/memory/nodes
pub(crate) async fn list(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let owner = owner(&claims, &db).await?;
    let audiences: Vec<AudienceLine> = page::audiences(&db, owner).await.map_err(failed)?;
    let names = futures::future::join_all(audiences.iter().map(|line| async move {
        match &line.group {
            Some(id) => group_name(id).await,
            None => None,
        }
    }))
    .await;
    let her = crate::services::agent::merope::get_persona(&db)
        .await
        .ok()
        .flatten()
        .map(|persona| persona.name.trim().to_string())
        .filter(|name| !name.is_empty());
    let audiences: Vec<Value> = audiences
        .into_iter()
        .zip(names)
        .map(|(line, name)| {
            let mut value = serde_json::to_value(&line).unwrap_or_default();
            value["groupName"] = json!(name);
            value
        })
        .collect();
    Ok(Json(json!({
        "mode": memory_nodes::mode().as_str(),
        "herName": her,
        "audiences": audiences,
    })))
}

#[derive(Debug, Deserialize)]
pub(crate) struct Which {
    audience: String,
    node: String,
}

/// GET /api/agent/memory/nodes/one?audience=&node=
pub(crate) async fn one(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Query(which): Query<Which>,
) -> Result<Json<Value>, HttpError> {
    let owner = owner(&claims, &db).await?;
    let node = page::node(&db, owner, &which.audience, &which.node)
        .await
        .map_err(failed)?
        .ok_or_else(|| refused(StatusCode::NOT_FOUND, "node_not_found"))?;
    Ok(Json(serde_json::to_value(node).unwrap_or_default()))
}

fn done() -> Json<Value> {
    Json(json!({ "success": true }))
}

#[derive(Debug, Deserialize)]
pub(crate) struct Rename {
    audience: String,
    node: String,
    name: String,
}

/// POST /api/agent/memory/nodes/rename
pub(crate) async fn rename(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Rename>,
) -> Result<Json<Value>, HttpError> {
    let owner = editor(&claims, &db).await?;
    page_edit::rename(&db, owner, &body.audience, &body.node, &body.name)
        .await
        .map_err(refusal)?;
    Ok(done())
}

#[derive(Debug, Deserialize)]
pub(crate) struct Forget {
    audience: String,
    node: String,
    /// One entry on it; none: the whole node.
    #[serde(default)]
    row: Option<String>,
}

/// POST /api/agent/memory/nodes/forget
pub(crate) async fn forget(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Forget>,
) -> Result<Json<Value>, HttpError> {
    let owner = editor(&claims, &db).await?;
    match &body.row {
        Some(row) => page_edit::forget_entry(&db, owner, &body.audience, &body.node, row).await,
        None => page_edit::forget_node(&db, owner, &body.audience, &body.node).await,
    }
    .map_err(refusal)?;
    Ok(done())
}

#[derive(Debug, Deserialize)]
pub(crate) struct Cancel {
    audience: String,
    node: String,
    intent: String,
}

/// POST /api/agent/memory/nodes/cancel
pub(crate) async fn cancel(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Cancel>,
) -> Result<Json<Value>, HttpError> {
    let owner = editor(&claims, &db).await?;
    page_edit::cancel_intent(&db, owner, &body.audience, &body.node, &body.intent)
        .await
        .map_err(refusal)?;
    Ok(done())
}

#[derive(Debug, Deserialize)]
pub(crate) struct Heard {
    audience: String,
    node: String,
    row: String,
    /// `confirmed` or `denied`.
    answer: String,
}

/// POST /api/agent/memory/nodes/heard
pub(crate) async fn heard(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(body): Json<Heard>,
) -> Result<Json<Value>, HttpError> {
    let owner = editor(&claims, &db).await?;
    let answer = match body.answer.as_str() {
        "confirmed" => Answer::Confirmed,
        "denied" => Answer::Denied,
        _ => return Err(refused(StatusCode::BAD_REQUEST, "bad_answer")),
    };
    page_edit::answer_heard(&db, owner, &body.audience, &body.node, &body.row, answer)
        .await
        .map_err(refusal)?;
    Ok(done())
}

/// The page's routes, each behind sign-in; the handlers check the owner.
pub(super) fn routes(app_state: crate::state::AppState) -> Router<crate::state::AppState> {
    use axum::middleware::from_fn_with_state;
    let auth = || from_fn_with_state(app_state.clone(), crate::middleware::auth::auth_middleware);
    Router::<crate::state::AppState>::new()
        .route("/memory/nodes", get(list).route_layer(auth()))
        .route("/memory/nodes/one", get(one).route_layer(auth()))
        .route("/memory/nodes/rename", post(rename).route_layer(auth()))
        .route("/memory/nodes/forget", post(forget).route_layer(auth()))
        .route("/memory/nodes/cancel", post(cancel).route_layer(auth()))
        .route("/memory/nodes/heard", post(heard).route_layer(auth()))
}
