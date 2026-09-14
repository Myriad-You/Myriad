//! Cached AI provider configuration for Tapp / Agent governed calls.
//!
//! Lives in services so AI task execution does not reach through HTTP-layer
//! `tapp_runtime::common` for dynamic config resolution.

use once_cell::sync::Lazy;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::GLOBAL_DYNAMIC_CONFIG;
use crate::config::ModelTier;
use crate::services::analyzer::AiProvider;

/// Text-generation AI provider config (key material included; never log).
#[derive(Clone)]
pub struct AiConfig {
    pub provider: AiProvider,
    pub api_key: String,
    pub model: String,
    pub base_url: Option<String>,
}

/// Image-generation AI config (resolution chosen by callers).
#[derive(Clone)]
pub struct AiImageConfig {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub base_url: String,
}

/// Domain error when no usable provider is configured.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiConfigError {
    NotConfigured,
    InvalidProvider(String),
}

impl AiConfigError {
    pub fn message(&self) -> &str {
        match self {
            Self::NotConfigured => myriad_agent_rules::AI_PROVIDER_NOT_CONFIGURED,
            Self::InvalidProvider(message) => message,
        }
    }
}

impl std::fmt::Display for AiConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for AiConfigError {}

struct SingleCache<V: Clone> {
    value: Option<V>,
    cached_at: Option<Instant>,
    ttl: Duration,
}

impl<V: Clone> SingleCache<V> {
    fn new(ttl: Duration) -> Self {
        Self {
            value: None,
            cached_at: None,
            ttl,
        }
    }

    fn get(&self) -> Option<V> {
        if let (Some(value), Some(cached_at)) = (&self.value, self.cached_at) {
            if cached_at.elapsed() < self.ttl {
                return Some(value.clone());
            }
        }
        None
    }

    fn set(&mut self, value: V) {
        self.value = Some(value);
        self.cached_at = Some(Instant::now());
    }

    fn clear(&mut self) {
        self.value = None;
        self.cached_at = None;
    }
}

static AI_CONFIG_CACHE: Lazy<RwLock<SingleCache<AiConfig>>> =
    Lazy::new(|| RwLock::new(SingleCache::new(Duration::from_secs(300))));
static AI_PRO_CONFIG_CACHE: Lazy<RwLock<SingleCache<AiConfig>>> =
    Lazy::new(|| RwLock::new(SingleCache::new(Duration::from_secs(300))));
static AI_LITE_CONFIG_CACHE: Lazy<RwLock<SingleCache<AiConfig>>> =
    Lazy::new(|| RwLock::new(SingleCache::new(Duration::from_secs(300))));
static AI_IMAGE_CONFIG_CACHE: Lazy<RwLock<SingleCache<AiImageConfig>>> =
    Lazy::new(|| RwLock::new(SingleCache::new(Duration::from_secs(300))));

fn cache_for_tier(tier: ModelTier) -> &'static RwLock<SingleCache<AiConfig>> {
    match tier {
        ModelTier::Lite => &AI_LITE_CONFIG_CACHE,
        ModelTier::Standard => &AI_CONFIG_CACHE,
        ModelTier::Pro => &AI_PRO_CONFIG_CACHE,
    }
}

pub async fn invalidate_ai_config_cache() {
    AI_CONFIG_CACHE.write().await.clear();
    AI_PRO_CONFIG_CACHE.write().await.clear();
    AI_LITE_CONFIG_CACHE.write().await.clear();
    AI_IMAGE_CONFIG_CACHE.write().await.clear();
}

/// Resolve text AI config for a model tier (5-minute process cache).
pub async fn get_ai_config_for_tier(tier: ModelTier) -> Result<AiConfig, AiConfigError> {
    let cache_ref = cache_for_tier(tier);
    {
        let cache = cache_ref.read().await;
        if let Some(config) = cache.get() {
            return Ok(config);
        }
    }

    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let resolved = config.resolve_ai_config(tier);
    let ai_config = resolved
        .text_ready()
        .then(|| {
            let provider = AiProvider::from_str(&resolved.api_format)
                .map_err(|error| AiConfigError::InvalidProvider(error.to_string()))?;
            let base_url = if resolved.base_url.is_empty() {
                None
            } else {
                Some(resolved.base_url.clone())
            };
            Ok(AiConfig {
                provider,
                api_key: resolved
                    .api_key
                    .filter(|key| !key.trim().is_empty())
                    .unwrap_or_default(),
                model: resolved.model.clone(),
                base_url,
            })
        })
        .transpose()?;

    match ai_config {
        Some(cfg) => {
            let mut cache = cache_ref.write().await;
            cache.set(cfg.clone());
            Ok(cfg)
        }
        None => Err(AiConfigError::NotConfigured),
    }
}

/// Resolve image AI config (5-minute process cache).
pub async fn get_ai_image_config() -> Result<AiImageConfig, AiConfigError> {
    {
        let cache = AI_IMAGE_CONFIG_CACHE.read().await;
        if let Some(config) = cache.get() {
            return Ok(config);
        }
    }

    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    let resolved = crate::services::image_generation::config_from_dynamic(&config)
        .map_err(|_| AiConfigError::NotConfigured)?;
    let image_config = AiImageConfig {
        provider: resolved.provider,
        model: resolved.model,
        api_key: resolved.api_key,
        base_url: resolved.base_url,
    };

    let mut cache = AI_IMAGE_CONFIG_CACHE.write().await;
    cache.set(image_config.clone());
    Ok(image_config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_configured_message_is_stable() {
        assert_eq!(
            AiConfigError::NotConfigured.message(),
            myriad_agent_rules::AI_PROVIDER_NOT_CONFIGURED
        );
        assert_eq!(
            AiConfigError::NotConfigured.to_string(),
            myriad_agent_rules::AI_PROVIDER_NOT_CONFIGURED
        );
    }

    #[test]
    fn single_cache_expires_after_ttl() {
        let mut cache = SingleCache::new(Duration::from_millis(5));
        cache.set(42);
        assert_eq!(cache.get(), Some(42));
        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(cache.get(), None);
    }
}
