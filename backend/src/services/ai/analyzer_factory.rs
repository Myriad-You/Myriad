//! AI 分析器工厂
//!
//! 统一创建 AI 分析器实例，支持 OpenAI 和 Gemini，支持 Lite/Standard/Pro 模型层级

use crate::config::ModelTier;
use crate::services::analyzer::{AiAnalyzer, AiProvider};
use crate::GLOBAL_DYNAMIC_CONFIG;
use std::sync::atomic::{AtomicBool, Ordering};

/// 每档只喊一次。这条在每次 AI 调用上都会命中，喊满日志反而没人看。
///
/// 站长改完配置不会立刻再看到它——但这条要提醒的是「你以为开了、其实没开」，
/// 那是个长期状态，进程起来时说一次就够。
fn warn_tier_fallback(tier: ModelTier, standard_model: &str) {
    static WARNED_LITE: AtomicBool = AtomicBool::new(false);
    static WARNED_PRO: AtomicBool = AtomicBool::new(false);
    let warned = match tier {
        ModelTier::Lite => &WARNED_LITE,
        ModelTier::Pro => &WARNED_PRO,
        ModelTier::Standard => return,
    };
    if warned.swap(true, Ordering::Relaxed) {
        return;
    }
    tracing::warn!(
        ?tier,
        standard_model,
        "该档已启用但没有配置自己的模型，实际会使用 Standard 的模型：\
         账单与延迟都按 Standard 计，日志里的档位名不代表真正跑的模型"
    );
}

/// 创建标准层级的 AI 分析器（默认，向后兼容）
pub async fn create_ai_analyzer() -> Option<AiAnalyzer> {
    create_ai_analyzer_for_tier(ModelTier::Standard).await
}

/// 根据模型层级创建 AI 分析器
pub async fn create_ai_analyzer_for_tier(tier: ModelTier) -> Option<AiAnalyzer> {
    create_ai_analyzer_for_tier_with_timeout(tier, None).await
}

/// 根据模型层级创建 AI 分析器，可为长任务指定单次请求超时
pub async fn create_ai_analyzer_for_tier_with_timeout(
    tier: ModelTier,
    request_timeout: Option<std::time::Duration>,
) -> Option<AiAnalyzer> {
    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    // Lite jobs must not silently spend Standard when the Lite tier is off.
    if tier == ModelTier::Lite && !config.lite_enabled {
        return None;
    }
    if config.tier_falls_back_to_standard(tier) {
        warn_tier_fallback(tier, &config.openai_model);
    }
    let resolved = config.resolve_ai_config(tier);

    let api_key = resolved.api_key.filter(|k| !k.is_empty())?;
    let provider = AiProvider::from_str(&resolved.provider);
    let base_url = if resolved.base_url.is_empty() {
        None
    } else {
        Some(resolved.base_url)
    };

    Some(match request_timeout {
        Some(timeout) => {
            AiAnalyzer::new_with_timeout(provider, api_key, resolved.model, base_url, timeout).await
        }
        None => AiAnalyzer::new(provider, api_key, resolved.model, base_url).await,
    })
}

/// Creates an analyzer only when an explicit Lite model is configured.
/// Credentials may still come from the shared provider vault, but the model
/// itself never falls back to the Standard tier.
pub async fn create_strict_lite_ai_analyzer_with_timeout(
    request_timeout: Option<std::time::Duration>,
) -> Option<AiAnalyzer> {
    let resolved = GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .resolve_strict_lite_ai_config()?;
    let api_key = resolved.api_key.filter(|key| !key.is_empty())?;
    let provider = AiProvider::from_str(&resolved.provider);
    let base_url = if resolved.base_url.is_empty() {
        None
    } else {
        Some(resolved.base_url)
    };

    Some(match request_timeout {
        Some(timeout) => {
            AiAnalyzer::new_with_timeout(provider, api_key, resolved.model, base_url, timeout).await
        }
        None => AiAnalyzer::new(provider, api_key, resolved.model, base_url).await,
    })
}
