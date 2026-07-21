//! Site public-identity domain change (non-federation).
//!
//! When an admin changes the public site origin, rewrite `.env` keys the backend
//! already owns: `BASE_URL`, `FRONTEND_URL`, `CORS_ORIGINS`. Does **not** touch
//! federation Move / ActivityPub actor rewrites (see federation domain-move work).

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use axum::{extract::State, http::StatusCode, Json};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use url::Url;

use crate::services::config_service::ConfigService;

/// Request body for `POST /api/admin/site/domain`.
#[derive(Debug, Clone, Deserialize)]
pub struct ChangeSiteDomainRequest {
    /// New public origin, e.g. `https://new.example`.
    pub new_origin: String,
    /// Optional previous origin used to rewrite CORS list entries.
    /// When omitted, falls back to current `BASE_URL` (env / DB).
    #[serde(default)]
    pub previous_origin: Option<String>,
}

/// Operator checklist returned after a successful domain change.
#[derive(Debug, Clone, Serialize)]
pub struct DomainMigrationChecklist {
    pub dns: ChecklistItem,
    pub tls: ChecklistItem,
    pub reverse_proxy_301: ChecklistItem,
    pub oauth_callbacks: ChecklistItem,
    pub federation_move_separate: ChecklistItem,
    /// CorsLayer is built at process start; restart (or stack recreate) so the
    /// new `CORS_ORIGINS` is applied to the HTTP layer.
    pub backend_restart_for_cors: ChecklistItem,
}

#[derive(Debug, Clone, Serialize)]
pub struct ChecklistItem {
    pub key: &'static str,
    /// `auto` = backend rewrote env/config; `manual` = operator action required.
    pub status: &'static str,
    pub summary: &'static str,
}

/// Validate and normalize a public site origin.
///
/// Rules:
/// - `https` always allowed; `http` only for localhost / 127.0.0.1 / ::1
/// - No path beyond `/`, no query, no fragment
/// - Returns canonical form without trailing slash: `scheme://host[:port]`
pub fn validate_and_normalize_origin(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("Origin must not be empty".to_string());
    }
    if trimmed.contains('*') {
        return Err("Wildcard origins are not allowed".to_string());
    }

    let parsed = Url::parse(trimmed).map_err(|e| format!("Invalid URL: {e}"))?;

    let scheme = parsed.scheme();
    let host = parsed
        .host_str()
        .ok_or_else(|| "Origin must include a host".to_string())?;

    let is_loopback = host.eq_ignore_ascii_case("localhost")
        || host == "127.0.0.1"
        || host == "[::1]"
        || host == "::1";

    match scheme {
        "https" => {}
        "http" if is_loopback => {}
        "http" => {
            return Err(
                "http is only allowed for localhost / 127.0.0.1; use https in production"
                    .to_string(),
            );
        }
        _ => {
            return Err(format!(
                "Unsupported scheme '{scheme}'; use https (or http for localhost only)"
            ));
        }
    }

    // Reject credentials, query, fragment, and non-root paths.
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return Err("Origin must not include credentials".to_string());
    }
    if parsed.query().is_some() {
        return Err("Origin must not include a query string".to_string());
    }
    if parsed.fragment().is_some() {
        return Err("Origin must not include a fragment".to_string());
    }

    let path = parsed.path();
    if path != "/" && !path.is_empty() {
        return Err(
            "Origin must not include a path (use https://example.com, not .../path)".to_string(),
        );
    }

    // Rebuild as scheme://host[:port] without trailing slash.
    let mut origin = format!("{scheme}://{host}");
    if let Some(port) = parsed.port() {
        // Url::port() only returns non-default ports, which is what we want.
        origin.push(':');
        origin.push_str(&port.to_string());
    }

    Ok(origin)
}

/// Merge / replace CORS origins for a domain change.
///
/// - Never emits `*`
/// - If `previous_origin` is present in the list, replace that entry with `new_origin`
/// - Otherwise ensure `new_origin` is present (append if missing)
/// - Preserve other origins (multi-origin setups)
/// - Empty / missing current list becomes just `new_origin`
pub fn merge_cors_origins(
    current_cors: &str,
    previous_origin: Option<&str>,
    new_origin: &str,
) -> String {
    let new_origin = new_origin.trim().trim_end_matches('/');
    let previous = previous_origin
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty());

    let mut seen = BTreeSet::new();
    let mut ordered: Vec<String> = Vec::new();

    for part in current_cors.split(',') {
        let origin = part.trim().trim_end_matches('/');
        if origin.is_empty() || origin == "*" {
            continue;
        }
        let next = if previous.as_deref() == Some(origin) {
            new_origin.to_string()
        } else {
            origin.to_string()
        };
        if seen.insert(next.clone()) {
            ordered.push(next);
        }
    }

    if !seen.contains(new_origin) {
        ordered.push(new_origin.to_string());
    }

    // Defensive: never return empty when we have a valid new origin.
    if ordered.is_empty() {
        return new_origin.to_string();
    }

    ordered.join(",")
}

/// Build operator checklist for post-domain-change work.
pub fn build_domain_migration_checklist() -> DomainMigrationChecklist {
    DomainMigrationChecklist {
        dns: ChecklistItem {
            key: "dns",
            status: "manual",
            summary: "Point DNS A/AAAA (or CNAME) for the new hostname to this host or load balancer",
        },
        tls: ChecklistItem {
            key: "tls",
            status: "manual",
            summary: "Issue or attach a TLS certificate covering the new hostname",
        },
        reverse_proxy_301: ChecklistItem {
            key: "reverse_proxy_301",
            status: "manual",
            summary: "Configure reverse proxy 301/308 redirects from the old origin to the new one",
        },
        oauth_callbacks: ChecklistItem {
            key: "oauth_callbacks",
            status: "manual",
            summary:
                "Update OAuth app callback URLs in GitHub/Google/etc. consoles to the new origin",
        },
        federation_move_separate: ChecklistItem {
            key: "federation_move_separate",
            status: "manual",
            summary:
                "Federation actor Move is separate — use federation domain-move when available; this API does not rewrite federation tables",
        },
        backend_restart_for_cors: ChecklistItem {
            key: "backend_restart_for_cors",
            status: "manual",
            summary:
                "Restart backend (or full compose stack) so the HTTP CorsLayer reloads CORS_ORIGINS from env",
        },
    }
}

/// Read a single key from raw `.env` content (best-effort, no full shell quoting).
pub fn read_env_key(content: &str, key: &str) -> Option<String> {
    let key_prefix = format!("{key}=");
    let commented = format!("# {key}=");
    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(&commented) {
            continue;
        }
        if let Some(rest) = trimmed.strip_prefix(&key_prefix) {
            let value = strip_env_quotes(rest.trim());
            if value.is_empty() {
                return None;
            }
            return Some(value);
        }
    }
    None
}

fn strip_env_quotes(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return value[1..value.len() - 1]
                .replace("\\\"", "\"")
                .replace("\\\\", "\\");
        }
    }
    value.to_string()
}

/// Apply domain-related keys onto `.env` content.
///
/// Writes `BASE_URL`, `FRONTEND_URL`, and merged `CORS_ORIGINS`.
/// Returns updated content and the normalized new origin.
pub fn apply_site_domain_to_env_content(
    env_content: &str,
    new_origin: &str,
    previous_origin: Option<&str>,
) -> Result<(String, String), String> {
    let normalized = validate_and_normalize_origin(new_origin)?;

    let previous = previous_origin
        .and_then(|s| {
            if s.trim().is_empty() {
                None
            } else {
                validate_and_normalize_origin(s).ok()
            }
        })
        .or_else(|| read_env_key(env_content, "BASE_URL"))
        .or_else(|| std::env::var("BASE_URL").ok().filter(|s| !s.is_empty()));

    let current_cors = read_env_key(env_content, "CORS_ORIGINS")
        .or_else(|| std::env::var("CORS_ORIGINS").ok())
        .unwrap_or_default();

    let merged_cors =
        merge_cors_origins(&current_cors, previous.as_deref(), &normalized);

    // Never leave production without an explicit CORS list after a domain change.
    if merged_cors.is_empty() {
        return Err("CORS_ORIGINS would be empty after domain change".to_string());
    }

    let mut content = env_content.to_string();
    content = crate::api::config::update_env_var(&content, "BASE_URL", &normalized);
    content = crate::api::config::update_env_var(&content, "FRONTEND_URL", &normalized);
    content = crate::api::config::update_env_var(&content, "CORS_ORIGINS", &merged_cors);

    Ok((content, normalized))
}

fn env_path() -> std::path::PathBuf {
    Path::new(".env").to_path_buf()
}

fn write_and_reload_env(content: &str) -> Result<(), String> {
    let path = env_path();
    let normalized = content.replace("\r\n", "\n");
    fs::write(&path, normalized.as_bytes()).map_err(|e| format!("Failed to write .env: {e}"))?;

    if let Err(e) = dotenvy::from_path_override(&path) {
        tracing::warn!("⚠️ Failed to reload .env after site domain change: {e}");
    } else {
        tracing::info!("♻️ Environment variables reloaded after site domain change");
    }

    crate::api::system::CONFIG_RELOAD_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}

/// `POST /api/admin/site/domain` — atomically update site public origin + env trio.
pub async fn change_site_domain(
    State(db): State<DatabaseConnection>,
    Json(payload): Json<ChangeSiteDomainRequest>,
) -> (StatusCode, Json<Value>) {
    let normalized = match validate_and_normalize_origin(&payload.new_origin) {
        Ok(o) => o,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "success": false,
                    "message": e,
                })),
            );
        }
    };

    let previous_hint = payload
        .previous_origin
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string());

    // Resolve previous from request → DB → env for CORS replace.
    let config_service = ConfigService::new(db);
    let db_previous = config_service
        .load_config()
        .await
        .ok()
        .and_then(|c| c.base_url)
        .filter(|s| !s.is_empty());

    let previous = previous_hint
        .or(db_previous)
        .or_else(|| std::env::var("BASE_URL").ok().filter(|s| !s.is_empty()));

    // 1) Persist base_url in configurations DB.
    if let Err(e) = config_service
        .update_config("base_url", json!(normalized.clone()))
        .await
    {
        tracing::error!("Failed to save base_url to database: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": format!("Failed to save base_url to database: {e}"),
            })),
        );
    }

    // 2) Rewrite .env keys.
    let path = env_path();
    let existing = if path.exists() {
        match fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                tracing::warn!("Failed to read .env as UTF-8: {e}, recovering");
                match fs::read(&path) {
                    Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                    Err(e2) => {
                        return (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(json!({
                                "success": false,
                                "message": format!("Failed to read .env: {e2}"),
                            })),
                        );
                    }
                }
            }
        }
    } else {
        String::new()
    };

    let (updated, applied_origin) = match apply_site_domain_to_env_content(
        &existing,
        &normalized,
        previous.as_deref(),
    ) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "success": false,
                    "message": e,
                })),
            );
        }
    };

    let cors_value = read_env_key(&updated, "CORS_ORIGINS").unwrap_or_default();

    if let Err(e) = write_and_reload_env(&updated) {
        tracing::error!("{e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": e,
            })),
        );
    }

    // 3) Refresh dynamic config cache (base_url used by OAuth URL builder etc.).
    match config_service.load_config().await {
        Ok(dynamic_config) => {
            *crate::GLOBAL_DYNAMIC_CONFIG.write().await = dynamic_config;
            tracing::info!("✅ Global dynamic configuration cache updated after domain change");
        }
        Err(e) => {
            tracing::warn!("⚠️ Failed to reload dynamic config after domain change: {e}");
        }
    }

    let checklist = build_domain_migration_checklist();

    tracing::info!(
        new_origin = %applied_origin,
        previous = ?previous,
        cors = %cors_value,
        "🌐 Site public domain updated (BASE_URL / FRONTEND_URL / CORS_ORIGINS)"
    );

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "Site domain updated. BASE_URL, FRONTEND_URL, and CORS_ORIGINS rewritten. Complete the operator checklist (DNS/TLS/OAuth/restart).",
            "applied": {
                "base_url": applied_origin,
                "frontend_url": applied_origin,
                "cors_origins": cors_value,
                "previous_origin": previous,
            },
            "checklist": checklist,
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn validates_https_origin() {
        assert_eq!(
            validate_and_normalize_origin("https://example.com").unwrap(),
            "https://example.com"
        );
        assert_eq!(
            validate_and_normalize_origin("  https://example.com/  ").unwrap(),
            "https://example.com"
        );
        assert_eq!(
            validate_and_normalize_origin("https://example.com:8443").unwrap(),
            "https://example.com:8443"
        );
    }

    #[test]
    fn allows_http_localhost_only() {
        assert_eq!(
            validate_and_normalize_origin("http://localhost:1102").unwrap(),
            "http://localhost:1102"
        );
        assert_eq!(
            validate_and_normalize_origin("http://127.0.0.1:3000").unwrap(),
            "http://127.0.0.1:3000"
        );
        assert!(validate_and_normalize_origin("http://example.com").is_err());
    }

    #[test]
    fn rejects_path_query_fragment_and_wildcard() {
        assert!(validate_and_normalize_origin("https://example.com/path").is_err());
        assert!(validate_and_normalize_origin("https://example.com?x=1").is_err());
        assert!(validate_and_normalize_origin("https://example.com#frag").is_err());
        assert!(validate_and_normalize_origin("https://*.example.com").is_err());
        assert!(validate_and_normalize_origin("*").is_err());
        assert!(validate_and_normalize_origin("").is_err());
        assert!(validate_and_normalize_origin("ftp://example.com").is_err());
    }

    #[test]
    fn cors_replace_previous_origin() {
        let merged = merge_cors_origins(
            "https://old.example,https://cdn.example",
            Some("https://old.example"),
            "https://new.example",
        );
        // Preserve relative order; only the matched previous entry is rewritten.
        assert_eq!(merged, "https://new.example,https://cdn.example");
    }

    #[test]
    fn cors_append_when_previous_absent() {
        let merged = merge_cors_origins(
            "https://other.example",
            Some("https://old.example"),
            "https://new.example",
        );
        assert_eq!(merged, "https://other.example,https://new.example");
    }

    #[test]
    fn cors_strips_star_and_never_empty() {
        let merged = merge_cors_origins("*", None, "https://new.example");
        assert_eq!(merged, "https://new.example");
        assert!(!merged.contains('*'));

        let empty = merge_cors_origins("", None, "https://new.example");
        assert_eq!(empty, "https://new.example");
    }

    #[test]
    fn cors_idempotent_when_already_present() {
        let merged = merge_cors_origins(
            "https://new.example,https://cdn.example",
            Some("https://old.example"),
            "https://new.example",
        );
        assert_eq!(merged, "https://new.example,https://cdn.example");
    }

    #[test]
    fn apply_env_writes_all_three_keys() {
        let input = "\
# comment
BASE_URL=https://old.example
FRONTEND_URL=https://old.example
CORS_ORIGINS=https://old.example,https://cdn.example
JWT_SECRET=keep-me
";
        let (out, origin) =
            apply_site_domain_to_env_content(input, "https://new.example/", Some("https://old.example"))
                .unwrap();
        assert_eq!(origin, "https://new.example");
        assert!(out.contains("BASE_URL=https://new.example"));
        assert!(out.contains("FRONTEND_URL=https://new.example"));
        assert!(out.contains("CORS_ORIGINS=https://new.example,https://cdn.example"));
        assert!(out.contains("JWT_SECRET=keep-me"));
        assert!(!out.contains("CORS_ORIGINS=*"));
    }

    #[test]
    fn read_env_key_skips_commented() {
        let content = "# BASE_URL=https://old.example\nBASE_URL=https://live.example\n";
        assert_eq!(
            read_env_key(content, "BASE_URL").as_deref(),
            Some("https://live.example")
        );
    }
}
