//! Stateful Agent Interaction between Myriad Agent tasks and Tapp runtimes.
//!
//! Registry, mailbox, expiry, and state transitions live here so Agent Executor
//! and runtime-grant teardown do not reach through `api::tapp_runtime`. The API
//! layer maps [`AgentInteractionError`] to Axum and owns SSE shells.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use chrono::Utc;
use myriad_tapp_contract::contract_rules::MAX_AGENT_SCHEMA_RESOURCE_BYTES;
use myriad_tapp_contract::manifest::{TappAgentInteractionDef, TappAgentManifest};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, FromQueryResult, Statement};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::models::entities::tapps;
use crate::services::agent_interaction::{
    AgentInteractionSnapshot, CreateAgentInteractionRequest, InteractionSource, InteractionState,
};
use crate::services::data_paths::paths;
use crate::services::json_schema_subset::{
    validate_inline_data_schema, validate_inline_json_value,
};
use crate::services::tapp_ownership;
use crate::services::tapp_registry::{self as shared_registry, RegistryIdentity};

const INTERACTION_TTL_SECONDS: i64 = 5 * 60;
const TERMINAL_RETENTION_SECONDS: i64 = 15 * 60;
const MAX_INTERACTIONS_PER_SUBJECT: usize = 64;
const MAX_INTERACTION_VALUE_BYTES: usize = 128 * 1024;
const MAX_REASON_BYTES: usize = 500;
const INTERACTION_NAMESPACE: &str = "agent_interaction";
const INTERACTION_PRESENCE_NAMESPACE: &str = "agent_presence";
const INTERACTION_MAILBOX_CHANNEL: &str = "agent_interaction_v2";
pub const HOST_INTENT_ADAPTERS: [&str; 3] = ["ui.open", "report.create", "dataExchange.request"];

/// Runtime identity for CAS updates / stream scope.
#[derive(Debug, Clone)]
pub struct InteractionRuntime {
    pub subject_id: i32,
    pub owner_id: i32,
    pub tapp_id: String,
    pub runtime_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StoredInteraction {
    pub subject_id: i32,
    pub owner_id: i32,
    pub snapshot: AgentInteractionSnapshot,
    pub accepted_runtime_id: Option<String>,
    pub result_schema: Option<Value>,
    pub intents: Vec<String>,
    pub result_idempotency_key: Option<String>,
    pub deadline_at: i64,
    pub retain_until: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct InteractionStream {
    subject_id: i32,
    owner_id: i32,
    tapp_id: String,
}

/// Domain errors (HTTP maps via status_hint + code).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentInteractionError {
    Unavailable { presence: bool },
    NotFound,
    NotFoundScoped,
    SerializationFailed,
    SchemaReadFailed,
    InvalidSchema { message: String },
    InvalidValue,
    InputTooLarge,
    ResultInvalid,
    InvalidRejection,
    IntentConfirmationRequired,
    IntentAdapterUnavailable,
    IntentNotAllowed,
    NotDeclared,
    InvalidManifest,
    ProtocolVersion,
    TypeNotDeclared { interaction_type: String },
    InputSchemaMismatch { message: String },
    ResultSchemaMismatch { message: String },
    TooMany,
    StateConflict { message: &'static str },
    RuntimeMismatch,
}

impl AgentInteractionError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable { presence: true } => "AGENT_REGISTRY_UNAVAILABLE",
            Self::Unavailable { .. } => "AGENT_REGISTRY_UNAVAILABLE",
            Self::NotFound | Self::NotFoundScoped => "AGENT_INTERACTION_NOT_FOUND",
            Self::SerializationFailed => "AGENT_INTERACTION_SERIALIZATION_FAILED",
            Self::SchemaReadFailed => "AGENT_SCHEMA_READ_FAILED",
            Self::InvalidSchema { .. } => "INVALID_AGENT_SCHEMA",
            Self::InvalidValue => "INVALID_AGENT_INTERACTION_VALUE",
            Self::InputTooLarge | Self::ResultInvalid => "INVALID_AGENT_INTERACTION_RESULT",
            Self::InvalidRejection => "INVALID_AGENT_REJECTION",
            Self::IntentConfirmationRequired => "AGENT_INTENT_CONFIRMATION_REQUIRED",
            Self::IntentAdapterUnavailable => "AGENT_INTENT_ADAPTER_UNAVAILABLE",
            Self::IntentNotAllowed => "AGENT_INTENT_NOT_ALLOWED",
            Self::NotDeclared => "AGENT_V2_NOT_DECLARED",
            Self::InvalidManifest | Self::ProtocolVersion => "INVALID_AGENT_V2_MANIFEST",
            Self::TypeNotDeclared { .. } => "AGENT_INTERACTION_TYPE_NOT_DECLARED",
            Self::InputSchemaMismatch { .. } => "AGENT_INPUT_SCHEMA_MISMATCH",
            Self::ResultSchemaMismatch { .. } => "AGENT_RESULT_SCHEMA_MISMATCH",
            Self::TooMany => "AGENT_INTERACTION_LIMIT",
            Self::StateConflict { .. } => "AGENT_INTERACTION_STATE_CONFLICT",
            Self::RuntimeMismatch => "AGENT_INTERACTION_RUNTIME_MISMATCH",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Unavailable { presence: true } => {
                "Agent presence registry is unavailable".to_string()
            }
            Self::Unavailable { .. } => "Agent interaction registry is unavailable".to_string(),
            Self::NotFound => "Agent interaction was not found or expired".to_string(),
            Self::NotFoundScoped => "Agent interaction was not found".to_string(),
            Self::SerializationFailed => "Agent interaction could not be serialized".to_string(),
            Self::SchemaReadFailed => "Declared Agent schema could not be read".to_string(),
            Self::InvalidSchema { message } => message.clone(),
            Self::InvalidValue => "Agent interaction value cannot be serialized".to_string(),
            Self::InputTooLarge => "Agent interaction input exceeds 128 KiB".to_string(),
            Self::ResultInvalid => "Agent interaction result is invalid or too large".to_string(),
            Self::InvalidRejection => "Rejection reason must contain 1-500 characters".to_string(),
            Self::IntentConfirmationRequired => {
                "Agent intent requires bounded params, reason, and host confirmation".to_string()
            }
            Self::IntentAdapterUnavailable => {
                "No trusted host adapter is registered for this intent type".to_string()
            }
            Self::IntentNotAllowed => {
                "Agent intent is not declared or interaction state does not allow it".to_string()
            }
            Self::NotDeclared => "Tapp manifest does not declare Agent Interaction".to_string(),
            Self::InvalidManifest => "Stored Tapp Agent declaration is invalid".to_string(),
            Self::ProtocolVersion => "Agent protocolVersion must be 2".to_string(),
            Self::TypeNotDeclared { interaction_type } => {
                format!("Tapp does not declare Agent interaction type {interaction_type}")
            }
            Self::InputSchemaMismatch { message } => {
                format!("Agent interaction input schema mismatch: {message}")
            }
            Self::ResultSchemaMismatch { message } => message.clone(),
            Self::TooMany => "Too many retained Agent interactions".to_string(),
            Self::StateConflict { message } => (*message).to_string(),
            Self::RuntimeMismatch => {
                "Only the runtime that accepted this interaction may submit a result".to_string()
            }
        }
    }

    pub fn status_hint(&self) -> u16 {
        match self {
            Self::Unavailable { .. } => 503,
            Self::SerializationFailed => 500,
            Self::NotFound | Self::NotFoundScoped => 404,
            Self::SchemaReadFailed
            | Self::InvalidSchema { .. }
            | Self::InvalidManifest
            | Self::ProtocolVersion
            | Self::ResultSchemaMismatch { .. } => 422,
            Self::InvalidValue
            | Self::InputTooLarge
            | Self::ResultInvalid
            | Self::InvalidRejection
            | Self::IntentConfirmationRequired
            | Self::IntentAdapterUnavailable => 400,
            Self::IntentNotAllowed | Self::NotDeclared | Self::RuntimeMismatch => 403,
            Self::TypeNotDeclared { .. } | Self::InputSchemaMismatch { .. } => 403,
            Self::TooMany => 429,
            Self::StateConflict { .. } => 409,
        }
    }

    /// String surface for Agent Executor create path.
    pub fn agent_message(&self) -> String {
        match self {
            Self::Unavailable { .. } => {
                format!("Agent interaction registry unavailable: {}", self.message())
            }
            other => other.message(),
        }
    }
}

impl std::fmt::Display for AgentInteractionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for AgentInteractionError {}

pub fn parse_agent_manifest(manifest: &Value) -> Result<TappAgentManifest, AgentInteractionError> {
    let value = manifest
        .get("agent")
        .cloned()
        .ok_or(AgentInteractionError::NotDeclared)?;
    serde_json::from_value(value).map_err(|_| AgentInteractionError::InvalidManifest)
}

pub fn same_interaction_scope(
    stream_subject_id: i32,
    stream_owner_id: i32,
    stream_tapp_id: &str,
    interaction_subject_id: i32,
    interaction_owner_id: i32,
    interaction_tapp_id: &str,
) -> bool {
    stream_subject_id == interaction_subject_id
        && stream_owner_id == interaction_owner_id
        && stream_tapp_id == interaction_tapp_id
}

fn value_size(value: &Value) -> Result<usize, AgentInteractionError> {
    serde_json::to_vec(value)
        .map(|value| value.len())
        .map_err(|_| AgentInteractionError::InvalidValue)
}

fn safe_resource_path(tapp_dir: &Path, relative: &str) -> Option<PathBuf> {
    if relative.is_empty() || relative.starts_with('/') || relative.starts_with('\\') {
        return None;
    }
    let candidate = Path::new(relative);
    if candidate.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(tapp_dir.join(relative))
}

fn installed_tapp_dir(owner_id: i32, tapp_id: &str) -> PathBuf {
    paths().tapp_user_dir(owner_id).join(tapp_id)
}

async fn read_schema(
    owner_id: i32,
    tapp_id: &str,
    relative: Option<&str>,
) -> Result<Option<Value>, AgentInteractionError> {
    let Some(relative) = relative else {
        return Ok(None);
    };
    let root = installed_tapp_dir(owner_id, tapp_id);
    let path = safe_resource_path(&root, relative).ok_or_else(|| {
        AgentInteractionError::InvalidSchema {
            message: "Agent schema path is invalid".to_string(),
        }
    })?;
    let bytes = tokio::fs::read(&path)
        .await
        .map_err(|_| AgentInteractionError::SchemaReadFailed)?;
    if bytes.len() > MAX_AGENT_SCHEMA_RESOURCE_BYTES {
        return Err(AgentInteractionError::InvalidSchema {
            message: format!("Agent schema exceeds {MAX_AGENT_SCHEMA_RESOURCE_BYTES} bytes"),
        });
    }
    let schema: Value =
        serde_json::from_slice(&bytes).map_err(|_| AgentInteractionError::InvalidSchema {
            message: "Agent schema is not valid JSON".to_string(),
        })?;
    validate_inline_data_schema(&schema)
        .map_err(|error| AgentInteractionError::InvalidSchema { message: error })?;
    Ok(Some(schema))
}

async fn load_interaction_raw(
    db: &DatabaseConnection,
    interaction_id: &str,
) -> Result<StoredInteraction, AgentInteractionError> {
    shared_registry::get(db, INTERACTION_NAMESPACE, interaction_id)
        .await
        .map_err(|_| AgentInteractionError::Unavailable { presence: false })?
        .ok_or(AgentInteractionError::NotFound)
}

async fn expire_interaction_if_due(
    db: &DatabaseConnection,
    interaction: &StoredInteraction,
) -> Result<Option<StoredInteraction>, AgentInteractionError> {
    if interaction.snapshot.state.terminal() || interaction.deadline_at > Utc::now().timestamp() {
        return Ok(None);
    }

    let mut expired = interaction.clone();
    expired.snapshot.state = InteractionState::Expired;
    expired.snapshot.rejection_reason = Some("Agent interaction expired".to_string());
    expired.snapshot.updated_at = Utc::now().to_rfc3339();
    expired.retain_until = Utc::now().timestamp() + TERMINAL_RETENTION_SECONDS;
    let payload =
        serde_json::to_value(&expired).map_err(|_| AgentInteractionError::SerializationFailed)?;
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE tapp_runtime_registry
SET payload = $1, expires_at = $2, updated_at = NOW()
WHERE namespace = $3 AND record_id = $4
  AND payload #>> '{snapshot,state}' IN ('pending', 'accepted')
  AND (payload ->> 'deadline_at')::BIGINT <= EXTRACT(EPOCH FROM NOW())::BIGINT
"#,
            vec![
                payload.into(),
                expired.retain_until.into(),
                INTERACTION_NAMESPACE.into(),
                expired.snapshot.interaction_id.clone().into(),
            ],
        ))
        .await
        .map_err(|_| AgentInteractionError::Unavailable { presence: false })?;
    if result.rows_affected() == 0 {
        return Ok(None);
    }
    resume_agent_task(db, &expired).await;
    Ok(Some(expired))
}

pub async fn load_interaction(
    db: &DatabaseConnection,
    interaction_id: &str,
) -> Result<StoredInteraction, AgentInteractionError> {
    let interaction = load_interaction_raw(db, interaction_id).await?;
    if let Some(expired) = expire_interaction_if_due(db, &interaction).await? {
        return Ok(expired);
    }
    // A different replica may have won the expiry CAS. Reload so this request
    // never acts on the stale pending/accepted snapshot.
    if !interaction.snapshot.state.terminal() && interaction.deadline_at <= Utc::now().timestamp() {
        return load_interaction_raw(db, interaction_id).await;
    }
    Ok(interaction)
}

async fn expire_due_interactions(db: &DatabaseConnection) -> Result<usize, AgentInteractionError> {
    let rows = shared_registry::RegistryRow::find_by_statement(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
SELECT record_id, runtime_id, payload
FROM tapp_runtime_registry
WHERE namespace = $1
  AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT
  AND payload #>> '{snapshot,state}' IN ('pending', 'accepted')
  AND (payload ->> 'deadline_at')::BIGINT <= EXTRACT(EPOCH FROM NOW())::BIGINT
ORDER BY updated_at ASC
LIMIT 128
"#,
        vec![INTERACTION_NAMESPACE.into()],
    ))
    .all(db)
    .await
    .map_err(|_| AgentInteractionError::Unavailable { presence: false })?;
    let mut expired = 0usize;
    for row in rows {
        let Ok(interaction) = serde_json::from_value::<StoredInteraction>(row.payload) else {
            tracing::warn!(
                interaction_id = %row.record_id,
                "[TAPP] Ignoring invalid Agent interaction registry payload"
            );
            continue;
        };
        if expire_interaction_if_due(db, &interaction).await?.is_some() {
            expired += 1;
        }
    }
    Ok(expired)
}

/// Start one local sweeper. PostgreSQL CAS makes it safe for every backend
/// replica to run the worker; only the winner resumes a given Agent task.
pub fn spawn_expiry_worker(db: DatabaseConnection) {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(5));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            match expire_due_interactions(&db).await {
                Ok(count) if count > 0 => {
                    tracing::info!(count, "[TAPP] Expired Agent interactions resumed")
                }
                Ok(_) => {}
                Err(error) => tracing::warn!(
                    error = %error.message(),
                    "[TAPP] Agent interaction expiry sweep failed"
                ),
            }
        }
    });
}

async fn conditional_save_interaction(
    db: &DatabaseConnection,
    interaction: &StoredInteraction,
    runtime: &InteractionRuntime,
    allow_pending: bool,
) -> Result<bool, AgentInteractionError> {
    let payload = serde_json::to_value(interaction)
        .map_err(|_| AgentInteractionError::SerializationFailed)?;
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE tapp_runtime_registry
SET payload = $1, runtime_id = $2, expires_at = $3, updated_at = NOW()
WHERE namespace = $4 AND record_id = $5
  AND subject_id = $6 AND owner_id = $7 AND tapp_id = $8
  AND (payload ->> 'deadline_at')::BIGINT > EXTRACT(EPOCH FROM NOW())::BIGINT
  AND (
    ($10::BOOLEAN AND payload #>> '{snapshot,state}' = 'pending')
    OR (
      payload #>> '{snapshot,state}' = 'accepted'
      AND payload ->> 'accepted_runtime_id' = $9
    )
  )
"#,
            vec![
                payload.into(),
                interaction.accepted_runtime_id.clone().into(),
                interaction.retain_until.into(),
                INTERACTION_NAMESPACE.into(),
                interaction.snapshot.interaction_id.clone().into(),
                runtime.subject_id.into(),
                runtime.owner_id.into(),
                runtime.tapp_id.clone().into(),
                runtime.runtime_id.clone().into(),
                allow_pending.into(),
            ],
        ))
        .await
        .map_err(|_| AgentInteractionError::Unavailable { presence: false })?;
    Ok(result.rows_affected() == 1)
}

async fn cancel_disconnected_interaction(
    db: &DatabaseConnection,
    interaction: &StoredInteraction,
    runtime_id: &str,
) -> Result<bool, AgentInteractionError> {
    let payload = serde_json::to_value(interaction)
        .map_err(|_| AgentInteractionError::SerializationFailed)?;
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
UPDATE tapp_runtime_registry
SET payload = $1, expires_at = $2, updated_at = NOW()
WHERE namespace = $3 AND record_id = $4
  AND payload #>> '{snapshot,state}' = 'accepted'
  AND payload ->> 'accepted_runtime_id' = $5
"#,
            vec![
                payload.into(),
                interaction.retain_until.into(),
                INTERACTION_NAMESPACE.into(),
                interaction.snapshot.interaction_id.clone().into(),
                runtime_id.into(),
            ],
        ))
        .await
        .map_err(|_| AgentInteractionError::Unavailable { presence: false })?;
    Ok(result.rows_affected() == 1)
}

async fn notify_pending(
    db: &DatabaseConnection,
    snapshot: &AgentInteractionSnapshot,
    subject_id: i32,
    owner_id: i32,
) -> Result<(), AgentInteractionError> {
    let streams = shared_registry::list(
        db,
        INTERACTION_PRESENCE_NAMESPACE,
        Some(subject_id),
        Some(&snapshot.tapp_id),
    )
    .await
    .map_err(|_| AgentInteractionError::Unavailable { presence: true })?;
    for row in streams {
        let Ok(stream) = serde_json::from_value::<InteractionStream>(row.payload) else {
            continue;
        };
        if !same_interaction_scope(
            stream.subject_id,
            stream.owner_id,
            &stream.tapp_id,
            subject_id,
            owner_id,
            &snapshot.tapp_id,
        ) {
            continue;
        }
        let runtime_id = row.runtime_id.as_deref().unwrap_or(&row.record_id);
        shared_registry::enqueue(
            db,
            INTERACTION_MAILBOX_CHANNEL,
            runtime_id,
            snapshot,
            Utc::now().timestamp() + INTERACTION_TTL_SECONDS,
        )
        .await
        .map_err(|_| AgentInteractionError::Unavailable { presence: false })?;
    }
    Ok(())
}

/// Create from trusted Agent Executor path (ownership resolved via services).
pub async fn create_from_agent(
    db: &DatabaseConnection,
    request: CreateAgentInteractionRequest,
) -> Result<AgentInteractionSnapshot, AgentInteractionError> {
    let CreateAgentInteractionRequest {
        subject_id,
        tapp_id,
        interaction_type,
        input,
        task_id,
    } = request;
    if value_size(&input)? > MAX_INTERACTION_VALUE_BYTES {
        return Err(AgentInteractionError::InputTooLarge);
    }
    let tapp = tapp_ownership::resolve_accessible_tapp(db, subject_id, &tapp_id)
        .await
        .map_err(|_| AgentInteractionError::NotFoundScoped)?;
    create_with_tapp(db, subject_id, &tapp, interaction_type, input, task_id).await
}

async fn create_with_tapp(
    db: &DatabaseConnection,
    subject_id: i32,
    tapp: &tapps::Model,
    interaction_type: String,
    input: Value,
    task_id: Option<String>,
) -> Result<AgentInteractionSnapshot, AgentInteractionError> {
    let manifest = parse_agent_manifest(&tapp.manifest)?;
    let definition: TappAgentInteractionDef = manifest
        .interactions
        .into_iter()
        .find(|definition| definition.interaction_type == interaction_type)
        .ok_or_else(|| AgentInteractionError::TypeNotDeclared {
            interaction_type: interaction_type.clone(),
        })?;
    let input_schema = read_schema(
        tapp.user_id,
        &tapp.tapp_id,
        definition.input_schema.as_deref(),
    )
    .await?;
    if let Some(schema) = &input_schema {
        validate_inline_json_value(schema, &input)
            .map_err(|error| AgentInteractionError::InputSchemaMismatch { message: error })?;
    }
    let result_schema = read_schema(
        tapp.user_id,
        &tapp.tapp_id,
        definition.result_schema.as_deref(),
    )
    .await?;

    let now = Utc::now();
    let deadline_at = now.timestamp() + INTERACTION_TTL_SECONDS;
    let snapshot = AgentInteractionSnapshot {
        version: 2,
        interaction_id: format!("agi_{}", Uuid::new_v4().simple()),
        interaction_type,
        tapp_id: tapp.tapp_id.clone(),
        state: InteractionState::Pending,
        input,
        input_schema: definition.input_schema,
        result_schema: definition.result_schema,
        deadline: (now + chrono::Duration::seconds(INTERACTION_TTL_SECONDS)).to_rfc3339(),
        source: InteractionSource {
            agent_id: "myriad.agent".to_string(),
            task_id,
        },
        created_at: now.to_rfc3339(),
        updated_at: now.to_rfc3339(),
        result: None,
        rejection_reason: None,
    };
    let stored = StoredInteraction {
        subject_id,
        owner_id: tapp.user_id,
        snapshot: snapshot.clone(),
        accepted_runtime_id: None,
        result_schema,
        intents: manifest.intents,
        result_idempotency_key: None,
        deadline_at,
        // Keep non-terminal rows beyond their action deadline so the expiry
        // worker can transition them and resume the waiting Agent task.
        retain_until: deadline_at + TERMINAL_RETENTION_SECONDS,
    };
    let inserted = shared_registry::put_with_subject_limit(
        db,
        INTERACTION_NAMESPACE,
        &stored.snapshot.interaction_id,
        RegistryIdentity {
            subject_id: Some(stored.subject_id),
            owner_id: Some(stored.owner_id),
            tapp_id: Some(stored.snapshot.tapp_id.as_str()),
            runtime_id: None,
        },
        &stored,
        stored.retain_until,
        MAX_INTERACTIONS_PER_SUBJECT,
    )
    .await
    .map_err(|_| AgentInteractionError::Unavailable { presence: false })?;
    if !inserted {
        return Err(AgentInteractionError::TooMany);
    }
    notify_pending(db, &snapshot, subject_id, tapp.user_id).await?;
    Ok(snapshot)
}

pub fn authorize_stored<'a>(
    interaction: &'a StoredInteraction,
    runtime: &InteractionRuntime,
) -> Result<&'a StoredInteraction, AgentInteractionError> {
    if interaction.subject_id != runtime.subject_id
        || interaction.owner_id != runtime.owner_id
        || interaction.snapshot.tapp_id != runtime.tapp_id
    {
        return Err(AgentInteractionError::NotFoundScoped);
    }
    Ok(interaction)
}

pub async fn accept_interaction(
    db: &DatabaseConnection,
    runtime: &InteractionRuntime,
    interaction_id: &str,
) -> Result<AgentInteractionSnapshot, AgentInteractionError> {
    let mut interaction = load_interaction(db, interaction_id).await?;
    authorize_stored(&interaction, runtime)?;
    match interaction.snapshot.state {
        InteractionState::Pending => {
            interaction.snapshot.state = InteractionState::Accepted;
            interaction.accepted_runtime_id = Some(runtime.runtime_id.clone());
            interaction.snapshot.updated_at = Utc::now().to_rfc3339();
        }
        InteractionState::Accepted
            if interaction.accepted_runtime_id.as_deref() == Some(runtime.runtime_id.as_str()) => {}
        _ => {
            return Err(AgentInteractionError::StateConflict {
                message: "Agent interaction cannot be accepted in its current state",
            })
        }
    }
    if !conditional_save_interaction(db, &interaction, runtime, true).await? {
        return Err(AgentInteractionError::StateConflict {
            message: "Agent interaction was accepted by another runtime",
        });
    }
    Ok(interaction.snapshot)
}

pub async fn submit_result(
    db: &DatabaseConnection,
    runtime: &InteractionRuntime,
    interaction_id: &str,
    data: Value,
    summary: Option<String>,
    idempotency_key: String,
) -> Result<AgentInteractionSnapshot, AgentInteractionError> {
    if value_size(&data)? > MAX_INTERACTION_VALUE_BYTES
        || summary.as_ref().is_some_and(|s| s.len() > 2_000)
        || idempotency_key.is_empty()
        || idempotency_key.len() > 128
    {
        return Err(AgentInteractionError::ResultInvalid);
    }
    let mut interaction = load_interaction(db, interaction_id).await?;
    if interaction.subject_id != runtime.subject_id
        || interaction.owner_id != runtime.owner_id
        || interaction.snapshot.tapp_id != runtime.tapp_id
        || interaction.accepted_runtime_id.as_deref() != Some(runtime.runtime_id.as_str())
    {
        return Err(AgentInteractionError::RuntimeMismatch);
    }
    if interaction.snapshot.state == InteractionState::Completed
        && interaction.result_idempotency_key.as_deref() == Some(idempotency_key.as_str())
    {
        return Ok(interaction.snapshot);
    }
    if interaction.snapshot.state != InteractionState::Accepted {
        return Err(AgentInteractionError::StateConflict {
            message: "Agent interaction is not accepting results",
        });
    }
    if let Some(schema) = &interaction.result_schema {
        validate_inline_json_value(schema, &data)
            .map_err(|error| AgentInteractionError::ResultSchemaMismatch { message: error })?;
    }
    interaction.snapshot.state = InteractionState::Completed;
    interaction.snapshot.result = Some(json!({
        "data": data,
        "summary": summary,
    }));
    interaction.snapshot.updated_at = Utc::now().to_rfc3339();
    interaction.result_idempotency_key = Some(idempotency_key);
    interaction.retain_until = Utc::now().timestamp() + TERMINAL_RETENTION_SECONDS;
    if !conditional_save_interaction(db, &interaction, runtime, false).await? {
        let latest = load_interaction(db, interaction_id).await?;
        if latest.snapshot.state == InteractionState::Completed
            && latest.result_idempotency_key.as_deref()
                == interaction.result_idempotency_key.as_deref()
        {
            return Ok(latest.snapshot);
        }
        return Err(AgentInteractionError::StateConflict {
            message: "Agent interaction result was already finalized",
        });
    }
    resume_agent_task(db, &interaction).await;
    Ok(interaction.snapshot)
}

pub async fn reject_interaction(
    db: &DatabaseConnection,
    runtime: &InteractionRuntime,
    interaction_id: &str,
    reason: String,
) -> Result<AgentInteractionSnapshot, AgentInteractionError> {
    if reason.is_empty() || reason.len() > MAX_REASON_BYTES {
        return Err(AgentInteractionError::InvalidRejection);
    }
    let mut interaction = load_interaction(db, interaction_id).await?;
    if interaction.subject_id != runtime.subject_id
        || interaction.owner_id != runtime.owner_id
        || interaction.snapshot.tapp_id != runtime.tapp_id
        || interaction.snapshot.state.terminal()
        || (interaction.snapshot.state == InteractionState::Accepted
            && interaction.accepted_runtime_id.as_deref() != Some(runtime.runtime_id.as_str()))
    {
        return Err(AgentInteractionError::StateConflict {
            message: "Agent interaction cannot be rejected",
        });
    }
    interaction.snapshot.state = InteractionState::Rejected;
    interaction.snapshot.rejection_reason = Some(reason);
    interaction.snapshot.updated_at = Utc::now().to_rfc3339();
    interaction.retain_until = Utc::now().timestamp() + TERMINAL_RETENTION_SECONDS;
    if !conditional_save_interaction(db, &interaction, runtime, true).await? {
        return Err(AgentInteractionError::StateConflict {
            message: "Agent interaction was already finalized",
        });
    }
    resume_agent_task(db, &interaction).await;
    Ok(interaction.snapshot)
}

#[derive(Debug, Clone)]
pub struct AuthorizedIntent {
    pub intent_id: String,
    pub interaction_id: String,
    pub intent_type: String,
}

pub async fn authorize_intent(
    db: &DatabaseConnection,
    runtime: &InteractionRuntime,
    interaction_id: &str,
    intent_type: &str,
    params: &Value,
    reason: &str,
    host_confirmed: bool,
) -> Result<AuthorizedIntent, AgentInteractionError> {
    if !host_confirmed
        || reason.is_empty()
        || reason.len() > MAX_REASON_BYTES
        || value_size(params)? > 16 * 1024
    {
        return Err(AgentInteractionError::IntentConfirmationRequired);
    }
    if !HOST_INTENT_ADAPTERS.contains(&intent_type) {
        return Err(AgentInteractionError::IntentAdapterUnavailable);
    }
    let interaction = load_interaction(db, interaction_id).await?;
    if interaction.subject_id != runtime.subject_id
        || interaction.owner_id != runtime.owner_id
        || interaction.snapshot.tapp_id != runtime.tapp_id
        || interaction.snapshot.state != InteractionState::Accepted
        || interaction.accepted_runtime_id.as_deref() != Some(runtime.runtime_id.as_str())
        || !interaction.intents.iter().any(|i| i == intent_type)
    {
        return Err(AgentInteractionError::IntentNotAllowed);
    }
    Ok(AuthorizedIntent {
        intent_id: format!("agi_int_{}", Uuid::new_v4().simple()),
        interaction_id: interaction_id.to_string(),
        intent_type: intent_type.to_string(),
    })
}

/// Register SSE presence and enqueue pending snapshots for this runtime.
pub async fn open_stream(
    db: &DatabaseConnection,
    runtime: &InteractionRuntime,
    expires_at: i64,
) -> Result<Vec<String>, AgentInteractionError> {
    // Caller validates protocol_version + returns interaction type list from manifest.
    let presence = InteractionStream {
        subject_id: runtime.subject_id,
        owner_id: runtime.owner_id,
        tapp_id: runtime.tapp_id.clone(),
    };
    shared_registry::put(
        db,
        INTERACTION_PRESENCE_NAMESPACE,
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
    .map_err(|_| AgentInteractionError::Unavailable { presence: true })?;

    let interactions = shared_registry::list(
        db,
        INTERACTION_NAMESPACE,
        Some(runtime.subject_id),
        Some(&runtime.tapp_id),
    )
    .await
    .map_err(|_| AgentInteractionError::Unavailable { presence: false })?;
    for row in interactions {
        let Ok(interaction) = serde_json::from_value::<StoredInteraction>(row.payload) else {
            continue;
        };
        if same_interaction_scope(
            runtime.subject_id,
            runtime.owner_id,
            &runtime.tapp_id,
            interaction.subject_id,
            interaction.owner_id,
            &interaction.snapshot.tapp_id,
        ) && interaction.snapshot.state == InteractionState::Pending
        {
            shared_registry::enqueue(
                db,
                INTERACTION_MAILBOX_CHANNEL,
                &runtime.runtime_id,
                &interaction.snapshot,
                interaction.retain_until,
            )
            .await
            .map_err(|_| AgentInteractionError::Unavailable { presence: false })?;
        }
    }
    Ok(vec![])
}

pub async fn drain_stream(
    db: &DatabaseConnection,
    runtime_id: &str,
) -> Vec<AgentInteractionSnapshot> {
    shared_registry::drain::<AgentInteractionSnapshot>(
        db,
        INTERACTION_MAILBOX_CHANNEL,
        runtime_id,
        32,
    )
    .await
    .unwrap_or_default()
}

pub async fn clear_presence(runtime_id: &str) {
    if let Ok(db) = shared_registry::database().await {
        let _ = shared_registry::delete(&db, INTERACTION_PRESENCE_NAMESPACE, runtime_id).await;
    }
}

/// Disconnect runtime presence and cancel accepted interactions owned by it.
pub async fn disconnect_runtime_interactions(runtime_id: &str) -> bool {
    let Ok(db) = shared_registry::database().await else {
        return false;
    };
    let disconnected = shared_registry::delete(&db, INTERACTION_PRESENCE_NAMESPACE, runtime_id)
        .await
        .unwrap_or(false);
    let interactions = shared_registry::list(&db, INTERACTION_NAMESPACE, None, None)
        .await
        .unwrap_or_default();
    for row in interactions {
        let Ok(mut interaction) = serde_json::from_value::<StoredInteraction>(row.payload) else {
            continue;
        };
        if interaction.accepted_runtime_id.as_deref() != Some(runtime_id)
            || interaction.snapshot.state != InteractionState::Accepted
        {
            continue;
        }
        interaction.snapshot.state = InteractionState::Cancelled;
        interaction.snapshot.rejection_reason = Some("Tapp runtime disconnected".to_string());
        interaction.snapshot.updated_at = Utc::now().to_rfc3339();
        interaction.retain_until = Utc::now().timestamp() + TERMINAL_RETENTION_SECONDS;
        if cancel_disconnected_interaction(&db, &interaction, runtime_id)
            .await
            .unwrap_or(false)
        {
            resume_agent_task(&db, &interaction).await;
        }
    }
    disconnected
}

async fn resume_agent_task(db: &DatabaseConnection, interaction: &StoredInteraction) {
    let Some(task_id) = interaction.snapshot.source.task_id.clone() else {
        return;
    };
    let question_id = format!("tapp_interaction:{}", interaction.snapshot.interaction_id);
    let (state, skipped, answer_value) = match interaction.snapshot.state {
        InteractionState::Completed => (
            "completed",
            false,
            json!({
                "state": "completed",
                "result": interaction.snapshot.result,
            }),
        ),
        InteractionState::Rejected => (
            "rejected",
            true,
            json!({
                "state": "rejected",
                "reason": interaction.snapshot.rejection_reason,
            }),
        ),
        InteractionState::Expired => (
            "expired",
            true,
            json!({
                "state": "expired",
                "reason": interaction.snapshot.rejection_reason,
            }),
        ),
        InteractionState::Cancelled => (
            "cancelled",
            true,
            json!({
                "state": "cancelled",
                "reason": interaction.snapshot.rejection_reason,
            }),
        ),
        _ => return,
    };
    let user_id = interaction.subject_id;
    let db = db.clone();
    tokio::spawn(async move {
        let agent = crate::services::agent::Agent::new(db).await;
        let answer = crate::services::agent::UserAnswer {
            question_id,
            task_id: task_id.clone(),
            answer: answer_value.to_string(),
            skipped,
        };
        if let Err(error) = agent.resume_task(&task_id, answer, user_id).await {
            tracing::error!(%task_id, interaction_state = state, %error, "[TAPP] Failed to resume Agent task from interaction");
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{same_interaction_scope, AgentInteractionError, HOST_INTENT_ADAPTERS};
    use crate::services::agent_interaction::InteractionState;

    #[test]
    fn interaction_states_have_terminal_boundary() {
        assert!(!InteractionState::Pending.terminal());
        assert!(!InteractionState::Accepted.terminal());
        assert!(InteractionState::Completed.terminal());
    }

    #[test]
    fn interaction_stream_scope_includes_install_owner() {
        assert!(same_interaction_scope(
            10,
            1,
            "com.example.app",
            10,
            1,
            "com.example.app",
        ));
        assert!(!same_interaction_scope(
            10,
            1,
            "com.example.app",
            10,
            10,
            "com.example.app",
        ));
    }

    #[test]
    fn host_intent_adapters_are_stable() {
        assert!(HOST_INTENT_ADAPTERS.contains(&"ui.open"));
        assert!(HOST_INTENT_ADAPTERS.contains(&"report.create"));
        assert!(HOST_INTENT_ADAPTERS.contains(&"dataExchange.request"));
    }

    #[test]
    fn error_codes_preserve_api_contract() {
        assert_eq!(
            AgentInteractionError::Unavailable { presence: false }.code(),
            "AGENT_REGISTRY_UNAVAILABLE"
        );
        assert_eq!(
            AgentInteractionError::NotFound.code(),
            "AGENT_INTERACTION_NOT_FOUND"
        );
        assert_eq!(
            AgentInteractionError::StateConflict { message: "x" }.code(),
            "AGENT_INTERACTION_STATE_CONFLICT"
        );
        assert_eq!(
            AgentInteractionError::RuntimeMismatch.code(),
            "AGENT_INTERACTION_RUNTIME_MISMATCH"
        );
        assert_eq!(
            AgentInteractionError::IntentNotAllowed.code(),
            "AGENT_INTENT_NOT_ALLOWED"
        );
        assert_eq!(
            AgentInteractionError::NotDeclared.code(),
            "AGENT_V2_NOT_DECLARED"
        );
    }

    #[test]
    fn status_hints_match_http_contract() {
        assert_eq!(
            AgentInteractionError::Unavailable { presence: false }.status_hint(),
            503
        );
        assert_eq!(AgentInteractionError::NotFound.status_hint(), 404);
        assert_eq!(
            AgentInteractionError::StateConflict { message: "x" }.status_hint(),
            409
        );
        assert_eq!(AgentInteractionError::TooMany.status_hint(), 429);
        assert_eq!(AgentInteractionError::RuntimeMismatch.status_hint(), 403);
    }
}
