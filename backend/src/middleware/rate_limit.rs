use axum::{
    extract::Request,
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

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

/// Global rate limiter state
pub struct RateLimiter {
    // IP -> (endpoint_pattern -> request_record)
    // Aggregate per-IP traffic is tracked under the reserved key `IP_TOTAL_KEY`.
    records: Arc<RwLock<HashMap<IpAddr, HashMap<String, RequestRecord>>>>,
    config: RateLimitConfig,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            records: Arc::new(RwLock::new(HashMap::new())),
            config,
        }
    }

    /// Per-IP hard ceiling across all endpoints for `IP_HARD_CAP_WINDOW`.
    ///
    /// Returns `true` if the request is allowed (and counts it), `false` if the
    /// aggregate cap is already exhausted for this window.
    ///
    /// Enforced **before** path-specific buckets so a blocked IP does not grow
    /// path-key memory further.
    pub async fn check_ip_hard_cap(&self, ip: IpAddr) -> bool {
        let mut records = self.records.write().await;
        let now = Instant::now();

        let ip_records = records.entry(ip).or_insert_with(HashMap::new);
        let record = ip_records
            .entry(IP_TOTAL_KEY.to_string())
            .or_insert(RequestRecord {
                count: 0,
                window_start: now,
            });

        if now.duration_since(record.window_start) > IP_HARD_CAP_WINDOW {
            record.count = 1;
            record.window_start = now;
            return true;
        }

        if record.count >= IP_HARD_CAP_MAX {
            return false;
        }

        record.count += 1;
        true
    }

    /// Check if request should be rate limited
    async fn check_limit(&self, ip: IpAddr, endpoint: &str) -> bool {
        let mut records = self.records.write().await;
        let now = Instant::now();

        // Get or create IP record
        let ip_records = records.entry(ip).or_insert_with(HashMap::new);

        // Get or create endpoint record
        let record = ip_records
            .entry(endpoint.to_string())
            .or_insert(RequestRecord {
                count: 0,
                window_start: now,
            });

        // Check if window expired
        if now.duration_since(record.window_start) > self.config.window {
            // Reset window
            record.count = 1;
            record.window_start = now;
            return true;
        }

        // Check if limit exceeded
        if record.count >= self.config.max_requests {
            return false;
        }

        // Increment counter
        record.count += 1;
        true
    }

    /// Clean up old records (call periodically)
    pub async fn cleanup(&self) {
        let mut records = self.records.write().await;
        let now = Instant::now();
        // Keep at least 2× the longest window we track so hard-cap + default buckets
        // are not pruned mid-window.
        let retain_for = self.config.window.max(IP_HARD_CAP_WINDOW) * 2;

        records.retain(|_, ip_records| {
            ip_records.retain(|_, record| now.duration_since(record.window_start) <= retain_for);
            !ip_records.is_empty()
        });
    }
}

/// Global rate limiter instance
static RATE_LIMITER: once_cell::sync::Lazy<RateLimiter> = once_cell::sync::Lazy::new(|| {
    // Spawn cleanup task
    let limiter = RateLimiter::new(RateLimitConfig::default());
    let limiter_clone = RateLimiter {
        records: limiter.records.clone(),
        config: limiter.config.clone(),
    };

    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(120)); // 每2分钟清理一次（优化内存）
        loop {
            interval.tick().await;
            limiter_clone.cleanup().await;
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
    if !RATE_LIMITER.check_ip_hard_cap(ip).await {
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
    // 使用全局单例，确保限流计数器跨请求持久化
    // 不同类型的端点使用不同的路径前缀来区分限流规则
    let (allowed, retry_after) = if is_sensitive_endpoint(&path) {
        // 敏感端点：5次请求/5分钟
        // 直接读取并递增计数器（不使用 check_limit 避免双重计数）
        let record_count = {
            let mut records = RATE_LIMITER.records.write().await;
            let now = std::time::Instant::now();
            let ip_records = records.entry(ip).or_insert_with(HashMap::new);
            let record = ip_records.entry(path.clone()).or_insert(RequestRecord {
                count: 0,
                window_start: now,
            });
            // 敏感端点使用 5 分钟窗口
            if now.duration_since(record.window_start) > Duration::from_secs(300) {
                record.count = 1;
                record.window_start = now;
            } else {
                record.count += 1;
            }
            record.count
        };
        (record_count <= 5, 300)
    } else if is_admin_updater_mutate(&path) {
        // Mutating updater admin routes: 10 / 5 min per IP (GET status/jobs stay default 100/min).
        // Prevents runaway update/rollback/rescue spam without slowing status polling UX.
        let record_count = {
            let mut records = RATE_LIMITER.records.write().await;
            let now = std::time::Instant::now();
            let ip_records = records.entry(ip).or_insert_with(HashMap::new);
            // Bucket all mutative updater actions together under one key.
            let key = "admin_updater_mutate".to_string();
            let record = ip_records.entry(key).or_insert(RequestRecord {
                count: 0,
                window_start: now,
            });
            if now.duration_since(record.window_start) > Duration::from_secs(300) {
                record.count = 1;
                record.window_start = now;
            } else {
                record.count += 1;
            }
            record.count
        };
        (record_count <= 10, 300)
    } else if is_image_proxy(&path) {
        // Media grids: high dedicated bucket (not compute 10/min).
        let record_count = {
            let mut records = RATE_LIMITER.records.write().await;
            let now = std::time::Instant::now();
            let ip_records = records.entry(ip).or_insert_with(HashMap::new);
            let record = ip_records
                .entry(IMAGE_PROXY_BUCKET.to_string())
                .or_insert(RequestRecord {
                    count: 0,
                    window_start: now,
                });
            if now.duration_since(record.window_start) > IMAGE_PROXY_WINDOW {
                record.count = 1;
                record.window_start = now;
            } else {
                record.count += 1;
            }
            record.count
        };
        (record_count <= IMAGE_PROXY_MAX, IMAGE_PROXY_WINDOW.as_secs())
    } else if is_compute_intensive(&path) {
        // Expensive / open-egress: COMPUTE_MAX per path per minute.
        let record_count = {
            let mut records = RATE_LIMITER.records.write().await;
            let now = std::time::Instant::now();
            let ip_records = records.entry(ip).or_insert_with(HashMap::new);
            let record = ip_records.entry(path.clone()).or_insert(RequestRecord {
                count: 0,
                window_start: now,
            });
            if now.duration_since(record.window_start) > Duration::from_secs(60) {
                record.count = 1;
                record.window_start = now;
            } else {
                record.count += 1;
            }
            record.count
        };
        (record_count <= COMPUTE_MAX, 60)
    } else if path.starts_with("/api/analytics/collect")
        || path.starts_with("/api/analytics/pageview")
    {
        // First-party beacons: allow healthy SPA batching, blunt spam floods.
        // Shared key covers collect + pageview together.
        let record_count = {
            let mut records = RATE_LIMITER.records.write().await;
            let now = std::time::Instant::now();
            let ip_records = records.entry(ip).or_insert_with(HashMap::new);
            let record = ip_records
                .entry("analytics_write".to_string())
                .or_insert(RequestRecord {
                    count: 0,
                    window_start: now,
                });
            if now.duration_since(record.window_start) > Duration::from_secs(60) {
                record.count = 1;
                record.window_start = now;
            } else {
                record.count += 1;
            }
            record.count
        };
        (record_count <= ANALYTICS_WRITE_MAX, 60)
    } else {
        // 标准端点：DEFAULT_PATH_MAX / 分钟（按 path 分桶）
        let allowed = RATE_LIMITER.check_limit(ip, &path).await;
        (allowed, 60)
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
    p == "/api/fetch"
        || p == "/api/analysis"
        || p == "/api/prompt/generate"
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
        assert!(is_compute_intensive("/api/fetch"));
        assert!(is_compute_intensive("/api/profile/fetch-all"));
        assert!(is_compute_intensive("/api/profile/fetch-platform"));
        assert!(is_compute_intensive("/api/agent/process"));
        assert!(is_compute_intensive("/api/agent/process/stream"));
        assert!(is_compute_intensive("/api/tapp/ai/v2/tasks"));
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

    #[tokio::test]
    async fn test_rate_limit_basic() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_requests: 3,
            window: Duration::from_secs(10),
        });

        let ip = IpAddr::from([127, 0, 0, 1]);
        let endpoint = "/api/test";

        // First 3 requests should pass
        assert!(limiter.check_limit(ip, endpoint).await);
        assert!(limiter.check_limit(ip, endpoint).await);
        assert!(limiter.check_limit(ip, endpoint).await);

        // 4th request should be blocked
        assert!(!limiter.check_limit(ip, endpoint).await);
    }

    #[tokio::test]
    async fn test_rate_limit_window_reset() {
        let limiter = RateLimiter::new(RateLimitConfig {
            max_requests: 2,
            window: Duration::from_millis(100),
        });

        let ip = IpAddr::from([127, 0, 0, 1]);
        let endpoint = "/api/test";

        // First 2 requests pass
        assert!(limiter.check_limit(ip, endpoint).await);
        assert!(limiter.check_limit(ip, endpoint).await);

        // 3rd is blocked
        assert!(!limiter.check_limit(ip, endpoint).await);

        // Wait for window to expire
        tokio::time::sleep(Duration::from_millis(150)).await;

        // Should pass again
        assert!(limiter.check_limit(ip, endpoint).await);
    }

    #[tokio::test]
    async fn test_ip_hard_cap_allows_up_to_max() {
        let limiter = RateLimiter::new(RateLimitConfig::default());
        let ip = IpAddr::from([10, 0, 0, 1]);

        for i in 0..IP_HARD_CAP_MAX {
            assert!(
                limiter.check_ip_hard_cap(ip).await,
                "request {} should be allowed under hard cap",
                i + 1
            );
        }

        // One over the ceiling
        assert!(!limiter.check_ip_hard_cap(ip).await);
        // Further attempts stay blocked within the window
        assert!(!limiter.check_ip_hard_cap(ip).await);
    }

    #[tokio::test]
    async fn test_ip_hard_cap_is_per_ip() {
        let limiter = RateLimiter::new(RateLimitConfig::default());
        let ip_a = IpAddr::from([10, 0, 0, 2]);
        let ip_b = IpAddr::from([10, 0, 0, 3]);

        for _ in 0..IP_HARD_CAP_MAX {
            assert!(limiter.check_ip_hard_cap(ip_a).await);
        }
        assert!(!limiter.check_ip_hard_cap(ip_a).await);

        // Different IP still has full budget
        assert!(limiter.check_ip_hard_cap(ip_b).await);
    }

    #[tokio::test]
    async fn test_ip_hard_cap_stored_under_reserved_key() {
        let limiter = RateLimiter::new(RateLimitConfig::default());
        let ip = IpAddr::from([10, 0, 0, 4]);

        assert!(limiter.check_ip_hard_cap(ip).await);
        assert!(limiter.check_limit(ip, "/api/other").await);

        let records = limiter.records.read().await;
        let ip_records = records.get(&ip).expect("ip map");
        assert!(
            ip_records.contains_key(IP_TOTAL_KEY),
            "hard cap must use reserved key {}",
            IP_TOTAL_KEY
        );
        assert_eq!(ip_records.get(IP_TOTAL_KEY).unwrap().count, 1);
        assert_eq!(ip_records.get("/api/other").unwrap().count, 1);
    }

    #[tokio::test]
    async fn test_ip_hard_cap_cleanup_prunes_stale() {
        let limiter = RateLimiter::new(RateLimitConfig::default());
        let ip_fresh = IpAddr::from([10, 0, 0, 5]);
        let ip_stale = IpAddr::from([10, 0, 0, 6]);

        assert!(limiter.check_ip_hard_cap(ip_fresh).await);

        // Inject a hard-cap record whose window_start is older than retain_for
        // (max(config.window, IP_HARD_CAP_WINDOW) * 2 = 120s).
        {
            let mut records = limiter.records.write().await;
            let ip_records = records.entry(ip_stale).or_insert_with(HashMap::new);
            ip_records.insert(
                IP_TOTAL_KEY.to_string(),
                RequestRecord {
                    count: 1,
                    window_start: Instant::now() - Duration::from_secs(200),
                },
            );
        }

        limiter.cleanup().await;

        let records = limiter.records.read().await;
        assert!(
            records
                .get(&ip_fresh)
                .and_then(|m| m.get(IP_TOTAL_KEY))
                .is_some(),
            "recent hard-cap record must survive cleanup"
        );
        assert!(
            !records.contains_key(&ip_stale),
            "stale hard-cap record must be pruned"
        );
    }
}
