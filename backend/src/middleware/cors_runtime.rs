//! Runtime-updatable CORS allowlist.
//!
//! `CorsLayer` is built once at process start; without a hot path, rewriting
//! `CORS_ORIGINS` in `.env` has no effect until restart. This module keeps the
//! allowed origins in a process-global list that `AllowOrigin::predicate` reads
//! on every request, and can be refreshed after site-domain changes.

use axum::http::HeaderValue;
use once_cell::sync::Lazy;
use std::sync::RwLock;

static CORS_ORIGINS: Lazy<RwLock<Vec<HeaderValue>>> = Lazy::new(|| RwLock::new(Vec::new()));

/// Replace the runtime allowlist (empty = deny all cross-origin; same-origin still works).
pub fn set_cors_origins(origins: impl IntoIterator<Item = String>) {
    let parsed: Vec<HeaderValue> = origins
        .into_iter()
        .filter_map(|o| {
            let t = o.trim();
            if t.is_empty() || t == "*" {
                return None;
            }
            t.parse::<HeaderValue>().ok()
        })
        .collect();
    match CORS_ORIGINS.write() {
        Ok(mut guard) => {
            tracing::info!(
                count = parsed.len(),
                origins = ?parsed
                    .iter()
                    .filter_map(|h| h.to_str().ok())
                    .collect::<Vec<_>>(),
                "♻️ CORS runtime allowlist updated"
            );
            *guard = parsed;
        }
        Err(poisoned) => {
            *poisoned.into_inner() = parsed;
        }
    }
}

/// Parse a comma-separated `CORS_ORIGINS` env-style string and install it.
pub fn set_cors_origins_csv(csv: &str) {
    let list: Vec<String> = csv
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
        .collect();
    set_cors_origins(list);
}

/// Predicate for `tower_http::cors::AllowOrigin::predicate`.
pub fn origin_is_allowed(origin: &HeaderValue) -> bool {
    let Ok(guard) = CORS_ORIGINS.read() else {
        return false;
    };
    guard.iter().any(|allowed| allowed == origin)
}

/// Snapshot for tests / diagnostics.
#[cfg(test)]
pub fn cors_origin_count() -> usize {
    CORS_ORIGINS.read().map(|g| g.len()).unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Single test: shared process-global allowlist must not race under --test-threads.
    #[test]
    fn set_match_and_reject_wildcard() {
        set_cors_origins(vec![
            "https://a.example".into(),
            "http://localhost:1102".into(),
        ]);
        assert!(origin_is_allowed(&HeaderValue::from_static(
            "https://a.example"
        )));
        assert!(origin_is_allowed(&HeaderValue::from_static(
            "http://localhost:1102"
        )));
        assert!(!origin_is_allowed(&HeaderValue::from_static(
            "https://evil.example"
        )));
        assert_eq!(cors_origin_count(), 2);

        set_cors_origins(vec!["*".into(), "https://ok.example".into()]);
        assert_eq!(cors_origin_count(), 1);
        assert!(origin_is_allowed(&HeaderValue::from_static(
            "https://ok.example"
        )));
        assert!(!origin_is_allowed(&HeaderValue::from_static(
            "https://a.example"
        )));
    }
}
