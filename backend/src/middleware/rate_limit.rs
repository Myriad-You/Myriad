use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::net::IpAddr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Per-IP aggregate hard ceiling across **all** endpoints (path buckets still apply separately).
/// Prevents a single IP from exhausting capacity by spreading traffic over many distinct paths.
///
/// Sized for SPA + media grids: a single feed/profile paint can fire hundreds of
/// `/api/proxy/image` loads on top of normal API traffic. Keep headroom above
/// `IMAGE_PROXY_MAX` so image traffic alone does not trip the aggregate cap.
const IP_HARD_CAP_MAX: usize = 1200;
const IP_HARD_CAP_WINDOW: Duration = Duration::from_secs(60);

/// Image proxy is allowlisted egress + streaming, not AI compute. Galleries,
/// RSS cards, and social report grids legitimately request many images per
/// minute — dedicated high bucket (not compute class). 240/min still 429'd
/// heavy boards; raise so more images load in one browsing session.
const IMAGE_PROXY_MAX: usize = 400;
const IMAGE_PROXY_WINDOW: Duration = Duration::from_secs(60);
const IMAGE_PROXY_BUCKET: &str = "proxy_image";

/// Default per-path budget for ordinary API traffic (polls, list/read, music meta).
/// 100 was tight when a widget board revalidates the same endpoint under load.
const DEFAULT_PATH_MAX: usize = 200;

/// Expensive / abuse-prone paths (AI, bulk external fetch, open egress helpers).
/// Shared key per path; 10 was far too low for multi-platform refresh UIs.
const COMPUTE_MAX: usize = 45;

/// First-party analytics collect+pageview shared bucket.
const ANALYTICS_WRITE_MAX: usize = 90;

/// Reserved path-bucket key for the per-IP aggregate counter (not a real endpoint).
const IP_TOTAL_KEY: &str = "__ip_total__";

/// Shard count for the in-process rate-limit map (MYR-017).
///
/// Every request used to take a **global exclusive write lock** on a single
/// `HashMap`. Under concurrent SPA traffic that serializes the whole process
/// on a hot path even when IPs are independent. Sharding by IP keeps window
/// counts the same while allowing different clients to update in parallel.
/// Power of two so index is a cheap mask.
const SHARD_COUNT: usize = 64;
const SHARD_MASK: usize = SHARD_COUNT - 1;

/// Rate limit configuration for different endpoint types
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub max_requests: usize,
    pub window: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: DEFAULT_PATH_MAX,
            window: Duration::from_secs(60),
        }
    }
}

/// Request record for tracking
#[derive(Debug, Clone)]
struct RequestRecord {
    count: usize,
    window_start: Instant,
}

/// One shard of IP → endpoint counters. Locked only for that IP slice.
#[derive(Debug, Default)]
struct Shard {
    // IP -> (endpoint_pattern -> request_record)
    // Aggregate per-IP traffic is tracked under the reserved key `IP_TOTAL_KEY`.
    records: HashMap<IpAddr, HashMap<String, RequestRecord>>,
}

/// Global rate limiter state (sharded; no process-wide exclusive map lock).
pub struct RateLimiter {
    shards: Arc<[Mutex<Shard>]>,
    config: RateLimitConfig,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        let shards: Vec<Mutex<Shard>> = (0..SHARD_COUNT)
            .map(|_| Mutex::new(Shard::default()))
            .collect();
        Self {
            shards: Arc::from(shards.into_boxed_slice()),
            config,
        }
    }

    #[inline]
    fn shard_index(ip: IpAddr) -> usize {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        ip.hash(&mut hasher);
        (hasher.finish() as usize) & SHARD_MASK
    }

    fn lock_shard(&self, ip: IpAddr) -> std::sync::MutexGuard<'_, Shard> {
        // Short critical section only; poison recovery keeps the process serving.
        self.shards[Self::shard_index(ip)]
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Touch a bucket: reset if window expired, else increment if under max.
    ///
    /// Returns `true` when the request is allowed (and counted).
    fn check_bucket(&self, ip: IpAddr, key: &str, max_requests: usize, window: Duration) -> bool {
        let mut shard = self.lock_shard(ip);
        let now = Instant::now();

        let ip_records = shard.records.entry(ip).or_default();
        let record = ip_records.entry(key.to_string()).or_insert(RequestRecord {
            count: 0,
            window_start: now,
        });

        if now.duration_since(record.window_start) > window {
            record.count = 1;
            record.window_start = now;
            return true;
        }

        if record.count >= max_requests {
            return false;
        }

        record.count += 1;
        true
    }

    /// Per-IP hard ceiling across all endpoints for `IP_HARD_CAP_WINDOW`.
    ///
    /// Returns `true` if the request is allowed (and counts it), `false` if the
    /// aggregate cap is already exhausted for this window.
    ///
    /// Enforced **before** path-specific buckets so a blocked IP does not grow
    /// path-key memory further.
    pub fn check_ip_hard_cap(&self, ip: IpAddr) -> bool {
        self.check_bucket(ip, IP_TOTAL_KEY, IP_HARD_CAP_MAX, IP_HARD_CAP_WINDOW)
    }

    /// Check if request should be rate limited (default path budget).
    fn check_limit(&self, ip: IpAddr, endpoint: &str) -> bool {
        self.check_bucket(ip, endpoint, self.config.max_requests, self.config.window)
    }

    /// Clean up old records (call periodically). Walks each shard independently.
    pub fn cleanup(&self) {
        let now = Instant::now();
        // Keep at least 2× the longest window we track so hard-cap + default buckets
        // are not pruned mid-window.
        let retain_for = self.config.window.max(IP_HARD_CAP_WINDOW) * 2;

        for shard_mtx in self.shards.iter() {
            let mut shard = shard_mtx
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            shard.records.retain(|_, ip_records| {
                ip_records
                    .retain(|_, record| now.duration_since(record.window_start) <= retain_for);
                !ip_records.is_empty()
            });
        }
    }

    /// Test / diagnostics: read a single bucket count without mutating.
    #[cfg(test)]
    fn bucket_count(&self, ip: IpAddr, key: &str) -> Option<usize> {
        let shard = self.lock_shard(ip);
        shard
            .records
            .get(&ip)
            .and_then(|m| m.get(key))
            .map(|r| r.count)
    }

    #[cfg(test)]
    fn shard_index_for_test(ip: IpAddr) -> usize {
        Self::shard_index(ip)
    }
}

/// Global rate limiter instance
static RATE_LIMITER: once_cell::sync::Lazy<RateLimiter> = once_cell::sync::Lazy::new(|| {
    // Spawn cleanup task
    let limiter = RateLimiter::new(RateLimitConfig::default());
    let limiter_clone = RateLimiter {
        shards: Arc::clone(&limiter.shards),
        config: limiter.config.clone(),
    };

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(120)); // 每2分钟清理一次（优化内存）
        loop {
            interval.tick().await;
            limiter_clone.cleanup();
            tracing::debug!("🧹 Rate limiter cleanup completed");
        }
    });

    limiter
});

/// Rate limiting middleware
pub async fn rate_limit_middleware(req: Request, next: Next) -> Response {
    // Forwarded headers are honored only when explicitly enabled for a trusted
    // edge proxy. Direct deployments key on the socket peer and ignore spoofed
    // client headers.
    let ip = crate::middleware::client_ip::extract_client_ip(&req)
        .unwrap_or_else(|| IpAddr::from([127, 0, 0, 1]));

    // Get endpoint path
    let path = req.uri().path().to_string();

    // 1) Per-IP aggregate hard ceiling across all endpoints (before path buckets).
    // If exceeded, reject immediately without counting path-specific keys.
    if !RATE_LIMITER.check_ip_hard_cap(ip) {
        let retry_after = IP_HARD_CAP_WINDOW.as_secs();
        tracing::warn!(
            "Rate limit (IP hard cap) exceeded for IP {} ({} req / {}s)",
            ip,
            IP_HARD_CAP_MAX,
            retry_after
        );
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(
                axum::http::header::RETRY_AFTER,
                retry_after.to_string(),
            )],
            Json(json!({
                "error": "Too many requests",
                "message": format!("Rate limit exceeded. Please try again in {} seconds.", retry_after),
                "retry_after": retry_after
            })),
        )
            .into_response();
    }

    // 2) Path-specific buckets (sensitive / admin / image / compute / analytics / default)
    // Same window counts as before — only the lock granularity changed (MYR-017).
    let (allowed, retry_after) = if is_sensitive_endpoint(&path) {
        // 敏感端点：5次请求/5分钟（unchanged; modest Argon2 path bucket only）
        (
            RATE_LIMITER.check_bucket(ip, &path, 5, Duration::from_secs(300)),
            300,
        )
    } else if is_admin_updater_mutate(&path) {
        // Mutating updater admin routes: 10 / 5 min per IP (GET status/jobs stay default).
        (
            RATE_LIMITER.check_bucket(ip, "admin_updater_mutate", 10, Duration::from_secs(300)),
            300,
        )
    } else if is_image_proxy(&path) {
        (
            RATE_LIMITER.check_bucket(ip, IMAGE_PROXY_BUCKET, IMAGE_PROXY_MAX, IMAGE_PROXY_WINDOW),
            IMAGE_PROXY_WINDOW.as_secs(),
        )
    } else if is_compute_intensive(&path) {
        (
            RATE_LIMITER.check_bucket(ip, &path, COMPUTE_MAX, Duration::from_secs(60)),
            60,
        )
    } else if path.starts_with("/api/analytics/collect")
        || path.starts_with("/api/analytics/pageview")
    {
        (
            RATE_LIMITER.check_bucket(
                ip,
                "analytics_write",
                ANALYTICS_WRITE_MAX,
                Duration::from_secs(60),
            ),
            60,
        )
    } else {
        // 标准端点：DEFAULT_PATH_MAX / 分钟（按 path 分桶）
        (RATE_LIMITER.check_limit(ip, &path), 60)
    };

    if !allowed {
        tracing::warn!("Rate limit exceeded for IP {} on endpoint {}", ip, path);
        return (
            StatusCode::TOO_MANY_REQUESTS,
            [(
                axum::http::header::RETRY_AFTER,
                retry_after.to_string(),
            )],
            Json(json!({
                "error": "Too many requests",
                "message": format!("Rate limit exceeded. Please try again in {} seconds.", retry_after),
                "retry_after": retry_after
            })),
        )
            .into_response();
    }

    next.run(req).await
}

/// Check if endpoint is sensitive (login, password change, registration, etc.)
///
/// Uses the **existing** sensitive budget (5 req / 5 min per IP per path).
/// Register and set-password are included so Argon2-heavy account creation is
/// not on the default high path bucket (MYR-006 — modest only, no extra quotas).
fn is_sensitive_endpoint(path: &str) -> bool {
    path.contains("/auth/login")
        || path.contains("/auth/register")
        || path.contains("/auth/change-password")
        || path.contains("/auth/me/set-password")
        || path.contains("/setup/create-admin")
        || path.contains("/setup/database-config")
        || path.contains("/setup/init-database")
}

/// Mutating `/api/admin/updater/*` paths only (not status/jobs polling).
fn is_admin_updater_mutate(path: &str) -> bool {
    let p = path.trim_end_matches('/');
    p.ends_with("/api/admin/updater/update")
        || p.ends_with("/api/admin/updater/rollback")
        || p.ends_with("/api/admin/updater/self-update")
        || p.ends_with("/api/admin/updater/prefs")
        || p.ends_with("/api/admin/updater/last-failed/dismiss")
        || p.ends_with("/api/admin/updater/self-update/last/dismiss")
        || p.ends_with("/api/admin/updater/proxy-update/last/dismiss")
        || p.contains("/api/admin/updater/rescue/")
        // DELETE /api/admin/updater/snapshots/{id}
        || p.contains("/api/admin/updater/snapshots/")
}

/// Image proxy — high volume media path (own bucket, not compute).
fn is_image_proxy(path: &str) -> bool {
    path == "/api/proxy/image" || path.starts_with("/api/proxy/image/")
}

/// Expensive or open-egress paths. Prefer **prefix / exact** matches so we do not
/// accidentally throttle high-volume reads that merely contain a substring
/// (e.g. storage lives under `/api/tapps/…/storage` and has its own Tapp limiter).
fn is_compute_intensive(path: &str) -> bool {
    if is_image_proxy(path) {
        return false;
    }
    let p = path.trim_end_matches('/');
    // Bulk / AI / analysis
    p == "/api/prompt/generate"
        || p == "/api/home/stickers/generate"
        || p == "/api/home/stickers/upload"
        || p == "/api/home/widget-fonts"
        || p == "/api/seo/generate-copy"
        || p == "/api/profile/refresh"
        || p == "/api/profile/fetch-all"
        || p == "/api/profile/fetch-platform"
        // Agent chat / stream (AI)
        || p == "/api/agent/process"
        || p.starts_with("/api/agent/process/")
        // Tapp AI task create (not status polls)
        || p == "/api/tapp/ai/v2/tasks"
        // Open egress helpers
        || p.starts_with("/api/proxy/hitokoto")
        || p.starts_with("/api/proxy/fetch-content")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::thread;

    #[test]
    fn admin_updater_mutate_paths() {
        assert!(is_admin_updater_mutate("/api/admin/updater/update"));
        assert!(is_admin_updater_mutate("/api/admin/updater/rollback"));
        assert!(is_admin_updater_mutate("/api/admin/updater/self-update"));
        assert!(is_admin_updater_mutate(
            "/api/admin/updater/rescue/continue"
        ));
        // Must match the registered public path (not the old /defaults typo).
        assert!(is_admin_updater_mutate("/api/admin/updater/prefs"));
        assert!(is_admin_updater_mutate("/api/admin/updater/prefs/"));
        assert!(is_admin_updater_mutate(
            "/api/admin/updater/last-failed/dismiss"
        ));
        assert!(is_admin_updater_mutate(
            "/api/admin/updater/self-update/last/dismiss"
        ));
        assert!(is_admin_updater_mutate(
            "/api/admin/updater/proxy-update/last/dismiss"
        ));
        assert!(!is_admin_updater_mutate("/api/admin/updater/defaults"));
        assert!(is_admin_updater_mutate(
            "/api/admin/updater/snapshots/snap-abc"
        ));
        assert!(!is_admin_updater_mutate("/api/admin/updater/status"));
        assert!(!is_admin_updater_mutate("/api/admin/updater/jobs"));
    }

    #[test]
    fn sensitive_auth_paths_include_register_and_password() {
        assert!(is_sensitive_endpoint("/api/auth/login"));
        assert!(is_sensitive_endpoint("/api/auth/register"));
        assert!(is_sensitive_endpoint("/api/auth/change-password"));
        assert!(is_sensitive_endpoint("/api/auth/me/set-password"));
        assert!(is_sensitive_endpoint("/api/setup/create-admin"));
        // Non-sensitive auth reads stay on default buckets.
        assert!(!is_sensitive_endpoint("/api/auth/me"));
        assert!(!is_sensitive_endpoint("/api/auth/oauth/providers"));
    }

    #[test]
    fn open_proxy_path_classification() {
        assert!(is_compute_intensive("/api/proxy/hitokoto"));
        assert!(is_compute_intensive("/api/proxy/fetch-content"));
        assert!(is_compute_intensive("/api/profile/fetch-all"));
        assert!(is_compute_intensive("/api/profile/fetch-platform"));
        assert!(is_compute_intensive("/api/agent/process"));
        assert!(is_compute_intensive("/api/agent/process/stream"));
        assert!(is_compute_intensive("/api/tapp/ai/v2/tasks"));
        assert!(is_compute_intensive("/api/home/stickers/generate"));
        assert!(is_compute_intensive("/api/home/stickers/upload"));
        assert!(is_compute_intensive("/api/home/widget-fonts"));
        assert!(!is_compute_intensive("/api/home/widget-fonts/abc.woff2"));
        // Image proxy is media volume, not compute.
        assert!(is_image_proxy("/api/proxy/image"));
        assert!(!is_compute_intensive("/api/proxy/image"));
        assert!(!is_compute_intensive("/api/proxy/client-geo"));
        assert!(!is_image_proxy("/api/proxy/client-geo"));
        // Must not catch high-volume reads via loose substring match.
        assert!(!is_compute_intensive("/api/tapps/com.example/storage/key"));
        assert!(!is_compute_intensive("/api/tapp/ai/v2/tasks/abc/events"));
        assert!(!is_compute_intensive("/api/tapp/ai/v2/usage"));
        assert!(!is_compute_intensive("/api/proxy/music/netease/play-url/1"));
        assert!(!is_admin_updater_mutate("/api/admin/updater/available"));
        assert!(!is_admin_updater_mutate("/api/admin/updater/snapshots"));
    }

    #[test]
    #[allow(clippy::assertions_on_constants)] // document intended floors for const limits
    fn image_proxy_limits_are_gallery_friendly() {
        assert!(IMAGE_PROXY_MAX >= 300);
        assert!(IP_HARD_CAP_MAX >= IMAGE_PROXY_MAX + 200);
        assert_eq!(IMAGE_PROXY_WINDOW, Duration::from_secs(60));
    }

    #[test]
    #[allow(clippy::assertions_on_constants)] // document intended floors for const limits
    fn default_and_compute_limits_are_usable() {
        assert!(DEFAULT_PATH_MAX >= 150);
        assert!(COMPUTE_MAX >= 30);
        assert!(ANALYTICS_WRITE_MAX >= 60);
        assert!(IP_HARD_CAP_MAX >= DEFAULT_PATH_MAX);
    }

    #[test]
    #[allow(clippy::assertions_on_constants)]
    fn shard_count_is_power_of_two() {
        assert!(SHARD_COUNT.is_power_of_two());
        assert_eq!(SHARD_MASK, SHARD_COUNT - 1);
        assert!(
            SHARD_COUNT >= 16,
            "need enough shards to cut cross-IP contention"
        );
    }

    #[test]
    fn test_rate_limit_basic() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_requests: 3,
            window: Duration::from_secs(10),
        });

        let ip = IpAddr::from([127, 0, 0, 1]);
        let endpoint = "/api/test";

        // First 3 requests should pass
        assert!(limiter.check_limit(ip, endpoint));
        assert!(limiter.check_limit(ip, endpoint));
        assert!(limiter.check_limit(ip, endpoint));

        // 4th request should be blocked
        assert!(!limiter.check_limit(ip, endpoint));
    }

    #[test]
    fn test_rate_limit_window_reset() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_requests: 2,
            window: Duration::from_millis(80),
        });

        let ip = IpAddr::from([127, 0, 0, 1]);
        let endpoint = "/api/test";

        // First 2 requests pass
        assert!(limiter.check_limit(ip, endpoint));
        assert!(limiter.check_limit(ip, endpoint));

        // 3rd is blocked
        assert!(!limiter.check_limit(ip, endpoint));

        // Wait for window to expire
        thread::sleep(Duration::from_millis(100));

        // Should pass again
        assert!(limiter.check_limit(ip, endpoint));
    }

    #[test]
    fn test_ip_hard_cap_allows_up_to_max() {
        let limiter = RateLimiter::new(RateLimitConfig::default());
        let ip = IpAddr::from([10, 0, 0, 1]);

        for i in 0..IP_HARD_CAP_MAX {
            assert!(
                limiter.check_ip_hard_cap(ip),
                "request {} should be allowed under hard cap",
                i + 1
            );
        }

        // One over the ceiling
        assert!(!limiter.check_ip_hard_cap(ip));
        // Further attempts stay blocked within the window
        assert!(!limiter.check_ip_hard_cap(ip));
    }

    #[test]
    fn test_ip_hard_cap_is_per_ip() {
        let limiter = RateLimiter::new(RateLimitConfig::default());
        let ip_a = IpAddr::from([10, 0, 0, 2]);
        let ip_b = IpAddr::from([10, 0, 0, 3]);

        for _ in 0..IP_HARD_CAP_MAX {
            assert!(limiter.check_ip_hard_cap(ip_a));
        }
        assert!(!limiter.check_ip_hard_cap(ip_a));

        // Different IP still has full budget
        assert!(limiter.check_ip_hard_cap(ip_b));
    }

    #[test]
    fn test_ip_hard_cap_stored_under_reserved_key() {
        let limiter = RateLimiter::new(RateLimitConfig::default());
        let ip = IpAddr::from([10, 0, 0, 4]);

        assert!(limiter.check_ip_hard_cap(ip));
        assert!(limiter.check_limit(ip, "/api/other"));

        assert_eq!(limiter.bucket_count(ip, IP_TOTAL_KEY), Some(1));
        assert_eq!(limiter.bucket_count(ip, "/api/other"), Some(1));
    }

    #[test]
    fn test_ip_hard_cap_cleanup_prunes_stale() {
        let limiter = RateLimiter::new(RateLimitConfig::default());
        let ip_fresh = IpAddr::from([10, 0, 0, 5]);
        let ip_stale = IpAddr::from([10, 0, 0, 6]);

        assert!(limiter.check_ip_hard_cap(ip_fresh));

        // Inject a hard-cap record whose window_start is older than retain_for
        // (max(config.window, IP_HARD_CAP_WINDOW) * 2 = 120s).
        {
            let mut shard = limiter.lock_shard(ip_stale);
            let ip_records = shard.records.entry(ip_stale).or_default();
            ip_records.insert(
                IP_TOTAL_KEY.to_string(),
                RequestRecord {
                    count: 1,
                    window_start: Instant::now() - Duration::from_secs(200),
                },
            );
        }

        limiter.cleanup();

        assert_eq!(
            limiter.bucket_count(ip_fresh, IP_TOTAL_KEY),
            Some(1),
            "recent hard-cap record must survive cleanup"
        );
        assert_eq!(
            limiter.bucket_count(ip_stale, IP_TOTAL_KEY),
            None,
            "stale hard-cap record must be pruned"
        );
    }

    #[test]
    fn different_ips_can_land_on_different_shards() {
        // Not a strict guarantee for every pair, but across a spread of IPs
        // we must observe more than one shard (otherwise sharding is broken).
        let mut seen = std::collections::HashSet::new();
        for i in 0u8..64 {
            let ip = IpAddr::from([10, 1, 0, i]);
            seen.insert(RateLimiter::shard_index_for_test(ip));
        }
        assert!(
            seen.len() > 1,
            "expected IPs to hash across multiple shards, got {seen:?}"
        );
    }

    #[test]
    fn concurrent_multi_ip_updates_do_not_lose_counts() {
        // MYR-017: sharded locks must remain correct under concurrent writers.
        let limiter = Arc::new(RateLimiter::new(RateLimitConfig {
            max_requests: 10_000,
            window: Duration::from_secs(60),
        }));
        let per_ip = 200usize;
        let ip_count = 32usize;
        let allowed = Arc::new(AtomicUsize::new(0));

        let mut handles = Vec::new();
        for i in 0..ip_count {
            let limiter = Arc::clone(&limiter);
            let allowed = Arc::clone(&allowed);
            handles.push(thread::spawn(move || {
                let ip = IpAddr::from([10, 2, (i / 256) as u8, (i % 256) as u8]);
                for _ in 0..per_ip {
                    if limiter.check_limit(ip, "/api/concurrent") {
                        allowed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            }));
        }
        for h in handles {
            h.join().expect("worker");
        }

        assert_eq!(allowed.load(Ordering::Relaxed), ip_count * per_ip);
        for i in 0..ip_count {
            let ip = IpAddr::from([10, 2, (i / 256) as u8, (i % 256) as u8]);
            assert_eq!(
                limiter.bucket_count(ip, "/api/concurrent"),
                Some(per_ip),
                "ip index {i}"
            );
        }
    }

    #[test]
    fn sensitive_and_image_buckets_keep_prior_caps() {
        let limiter = RateLimiter::new(RateLimitConfig::default());
        let ip = IpAddr::from([10, 9, 9, 9]);

        for _ in 0..5 {
            assert!(limiter.check_bucket(ip, "/api/auth/login", 5, Duration::from_secs(300)));
        }
        assert!(!limiter.check_bucket(ip, "/api/auth/login", 5, Duration::from_secs(300)));

        // Image proxy still uses its dedicated high bucket (unchanged numbers).
        for _ in 0..IMAGE_PROXY_MAX {
            assert!(limiter.check_bucket(
                ip,
                IMAGE_PROXY_BUCKET,
                IMAGE_PROXY_MAX,
                IMAGE_PROXY_WINDOW
            ));
        }
        assert!(!limiter.check_bucket(ip, IMAGE_PROXY_BUCKET, IMAGE_PROXY_MAX, IMAGE_PROXY_WINDOW));
    }
}
