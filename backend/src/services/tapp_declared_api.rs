//! Declared Tapp API catalog (manifest `apis` parse cache + install binding).
//!
//! Domain lives in services so install/uninstall paths and future agent callers
//! can invalidate/list without importing `api::tapp_runtime::declared_api`.
//! HTTP handlers map [`DeclaredApiError`] to Axum and own grant/rate-limit checks.
//! Actual outbound execution remains [`crate::services::tapp_api_service`].

use once_cell::sync::Lazy;
use sea_orm::DatabaseConnection;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::models::entities::tapps;
use crate::services::permission_service::{TappPermission, TappPermissionService, UserRole};
use crate::services::tapp_ownership::{self, TappAccessError};
use crate::GLOBAL_DYNAMIC_CONFIG;
use myriad_tapp_contract::manifest::{TappApiAccess, TappApiDef};

/// 缓存条目：已解析的 API 定义 + 缓存时间
struct ApisCacheEntry {
    tapp_id: String,
    cache_scope: String,
    apis: HashMap<String, TappApiDef>,
    cached_at: Instant,
}

/// 进程内解析缓存。key 包含 Manifest APIs 内容指纹，因此其他副本更新数据库后，
/// 本副本下一次请求也不会继续命中旧定义。
static TAPP_APIS_CACHE: Lazy<RwLock<HashMap<String, ApisCacheEntry>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

const APIS_CACHE_TTL: Duration = Duration::from_secs(300);
const MAX_APIS_CACHE_ENTRIES: usize = 1024;

/// Domain errors for declared-API catalog / install binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeclaredApiError {
    Access(TappAccessError),
    GrantScopeChanged,
    InvalidUser,
    ApiNotFound { api_name: String },
}

impl DeclaredApiError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Access(err) => match err {
                TappAccessError::Database => "DATABASE_ERROR",
                TappAccessError::NoAdmin => "NO_ADMIN",
                TappAccessError::AccessDenied { .. } => "ACCESS_DENIED",
                TappAccessError::PermissionNotGranted { .. } => "TAPP_PERMISSION_NOT_GRANTED",
            },
            Self::GrantScopeChanged => "INVALID_RUNTIME_GRANT",
            Self::InvalidUser => "INVALID_USER",
            Self::ApiNotFound { .. } => "API_NOT_FOUND",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Access(err) => err.message(),
            Self::GrantScopeChanged => "Runtime grant installation scope changed".to_string(),
            Self::InvalidUser => "Invalid user".to_string(),
            Self::ApiNotFound { api_name } => {
                format!("API '{api_name}' not defined in manifest")
            }
        }
    }

    pub fn status_hint(&self) -> u16 {
        match self {
            Self::Access(err) => match err {
                TappAccessError::Database | TappAccessError::NoAdmin => 500,
                TappAccessError::AccessDenied { .. }
                | TappAccessError::PermissionNotGranted { .. } => 403,
            },
            Self::GrantScopeChanged | Self::InvalidUser => 401,
            Self::ApiNotFound { .. } => 404,
        }
    }
}

impl std::fmt::Display for DeclaredApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for DeclaredApiError {}

pub(crate) fn manifest_apis_fingerprint(manifest: &Value) -> String {
    let encoded = serde_json::to_vec(manifest.get("apis").unwrap_or(&Value::Null))
        .unwrap_or_else(|_| b"null".to_vec());
    hex::encode(Sha256::digest(encoded))
}

/// 从 manifest JSON 解析 API 定义，优先命中内存缓存
pub async fn get_tapp_apis(
    cache_scope: &str,
    tapp_id: &str,
    manifest: &Value,
) -> HashMap<String, TappApiDef> {
    let cache_key = format!("{cache_scope}:{}", manifest_apis_fingerprint(manifest));
    // 读缓存
    {
        let cache = TAPP_APIS_CACHE.read().await;
        if let Some(entry) = cache.get(&cache_key) {
            if entry.cached_at.elapsed() < APIS_CACHE_TTL {
                return entry.apis.clone();
            }
        }
    }

    // 缓存未命中，解析 manifest
    let apis: HashMap<String, TappApiDef> = manifest
        .get("apis")
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .unwrap_or_default();

    // 写缓存
    {
        let mut cache = TAPP_APIS_CACHE.write().await;
        cache.retain(|_, entry| {
            entry.cached_at.elapsed() < APIS_CACHE_TTL && entry.cache_scope != cache_scope
        });
        while cache.len() >= MAX_APIS_CACHE_ENTRIES {
            let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, entry)| entry.cached_at)
                .map(|(key, _)| key.clone())
            else {
                break;
            };
            cache.remove(&oldest);
        }
        cache.insert(
            cache_key,
            ApisCacheEntry {
                tapp_id: tapp_id.to_string(),
                cache_scope: cache_scope.to_string(),
                apis: apis.clone(),
                cached_at: Instant::now(),
            },
        );
    }

    apis
}

/// Tapp 更新/卸载时使缓存失效
pub async fn invalidate_tapp_apis_cache(tapp_id: &str) {
    let mut cache = TAPP_APIS_CACHE.write().await;
    cache.retain(|_, entry| entry.tapp_id != tapp_id);
}

/// Resolve the install used for declared APIs, matching Runtime Grant issuance:
/// private subject install first, then site-owner public install.
/// When a Runtime Grant is present, require its owner_id to match so apis and
/// approved_permissions come from the same install the grant was issued for.
pub async fn resolve_declared_api_tapp(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    grant_owner_id: i32,
) -> Result<tapps::Model, DeclaredApiError> {
    let tapp = tapp_ownership::resolve_accessible_tapp(db, user_id, tapp_id)
        .await
        .map_err(DeclaredApiError::Access)?;
    if tapp.user_id != grant_owner_id {
        return Err(DeclaredApiError::GrantScopeChanged);
    }
    Ok(tapp)
}

/// Filter installed permissions through the caller's current role + dynamic config.
pub async fn filter_granted_permissions(
    installed_permissions: Vec<String>,
    role: UserRole,
) -> Vec<String> {
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    installed_permissions
        .into_iter()
        .filter(|permission| {
            TappPermission::from_str(permission).is_some_and(|permission| {
                TappPermissionService::check(&config, role, permission)
            })
        })
        .collect()
}

/// Parse approved_permissions JSON array from a Tapp install row.
pub fn installed_permissions_from_tapp(tapp: &tapps::Model) -> Vec<String> {
    tapp.approved_permissions
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect()
        })
        .unwrap_or_default()
}

/// Lookup a named API definition from a cached/parsed map.
pub fn require_api_def<'a>(
    apis: &'a HashMap<String, TappApiDef>,
    api_name: &str,
) -> Result<&'a TappApiDef, DeclaredApiError> {
    apis.get(api_name)
        .ok_or_else(|| DeclaredApiError::ApiNotFound {
            api_name: api_name.to_string(),
        })
}

/// Public list view of declared APIs (no secrets).
pub fn list_api_summaries(apis: &HashMap<String, TappApiDef>) -> Vec<Value> {
    apis.iter()
        .map(|(name, def)| {
            json!({
                "name": name,
                "access": match def.access {
                    TappApiAccess::Public => "public",
                    TappApiAccess::Protected => "protected",
                    TappApiAccess::Manager => "manager",
                },
                "type": def.api_type,
                "description": def.description,
                "cacheTtl": def.cache_ttl,
            })
        })
        .collect()
}

/// AI model tier from manifest `/ai/modelTier`.
pub fn ai_model_tier_from_manifest(
    manifest: &Value,
) -> Option<crate::config::ModelTier> {
    manifest
        .pointer("/ai/modelTier")
        .and_then(Value::as_str)
        .map(|tier| {
            if tier == "pro" {
                crate::config::ModelTier::Pro
            } else {
                crate::config::ModelTier::Standard
            }
        })
}

#[cfg(test)]
mod tests {
    use super::{
        list_api_summaries, manifest_apis_fingerprint, DeclaredApiError,
    };
    use crate::services::tapp_ownership::tapp_owner_priority;
    use myriad_tapp_contract::manifest::{TappApiAccess, TappApiDef};
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn declared_api_cache_fingerprint_tracks_only_api_contract() {
        let first = json!({
            "name": "Example",
            "apis": { "weather": { "endpoint": "https://one.example" } }
        });
        let metadata_only = json!({
            "name": "Renamed",
            "apis": { "weather": { "endpoint": "https://one.example" } }
        });
        let changed_api = json!({
            "name": "Example",
            "apis": { "weather": { "endpoint": "https://two.example" } }
        });

        assert_eq!(
            manifest_apis_fingerprint(&first),
            manifest_apis_fingerprint(&metadata_only)
        );
        assert_ne!(
            manifest_apis_fingerprint(&first),
            manifest_apis_fingerprint(&changed_api)
        );
    }

    /// Declared API resolution uses `resolve_accessible_tapp`, which sorts by
    /// `tapp_owner_priority`. Keep this contract aligned with ownership:
    /// private install precedes same-id admin public install.
    #[test]
    fn declared_api_install_priority_matches_runtime_private_first() {
        assert_eq!(tapp_owner_priority(42, 42, 1), 0);
        assert_eq!(tapp_owner_priority(1, 42, 1), 1);
        assert_eq!(tapp_owner_priority(99, 42, 1), 2);
        assert_eq!(tapp_owner_priority(1, -1, 1), 1);
        assert_eq!(tapp_owner_priority(99, -1, 1), 2);
    }

    #[test]
    fn error_codes_preserve_api_contract() {
        assert_eq!(
            DeclaredApiError::GrantScopeChanged.code(),
            "INVALID_RUNTIME_GRANT"
        );
        assert_eq!(
            DeclaredApiError::ApiNotFound {
                api_name: "weather".into()
            }
            .code(),
            "API_NOT_FOUND"
        );
        assert_eq!(
            DeclaredApiError::ApiNotFound {
                api_name: "weather".into()
            }
            .message(),
            "API 'weather' not defined in manifest"
        );
        assert_eq!(DeclaredApiError::GrantScopeChanged.status_hint(), 401);
        assert_eq!(
            DeclaredApiError::ApiNotFound {
                api_name: "x".into()
            }
            .status_hint(),
            404
        );
    }

    #[test]
    fn list_summaries_expose_access_without_secrets() {
        let mut apis = HashMap::new();
        apis.insert(
            "weather".to_string(),
            TappApiDef {
                access: TappApiAccess::Public,
                api_type: "http".into(),
                endpoint: None,
                method: "GET".into(),
                headers: None,
                credential: None,
                body_mode: myriad_tapp_contract::manifest::TappHttpBodyMode::Json,
                body: None,
                builtin: None,
                inject: None,
                cache_ttl: 60,
                spoof: None,
                description: Some("Weather".into()),
            },
        );
        let list = list_api_summaries(&apis);
        assert_eq!(list.len(), 1);
        assert_eq!(list[0]["name"], "weather");
        assert_eq!(list[0]["access"], "public");
        assert_eq!(list[0]["type"], "http");
        assert!(list[0].get("endpoint").is_none());
    }
}
