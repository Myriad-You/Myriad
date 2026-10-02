//! AI 分析器工厂
//!
//! 统一创建 AI 分析器实例，支持 OpenAI 和 Gemini，支持 Lite/Standard/Pro 模型层级

use crate::GLOBAL_DYNAMIC_CONFIG;
use crate::config::ModelTier;
use crate::services::analyzer::{AiAnalyzer, AiProvider};
use std::sync::atomic::{AtomicBool, Ordering};

/// 每档只喊一次。这条在每次 AI 调用上都会命中，喊满日志反而没人看。
///
/// 站长改完配置不会立刻再看到它。这条提醒的是：档已开但没配该档模型，
/// 实际会跑 Standard。进程寿命内只说一次。
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

/// 创建 Standard 档的 AI 分析器。
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
    // Lite jobs must not silently spend Standard when Lite has no model.
    if tier == ModelTier::Lite && !config.lite_on() {
        return None;
    }
    if config.tier_falls_back_to_standard(tier) {
        warn_tier_fallback(tier, &config.ai_model);
    }
    let resolved = config.resolve_ai_config(tier);

    if !resolved.text_ready() {
        return None;
    }
    let api_key = resolved
        .api_key
        .filter(|key| !key.trim().is_empty())
        .unwrap_or_default();
    let provider = match AiProvider::from_str(&resolved.api_format) {
        Ok(provider) => provider,
        Err(error) => {
            tracing::error!(%error, "invalid AI provider configuration");
            return None;
        }
    };
    let base_url = if resolved.base_url.is_empty() {
        None
    } else {
        Some(resolved.base_url)
    };

    let analyzer = match request_timeout {
        Some(timeout) => {
            AiAnalyzer::new_with_timeout(provider, api_key, resolved.model, base_url, timeout).await
        }
        None => AiAnalyzer::new(provider, api_key, resolved.model, base_url).await,
    };
    Some(if tier == ModelTier::Lite {
        analyzer.with_output_cap(LITE_OUTPUT_CAP)
    } else {
        analyzer
    })
}

/// Most output a Lite call asks for when it names none, thinking included.
/// Lite talks and judges in short replies; asking for the model's whole
/// window instead has the gateway hold credit for all of it, and refuse the
/// call outright when the balance is below that.
const LITE_OUTPUT_CAP: u32 = 8192;

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
    lite_analyzer(resolved, request_timeout).await
}

/// Lite for small typed judgments (see `aux_judge_model`): a fast model
/// deciding, while Lite's own model speaks. Judgments are short, so it
/// thinks little: measured on the judgment suite, the same decisions in
/// about two thirds of the time.
pub async fn create_lite_judge_ai_analyzer_with_timeout(
    request_timeout: Option<std::time::Duration>,
) -> Option<AiAnalyzer> {
    let resolved = GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .resolve_lite_judge_ai_config()?;
    lite_analyzer(resolved, request_timeout)
        .await
        .map(AiAnalyzer::with_light_thinking)
}

/// The embedding model (`aux_embedding_model`), when one is set: what a
/// memory means, as a vector. `None`: recall goes by words alone.
pub async fn create_lite_embedding_analyzer_with_timeout(
    request_timeout: Option<std::time::Duration>,
) -> Option<AiAnalyzer> {
    let resolved = GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .resolve_lite_embedding_ai_config()?;
    lite_analyzer(resolved, request_timeout).await
}

async fn lite_analyzer(
    resolved: crate::config::ResolvedAiConfig,
    request_timeout: Option<std::time::Duration>,
) -> Option<AiAnalyzer> {
    if !resolved.text_ready() {
        return None;
    }
    let api_key = resolved
        .api_key
        .filter(|key| !key.trim().is_empty())
        .unwrap_or_default();
    let provider = match AiProvider::from_str(&resolved.api_format) {
        Ok(provider) => provider,
        Err(error) => {
            tracing::error!(%error, "invalid Lite AI provider configuration");
            return None;
        }
    };
    let base_url = if resolved.base_url.is_empty() {
        None
    } else {
        Some(resolved.base_url)
    };

    let analyzer = match request_timeout {
        Some(timeout) => {
            AiAnalyzer::new_with_timeout(provider, api_key, resolved.model, base_url, timeout).await
        }
        None => AiAnalyzer::new(provider, api_key, resolved.model, base_url).await,
    };
    Some(analyzer.with_output_cap(LITE_OUTPUT_CAP))
}
