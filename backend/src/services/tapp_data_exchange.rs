//! One-shot, consent-gated data exchange between installed Tapps.
//!
//! Private Tapp storage is never exposed here. A requester and provider must
//! both declare the named contract in their manifests, and the host must
//! authorize every prepared request before a provider response can be consumed.
//!
//! Domain lives in services. Grant teardown still calls through
//! `api::tapp_runtime::data_exchange` wrappers. The API layer maps
//! [`DataExchangeError`] to Axum responses and resolves accessible Tapp installs.

use chrono::Utc;
use myriad_tapp_contract::manifest::{TappDataExchangeManifest, TappDataExport};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::time::Duration;
use uuid::Uuid;

use crate::services::json_schema_subset::validate_inline_json_value;
use crate::services::tapp_registry::{self as shared_registry, RegistryIdentity};

const PREPARED_REQUEST_TTL: Duration = Duration::from_secs(2 * 60);
const DATA_ACCESS_GRANT_TTL: Duration = Duration::from_secs(60);
const MAX_PURPOSE_LENGTH: usize = 500;
const MAX_PARAMS_BYTES: usize = 64 * 1024;
const MAX_PENDING_REQUESTS_PER_SUBJECT: usize = 32;
const MAX_ACTIVE_GRANTS_PER_SUBJECT: usize = 32;

const PREPARED_NAMESPACE: &str = "data_exchange_request";
const DATA_GRANT_NAMESPACE: &str = "data_exchange_grant";

/// Runtime identity snapshot from a validated Runtime Grant.
#[derive(Debug, Clone)]
pub struct DataExchangeRuntime {
    pub runtime_id: String,
    pub tapp_id: String,
    pub owner_id: i32,
    pub subject_id: i32,
}

/// Install party already resolved by the API (ownership HTTP mapping stays there).
#[derive(Debug, Clone)]
pub struct ExchangeParty {
    pub tapp_id: String,
    pub owner_id: i32,
    pub name: String,
    pub manifest: Value,
}

#[derive(Debug, Clone)]
pub struct PrepareExchangeInput {
    pub target_tapp_id: String,
    pub export_id: String,
    pub params: Value,
    pub purpose: String,
}

#[derive(Debug, Clone)]
pub struct PreparedExchange {
    pub request_id: String,
    pub requester_tapp_id: String,
    pub requester_name: String,
    pub provider_tapp_id: String,
    pub provider_owner_id: i32,
    pub provider_name: String,
    pub export_id: String,
    pub export_description: Option<String>,
    pub params: Value,
    pub purpose: String,
    pub max_bytes: usize,
    pub max_records: Option<usize>,
    pub expires_at: i64,
}

#[derive(Debug, Clone)]
pub struct AuthorizedAccessGrant {
    pub version: u8,
    pub grant_id: String,
    pub token: String,
    pub request_id: String,
    pub provider_tapp_id: String,
    pub provider_owner_id: i32,
    pub export_id: String,
    pub params: Value,
    pub purpose: String,
    pub request_hash: String,
    pub max_bytes: usize,
    pub max_records: Option<usize>,
    pub expires_at: i64,
}

#[derive(Debug, Clone)]
pub struct ConsumedExchange {
    pub grant_id: String,
    pub request_hash: String,
    pub data: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct PreparedRequest {
    request_id: String,
    requester_runtime_id: String,
    requester_tapp_id: String,
    provider_tapp_id: String,
    provider_owner_id: i32,
    subject_id: i32,
    export: TappDataExport,
    params: Value,
    purpose: String,
    request_hash: String,
    expires_at: i64,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
struct StoredDataAccessGrant {
    grant_id: String,
    request_id: String,
    requester_runtime_id: String,
    requester_tapp_id: String,
    provider_tapp_id: String,
    provider_owner_id: i32,
    subject_id: i32,
    export: TappDataExport,
    params: Value,
    purpose: String,
    request_hash: String,
    expires_at: i64,
}

/// Domain errors for data-exchange operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DataExchangeError {
    Unavailable,
    InvalidPurpose,
    InvalidParams,
    ParamsTooLarge,
    SelfRequest,
    InstallationMismatch,
    NotDeclared,
    InvalidManifest,
    ImportNotDeclared,
    ExportNotDeclared,
    PendingLimit,
    RequestExpired,
    RequestMismatch,
    RequestAlreadyUsed,
    GrantLimit,
    InvalidGrant,
    ProviderMismatch,
    ResponseInvalid,
    ResponseTooLarge { actual: usize, max: usize },
    RecordLimit { actual: usize, max: usize },
    SchemaMismatch { message: String },
}

impl DataExchangeError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Unavailable => "DATA_EXCHANGE_REGISTRY_UNAVAILABLE",
            Self::InvalidPurpose => "INVALID_DATA_EXCHANGE_PURPOSE",
            Self::InvalidParams => "INVALID_DATA_EXCHANGE_PARAMS",
            Self::ParamsTooLarge => "DATA_EXCHANGE_PARAMS_TOO_LARGE",
            Self::SelfRequest => "DATA_EXCHANGE_SELF_REQUEST",
            Self::InstallationMismatch => "RUNTIME_GRANT_INSTALLATION_MISMATCH",
            Self::NotDeclared => "DATA_EXCHANGE_NOT_DECLARED",
            Self::InvalidManifest => "INVALID_DATA_EXCHANGE_MANIFEST",
            Self::ImportNotDeclared => "DATA_EXCHANGE_IMPORT_NOT_DECLARED",
            Self::ExportNotDeclared => "DATA_EXCHANGE_EXPORT_NOT_DECLARED",
            Self::PendingLimit => "DATA_EXCHANGE_PENDING_LIMIT",
            Self::RequestExpired => "DATA_EXCHANGE_REQUEST_EXPIRED",
            Self::RequestMismatch => "DATA_EXCHANGE_REQUEST_MISMATCH",
            Self::RequestAlreadyUsed => "DATA_EXCHANGE_REQUEST_ALREADY_USED",
            Self::GrantLimit => "DATA_ACCESS_GRANT_LIMIT",
            Self::InvalidGrant => "INVALID_DATA_ACCESS_GRANT",
            Self::ProviderMismatch => "DATA_ACCESS_PROVIDER_MISMATCH",
            Self::ResponseInvalid => "DATA_EXCHANGE_RESPONSE_INVALID",
            Self::ResponseTooLarge { .. } => "DATA_EXCHANGE_RESPONSE_TOO_LARGE",
            Self::RecordLimit { .. } => "DATA_EXCHANGE_RECORD_LIMIT",
            Self::SchemaMismatch { .. } => "DATA_EXCHANGE_SCHEMA_MISMATCH",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Unavailable => "Data Exchange registry is unavailable".to_string(),
            Self::InvalidPurpose => {
                format!("purpose must contain 1-{MAX_PURPOSE_LENGTH} bytes")
            }
            Self::InvalidParams => "params cannot be serialized".to_string(),
            Self::ParamsTooLarge => {
                format!("params exceed {MAX_PARAMS_BYTES} bytes")
            }
            Self::SelfRequest => "Use the Tapp's private API for same-Tapp data".to_string(),
            Self::InstallationMismatch => {
                "Runtime Grant no longer matches the installed Tapp".to_string()
            }
            Self::NotDeclared => "Tapp manifest does not declare dataExchange".to_string(),
            Self::InvalidManifest => "Stored dataExchange manifest is invalid".to_string(),
            Self::ImportNotDeclared => {
                "Requester manifest does not declare this import".to_string()
            }
            Self::ExportNotDeclared => "Provider manifest does not declare this export".to_string(),
            Self::PendingLimit => "Too many pending Data Exchange requests".to_string(),
            Self::RequestExpired => {
                "Prepared Data Exchange request is missing or expired".to_string()
            }
            Self::RequestMismatch => "Prepared request does not belong to this runtime".to_string(),
            Self::RequestAlreadyUsed => {
                "Prepared Data Exchange request was already consumed".to_string()
            }
            Self::GrantLimit => "Too many active one-shot Data Access Grants".to_string(),
            Self::InvalidGrant => {
                "Data Access Grant is missing, expired, consumed, or revoked".to_string()
            }
            Self::ProviderMismatch => {
                "Data Access Grant does not belong to this provider runtime".to_string()
            }
            Self::ResponseInvalid => "Provider response cannot be serialized".to_string(),
            Self::ResponseTooLarge { actual, max } => {
                format!("Provider response is {actual} bytes; maximum is {max}")
            }
            Self::RecordLimit { actual, max } => {
                format!("Provider response contains {actual} records; maximum is {max}")
            }
            Self::SchemaMismatch { message } => message.clone(),
        }
    }

    pub fn status_hint(&self) -> u16 {
        match self {
            Self::Unavailable => 503,
            Self::InvalidPurpose | Self::InvalidParams | Self::SelfRequest => 400,
            Self::ParamsTooLarge | Self::ResponseTooLarge { .. } => 413,
            Self::InstallationMismatch
            | Self::NotDeclared
            | Self::ImportNotDeclared
            | Self::RequestMismatch
            | Self::ProviderMismatch => 403,
            Self::InvalidManifest
            | Self::ResponseInvalid
            | Self::RecordLimit { .. }
            | Self::SchemaMismatch { .. } => 422,
            Self::ExportNotDeclared | Self::RequestExpired => 404,
            Self::PendingLimit | Self::GrantLimit => 429,
            Self::RequestAlreadyUsed => 409,
            Self::InvalidGrant => 401,
        }
    }
}

impl std::fmt::Display for DataExchangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for DataExchangeError {}

pub(crate) fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn new_token() -> String {
    format!("dxg_{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

pub(crate) fn same_provider_scope(
    grant_subject_id: i32,
    grant_owner_id: i32,
    grant_tapp_id: &str,
    runtime_subject_id: i32,
    runtime_owner_id: i32,
    runtime_tapp_id: &str,
) -> bool {
    grant_subject_id == runtime_subject_id
        && grant_owner_id == runtime_owner_id
        && grant_tapp_id == runtime_tapp_id
}

fn parse_exchange_manifest(
    manifest: &Value,
) -> Result<TappDataExchangeManifest, DataExchangeError> {
    let value = manifest
        .get("dataExchange")
        .ok_or(DataExchangeError::NotDeclared)?;
    serde_json::from_value(value.clone()).map_err(|_| DataExchangeError::InvalidManifest)
}

fn request_hash(
    requester_tapp_id: &str,
    provider_tapp_id: &str,
    export_id: &str,
    params: &Value,
    purpose: &str,
) -> Result<String, DataExchangeError> {
    let encoded = serde_json::to_vec(&json!({
        "requesterTappId": requester_tapp_id,
        "providerTappId": provider_tapp_id,
        "exportId": export_id,
        "params": params,
        "purpose": purpose,
    }))
    .map_err(|_| DataExchangeError::InvalidParams)?;
    Ok(format!("sha256:{}", hex::encode(Sha256::digest(encoded))))
}

/// Prepare a consent-gated exchange request (registry write).
pub async fn prepare_exchange(
    db: &DatabaseConnection,
    runtime: &DataExchangeRuntime,
    requester: &ExchangeParty,
    provider: &ExchangeParty,
    input: PrepareExchangeInput,
) -> Result<PreparedExchange, DataExchangeError> {
    let purpose = input.purpose.trim().to_string();
    if purpose.is_empty() || purpose.len() > MAX_PURPOSE_LENGTH {
        return Err(DataExchangeError::InvalidPurpose);
    }
    let params_bytes =
        serde_json::to_vec(&input.params).map_err(|_| DataExchangeError::InvalidParams)?;
    if params_bytes.len() > MAX_PARAMS_BYTES {
        return Err(DataExchangeError::ParamsTooLarge);
    }
    if input.target_tapp_id == runtime.tapp_id {
        return Err(DataExchangeError::SelfRequest);
    }
    if requester.owner_id != runtime.owner_id {
        return Err(DataExchangeError::InstallationMismatch);
    }

    let requester_exchange = parse_exchange_manifest(&requester.manifest)?;
    if !requester_exchange
        .imports
        .iter()
        .any(|import| import.tapp_id == input.target_tapp_id && import.export_id == input.export_id)
    {
        return Err(DataExchangeError::ImportNotDeclared);
    }
    let provider_exchange = parse_exchange_manifest(&provider.manifest)?;
    let export = provider_exchange
        .exports
        .into_iter()
        .find(|export| export.id == input.export_id)
        .ok_or(DataExchangeError::ExportNotDeclared)?;

    let now = Utc::now().timestamp();
    let expires_at = now + PREPARED_REQUEST_TTL.as_secs() as i64;
    let request_id = format!("dxr_{}", Uuid::new_v4().simple());
    let hash = request_hash(
        &runtime.tapp_id,
        &input.target_tapp_id,
        &input.export_id,
        &input.params,
        &purpose,
    )?;
    let prepared = PreparedRequest {
        request_id: request_id.clone(),
        requester_runtime_id: runtime.runtime_id.clone(),
        requester_tapp_id: runtime.tapp_id.clone(),
        provider_tapp_id: input.target_tapp_id.clone(),
        provider_owner_id: provider.owner_id,
        subject_id: runtime.subject_id,
        export: export.clone(),
        params: input.params.clone(),
        purpose: purpose.clone(),
        request_hash: hash,
        expires_at,
    };

    let inserted = shared_registry::put_with_subject_limit(
        db,
        PREPARED_NAMESPACE,
        &request_id,
        RegistryIdentity {
            subject_id: Some(runtime.subject_id),
            owner_id: Some(runtime.owner_id),
            tapp_id: Some(runtime.tapp_id.as_str()),
            runtime_id: Some(runtime.runtime_id.as_str()),
        },
        &prepared,
        expires_at,
        MAX_PENDING_REQUESTS_PER_SUBJECT,
    )
    .await
    .map_err(|_| DataExchangeError::Unavailable)?;
    if !inserted {
        return Err(DataExchangeError::PendingLimit);
    }

    Ok(PreparedExchange {
        request_id,
        requester_tapp_id: runtime.tapp_id.clone(),
        requester_name: requester.name.clone(),
        provider_tapp_id: provider.tapp_id.clone(),
        provider_owner_id: provider.owner_id,
        provider_name: provider.name.clone(),
        export_id: export.id,
        export_description: export.description,
        params: prepared.params,
        purpose,
        max_bytes: export.max_bytes,
        max_records: export.max_records,
        expires_at,
    })
}

/// Host authorizes a prepared request → one-shot Data Access Grant.
pub async fn authorize_exchange(
    db: &DatabaseConnection,
    runtime: &DataExchangeRuntime,
    request_id: &str,
) -> Result<AuthorizedAccessGrant, DataExchangeError> {
    let now = Utc::now().timestamp();
    let prepared = {
        let prepared = shared_registry::get::<PreparedRequest>(db, PREPARED_NAMESPACE, request_id)
            .await
            .map_err(|_| DataExchangeError::Unavailable)?
            .ok_or(DataExchangeError::RequestExpired)?;
        if prepared.subject_id != runtime.subject_id
            || prepared.requester_runtime_id != runtime.runtime_id
            || prepared.requester_tapp_id != runtime.tapp_id
        {
            return Err(DataExchangeError::RequestMismatch);
        }
        if !shared_registry::delete(db, PREPARED_NAMESPACE, request_id)
            .await
            .map_err(|_| DataExchangeError::Unavailable)?
        {
            return Err(DataExchangeError::RequestAlreadyUsed);
        }
        prepared
    };

    let token = new_token();
    let grant_id = format!("dxg_{}", Uuid::new_v4().simple());
    let expires_at = now + DATA_ACCESS_GRANT_TTL.as_secs() as i64;
    let stored = StoredDataAccessGrant {
        grant_id: grant_id.clone(),
        request_id: prepared.request_id.clone(),
        requester_runtime_id: prepared.requester_runtime_id.clone(),
        requester_tapp_id: prepared.requester_tapp_id.clone(),
        provider_tapp_id: prepared.provider_tapp_id.clone(),
        provider_owner_id: prepared.provider_owner_id,
        subject_id: prepared.subject_id,
        export: prepared.export.clone(),
        params: prepared.params.clone(),
        purpose: prepared.purpose.clone(),
        request_hash: prepared.request_hash.clone(),
        expires_at,
    };

    let inserted = shared_registry::put_with_subject_limit(
        db,
        DATA_GRANT_NAMESPACE,
        &token_hash(&token),
        RegistryIdentity {
            subject_id: Some(runtime.subject_id),
            owner_id: Some(runtime.owner_id),
            tapp_id: Some(prepared.requester_tapp_id.as_str()),
            runtime_id: Some(prepared.requester_runtime_id.as_str()),
        },
        &stored,
        expires_at,
        MAX_ACTIVE_GRANTS_PER_SUBJECT,
    )
    .await
    .map_err(|_| DataExchangeError::Unavailable)?;
    if !inserted {
        return Err(DataExchangeError::GrantLimit);
    }

    tracing::info!(
        grant_id = %grant_id,
        request_id = %prepared.request_id,
        requester_tapp_id = %prepared.requester_tapp_id,
        provider_tapp_id = %prepared.provider_tapp_id,
        export_id = %prepared.export.id,
        subject_id = prepared.subject_id,
        "[TAPP] One-shot Data Access Grant authorized"
    );

    Ok(AuthorizedAccessGrant {
        version: 1,
        grant_id,
        token,
        request_id: prepared.request_id,
        provider_tapp_id: prepared.provider_tapp_id,
        provider_owner_id: prepared.provider_owner_id,
        export_id: prepared.export.id,
        params: prepared.params,
        purpose: prepared.purpose,
        request_hash: prepared.request_hash,
        max_bytes: prepared.export.max_bytes,
        max_records: prepared.export.max_records,
        expires_at,
    })
}

/// Cancel a prepared request and/or its authorized grant for this runtime.
pub async fn cancel_exchange(
    db: &DatabaseConnection,
    runtime: &DataExchangeRuntime,
    request_id: &str,
) -> Result<bool, DataExchangeError> {
    let request = shared_registry::get::<PreparedRequest>(db, PREPARED_NAMESPACE, request_id)
        .await
        .map_err(|_| DataExchangeError::Unavailable)?;
    let prepared_belongs = request.as_ref().is_some_and(|request| {
        request.subject_id == runtime.subject_id
            && request.requester_runtime_id == runtime.runtime_id
            && request.requester_tapp_id == runtime.tapp_id
    });
    let mut cancelled = false;
    if prepared_belongs {
        cancelled = shared_registry::delete(db, PREPARED_NAMESPACE, request_id)
            .await
            .map_err(|_| DataExchangeError::Unavailable)?;
    }

    // Once authorized, the prepared request has already been removed. Locate
    // the host-only one-shot grant by its request id so `cancel_exchange`
    // revokes it immediately instead of waiting for TTL.
    cancelled |= shared_registry::delete_matching_payload_text(
        db,
        DATA_GRANT_NAMESPACE,
        Some(runtime.subject_id),
        Some(runtime.tapp_id.as_str()),
        Some(runtime.runtime_id.as_str()),
        "request_id",
        request_id,
    )
    .await
    .map_err(|_| DataExchangeError::Unavailable)?
        > 0;
    Ok(cancelled)
}

/// Provider consumes a one-shot grant with a schema-validated response payload.
pub async fn consume_exchange(
    db: &DatabaseConnection,
    provider: &DataExchangeRuntime,
    grant_token: &str,
    response: Value,
) -> Result<ConsumedExchange, DataExchangeError> {
    // Removal happens before any payload validation: success and failure both
    // exhaust the one-shot token and make replay impossible.
    let grant = shared_registry::take::<StoredDataAccessGrant>(
        db,
        DATA_GRANT_NAMESPACE,
        &token_hash(grant_token),
    )
    .await
    .map_err(|_| DataExchangeError::Unavailable)?
    .ok_or(DataExchangeError::InvalidGrant)?;

    if !same_provider_scope(
        grant.subject_id,
        grant.provider_owner_id,
        &grant.provider_tapp_id,
        provider.subject_id,
        provider.owner_id,
        &provider.tapp_id,
    ) {
        return Err(DataExchangeError::ProviderMismatch);
    }

    let encoded = serde_json::to_vec(&response).map_err(|_| DataExchangeError::ResponseInvalid)?;
    if encoded.len() > grant.export.max_bytes {
        return Err(DataExchangeError::ResponseTooLarge {
            actual: encoded.len(),
            max: grant.export.max_bytes,
        });
    }
    if let (Some(limit), Some(records)) = (grant.export.max_records, response.as_array()) {
        if records.len() > limit {
            return Err(DataExchangeError::RecordLimit {
                actual: records.len(),
                max: limit,
            });
        }
    }
    validate_inline_json_value(&grant.export.schema, &response)
        .map_err(|message| DataExchangeError::SchemaMismatch { message })?;

    tracing::info!(
        grant_id = %grant.grant_id,
        requester_runtime_id = %grant.requester_runtime_id,
        requester_tapp_id = %grant.requester_tapp_id,
        provider_tapp_id = %grant.provider_tapp_id,
        export_id = %grant.export.id,
        request_hash = %grant.request_hash,
        purpose_bytes = grant.purpose.len(),
        params_bytes = serde_json::to_vec(&grant.params).map_or(0, |value| value.len()),
        response_bytes = encoded.len(),
        subject_id = grant.subject_id,
        "[TAPP] One-shot Data Exchange completed"
    );

    Ok(ConsumedExchange {
        grant_id: grant.grant_id,
        request_hash: grant.request_hash,
        data: response,
    })
}

async fn delete_exchange_scope(
    namespace: &str,
    subject_id: Option<i32>,
    tapp_id: Option<&str>,
    runtime_id: Option<&str>,
) {
    match shared_registry::database().await {
        Ok(db) => {
            if let Err(error) =
                shared_registry::delete_matching(&db, namespace, subject_id, tapp_id, runtime_id)
                    .await
            {
                tracing::error!(%error, namespace, "[TAPP] Failed to revoke Data Exchange state");
            }
        }
        Err(error) => {
            tracing::error!(%error, namespace, "[TAPP] Data Exchange registry is unavailable during revocation");
        }
    }
}

async fn delete_provider_exchange_scope(subject_id: Option<i32>, provider_tapp_id: &str) {
    let db = match shared_registry::database().await {
        Ok(db) => db,
        Err(error) => {
            tracing::error!(%error, "[TAPP] Data Exchange registry is unavailable during provider revocation");
            return;
        }
    };

    for namespace in [PREPARED_NAMESPACE, DATA_GRANT_NAMESPACE] {
        if let Err(error) = shared_registry::delete_matching_payload_text(
            &db,
            namespace,
            subject_id,
            None,
            None,
            "provider_tapp_id",
            provider_tapp_id,
        )
        .await
        {
            tracing::error!(%error, namespace, "[TAPP] Failed to revoke provider Data Exchange state");
        }
    }
}

/// Revoke prepared + grant state for one runtime (grant teardown).
pub async fn cancel_runtime_data_exchanges(subject_id: i32, tapp_id: &str, runtime_id: &str) {
    delete_exchange_scope(
        PREPARED_NAMESPACE,
        Some(subject_id),
        Some(tapp_id),
        Some(runtime_id),
    )
    .await;
    delete_exchange_scope(
        DATA_GRANT_NAMESPACE,
        Some(subject_id),
        Some(tapp_id),
        Some(runtime_id),
    )
    .await;
}

/// Revoke for a subject+tapp pair (stop). Uninstall uses `cancel_all_tapp_data_exchanges`.
pub async fn cancel_tapp_data_exchanges(subject_id: i32, tapp_id: &str) {
    delete_exchange_scope(PREPARED_NAMESPACE, Some(subject_id), Some(tapp_id), None).await;
    delete_exchange_scope(DATA_GRANT_NAMESPACE, Some(subject_id), Some(tapp_id), None).await;
    delete_provider_exchange_scope(Some(subject_id), tapp_id).await;
}

/// Revoke all subjects for an installation removal.
pub async fn cancel_all_tapp_data_exchanges(tapp_id: &str) {
    delete_exchange_scope(PREPARED_NAMESPACE, None, Some(tapp_id), None).await;
    delete_exchange_scope(DATA_GRANT_NAMESPACE, None, Some(tapp_id), None).await;
    delete_provider_exchange_scope(None, tapp_id).await;
}

#[cfg(test)]
mod tests {
    use super::{same_provider_scope, token_hash, DataExchangeError};
    use crate::services::json_schema_subset::validate_inline_json_value;
    use serde_json::json;

    #[test]
    fn validates_declared_object_shape() {
        let schema = json!({
            "type": "object",
            "required": ["title", "tracks"],
            "properties": {
                "title": { "type": "string", "maxLength": 20 },
                "tracks": {
                    "type": "array",
                    "maxItems": 2,
                    "items": {
                        "type": "object",
                        "required": ["id"],
                        "properties": { "id": { "type": "string" } },
                        "additionalProperties": false
                    }
                }
            },
            "additionalProperties": false
        });

        assert!(validate_inline_json_value(
            &schema,
            &json!({"title": "Now", "tracks": [{"id": "1"}]}),
        )
        .is_ok());
        assert!(validate_inline_json_value(
            &schema,
            &json!({"title": "Now", "tracks": [{"id": "1", "secret": true}]}),
        )
        .is_err());
    }

    #[test]
    fn data_access_tokens_are_stored_as_hashes() {
        let hash = token_hash("dxg_secret");
        assert_eq!(hash.len(), 64);
        assert_eq!(hash, token_hash("dxg_secret"));
        assert_ne!(hash, token_hash("dxg_other"));
    }

    #[test]
    fn data_access_grant_scope_includes_provider_owner() {
        assert!(same_provider_scope(
            42,
            1,
            "com.example.provider",
            42,
            1,
            "com.example.provider",
        ));
        assert!(!same_provider_scope(
            42,
            1,
            "com.example.provider",
            42,
            42,
            "com.example.provider",
        ));
    }

    #[test]
    fn error_codes_preserve_api_contract() {
        assert_eq!(
            DataExchangeError::Unavailable.code(),
            "DATA_EXCHANGE_REGISTRY_UNAVAILABLE"
        );
        assert_eq!(
            DataExchangeError::InvalidGrant.code(),
            "INVALID_DATA_ACCESS_GRANT"
        );
        assert_eq!(
            DataExchangeError::ProviderMismatch.code(),
            "DATA_ACCESS_PROVIDER_MISMATCH"
        );
        assert_eq!(
            DataExchangeError::SchemaMismatch {
                message: "x".into()
            }
            .code(),
            "DATA_EXCHANGE_SCHEMA_MISMATCH"
        );
        assert_eq!(
            DataExchangeError::InstallationMismatch.code(),
            "RUNTIME_GRANT_INSTALLATION_MISMATCH"
        );
        assert_eq!(
            DataExchangeError::PendingLimit.code(),
            "DATA_EXCHANGE_PENDING_LIMIT"
        );
        assert_eq!(
            DataExchangeError::GrantLimit.code(),
            "DATA_ACCESS_GRANT_LIMIT"
        );
    }

    #[test]
    fn status_hints_match_http_contract() {
        assert_eq!(DataExchangeError::Unavailable.status_hint(), 503);
        assert_eq!(DataExchangeError::InvalidGrant.status_hint(), 401);
        assert_eq!(DataExchangeError::ParamsTooLarge.status_hint(), 413);
        assert_eq!(DataExchangeError::RequestAlreadyUsed.status_hint(), 409);
        assert_eq!(DataExchangeError::PendingLimit.status_hint(), 429);
        assert_eq!(DataExchangeError::ExportNotDeclared.status_hint(), 404);
        assert_eq!(
            DataExchangeError::SchemaMismatch {
                message: "m".into()
            }
            .status_hint(),
            422
        );
    }
}
