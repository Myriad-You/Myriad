//! Structural invariants for backend memory management.
//!
//! Drives shipped constants and APIs so budgets/alerts cannot silently drift.

#[cfg(test)]
mod tests {
    use myriad_process_info::{MEMORY_CRITICAL_MB, MEMORY_WARNING_MB};

    /// Alert line: `MEMORY_WARNING_MB` 600, `MEMORY_CRITICAL_MB` 750.
    #[test]
    fn shipped_memory_alerts_are_600_warn_750_critical() {
        assert_eq!(MEMORY_WARNING_MB, 600);
        assert_eq!(MEMORY_CRITICAL_MB, 750);
        let metrics = include_str!("api/metrics.rs");
        let diagnostics = include_str!("api/diagnostics.rs");
        assert!(metrics.contains("MEMORY_WARNING_MB") || metrics.contains("memory_profile"));
        assert!(diagnostics.contains("MEMORY_WARNING_MB") || diagnostics.contains("warning_mb"));
    }

    #[test]
    fn public_federation_limits_route_is_mounted() {
        let src = include_str!("router/base.rs");
        assert!(src.contains("/api/federation/public/limits"));
        assert!(src.contains("federation::limits::public_limits"));
    }

    #[test]
    fn diagnostics_surfaces_profile_and_thresholds() {
        let src = include_str!("api/diagnostics.rs");
        assert!(src.contains("warning_mb"));
        assert!(src.contains("critical_mb"));
        assert!(src.contains("memory_profile"));
    }

    #[test]
    fn metrics_exposes_memory_profile_snapshot() {
        let src = include_str!("api/metrics.rs");
        assert!(src.contains("memory_profile"));
        assert!(src.contains("metrics_snapshot"));
    }

    #[test]
    fn db_pool_uses_memory_profile_helpers() {
        let src = include_str!("db/connection.rs");
        assert!(
            src.contains("db_min_connections") && src.contains("db_max_connections"),
            "pool min/max must come from memory_profile"
        );
        assert_eq!(
            crate::services::memory_profile::DEFAULT_DB_MIN_CONNECTIONS,
            2
        );
        assert_eq!(
            crate::services::memory_profile::DEFAULT_DB_MAX_CONNECTIONS,
            24
        );
    }

    #[test]
    fn api_cache_has_entry_and_byte_budgets() {
        let src = include_str!("services/tapp_api_service.rs");
        assert!(src.contains("max_api_cache_entries"));
        assert!(src.contains("max_api_cache_bytes"));
        assert!(src.contains("size_bytes"));
        const {
            assert!(
                crate::services::memory_profile::DEFAULT_MAX_API_CACHE_BYTES
                    > crate::services::memory_profile::SAVER_MAX_API_CACHE_BYTES
            );
        }
    }

    #[test]
    fn image_proxy_uses_read_limited_body() {
        let src = include_str!("api/proxy/image_music_geo.rs");
        assert!(src.contains("read_limited_body"));
        assert!(
            src.contains("memory_profile::max_audio_bytes()"),
            "audio proxy must read the live profile cap, not a local 128 MiB constant"
        );
        assert!(!src.contains("const MAX_AUDIO_BYTES"));
        assert!(src.contains("MEDIA_FETCH_CLIENT"));
    }

    #[test]
    fn argon2_permits_match_default_profile() {
        let src = include_str!("api/auth_local.rs");
        assert!(src.contains("spawn_blocking"));
        assert!(src.contains("memory_profile::argon2_permits()"));
        assert!(!src.contains("PASSWORD_HASH_PERMITS"));
        assert_eq!(crate::services::memory_profile::DEFAULT_ARGON2_PERMITS, 4);
    }

    #[tokio::test]
    async fn inbox_inflight_permit_api_is_live() {
        use crate::federation::limits::{
            inbox_inflight_raw_bytes, try_acquire_inbox_inflight, INBOX_BODY_LIMIT,
            INBOX_INFLIGHT_RAW_BUDGET,
        };

        let before = inbox_inflight_raw_bytes();
        let permit = try_acquire_inbox_inflight(1024);
        if let Some(p) = permit {
            assert_eq!(p.bytes(), 1024);
            assert!(inbox_inflight_raw_bytes() >= before.saturating_add(1024) || before > 0);
            drop(p);
        } else {
            const {
                assert!(INBOX_INFLIGHT_RAW_BUDGET >= INBOX_BODY_LIMIT);
            }
        }
    }

    #[test]
    fn metrics_snapshot_includes_profile_key() {
        let snap = crate::services::memory_profile::metrics_snapshot();
        assert_eq!(
            snap.get("profile").and_then(|v| v.as_str()),
            Some(crate::services::memory_profile::active_profile().as_str())
        );
        assert!(snap.get("api_cache").is_some());
        assert!(snap.get("inbox_inflight_raw_budget_mb").is_some());
    }
}
