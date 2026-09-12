//! Interpret backend `/health` JSON for upgrade and rollback waits.
//!
//! Soft ticks (image-tag identity, proxy-down, maintenance HTML) are degraded
//! observations. They must never complete a job as success.

use serde_json::Value;

pub fn backend_storage_writable(health: &Value) -> bool {
    // Missing means an older backend from before the storage-preflight field;
    // preserve rollback/upgrade compatibility for those images.
    match health.get("storage_writable") {
        None => true,
        Some(value) => value.as_bool().unwrap_or(false),
    }
}

pub fn backend_routes_full(health: &Value) -> bool {
    match health.get("routes_full") {
        Some(value) => value.as_bool().unwrap_or(false),
        None => health.get("mode").and_then(|value| value.as_str()) == Some("full"),
    }
}

/// Database probe + migrations + full routes + storage.
/// Soft HTTP 200 is not enough. Missing `storage_writable` stays compatible.
pub fn backend_business_ready(health: &Value) -> bool {
    let db = health
        .get("db_connected")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    let mig = health
        .get("migrations_applied")
        .and_then(|value| value.as_bool())
        .unwrap_or(false);
    db && mig && backend_routes_full(health) && backend_storage_writable(health)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn storage_missing_stays_compatible() {
        assert!(backend_storage_writable(&json!({})));
        assert!(backend_storage_writable(
            &json!({ "storage_writable": true })
        ));
        assert!(!backend_storage_writable(
            &json!({ "storage_writable": false })
        ));
        assert!(!backend_storage_writable(
            &json!({ "storage_writable": "invalid-old-shape" })
        ));
    }

    #[test]
    fn routes_full_falls_back_to_mode_on_old_images() {
        assert!(backend_routes_full(&json!({ "routes_full": true })));
        assert!(!backend_routes_full(&json!({ "routes_full": false })));
        assert!(backend_routes_full(&json!({ "mode": "full" })));
        assert!(!backend_routes_full(&json!({ "mode": "configuration" })));
        assert!(!backend_routes_full(&json!({})));
    }

    #[test]
    fn handle_without_probe_is_not_business_ready() {
        let health = json!({
            "db_connected": false,
            "db_handle_present": true,
            "migrations_applied": true,
            "routes_full": false,
            "mode": "configuration"
        });
        assert!(!backend_business_ready(&health));
    }

    #[test]
    fn http_200_without_routes_is_not_ready() {
        let health = json!({
            "db_connected": true,
            "migrations_applied": true,
            "routes_full": false,
            "mode": "configuration"
        });
        assert!(!backend_business_ready(&health));
        let ready = json!({
            "db_connected": true,
            "migrations_applied": true,
            "routes_full": true
        });
        assert!(backend_business_ready(&ready));
    }

    #[test]
    fn storage_writable_false_blocks_rollback_ready() {
        let health = json!({
            "db_connected": true,
            "migrations_applied": true,
            "routes_full": true,
            "storage_writable": false
        });
        assert!(!backend_business_ready(&health));
    }
}
