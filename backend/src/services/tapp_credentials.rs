//! Installation-scoped, write-only credentials for declared Tapp HTTP APIs.
//!
//! Credential values never enter the Tapp template context. Each stored value
//! is bound to a fingerprint of the declaring credential plus every API that
//! can use it, so a manifest update that changes an endpoint/header/access must
//! be explicitly re-authorized by the installation manager.

use crate::models::entities::{tapp_storage, tapps};
use myriad_tapp_contract::contract_rules::MAX_CREDENTIAL_VALUE_LEN;
use myriad_tapp_contract::manifest::{TappApiDef, TappApiRoute, TappCredentialDef};
use sea_orm::{
    prelude::DateTimeWithTimeZone, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, EntityTrait, FromQueryResult, QueryFilter, Statement,
};
use serde::Serialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};

const CREDENTIAL_STORAGE_PREFIX: &str = "_credentials.";

fn storage_key(key: &str) -> String {
    format!("{CREDENTIAL_STORAGE_PREFIX}{key}")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TappCredentialError {
    InvalidDefinition(String),
    InvalidValue,
    Missing,
    ReauthorizationRequired,
    Encryption,
    Database,
}

impl TappCredentialError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidDefinition(_) => "TAPP_CREDENTIAL_INVALID",
            Self::InvalidValue => "TAPP_CREDENTIAL_VALUE_INVALID",
            Self::Missing => "TAPP_CREDENTIAL_MISSING",
            Self::ReauthorizationRequired => "TAPP_CREDENTIAL_REAUTH_REQUIRED",
            Self::Encryption => "TAPP_CREDENTIAL_ENCRYPTION_FAILED",
            Self::Database => "DATABASE_ERROR",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::InvalidDefinition(message) => message.clone(),
            Self::InvalidValue => {
                "Credential must be a non-empty string within the size limit".into()
            }
            Self::Missing => "Required Tapp credential is not configured".into(),
            Self::ReauthorizationRequired => {
                "Tapp credential binding changed and must be re-authorized".into()
            }
            Self::Encryption => "Credential encryption is unavailable".into(),
            Self::Database => "Database error".into(),
        }
    }
}

impl std::fmt::Display for TappCredentialError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for TappCredentialError {}

/// Secret material passed only inside the backend execution path.
#[derive(Clone)]
pub struct ResolvedApiCredential {
    value: String,
    revision: String,
}

impl std::fmt::Debug for ResolvedApiCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedApiCredential")
            .field("value", &"[REDACTED]")
            .field("revision", &self.revision)
            .finish()
    }
}

impl ResolvedApiCredential {
    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn revision(&self) -> &str {
        &self.revision
    }

    #[cfg(test)]
    pub(crate) fn for_test(value: &str) -> Self {
        Self {
            value: value.to_string(),
            revision: hex::encode(Sha256::digest(value.as_bytes())),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TappCredentialBindingSummary {
    pub api: String,
    pub method: String,
    pub endpoint: String,
    pub access: String,
    pub placement: String,
    pub field: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sign_alg: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sign_over: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TappCredentialStatus {
    pub key: String,
    pub configured: bool,
    pub needs_reauthorization: bool,
    pub origins: Vec<String>,
    pub bindings: Vec<TappCredentialBindingSummary>,
    pub updated_at: Option<String>,
}

#[derive(FromQueryResult)]
struct StoredCredentialStatus {
    key: String,
    configured: bool,
    binding_fingerprint: Option<String>,
    updated_at: DateTimeWithTimeZone,
}

fn parse_credentials(manifest: &Value) -> Result<Vec<TappCredentialDef>, TappCredentialError> {
    manifest
        .get("credentials")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map(|value| value.unwrap_or_default())
        .map_err(|_| {
            TappCredentialError::InvalidDefinition("Invalid credential declaration".into())
        })
}

fn parse_apis(manifest: &Value) -> Result<BTreeMap<String, TappApiDef>, TappCredentialError> {
    manifest
        .get("apis")
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map(|value| value.unwrap_or_default())
        .map_err(|_| TappCredentialError::InvalidDefinition("Invalid API declaration".into()))
}

fn canonicalize_json(value: Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize_json).collect()),
        Value::Object(fields) => {
            let mut fields: Vec<_> = fields.into_iter().collect();
            fields.sort_by(|left, right| left.0.cmp(&right.0));
            Value::Object(
                fields
                    .into_iter()
                    .map(|(key, value)| (key, canonicalize_json(value)))
                    .collect(),
            )
        }
        scalar => scalar,
    }
}

pub fn credential_definition(
    manifest: &Value,
    key: &str,
) -> Result<TappCredentialDef, TappCredentialError> {
    parse_credentials(manifest)?
        .into_iter()
        .find(|definition| definition.key == key)
        .ok_or_else(|| {
            TappCredentialError::InvalidDefinition(format!(
                "Credential '{key}' is not declared by this Tapp"
            ))
        })
}

fn api_uses_outbound_credential(api: &TappApiDef, key: &str) -> bool {
    api.credential
        .as_ref()
        .is_some_and(|binding| binding.key == key)
}

fn api_uses_inbound_verify(api: &TappApiDef, key: &str) -> bool {
    api.route
        .as_ref()
        .is_some_and(|route| route.verify.key == key)
}

#[derive(Serialize)]
struct InboundVerifyContract {
    api: String,
    path: String,
    methods: Vec<String>,
    verify: TappApiRoute,
}

pub fn credential_binding_fingerprint(
    manifest: &Value,
    key: &str,
) -> Result<String, TappCredentialError> {
    let definition = credential_definition(manifest, key)?;
    let apis = parse_apis(manifest)?;
    let bindings: BTreeMap<String, TappApiDef> = apis
        .iter()
        .filter(|(_, api)| api_uses_outbound_credential(api, key))
        .map(|(name, api)| {
            let mut api = api.clone();
            // Inbound mounts are fingerprinted separately so adding a route
            // that uses another key does not force re-entry of outbound secrets.
            api.route = None;
            (name.clone(), api)
        })
        .collect();
    let inbound: BTreeMap<String, InboundVerifyContract> = apis
        .into_iter()
        .filter(|(_, api)| api_uses_inbound_verify(api, key))
        .map(|(name, api)| {
            let route = api.route.expect("checked inbound verify");
            (
                name.clone(),
                InboundVerifyContract {
                    api: name,
                    path: route.path.clone(),
                    methods: route.methods.clone(),
                    verify: route,
                },
            )
        })
        .collect();
    if bindings.is_empty() && inbound.is_empty() {
        return Err(TappCredentialError::InvalidDefinition(format!(
            "Credential '{key}' is not bound to any declared API"
        )));
    }
    #[derive(Serialize)]
    struct BindingContract {
        definition: TappCredentialDef,
        bindings: BTreeMap<String, TappApiDef>,
        #[serde(skip_serializing_if = "BTreeMap::is_empty")]
        inbound: BTreeMap<String, InboundVerifyContract>,
    }
    let binding_contract = serde_json::to_value(BindingContract {
        definition,
        bindings,
        inbound,
    })
    .map_err(|_| TappCredentialError::InvalidDefinition("Credential binding is invalid".into()))?;
    let encoded = serde_json::to_vec(&canonicalize_json(binding_contract)).map_err(|_| {
        TappCredentialError::InvalidDefinition("Credential binding is invalid".into())
    })?;
    Ok(hex::encode(Sha256::digest(encoded)))
}

pub fn credential_binding_origins(
    manifest: &Value,
    key: &str,
) -> Result<Vec<String>, TappCredentialError> {
    let mut origins = BTreeSet::new();
    for api in parse_apis(manifest)?.into_values().filter(|api| {
        api.credential
            .as_ref()
            .is_some_and(|binding| binding.key == key)
    }) {
        let endpoint = api.endpoint.as_deref().ok_or_else(|| {
            TappCredentialError::InvalidDefinition("Credential API has no endpoint".into())
        })?;
        let url = url::Url::parse(endpoint).map_err(|_| {
            TappCredentialError::InvalidDefinition(
                "Credential-bound API endpoint must have a fixed absolute origin".into(),
            )
        })?;
        let origin = url.origin().ascii_serialization();
        origins.insert(origin);
    }
    if origins.is_empty()
        && !parse_apis(manifest)?
            .values()
            .any(|api| api_uses_inbound_verify(api, key))
    {
        return Err(TappCredentialError::InvalidDefinition(format!(
            "Credential '{key}' is not bound to any declared API"
        )));
    }
    Ok(origins.into_iter().collect())
}

pub fn credential_binding_summaries(
    manifest: &Value,
    key: &str,
) -> Result<Vec<TappCredentialBindingSummary>, TappCredentialError> {
    let mut summaries = Vec::new();
    for (api_name, api) in parse_apis(manifest)? {
        let Some(binding) = &api.credential else {
            continue;
        };
        if binding.key != key {
            continue;
        }
        let resolved = binding
            .resolve()
            .map_err(TappCredentialError::InvalidDefinition)?;
        summaries.push(TappCredentialBindingSummary {
            api: api_name,
            method: api.method,
            endpoint: api.endpoint.unwrap_or_default(),
            access: match api.access {
                myriad_tapp_contract::manifest::TappApiAccess::Public => "public".into(),
                myriad_tapp_contract::manifest::TappApiAccess::Protected => "protected".into(),
                myriad_tapp_contract::manifest::TappApiAccess::Manager => "manager".into(),
            },
            placement: resolved.placement.as_str().to_string(),
            field: resolved.field,
            sign_alg: resolved
                .sign
                .as_ref()
                .map(|sign| sign.alg.as_str().to_string()),
            sign_over: resolved.sign.map(|sign| sign.over).unwrap_or_default(),
        });
    }
    for (api_name, api) in parse_apis(manifest)? {
        let Some(route) = &api.route else {
            continue;
        };
        if route.verify.key != key {
            continue;
        }
        summaries.push(TappCredentialBindingSummary {
            api: api_name,
            method: route.methods.join(","),
            endpoint: route.path.clone(),
            access: match api.access {
                myriad_tapp_contract::manifest::TappApiAccess::Public => "public".into(),
                myriad_tapp_contract::manifest::TappApiAccess::Protected => "protected".into(),
                myriad_tapp_contract::manifest::TappApiAccess::Manager => "manager".into(),
            },
            placement: "verify".into(),
            field: route.verify.header.clone(),
            sign_alg: Some(route.verify.alg.as_str().to_string()),
            sign_over: vec![route.verify.over.as_str().to_string()],
        });
    }
    summaries.sort_by(|left, right| {
        left.api
            .cmp(&right.api)
            .then(left.placement.cmp(&right.placement))
    });
    Ok(summaries)
}

pub async fn put_credential(
    db: &impl ConnectionTrait,
    owner_id: i32,
    tapp_id: &str,
    key: &str,
    value: &str,
    binding_fingerprint: &str,
) -> Result<(), TappCredentialError> {
    if value.is_empty() || value.len() > MAX_CREDENTIAL_VALUE_LEN {
        return Err(TappCredentialError::InvalidValue);
    }
    let encrypted = crate::services::data_key::data_key()
        .encrypt(value)
        .map_err(|error| {
            tracing::error!(tapp_id, owner_id, credential_key = key, %error, "Failed to encrypt Tapp credential");
            TappCredentialError::Encryption
        })?;

    // A credential replacing a legacy setting with the same key must remove
    // the publicly readable copy in the same transaction as the encrypted
    // write. Doing this first also frees its quota before the encrypted row is
    // measured; the caller transaction restores it if the upsert fails.
    tapp_storage::Entity::delete_many()
        .filter(tapp_storage::Column::UserId.eq(owner_id))
        .filter(tapp_storage::Column::TappId.eq(tapp_id))
        .filter(tapp_storage::Column::Key.eq(format!("_settings.{key}")))
        .exec(db)
        .await
        .map_err(|error| {
            tracing::error!(tapp_id, owner_id, credential_key = key, %error, "Failed to remove legacy public Tapp setting");
            TappCredentialError::Database
        })?;

    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"
INSERT INTO tapp_storage
    (user_id, tapp_id, key, value, encrypted_value, binding_fingerprint, created_at, updated_at)
VALUES ($1, $2, $3, '{"kind":"credential","version":1}'::jsonb, $4, $5, NOW(), NOW())
ON CONFLICT (user_id, tapp_id, key) DO UPDATE SET
    value = EXCLUDED.value,
    encrypted_value = EXCLUDED.encrypted_value,
    binding_fingerprint = EXCLUDED.binding_fingerprint,
    updated_at = NOW()
"#,
        vec![
            owner_id.into(),
            tapp_id.into(),
            storage_key(key).into(),
            encrypted.into(),
            binding_fingerprint.into(),
        ],
    ))
    .await
    .map_err(|error| {
        tracing::error!(tapp_id, owner_id, credential_key = key, %error, "Failed to store Tapp credential");
        TappCredentialError::Database
    })?;

    Ok(())
}

pub async fn delete_credential(
    db: &impl ConnectionTrait,
    owner_id: i32,
    tapp_id: &str,
    key: &str,
) -> Result<(), TappCredentialError> {
    tapp_storage::Entity::delete_many()
        .filter(tapp_storage::Column::UserId.eq(owner_id))
        .filter(tapp_storage::Column::TappId.eq(tapp_id))
        .filter(tapp_storage::Column::Key.eq(storage_key(key)))
        .exec(db)
        .await
        .map_err(|_| TappCredentialError::Database)?;
    Ok(())
}

pub async fn credential_statuses(
    db: &DatabaseConnection,
    tapp: &tapps::Model,
) -> Result<Vec<TappCredentialStatus>, TappCredentialError> {
    let stored = stored_credential_statuses(db, tapp.user_id, &tapp.tapp_id).await?;

    parse_credentials(&tapp.manifest)?
        .into_iter()
        .map(|definition| {
            let fingerprint = credential_binding_fingerprint(&tapp.manifest, &definition.key)?;
            let origins = credential_binding_origins(&tapp.manifest, &definition.key)?;
            let bindings = credential_binding_summaries(&tapp.manifest, &definition.key)?;
            let row = stored.get(&definition.key);
            Ok(TappCredentialStatus {
                key: definition.key,
                configured: row.is_some_and(|row| row.configured),
                needs_reauthorization: row.filter(|row| row.configured).is_some_and(|row| {
                    row.binding_fingerprint.as_deref() != Some(fingerprint.as_str())
                }),
                origins,
                bindings,
                updated_at: row.map(|row| row.updated_at.to_rfc3339()),
            })
        })
        .collect()
}

async fn stored_credential_statuses(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
) -> Result<BTreeMap<String, StoredCredentialStatus>, TappCredentialError> {
    // Status/UI callers need only presence and authorization metadata. Keep
    // ciphertext out of this code path entirely instead of loading a full
    // tapp_storage model and relying only on response serialization filters.
    let stored = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT key,
       encrypted_value IS NOT NULL AS configured,
       binding_fingerprint,
       updated_at
FROM tapp_storage
WHERE user_id = $1
  AND tapp_id = $2
  AND starts_with(key, $3)
"#,
            vec![
                owner_id.into(),
                tapp_id.into(),
                CREDENTIAL_STORAGE_PREFIX.into(),
            ],
        ))
        .await
        .map_err(|_| TappCredentialError::Database)?
        .into_iter()
        .map(|row| {
            StoredCredentialStatus::from_query_result(&row, "")
                .map_err(|_| TappCredentialError::Database)
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(stored
        .into_iter()
        .filter_map(|row| {
            let key = row.key.strip_prefix(CREDENTIAL_STORAGE_PREFIX)?.to_string();
            Some((key, row))
        })
        .collect())
}

pub async fn resolve_credential_by_key(
    db: &DatabaseConnection,
    tapp: &tapps::Model,
    key: &str,
) -> Result<ResolvedApiCredential, TappCredentialError> {
    let expected = credential_binding_fingerprint(&tapp.manifest, key)?;
    let row = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(tapp.user_id))
        .filter(tapp_storage::Column::TappId.eq(&tapp.tapp_id))
        .filter(tapp_storage::Column::Key.eq(storage_key(key)))
        .one(db)
        .await
        .map_err(|_| TappCredentialError::Database)?
        .ok_or(TappCredentialError::Missing)?;
    if row.binding_fingerprint.as_deref() != Some(expected.as_str()) {
        return Err(TappCredentialError::ReauthorizationRequired);
    }
    let encrypted_value = row.encrypted_value.ok_or(TappCredentialError::Missing)?;
    let value = crate::services::data_key::data_key()
        .decrypt(&encrypted_value)
        .map_err(|error| {
            tracing::error!(
                tapp_id = %tapp.tapp_id,
                owner_id = tapp.user_id,
                credential_key = %key,
                %error,
                "Failed to decrypt Tapp credential"
            );
            TappCredentialError::Encryption
        })?;
    let revision = hex::encode(Sha256::digest(encrypted_value.as_bytes()));
    Ok(ResolvedApiCredential { value, revision })
}

pub async fn resolve_api_credential(
    db: &DatabaseConnection,
    tapp: &tapps::Model,
    api_def: &TappApiDef,
) -> Result<Option<ResolvedApiCredential>, TappCredentialError> {
    let Some(binding) = &api_def.credential else {
        return Ok(None);
    };
    resolve_credential_by_key(db, tapp, &binding.key)
        .await
        .map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest(endpoint: &str, header: &str) -> Value {
        json!({
            "credentials": [{ "key": "wegame", "label": "WeGame API Key" }],
            "apis": {
                "games": {
                    "type": "http",
                    "access": "public",
                    "endpoint": endpoint,
                    "headers": { "Accept": "application/json" },
                    "credential": {
                        "key": "wegame",
                        "header": header,
                        "prefix": "Bearer "
                    }
                }
            }
        })
    }

    #[test]
    fn binding_fingerprint_tracks_secret_destination_and_header() {
        let first = credential_binding_fingerprint(
            &manifest("https://api.example.com/games", "Authorization"),
            "wegame",
        )
        .unwrap();
        let same = credential_binding_fingerprint(
            &manifest("https://api.example.com/games", "Authorization"),
            "wegame",
        )
        .unwrap();
        let changed_endpoint = credential_binding_fingerprint(
            &manifest("https://evil.example/collect", "Authorization"),
            "wegame",
        )
        .unwrap();
        let changed_header = credential_binding_fingerprint(
            &manifest("https://api.example.com/games", "X-Api-Key"),
            "wegame",
        )
        .unwrap();
        assert_eq!(first, same);
        assert_ne!(first, changed_endpoint);
        assert_ne!(first, changed_header);
    }

    #[test]
    fn binding_fingerprint_is_stable_across_nested_map_order_and_reparsing() {
        let first: Value = serde_json::from_str(
            r#"{
                "credentials":[{"key":"wegame","label":"WeGame API Key"}],
                "apis":{"games":{
                    "type":"http","access":"public","endpoint":"https://api.example.com/games",
                    "headers":{"X-Z":"z","X-A":"a","X-M":"m"},
                    "inject":{"zone":"{{geo.region}}","city":"{{geo.city}}"},
                    "credential":{"key":"wegame","header":"Authorization","prefix":"Bearer "}
                }}
            }"#,
        )
        .unwrap();
        let reordered: Value = serde_json::from_str(
            r#"{
                "apis":{"games":{
                    "inject":{"city":"{{geo.city}}","zone":"{{geo.region}}"},
                    "headers":{"X-M":"m","X-A":"a","X-Z":"z"},
                    "credential":{"prefix":"Bearer ","header":"Authorization","key":"wegame"},
                    "endpoint":"https://api.example.com/games","access":"public","type":"http"
                }},
                "credentials":[{"label":"WeGame API Key","key":"wegame"}]
            }"#,
        )
        .unwrap();

        let expected = credential_binding_fingerprint(&first, "wegame").unwrap();
        assert_eq!(
            expected,
            credential_binding_fingerprint(&reordered, "wegame").unwrap()
        );
        for _ in 0..32 {
            assert_eq!(
                expected,
                credential_binding_fingerprint(&first, "wegame").unwrap()
            );
        }
    }

    #[test]
    fn inbound_verify_changes_fingerprint_and_allows_empty_origins() {
        let outbound = manifest("https://api.example.com/games", "Authorization");
        let inbound_only = json!({
            "credentials": [{ "key": "wegame", "label": "WeGame API Key" }],
            "apis": {
                "games": {
                    "type": "http",
                    "access": "public",
                    "endpoint": "https://api.example.com/games",
                    "route": {
                        "path": "/sponsors",
                        "methods": ["GET"],
                        "verify": {
                            "key": "wegame",
                            "alg": "hmac-sha256-raw",
                            "header": "X-Signature",
                            "over": "canonical-query",
                            "timestampHeader": "X-Timestamp",
                            "nonceHeader": "X-Nonce"
                        }
                    }
                }
            }
        });
        let outbound_fp = credential_binding_fingerprint(&outbound, "wegame").unwrap();
        let inbound_fp = credential_binding_fingerprint(&inbound_only, "wegame").unwrap();
        assert_ne!(outbound_fp, inbound_fp);
        assert!(credential_binding_origins(&inbound_only, "wegame")
            .unwrap()
            .is_empty());
        let summaries = credential_binding_summaries(&inbound_only, "wegame").unwrap();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].placement, "verify");
        assert_eq!(summaries[0].endpoint, "/sponsors");
    }

    #[test]
    fn outbound_fingerprint_ignores_unrelated_inbound_route() {
        let outbound = manifest("https://api.example.com/games", "Authorization");
        let mut with_other_route = outbound.clone();
        with_other_route["credentials"]
            .as_array_mut()
            .unwrap()
            .push(json!({ "key": "inbound", "label": "Inbound HMAC" }));
        with_other_route["apis"]["games"]["route"] = json!({
            "path": "/sponsors",
            "methods": ["GET"],
            "verify": {
                "key": "inbound",
                "alg": "hmac-sha256-raw",
                "header": "X-Signature",
                "over": "canonical-query",
                "timestampHeader": "X-Timestamp",
                "nonceHeader": "X-Nonce"
            }
        });
        assert_eq!(
            credential_binding_fingerprint(&outbound, "wegame").unwrap(),
            credential_binding_fingerprint(&with_other_route, "wegame").unwrap()
        );
        assert!(credential_binding_fingerprint(&with_other_route, "inbound").is_ok());
    }

    #[test]
    fn origins_never_include_paths_or_queries() {
        assert_eq!(
            credential_binding_origins(
                &manifest(
                    "https://api.example.com/v1/games?user={{params.id}}",
                    "X-Key"
                ),
                "wegame",
            )
            .unwrap(),
            vec!["https://api.example.com"]
        );
    }

    #[test]
    fn public_status_and_debug_output_never_serialize_secret_material() {
        let status = TappCredentialStatus {
            key: "wegame".into(),
            configured: true,
            needs_reauthorization: false,
            origins: vec!["https://api.example.com".into()],
            bindings: Vec::new(),
            updated_at: None,
        };
        let serialized = serde_json::to_value(status).unwrap();
        assert!(serialized.get("value").is_none());
        assert!(serialized.get("encryptedValue").is_none());

        let resolved = ResolvedApiCredential::for_test("top-secret");
        assert!(!format!("{resolved:?}").contains("top-secret"));
    }

    #[tokio::test]
    async fn postgres_status_projection_reads_metadata_without_credential_payload() {
        let Ok(url) = std::env::var("MYRIAD_TAPP_STORAGE_GUARD_DB") else {
            eprintln!("skipping: set MYRIAD_TAPP_STORAGE_GUARD_DB for the PostgreSQL guard test");
            return;
        };
        let db = sea_orm::Database::connect(&url)
            .await
            .expect("connect guard database");
        let owner_id = 2_147_483_599_i32;
        let tapp_id = "codex.credential.status.guard";
        let cleanup = || {
            Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM tapp_storage WHERE user_id = $1 AND tapp_id = $2",
                vec![owner_id.into(), tapp_id.into()],
            )
        };
        db.execute_raw(cleanup()).await.expect("clean guard rows");
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
INSERT INTO tapp_storage
    (user_id, tapp_id, key, value, encrypted_value, binding_fingerprint, created_at, updated_at)
VALUES
    ($1, $2, '_credentials.wegame', '{"kind":"credential","version":1}'::jsonb,
     'ciphertext-must-not-be-selected', $3, NOW(), NOW())
"#,
            vec![owner_id.into(), tapp_id.into(), "f".repeat(64).into()],
        ))
        .await
        .expect("insert credential guard row");

        let statuses = stored_credential_statuses(&db, owner_id, tapp_id)
            .await
            .expect("project credential status");
        let status = statuses.get("wegame").expect("wegame status");
        assert!(status.configured);
        assert_eq!(
            status.binding_fingerprint.as_deref(),
            Some("f".repeat(64).as_str())
        );

        db.execute_raw(cleanup()).await.expect("remove guard rows");
    }
}
