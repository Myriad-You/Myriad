//! Manifest-scoped, online at-most-once event broker.
//!
//! Domain publish / presence / mailbox live in [`crate::services::tapp_events`].
//! This module owns permission checks, rate limits, ownership resolution, and
//! the SSE stream shell.

use std::{collections::HashSet, convert::Infallible, time::Duration};

use axum::{
    extract::State,
    http::StatusCode,
    response::sse::{Event, KeepAlive, Sse},
    Extension, Json,
};
use chrono::Utc;
use futures::Stream;
use sea_orm::DatabaseConnection;
use serde_json::{json, Value};

use crate::error::HttpError;
use crate::{
    middleware::auth::Claims,
    services::permission_service::TappPermission,
    services::tapp_events::{
        self, EventError, EventRuntime, PublishEventRequest,
    },
};

use super::{
    common::{authorize_tapp_permission, check_rate_limit, resolve_accessible_tapp},
    runtime_grant::RuntimeGrantContext,
};

// Preserve historical public type paths used by docs / clients (binary crate).
#[allow(unused_imports)]
pub use crate::services::tapp_events::{EventScope, EventSource, TappEventEnvelope};

type ApiError = HttpError;

fn event_http_error(err: EventError) -> ApiError {
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

fn runtime_from_grant(grant: &RuntimeGrantContext) -> EventRuntime {
    EventRuntime {
        runtime_id: grant.runtime_id().to_string(),
        tapp_id: grant.tapp_id().to_string(),
        owner_id: grant.owner_id(),
        subject_id: grant.subject_id(),
    }
}

struct SubscriptionGuard {
    runtime_id: String,
}

impl Drop for SubscriptionGuard {
    fn drop(&mut self) {
        let runtime_id = self.runtime_id.clone();
        if let Ok(handle) = tokio::runtime::Handle::try_current() {
            handle.spawn(async move {
                tapp_events::clear_subscription(&runtime_id).await;
            });
        }
    }
}

/// POST /api/tapp/events/publish
pub async fn publish_event(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime: RuntimeGrantContext,
    Json(request): Json<PublishEventRequest>,
) -> Result<Json<Value>, ApiError> {
    runtime.require(TappPermission::EventPublish)?;
    let user_id =
        authorize_tapp_permission(&db, &claims, runtime.tapp_id(), TappPermission::EventPublish, &dynamic_config).await?;

    let tapp = resolve_accessible_tapp(&db, user_id, runtime.tapp_id()).await?;
    let declaration = tapp_events::parse_event_manifest(&tapp.manifest).map_err(event_http_error)?;
    let allowed: HashSet<String> = declaration.publish.into_iter().collect();

    check_rate_limit(&db, user_id, runtime.tapp_id(), "event.publish").await?;

    let result = tapp_events::publish_event(&db, &runtime_from_grant(&runtime), request, &allowed)
        .await
        .map_err(event_http_error)?;

    Ok(Json(json!({
        "success": true,
        "accepted": result.accepted,
        "deduplicated": result.deduplicated,
        "delivered": result.delivered,
        "event": result.event,
    })))
}

/// GET /api/tapp/events/stream
pub async fn stream_events(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Extension(claims): Extension<Claims>,
    runtime: RuntimeGrantContext,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    runtime.require(TappPermission::EventSubscribe)?;
    let user_id = authorize_tapp_permission(
        &db,
        &claims,
        runtime.tapp_id(),
        TappPermission::EventSubscribe,
        &dynamic_config).await?;
    let tapp = resolve_accessible_tapp(&db, user_id, runtime.tapp_id()).await?;
    let declaration = tapp_events::parse_event_manifest(&tapp.manifest).map_err(event_http_error)?;
    let topics = declaration.subscribe.into_iter().collect::<HashSet<_>>();
    let event_runtime = runtime_from_grant(&runtime);

    tapp_events::register_subscription(&db, &event_runtime, topics.clone(), runtime.expires_at())
        .await
        .map_err(event_http_error)?;

    let guard = SubscriptionGuard {
        runtime_id: runtime.runtime_id().to_string(),
    };
    let expires_in = (runtime.expires_at() - Utc::now().timestamp()).max(1) as u64;
    let ready = json!({
        "runtimeId": runtime.runtime_id(),
        "tappId": runtime.tapp_id(),
        "topics": topics,
        "delivery": "online-at-most-once",
    });
    let runtime_id = runtime.runtime_id().to_string();
    let stream = async_stream::stream! {
        let _guard = guard;
        yield Ok(Event::default().event("ready").json_data(ready).unwrap_or_default());
        let deadline = tokio::time::sleep(Duration::from_secs(expires_in));
        tokio::pin!(deadline);
        let mut poll = tokio::time::interval(Duration::from_millis(250));
        poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = &mut deadline => {
                    yield Ok(Event::default().event("grant-expired").data("reconnect"));
                    break;
                }
                _ = poll.tick() => {
                    let events = tapp_events::drain_events(&db, &runtime_id).await;
                    for event in events {
                        yield Ok(Event::default().event("event").json_data(event).unwrap_or_default());
                    }
                }
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

pub(super) async fn disconnect_runtime_events(runtime_id: &str) -> bool {
    tapp_events::disconnect_runtime_events(runtime_id).await
}

pub(super) async fn disconnect_tapp_events(subject_id: i32, tapp_id: &str) -> usize {
    tapp_events::disconnect_tapp_events(subject_id, tapp_id).await
}

pub(super) async fn disconnect_all_tapp_events(tapp_id: &str) -> usize {
    tapp_events::disconnect_all_tapp_events(tapp_id).await
}
