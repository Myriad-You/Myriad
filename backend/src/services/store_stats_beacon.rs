//! Fire-and-forget install stats to the official edge.
//!
//! **No secrets in Myriad.** Posts plain JSON with instance_hash.
//! Cap on edge: 1 / instance / app / event / UTC day.
//!
//! Local/dev: OFF unless TAPP_STORE_STATS_ENABLED=true.
//! Production: auto-on when ENVIRONMENT=production and non-localhost BASE_URL.

use crate::services::http_client::TAPP_HTTP_CLIENT;
use sha2::{Digest, Sha256};
use std::env;
use std::time::Duration;

const DEFAULT_STATS_URL: &str = "https://stats.store.myriad.you";

/// Spawn install/update hit (instance-day cap enforced on edge via instance_hash).
pub fn spawn_store_stats_hit(app_id: &str, version: &str, event: &str) {
    spawn_store_stats_hit_with_key(app_id, version, event, None);
}

pub fn spawn_store_stats_hit_with_key(
    app_id: &str,
    version: &str,
    event: &str,
    _idempotency_key: Option<String>,
) {
    let app_id = app_id.to_string();
    let version = version.to_string();
    let event = event.to_string();
    tokio::spawn(async move {
        if let Err(err) = send_hit(&app_id, &version, &event).await {
            tracing::debug!(
                target: "store_stats",
                error = %err,
                app_id = %app_id,
                event = %event,
                "store stats beacon failed (ignored)"
            );
        }
    });
}

/// 8–64 hex instance fingerprint for edge.
pub fn instance_hash() -> String {
    stable_key(&instance_material())
}

pub fn instance_day_idempotency_key(app_id: &str, event: &str) -> String {
    let day = chrono::Utc::now().format("%Y-%m-%d");
    let inst = instance_hash();
    stable_key(&format!("inst|{inst}|{app_id}|{event}|{day}"))
}

fn instance_material() -> String {
    if let Ok(id) = env::var("TAPP_STORE_INSTANCE_ID") {
        let t = id.trim();
        if !t.is_empty() {
            return format!("id:{t}");
        }
    }
    if let Ok(base) = env::var("BASE_URL") {
        let t = normalize_origin(&base);
        if !t.is_empty() {
            return t;
        }
    }
    if let Ok(front) = env::var("FRONTEND_URL") {
        let t = normalize_origin(&front);
        if !t.is_empty() {
            return t;
        }
    }
    let host = env::var("SERVER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = env::var("SERVER_PORT").unwrap_or_else(|_| "1103".to_string());
    let host = if host == "0.0.0.0" || host == "::" {
        "127.0.0.1".to_string()
    } else {
        host
    };
    format!("http://{host}:{port}")
}

fn normalize_origin(raw: &str) -> String {
    raw.trim().trim_end_matches('/').to_string()
}

fn stable_key(material: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(material.as_bytes());
    let digest = hasher.finalize();
    digest.iter().take(16).map(|b| format!("{b:02x}")).collect()
}

fn stats_enabled() -> bool {
    if let Ok(v) = env::var("TAPP_STORE_STATS_ENABLED") {
        let t = v.trim().to_ascii_lowercase();
        return matches!(t.as_str(), "1" | "true" | "yes" | "on");
    }
    is_production_public_instance()
}

fn is_production_public_instance() -> bool {
    let env_prod = env::var("ENVIRONMENT")
        .map(|s| s.trim() == "production")
        .unwrap_or(false);
    if !env_prod {
        return false;
    }
    !instance_looks_local()
}

fn instance_looks_local() -> bool {
    let material = instance_material().to_ascii_lowercase();
    material.contains("127.0.0.1")
        || material.contains("localhost")
        || material.contains("[::1]")
        || material.contains("0.0.0.0")
        || material.starts_with("id:dev")
}

fn stats_base_url() -> Option<String> {
    if !stats_enabled() {
        return None;
    }
    let raw = env::var("TAPP_STORE_STATS_URL")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_STATS_URL.to_string());
    if raw.is_empty() {
        return None;
    }
    Some(raw.trim_end_matches('/').to_string())
}

async fn send_hit(app_id: &str, version: &str, event: &str) -> Result<(), String> {
    let base = match stats_base_url() {
        Some(u) => u,
        None => return Ok(()),
    };

    let inst = instance_hash();
    let key = instance_day_idempotency_key(app_id, event);

    let body = serde_json::json!({
        "app_id": app_id,
        "version": version,
        "event": event,
        "idempotency_key": key,
        "instance_hash": inst,
        "client": "myriad-backend",
        "source": "official",
        "myriad_version": env!("CARGO_PKG_VERSION"),
    });

    let url = format!("{base}/v1/hit");
    let res = TAPP_HTTP_CLIENT
        .post(&url)
        .timeout(Duration::from_secs(2))
        .header("content-type", "application/json")
        .header("user-agent", "Myriad-Store-Stats/1.0")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("request: {e}"))?;

    let status = res.status();
    if !status.is_success() {
        let text = res.text().await.unwrap_or_default();
        return Err(format!("HTTP {status}: {text}"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_day_key_stable() {
        let a = instance_day_idempotency_key("com.a.b", "install");
        let b = instance_day_idempotency_key("com.a.b", "install");
        assert_eq!(a, b);
    }

    #[test]
    fn normalize_strips_slash() {
        assert_eq!(normalize_origin("https://a.com/"), "https://a.com");
    }
}
