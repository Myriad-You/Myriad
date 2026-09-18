//! Manifest-scoped, online at-most-once event broker.
//!
//! Events are transient notifications, not a data transport or task queue.
//! Cross-Tapp data bodies must use the consent-gated Data Exchange API.
//!
//! Domain lives in services. The API layer maps [`EventError`] to
//! Axum, enforces permissions/rate limits, and owns the SSE stream shell.

use std::collections::HashSet;

use chrono::Utc;
use myriad_tapp_contract::manifest::TappEventsManifest;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use uuid::Uuid;

use crate::services::tapp_registry::{self as shared_registry, RegistryIdentity};

const MAX_EVENT_TOPIC_BYTES: usize = 128;
const MAX_INSTANCE_PAYLOAD_BYTES: usize = 64 * 1024;
const MAX_OWNER_METADATA_BYTES: usize = 8 * 1024;
const MAX_DEDUPE_KEY_BYTES: usize = 128;
const DEDUPE_TTL_SECONDS: i64 = 30;
pub const EVENT_CHANNEL_CAPACITY: usize = 64;
const EVENT_PRESENCE_NAMESPACE: &str = "event_presence";
const EVENT_DEDUPE_NAMESPACE: &str = "event_dedupe";
const EVENT_MAILBOX_CHANNEL: &str = "event";

/// Runtime identity snapshot from a validated Runtime Grant.
#[derive(Debug, Clone)]
pub struct EventRuntime {
    pub runtime_id: String,
    pub tapp_id: String,
    pub owner_id: i32,
    pub subject_id: i32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum EventScope {
    Instance,
    Owner,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EventSource {
    pub tapp_id: String,
    pub runtime_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TappEventEnvelope {
    pub version: u8,
    pub event_id: String,
    pub topic: String,
    pub scope: EventScope,
    pub source: EventSource,
    pub payload: Value,
    pub occurred_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dedupe_key: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PublishEventRequest {
    pub topic: String,
    pub scope: EventScope,
    #[serde(default)]
    pub payload: Value,
    #[serde(default)]
    pub dedupe_key: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PublishEventResult {
    pub accepted: bool,
    pub deduplicated: bool,
    pub delivered: usize,
    pub event: TappEventEnvelope,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct OnlineSubscriber {
    subject_id: i32,
    owner_id: i32,
    tapp_id: String,
    topics: HashSet<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct DedupeRecord {
    request_hash: [u8; 32],
    event: TappEventEnvelope,
    expires_at: i64,
}

/// Domain errors for the event broker.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EventError {
    Unavailable { detail: UnavailableDetail },
    InvalidTopic,
    InvalidDedupeKey,
    GuestOwnerUnavailable,
    PayloadLimit,
    OwnerMetadataOnly,
    InvalidPayload,
    InvalidRequest,
    NotDeclared,
    InvalidManifest,
    TopicNotDeclared,
    DedupeKeyReused,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UnavailableDetail {
    Registry,
    Mailbox,
    Dedupe,
    Subscription,
}

impl EventError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable { .. } => "EVENT_REGISTRY_UNAVAILABLE",
            Self::InvalidTopic => "INVALID_EVENT_TOPIC",
            Self::InvalidDedupeKey => "INVALID_EVENT_DEDUPE_KEY",
            Self::GuestOwnerUnavailable => "GUEST_OWNER_EVENT_UNAVAILABLE",
            Self::PayloadLimit => "EVENT_PAYLOAD_LIMIT",
            Self::OwnerMetadataOnly => "OWNER_EVENT_METADATA_ONLY",
            Self::InvalidPayload => "INVALID_EVENT_PAYLOAD",
            Self::InvalidRequest => "INVALID_EVENT_REQUEST",
            Self::NotDeclared => "EVENT_V2_NOT_DECLARED",
            Self::InvalidManifest => "INVALID_EVENT_V2_MANIFEST",
            Self::TopicNotDeclared => "EVENT_TOPIC_NOT_DECLARED",
            Self::DedupeKeyReused => "EVENT_DEDUPE_KEY_REUSED",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Unavailable {
                detail: UnavailableDetail::Mailbox,
            } => "Event mailbox is unavailable".to_string(),
            Self::Unavailable {
                detail: UnavailableDetail::Dedupe,
            } => "Event deduplication registry is unavailable".to_string(),
            Self::Unavailable {
                detail: UnavailableDetail::Subscription,
            } => "Event subscription registry is unavailable".to_string(),
            Self::Unavailable { .. } => "Event registry is unavailable".to_string(),
            Self::InvalidTopic => {
                "Tapp publishers cannot publish system or invalid topics".to_string()
            }
            Self::InvalidDedupeKey => {
                "dedupeKey must use 1-128 safe ASCII characters".to_string()
            }
            Self::GuestOwnerUnavailable => {
                "Guest runtimes may only publish instance-scoped events".to_string()
            }
            Self::PayloadLimit => "Instance event payload exceeds 64 KiB".to_string(),
            Self::OwnerMetadataOnly => {
                "Owner events accept only bounded status metadata; use one-shot Data Exchange for cross-Tapp data".to_string()
            }
            Self::InvalidPayload => "Event payload cannot be serialized".to_string(),
            Self::InvalidRequest => "Event request cannot be serialized".to_string(),
            Self::NotDeclared => "Tapp manifest does not declare Event Broker".to_string(),
            Self::InvalidManifest => "Stored Tapp events declaration is invalid".to_string(),
            Self::TopicNotDeclared => {
                "Event publish topic is not declared by this Tapp".to_string()
            }
            Self::DedupeKeyReused => {
                "dedupeKey was already used for a different event".to_string()
            }
        }
    }

    pub fn status_hint(&self) -> u16 {
        match self {
            Self::Unavailable { .. } => 503,
            Self::InvalidTopic
            | Self::InvalidDedupeKey
            | Self::OwnerMetadataOnly
            | Self::InvalidPayload
            | Self::InvalidRequest => 400,
            Self::PayloadLimit => 413,
            Self::GuestOwnerUnavailable | Self::NotDeclared | Self::TopicNotDeclared => 403,
            Self::InvalidManifest => 422,
            Self::DedupeKeyReused => 409,
        }
    }
}

impl std::fmt::Display for EventError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for EventError {}

pub fn valid_topic(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_EVENT_TOPIC_BYTES
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

pub fn valid_dedupe_key(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_DEDUPE_KEY_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

pub(crate) fn dedupe_record_id(runtime_id: &str, key: &str) -> String {
    let mut digest = Sha256::new();
    digest.update(runtime_id.as_bytes());
    digest.update([0]);
    digest.update(key.as_bytes());
    hex::encode(digest.finalize())
}

pub fn parse_event_manifest(manifest: &Value) -> Result<TappEventsManifest, EventError> {
    let value = manifest
        .get("events")
        .cloned()
        .ok_or(EventError::NotDeclared)?;
    serde_json::from_value(value).map_err(|_| EventError::InvalidManifest)
}

fn payload_size(payload: &Value) -> Result<usize, EventError> {
    serde_json::to_vec(payload)
        .map(|value| value.len())
        .map_err(|_| EventError::InvalidPayload)
}

fn validate_owner_metadata(value: &Value, depth: usize, nodes: &mut usize) -> bool {
    if depth > 4 || *nodes > 64 {
        return false;
    }
    *nodes += 1;
    match value {
        Value::Null | Value::Bool(_) | Value::Number(_) => true,
        Value::String(value) => value.len() <= 1_024,
        Value::Array(values) => {
            values.len() <= 32
                && values
                    .iter()
                    .all(|value| validate_owner_metadata(value, depth + 1, nodes))
        }
        Value::Object(values) => {
            values.len() <= 32
                && values.iter().all(|(key, value)| {
                    !matches!(
                        key.to_ascii_lowercase().as_str(),
                        "data" | "content" | "records" | "items" | "body" | "blob" | "bytes"
                    ) && validate_owner_metadata(value, depth + 1, nodes)
                })
        }
    }
}

pub fn validate_payload(scope: EventScope, payload: &Value) -> Result<(), EventError> {
    let size = payload_size(payload)?;
    match scope {
        EventScope::Instance if size <= MAX_INSTANCE_PAYLOAD_BYTES => Ok(()),
        EventScope::Instance => Err(EventError::PayloadLimit),
        EventScope::Owner => {
            let mut nodes = 0;
            if size > MAX_OWNER_METADATA_BYTES
                || !payload.is_object()
                || !validate_owner_metadata(payload, 0, &mut nodes)
            {
                return Err(EventError::OwnerMetadataOnly);
            }
            Ok(())
        }
    }
}

pub fn subject_can_publish_scope(subject_id: i32, scope: EventScope) -> bool {
    subject_id >= 0 || scope == EventScope::Instance
}

fn request_hash(request: &PublishEventRequest) -> Result<[u8; 32], EventError> {
    serde_json::to_vec(request)
        .map(|encoded| Sha256::digest(encoded).into())
        .map_err(|_| EventError::InvalidRequest)
}

async fn deliver_event(
    db: &impl ConnectionTrait,
    runtime: &EventRuntime,
    event: &TappEventEnvelope,
) -> Result<usize, EventError> {
    let presences = shared_registry::list(
        db,
        EVENT_PRESENCE_NAMESPACE,
        match event.scope {
            EventScope::Instance => None,
            EventScope::Owner => Some(runtime.subject_id),
        },
        None,
    )
    .await
    .map_err(|_| EventError::Unavailable {
        detail: UnavailableDetail::Registry,
    })?;
    let mut delivered = 0usize;
    for presence in presences {
        let Ok(subscriber) = serde_json::from_value::<OnlineSubscriber>(presence.payload) else {
            continue;
        };
        let runtime_id = presence
            .runtime_id
            .as_deref()
            .unwrap_or(&presence.record_id);
        let addressed = subscriber.topics.contains(&event.topic)
            && match event.scope {
                EventScope::Instance => runtime_id == runtime.runtime_id,
                // `subject_id` is the current user's data-owner space. Using
                // installation owner_id here would leak shared admin-Tapp
                // events across ordinary users.
                EventScope::Owner => subscriber.subject_id == runtime.subject_id,
            };
        if !addressed {
            continue;
        }
        shared_registry::enqueue(
            db,
            EVENT_MAILBOX_CHANNEL,
            runtime_id,
            event,
            Utc::now().timestamp() + 30,
        )
        .await
        .map_err(|_| EventError::Unavailable {
            detail: UnavailableDetail::Mailbox,
        })?;
        delivered += 1;
    }
    Ok(delivered)
}

/// Publish after the API has authorized EventPublish + rate limit + resolved install.
///
/// `allowed_publish_topics` is the Tapp's declared `events.publish` list.
pub async fn publish_event(
    db: &DatabaseConnection,
    runtime: &EventRuntime,
    request: PublishEventRequest,
    allowed_publish_topics: &HashSet<String>,
) -> Result<PublishEventResult, EventError> {
    if !valid_topic(&request.topic) || request.topic.starts_with("system.") {
        return Err(EventError::InvalidTopic);
    }
    if request
        .dedupe_key
        .as_deref()
        .is_some_and(|value| !valid_dedupe_key(value))
    {
        return Err(EventError::InvalidDedupeKey);
    }
    if !subject_can_publish_scope(runtime.subject_id, request.scope) {
        return Err(EventError::GuestOwnerUnavailable);
    }
    validate_payload(request.scope, &request.payload)?;

    if !allowed_publish_topics.contains(&request.topic) {
        return Err(EventError::TopicNotDeclared);
    }

    let hash = request_hash(&request)?;
    let dedupe_scope = request
        .dedupe_key
        .as_ref()
        .map(|key| dedupe_record_id(&runtime.runtime_id, key));
    let event = TappEventEnvelope {
        version: 2,
        event_id: format!("evt_{}", Uuid::new_v4().simple()),
        topic: request.topic,
        scope: request.scope,
        source: EventSource {
            tapp_id: runtime.tapp_id.clone(),
            runtime_id: runtime.runtime_id.clone(),
        },
        payload: request.payload,
        occurred_at: Utc::now().to_rfc3339(),
        dedupe_key: request.dedupe_key,
    };

    let delivered = if let Some(dedupe_scope) = dedupe_scope {
        // Serialize one runtime/dedupe key across every backend replica. The
        // mailbox writes and dedupe record commit together, so a retry cannot
        // observe a half-published event.
        let txn = db.begin().await.map_err(|_| EventError::Unavailable {
            detail: UnavailableDetail::Registry,
        })?;
        txn.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            vec![format!("tapp-event-dedupe:{dedupe_scope}").into()],
        ))
        .await
        .map_err(|_| EventError::Unavailable {
            detail: UnavailableDetail::Registry,
        })?;
        if let Some(existing) =
            shared_registry::get::<DedupeRecord>(&txn, EVENT_DEDUPE_NAMESPACE, &dedupe_scope)
                .await
                .map_err(|_| EventError::Unavailable {
                    detail: UnavailableDetail::Registry,
                })?
        {
            txn.commit().await.ok();
            if existing.request_hash != hash {
                return Err(EventError::DedupeKeyReused);
            }
            return Ok(PublishEventResult {
                accepted: true,
                deduplicated: true,
                delivered: 0,
                event: existing.event,
            });
        }

        let delivered = deliver_event(&txn, runtime, &event).await?;
        let expires_at = Utc::now().timestamp() + DEDUPE_TTL_SECONDS;
        shared_registry::put(
            &txn,
            EVENT_DEDUPE_NAMESPACE,
            &dedupe_scope,
            RegistryIdentity {
                subject_id: Some(runtime.subject_id),
                owner_id: Some(runtime.owner_id),
                tapp_id: Some(runtime.tapp_id.as_str()),
                runtime_id: Some(runtime.runtime_id.as_str()),
            },
            &DedupeRecord {
                request_hash: hash,
                event: event.clone(),
                expires_at,
            },
            expires_at,
        )
        .await
        .map_err(|_| EventError::Unavailable {
            detail: UnavailableDetail::Dedupe,
        })?;
        txn.commit().await.map_err(|_| EventError::Unavailable {
            detail: UnavailableDetail::Registry,
        })?;
        delivered
    } else {
        deliver_event(db, runtime, &event).await?
    };

    Ok(PublishEventResult {
        accepted: true,
        deduplicated: false,
        delivered,
        event,
    })
}

/// Register online presence for a subscriber runtime (SSE stream setup).
pub async fn register_subscription(
    db: &DatabaseConnection,
    runtime: &EventRuntime,
    topics: HashSet<String>,
    expires_at: i64,
) -> Result<(), EventError> {
    let presence = OnlineSubscriber {
        subject_id: runtime.subject_id,
        owner_id: runtime.owner_id,
        tapp_id: runtime.tapp_id.clone(),
        topics,
    };
    shared_registry::put(
        db,
        EVENT_PRESENCE_NAMESPACE,
        &runtime.runtime_id,
        RegistryIdentity {
            subject_id: Some(runtime.subject_id),
            owner_id: Some(runtime.owner_id),
            tapp_id: Some(runtime.tapp_id.as_str()),
            runtime_id: Some(runtime.runtime_id.as_str()),
        },
        &presence,
        expires_at,
    )
    .await
    .map_err(|_| EventError::Unavailable {
        detail: UnavailableDetail::Subscription,
    })?;
    Ok(())
}

/// Drain pending mailbox events for a runtime (SSE poll).
pub async fn drain_events(db: &DatabaseConnection, runtime_id: &str) -> Vec<TappEventEnvelope> {
    shared_registry::drain::<TappEventEnvelope>(
        db,
        EVENT_MAILBOX_CHANNEL,
        runtime_id,
        EVENT_CHANNEL_CAPACITY as i64,
    )
    .await
    .unwrap_or_default()
}

/// Drop online presence when the SSE stream ends.
pub async fn clear_subscription(runtime_id: &str) {
    if let Ok(db) = shared_registry::database() {
        let _ = shared_registry::delete(&db, EVENT_PRESENCE_NAMESPACE, runtime_id).await;
    }
}

/// Disconnect one runtime's event presence (grant revoke).
pub async fn disconnect_runtime_events(runtime_id: &str) -> bool {
    match shared_registry::database() {
        Ok(db) => shared_registry::delete(&db, EVENT_PRESENCE_NAMESPACE, runtime_id)
            .await
            .unwrap_or(false),
        Err(_) => false,
    }
}

pub async fn disconnect_tapp_events(subject_id: i32, tapp_id: &str) -> usize {
    match shared_registry::database() {
        Ok(db) => shared_registry::delete_matching(
            &db,
            EVENT_PRESENCE_NAMESPACE,
            Some(subject_id),
            None,
            Some(tapp_id),
            None,
        )
        .await
        .unwrap_or(0) as usize,
        Err(_) => 0,
    }
}

pub async fn disconnect_all_tapp_events(owner_id: i32, tapp_id: &str) -> usize {
    match shared_registry::database() {
        Ok(db) => shared_registry::delete_matching(
            &db,
            EVENT_PRESENCE_NAMESPACE,
            None,
            Some(owner_id),
            Some(tapp_id),
            None,
        )
        .await
        .unwrap_or(0) as usize,
        Err(_) => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::{
        EventError, EventScope, dedupe_record_id, subject_can_publish_scope, valid_topic,
        validate_payload,
    };
    use serde_json::json;

    #[test]
    fn validates_namespaced_topics() {
        assert!(valid_topic("tapp.com.example.player.track.changed"));
        assert!(valid_topic("system.theme.changed"));
        assert!(!valid_topic("tapp..changed"));
        assert!(!valid_topic("tapp/bad"));
    }

    #[test]
    fn owner_events_reject_data_bodies() {
        assert!(
            validate_payload(
                EventScope::Owner,
                &json!({ "status": "changed", "revision": 3 })
            )
            .is_ok()
        );
        assert!(
            validate_payload(EventScope::Owner, &json!({ "items": [{ "secret": true }] })).is_err()
        );
    }

    #[test]
    fn guest_subjects_cannot_broadcast_owner_events() {
        assert!(subject_can_publish_scope(-42, EventScope::Instance));
        assert!(!subject_can_publish_scope(-42, EventScope::Owner));
        assert!(subject_can_publish_scope(42, EventScope::Owner));
    }

    #[test]
    fn event_dedupe_record_ids_are_bounded_and_runtime_scoped() {
        let key = "x".repeat(128);
        let id = dedupe_record_id("rt_0123456789abcdef0123456789abcdef", &key);

        assert_eq!(id.len(), 64);
        assert_eq!(
            id,
            dedupe_record_id("rt_0123456789abcdef0123456789abcdef", &key)
        );
        assert_ne!(id, dedupe_record_id("rt_other", &key));
    }

    #[test]
    fn error_codes_preserve_api_contract() {
        assert_eq!(
            EventError::Unavailable {
                detail: super::UnavailableDetail::Registry
            }
            .code(),
            "EVENT_REGISTRY_UNAVAILABLE"
        );
        assert_eq!(EventError::InvalidTopic.code(), "INVALID_EVENT_TOPIC");
        assert_eq!(
            EventError::GuestOwnerUnavailable.code(),
            "GUEST_OWNER_EVENT_UNAVAILABLE"
        );
        assert_eq!(EventError::PayloadLimit.code(), "EVENT_PAYLOAD_LIMIT");
        assert_eq!(
            EventError::OwnerMetadataOnly.code(),
            "OWNER_EVENT_METADATA_ONLY"
        );
        assert_eq!(EventError::NotDeclared.code(), "EVENT_V2_NOT_DECLARED");
        assert_eq!(
            EventError::TopicNotDeclared.code(),
            "EVENT_TOPIC_NOT_DECLARED"
        );
        assert_eq!(
            EventError::DedupeKeyReused.code(),
            "EVENT_DEDUPE_KEY_REUSED"
        );
    }

    #[test]
    fn status_hints_match_http_contract() {
        assert_eq!(
            EventError::Unavailable {
                detail: super::UnavailableDetail::Registry
            }
            .status_hint(),
            503
        );
        assert_eq!(EventError::PayloadLimit.status_hint(), 413);
        assert_eq!(EventError::DedupeKeyReused.status_hint(), 409);
        assert_eq!(EventError::InvalidManifest.status_hint(), 422);
        assert_eq!(EventError::TopicNotDeclared.status_hint(), 403);
    }
}
