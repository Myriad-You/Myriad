//! Token authentication + failure throttling.
//!
//! # 这里修过一个可用性缺陷
//!
//! 原实现在**校验 token 之前**先查封锁状态，且封锁计数器是全局的
//! （`let key = "global"`）。两者叠加的后果：任何来源连续 6 次失败，
//! 就把**所有人**挡在门外 10 分钟 —— 包括拿着正确 token 的运维。
//!
//! 触发它不需要攻击者，一个拿着过期 token 反复重试的客户端就够了；
//! 而这恰好发生在运维最需要进 updater 的时候（换了 token、想回滚）。
//!
//! 现在的顺序是**先校验、后限流**：
//!
//! - 正确的凭据永远放行，不受任何计数器影响；
//! - 只有失败才计数，超限的**失败**返回 429；
//! - 计数按来源分桶，一个来源的失败不影响其他来源。
//!
//! 暴力破解防护没有减弱：攻击者提交错误 token 仍会在 N 次后被限流，
//! 而合法持有者不再被连坐。

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use axum::{
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use once_cell::sync::Lazy;

use crate::api::ApiState;

const MAX_FAILED_PER_MIN: u32 = 5;
const BLOCK_DURATION: Duration = Duration::from_secs(600);
const FAILURE_WINDOW: Duration = Duration::from_secs(60);

/// 单个来源桶太多会变成内存增长面，超过上限时丢弃最老的空闲桶。
const MAX_TRACKED_SOURCES: usize = 1024;

#[derive(Default)]
struct Limiter {
    /// 来源标识 → (窗口内失败时刻, 封锁到期时刻)
    counters: HashMap<String, (Vec<Instant>, Option<Instant>)>,
}

static LIMITER: Lazy<Mutex<Limiter>> = Lazy::new(|| Mutex::new(Limiter::default()));

/// 限流分桶的来源标识。
///
/// updater 位于 `myriad-admin-net` 内，请求经 gateway 转发，因此对端地址对所有
/// 调用方都相同 —— 用 `X-Forwarded-For` 的首段才能区分真实来源。取不到时退回
/// 单一桶：此时限流退化成全局，但**不会**再连坐正确凭据（见模块文档）。
fn source_key(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.split(',').next())
        .map(|v| v.trim())
        .filter(|v| !v.is_empty())
        .unwrap_or("unknown")
        .to_string()
}

/// 记录一次失败；返回该来源是否已进入封锁。
pub fn record_failure(key: &str) -> bool {
    let mut l = LIMITER.lock().unwrap();
    prune(&mut l);
    let entry = l.counters.entry(key.to_string()).or_default();
    let now = Instant::now();
    entry.0.retain(|t| now.duration_since(*t) < FAILURE_WINDOW);
    entry.0.push(now);
    if entry.0.len() as u32 > MAX_FAILED_PER_MIN {
        entry.1 = Some(now + BLOCK_DURATION);
        true
    } else {
        false
    }
}

/// 该来源当前是否处于封锁期。
pub fn is_blocked(key: &str) -> bool {
    let mut l = LIMITER.lock().unwrap();
    let now = Instant::now();
    if let Some(entry) = l.counters.get_mut(key) {
        if let Some(until) = entry.1 {
            if now < until {
                return true;
            }
            entry.1 = None;
            entry.0.clear();
        }
    }
    false
}

/// 凭据校验通过后清空该来源的失败记录。
///
/// 没有这一步的话，一次成功之后残留的失败次数会把后续正常请求推过阈值。
fn clear_failures(key: &str) {
    let mut l = LIMITER.lock().unwrap();
    l.counters.remove(key);
}

/// 丢弃既没有封锁也没有近期失败的空闲桶，避免 map 无限增长。
fn prune(l: &mut Limiter) {
    if l.counters.len() < MAX_TRACKED_SOURCES {
        return;
    }
    let now = Instant::now();
    l.counters.retain(|_, (fails, blocked)| {
        blocked.is_some_and(|until| now < until)
            || fails
                .iter()
                .any(|t| now.duration_since(*t) < FAILURE_WINDOW)
    });
}

pub fn extract_token(headers: &HeaderMap) -> Option<String> {
    let h = headers.get("X-Update-Token")?.to_str().ok()?;
    Some(h.trim().to_string())
}

/// 凭据是否匹配（常数时间比较）。
fn token_matches(headers: &HeaderMap, expected: &str) -> bool {
    match extract_token(headers) {
        Some(provided) => constant_time_eq(provided.as_bytes(), expected.as_bytes()),
        None => false,
    }
}

/// 鉴权裁决。
#[derive(Debug, PartialEq, Eq)]
pub enum AuthDecision {
    /// 凭据正确，放行。
    Allow,
    /// 凭据错误，且该来源尚未超过失败额度。
    Unauthorized,
    /// 凭据错误，且该来源已超额 —— 限流。
    Throttled,
}

/// 纯粹的鉴权决策：先校验凭据，只对失败计数。
///
/// 抽成独立函数是为了能不构造 `ApiState` 就穷举测试 —— 这里的顺序
/// （校验在前、限流在后）正是本次修复的核心，必须可测。
pub fn decide(headers: &HeaderMap, expected: &str) -> AuthDecision {
    let key = source_key(headers);

    // 正确的 token 永远放行：不查封锁状态，也就不会被别人（或自己早先的
    // 重试）制造的失败计数连坐。
    if token_matches(headers, expected) {
        clear_failures(&key);
        return AuthDecision::Allow;
    }

    // 凭据错误：记一次失败，超额则限流。
    let now_blocked = record_failure(&key);
    if now_blocked || is_blocked(&key) {
        AuthDecision::Throttled
    } else {
        AuthDecision::Unauthorized
    }
}

pub async fn token_required(
    State(state): State<ApiState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    match decide(req.headers(), state.config.update_token.expose()) {
        AuthDecision::Allow => Ok(next.run(req).await),
        AuthDecision::Throttled => Err(StatusCode::TOO_MANY_REQUESTS),
        AuthDecision::Unauthorized => Err(StatusCode::UNAUTHORIZED),
    }
}

pub async fn token_and_manual_required(
    State(state): State<ApiState>,
    req: Request,
    next: Next,
) -> Result<Response, Response> {
    if !state.state.manual_override_enabled() {
        // Explicit body so UIs don't confuse this with CSRF / admin 403.
        return Err((
            StatusCode::FORBIDDEN,
            axum::Json(serde_json::json!({
                "error": "manual override required",
                "message": "touch state/manual-override on the host (dev: .dev-updater/state/manual-override) before calling rescue endpoints"
            })),
        )
            .into_response());
    }
    token_required(State(state), req, next)
        .await
        .map_err(|status| status.into_response())
}

pub(crate) fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff: u8 = 0;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers_with(name: &str, value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(
            axum::http::HeaderName::from_bytes(name.as_bytes()).unwrap(),
            value.parse().unwrap(),
        );
        h
    }

    fn fresh_key(tag: &str) -> String {
        // 每个用例用独立的桶：LIMITER 是进程全局的
        format!("test-{tag}-{}", uuid::Uuid::new_v4())
    }

    #[test]
    fn source_key_uses_first_forwarded_hop() {
        let h = headers_with("x-forwarded-for", "203.0.113.7, 10.0.0.1");
        assert_eq!(source_key(&h), "203.0.113.7");
    }

    #[test]
    fn source_key_falls_back_when_header_absent_or_empty() {
        assert_eq!(source_key(&HeaderMap::new()), "unknown");
        assert_eq!(
            source_key(&headers_with("x-forwarded-for", "  ")),
            "unknown"
        );
    }

    #[test]
    fn failures_block_only_after_the_threshold() {
        let key = fresh_key("threshold");
        for _ in 0..MAX_FAILED_PER_MIN {
            assert!(!record_failure(&key), "must not block within the allowance");
        }
        assert!(record_failure(&key), "the next failure blocks");
        assert!(is_blocked(&key));
    }

    /// 回归点：一个来源的失败不得影响其他来源。
    #[test]
    fn one_source_cannot_lock_out_another() {
        let noisy = fresh_key("noisy");
        let quiet = fresh_key("quiet");
        for _ in 0..=MAX_FAILED_PER_MIN {
            record_failure(&noisy);
        }
        assert!(is_blocked(&noisy));
        assert!(
            !is_blocked(&quiet),
            "a different source must stay unaffected — the old global counter \
             locked out every caller at once"
        );
    }

    /// 回归点：凭据正确时清空计数，之前的失败不会累积到后续请求上。
    #[test]
    fn success_clears_prior_failures() {
        let key = fresh_key("clears");
        for _ in 0..MAX_FAILED_PER_MIN {
            record_failure(&key);
        }
        clear_failures(&key);
        // 清空后必须重新拿到完整额度
        for _ in 0..MAX_FAILED_PER_MIN {
            assert!(!record_failure(&key));
        }
    }

    #[test]
    fn token_matches_is_exact_and_rejects_missing() {
        assert!(token_matches(
            &headers_with("x-update-token", "s3cret"),
            "s3cret"
        ));
        assert!(!token_matches(
            &headers_with("x-update-token", "s3cre"),
            "s3cret"
        ));
        assert!(!token_matches(
            &headers_with("x-update-token", "s3crett"),
            "s3cret"
        ));
        assert!(!token_matches(&HeaderMap::new(), "s3cret"));
        // 首尾空白被 trim（与 extract_token 一致）
        assert!(token_matches(
            &headers_with("x-update-token", " s3cret "),
            "s3cret"
        ));
    }

    #[test]
    fn constant_time_eq_matches_semantics_of_plain_compare() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"ab"));
        assert!(constant_time_eq(b"", b""));
    }

    /// **本次修复的核心断言。**
    ///
    /// 同一来源打满失败额度进入封锁后，拿着**正确** token 的请求仍必须放行。
    ///
    /// 修复前的顺序是「先查封锁、后校验」，所以封锁期内正确凭据一样被拒 ——
    /// 一个反复重试过期 token 的客户端就能把运维锁在门外 10 分钟。
    #[test]
    fn correct_token_is_never_blocked_by_prior_failures() {
        const TOKEN: &str = "the-real-token";
        let src = fresh_key("lockout");
        let mut bad = headers_with("x-forwarded-for", &src);
        bad.insert("x-update-token", "wrong".parse().unwrap());

        // 打到封锁
        let mut throttled = false;
        for _ in 0..=MAX_FAILED_PER_MIN {
            if decide(&bad, TOKEN) == AuthDecision::Throttled {
                throttled = true;
            }
        }
        assert!(throttled, "repeated bad tokens must eventually throttle");
        assert!(is_blocked(&src));

        // 同一来源、正确 token —— 必须放行
        let mut good = headers_with("x-forwarded-for", &src);
        good.insert("x-update-token", TOKEN.parse().unwrap());
        assert_eq!(
            decide(&good, TOKEN),
            AuthDecision::Allow,
            "a valid token must never be rejected because of earlier failed attempts"
        );

        // 且成功已清空计数，后续正常请求不会被残留计数推过阈值
        assert!(!is_blocked(&src));
    }

    #[test]
    fn wrong_token_is_unauthorized_before_the_threshold() {
        let src = fresh_key("unauth");
        let mut bad = headers_with("x-forwarded-for", &src);
        bad.insert("x-update-token", "nope".parse().unwrap());
        assert_eq!(decide(&bad, "real"), AuthDecision::Unauthorized);
    }

    #[test]
    fn missing_token_counts_as_a_failure() {
        let src = fresh_key("missing");
        let h = headers_with("x-forwarded-for", &src);
        assert_eq!(decide(&h, "real"), AuthDecision::Unauthorized);
    }
}
