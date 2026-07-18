//! OAuth CSRF state 内存存储
//!
//! 通用版本：state 中带 `provider_slug` 区分回调路由，
//! 同时支持 Login 和 LinkAccount 两种用途。
//!
//! 详见 docs/oauth-refactor-plan.md

use once_cell::sync::Lazy;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

/// state 有效期 10 分钟
pub const STATE_TTL: Duration = Duration::from_secs(600);
/// 防内存耗尽
const MAX_STATES: usize = 10_000;

#[derive(Debug, Clone, PartialEq)]
pub enum OAuthPurpose {
    Login,
    /// LinkAccount 时携带"当前已登录用户 id"
    LinkAccount(i32),
    /// 数据平台授权（如 Discord 同步）：写入平台 token，不登录/不绑 identity
    PlatformData {
        user_id: i32,
        platform: String,
    },
}

#[derive(Debug, Clone)]
pub struct StoredState {
    pub provider_slug: String,
    pub purpose: OAuthPurpose,
    pub created_at: Instant,
}

/// Why `consume_state` failed — distinguishes never-seen vs TTL-elapsed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConsumeStateError {
    /// State key was not present (already consumed, never issued, or evicted).
    Missing,
    /// State was present but past [`STATE_TTL`].
    Expired,
}

impl ConsumeStateError {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Missing => "missing",
            Self::Expired => "expired",
        }
    }
}

pub static OAUTH_STATES: Lazy<Arc<RwLock<HashMap<String, StoredState>>>> = Lazy::new(|| {
    let store: Arc<RwLock<HashMap<String, StoredState>>> = Arc::new(RwLock::new(HashMap::new()));
    let store_clone = store.clone();

    // 周期清理过期 state
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(60));
        loop {
            interval.tick().await;
            let mut states = store_clone.write().await;
            let now = Instant::now();
            let before = states.len();
            states.retain(|_, s| now.duration_since(s.created_at) < STATE_TTL);
            let removed = before - states.len();
            if removed > 0 {
                tracing::debug!(
                    "🧹 OAuth state cleanup: removed {} expired, {} remain",
                    removed,
                    states.len()
                );
            }
        }
    });

    store
});

/// First 8 chars of state for safe logging (full state is a CSRF secret).
fn state_prefix(state: &str) -> &str {
    // States are hex; this is byte-safe for ASCII. Char-boundary fallback for generality.
    match state.char_indices().nth(8) {
        Some((i, _)) => &state[..i],
        None => state,
    }
}

/// 写入 state（已带容量保护：超限时淘汰最旧）
pub async fn insert_state(state: String, stored: StoredState) {
    let prefix = state_prefix(&state).to_string();
    let provider_slug = stored.provider_slug.clone();

    let mut states = OAUTH_STATES.write().await;
    if states.len() >= MAX_STATES {
        if let Some(oldest) = states
            .iter()
            .min_by_key(|(_, v)| v.created_at)
            .map(|(k, _)| k.clone())
        {
            states.remove(&oldest);
        }
    }
    states.insert(state, stored);
    let store_size = states.len();
    drop(states);

    tracing::debug!(
        store_size,
        state_prefix = %prefix,
        provider_slug = %provider_slug,
        "OAuth state inserted"
    );
}

/// 一次性消费 state（验证后立即删除）。
///
/// Returns [`ConsumeStateError::Missing`] if the key is absent, or
/// [`ConsumeStateError::Expired`] if it was present but past TTL.
pub async fn consume_state(state: &str) -> Result<StoredState, ConsumeStateError> {
    let prefix = state_prefix(state);
    let mut states = OAUTH_STATES.write().await;
    let store_size = states.len();

    let Some(stored) = states.remove(state) else {
        tracing::warn!(
            reason = "missing",
            store_size,
            state_prefix = %prefix,
            "OAuth state consume failed"
        );
        return Err(ConsumeStateError::Missing);
    };

    if Instant::now().duration_since(stored.created_at) >= STATE_TTL {
        let store_size_after = states.len();
        drop(states);
        tracing::warn!(
            reason = "expired",
            store_size = store_size_after,
            state_prefix = %prefix,
            "OAuth state consume failed"
        );
        return Err(ConsumeStateError::Expired);
    }

    Ok(stored)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_state(label: &str) -> String {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        format!(
            "test_{}_{}_{}",
            label,
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        )
    }

    fn sample_stored(created_at: Instant) -> StoredState {
        StoredState {
            provider_slug: "github".to_string(),
            purpose: OAuthPurpose::Login,
            created_at,
        }
    }

    #[tokio::test]
    async fn consume_missing_state_returns_missing() {
        let key = unique_state("missing");
        let err = consume_state(&key).await.expect_err("expected missing");
        assert_eq!(err, ConsumeStateError::Missing);
        assert_eq!(err.as_str(), "missing");
    }

    #[tokio::test]
    async fn consume_expired_state_returns_expired() {
        let key = unique_state("expired");
        insert_state(
            key.clone(),
            sample_stored(Instant::now() - STATE_TTL - Duration::from_secs(1)),
        )
        .await;

        let err = consume_state(&key).await.expect_err("expected expired");
        assert_eq!(err, ConsumeStateError::Expired);
        assert_eq!(err.as_str(), "expired");

        // One-shot: expired entry was removed; second consume is missing
        let err2 = consume_state(&key).await.expect_err("expected missing after consume");
        assert_eq!(err2, ConsumeStateError::Missing);
    }

    #[tokio::test]
    async fn consume_valid_state_ok_and_one_shot() {
        let key = unique_state("valid");
        insert_state(key.clone(), sample_stored(Instant::now())).await;

        let stored = consume_state(&key).await.expect("valid state");
        assert_eq!(stored.provider_slug, "github");
        assert_eq!(stored.purpose, OAuthPurpose::Login);

        let err = consume_state(&key)
            .await
            .expect_err("second consume should miss");
        assert_eq!(err, ConsumeStateError::Missing);
    }

    #[test]
    fn state_prefix_truncates_to_eight() {
        assert_eq!(state_prefix("abcdefghij"), "abcdefgh");
        assert_eq!(state_prefix("abc"), "abc");
        assert_eq!(state_prefix(""), "");
    }
}
