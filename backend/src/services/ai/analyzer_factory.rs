//! AI 分析器工厂
//!
//! 统一创建 AI 分析器实例，支持 OpenAI 和 Gemini，支持 Standard/Pro 模型层级

use crate::config::ModelTier;
use crate::services::analyzer::{AiAnalyzer, AiProvider};
use crate::GLOBAL_DYNAMIC_CONFIG;

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
            AiAnalyzer::new_with_timeout(provider, api_key, resolved.model, base_url, timeout)
                .await
        }
        None => AiAnalyzer::new(provider, api_key, resolved.model, base_url).await,
    })
}
