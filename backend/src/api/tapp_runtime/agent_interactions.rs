//! Stateful Agent Interaction between Myriad Agent tasks and Tapp runtimes.
//!
//! Domain registry / state machine: [`crate::services::tapp_agent_interaction`].
//! This module owns Axum handlers, SSE shells, and create-executor install.

use std::{convert::Infallible, time::Duration};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Extension, Json,
};
use chrono::Utc;
use futures::Stream;
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::HttpError;
use crate::{
    middleware::auth::Claims,
    services::agent_interaction::AgentInteractionSnapshot,
    services::tapp_agent_interaction::{self, AgentInteractionError, InteractionRuntime},
};

use super::{
    common::{parse_user_id, resolve_accessible_tapp},
    RuntimeGrantContext,
};

type ApiError = HttpError;

fn interaction_http_error(err: AgentInteractionError) -> ApiError {
    let status =
        StatusCode::from_u16(err.status_hint()).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    HttpError::from((
        status,
        Json(json!({
            "error": err.message(),
            "code": err.code(),
        })),
    ))
}

fn runtime_from_grant(grant: &RuntimeGrantContext) -> InteractionRuntime {
    InteractionRuntime {
        subject_id: grant.subject_id(),
        owner_id: grant.owner_id(),
        tapp_id: grant.tapp_id().to_string(),
        runtime_id: grant.runtime_id().to_string(),
    }
}

struct StreamGuard {
    runtime_id: String,
}

impl Drop for StreamGuard {
    fn drop(&mut self) {
        let runtime_id = self.runtime_id.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                tapp_agent_interaction::clear_presence(&runtime_id).await;
            });
        }
    }
}

/// Start one local sweeper (PostgreSQL CAS-safe across replicas).
pub fn spawn_agent_interaction_expiry_worker(db: DatabaseConnection) {
    tapp_agent_interaction::spawn_expiry_worker(db);
}

pub async fn get_agent_interaction(
    State(db): State<DatabaseConnection>,
    runtime: RuntimeGrantContext,
    Path(interaction_id): Path<String>,
) -> Result<Json<AgentInteractionSnapshot>, ApiError> {
    let interaction = tapp_agent_interaction::load_interaction(&db, &interaction_id)
        .await
        .map_err(interaction_http_error)?;
    tapp_agent_interaction::authorize_stored(&interaction, &runtime_from_grant(&runtime))
        .map_err(interaction_http_error)?;
    Ok(Json(interaction.snapshot.clone()))
}

pub async fn accept_agent_interaction(
    State(db): State<DatabaseConnection>,
    runtime: RuntimeGrantContext,
    Path(interaction_id): Path<String>,
) -> Result<Json<AgentInteractionSnapshot>, ApiError> {
    let snapshot = tapp_agent_interaction::accept_interaction(
        &db,
        &runtime_from_grant(&runtime),
        &interaction_id,
    )
    .await
    .map_err(interaction_http_error)?;
    Ok(Json(snapshot))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SubmitResultRequest {
    data: Value,
    #[serde(default)]
    summary: Option<String>,
    idempotency_key: String,
}

pub async fn submit_agent_interaction_result(
    State(db): State<DatabaseConnection>,
    runtime: RuntimeGrantContext,
    Path(interaction_id): Path<String>,
    Json(request): Json<SubmitResultRequest>,
) -> Result<Json<AgentInteractionSnapshot>, ApiError> {
    let snapshot = tapp_agent_interaction::submit_result(
        &db,
        &runtime_from_grant(&runtime),
        &interaction_id,
        request.data,
        request.summary,
        request.idempotency_key,
    )
    .await
    .map_err(interaction_http_error)?;
    Ok(Json(snapshot))
}

#[derive(Debug, Deserialize)]
pub struct RejectInteractionRequest {
    reason: String,
}

pub async fn reject_agent_interaction(
    State(db): State<DatabaseConnection>,
    runtime: RuntimeGrantContext,
    Path(interaction_id): Path<String>,
    Json(request): Json<RejectInteractionRequest>,
) -> Result<Json<AgentInteractionSnapshot>, ApiError> {
    let snapshot = tapp_agent_interaction::reject_interaction(
        &db,
        &runtime_from_grant(&runtime),
        &interaction_id,
        request.reason,
    )
    .await
    .map_err(interaction_http_error)?;
    Ok(Json(snapshot))
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestIntentRequest {
    #[serde(rename = "type")]
    intent_type: String,
    #[serde(default)]
    params: Value,
    reason: String,
    host_confirmed: bool,
}

pub async fn request_agent_intent(
    State(db): State<DatabaseConnection>,
    runtime: RuntimeGrantContext,
    Path(interaction_id): Path<String>,
    Json(request): Json<RequestIntentRequest>,
) -> Result<Json<Value>, ApiError> {
    let authorized = tapp_agent_interaction::authorize_intent(
        &db,
        &runtime_from_grant(&runtime),
        &interaction_id,
        &request.intent_type,
        &request.params,
        &request.reason,
        request.host_confirmed,
    )
    .await
    .map_err(interaction_http_error)?;
    Ok(Json(json!({
        "intentId": authorized.intent_id,
        "interactionId": authorized.interaction_id,
        "type": authorized.intent_type,
        "status": "authorized",
        "hostConfirmed": true,
        "executed": false,
        "adapter": authorized.intent_type,
        "note": "The trusted host adapter may execute this one authorized operation"
    })))
}

pub async fn stream_agent_interactions(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime: RuntimeGrantContext,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let subject_id = parse_user_id(&claims)?;
    let tapp = resolve_accessible_tapp(&db, subject_id, runtime.tapp_id()).await?;
    let manifest = tapp_agent_interaction::parse_agent_manifest(&tapp.manifest)
        .map_err(interaction_http_error)?;
    if manifest.protocol_version != 2 {
        return Err(interaction_http_error(
            AgentInteractionError::ProtocolVersion,
        ));
    }
    let event_runtime = InteractionRuntime {
        subject_id,
        owner_id: runtime.owner_id(),
        tapp_id: runtime.tapp_id().to_string(),
        runtime_id: runtime.runtime_id().to_string(),
    };
    tapp_agent_interaction::open_stream(&db, &event_runtime, runtime.expires_at())
        .await
        .map_err(interaction_http_error)?;

    let guard = StreamGuard {
        runtime_id: runtime.runtime_id().to_string(),
    };
    let expires_in = (runtime.expires_at() - Utc::now().timestamp()).max(1) as u64;
    let interaction_types: Vec<String> = manifest
        .interactions
        .iter()
        .map(|value| value.interaction_type.clone())
        .collect();
    let runtime_id = runtime.runtime_id().to_string();
    let ready = json!({
        "runtimeId": runtime.runtime_id(),
        "interactions": interaction_types,
    });
    let stream = async_stream::stream! {
        let _guard = guard;
        yield Ok(Event::default().event("ready").json_data(ready).unwrap_or_default());
        let deadline = tokio::time::sleep(Duration::from_secs(expires_in));
        tokio::pin!(deadline);
        let mut poll = tokio::time::interval(Duration::from_millis(250));
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = &mut deadline => break,
                _ = poll.tick() => {
                    let interactions = tapp_agent_interaction::drain_stream(&db, &runtime_id).await;
                    for interaction in interactions {
                        yield Ok(Event::default().event("interaction").json_data(interaction).unwrap_or_default());
                    }
                }
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

pub(super) async fn disconnect_runtime_interactions(runtime_id: &str) -> bool {
    tapp_agent_interaction::disconnect_runtime_interactions(runtime_id).await
}
