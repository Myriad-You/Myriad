/// Public site URL for OAuth callback / Cookie Secure / frontend redirect.
/// Federation Actor URLs use a separate `GLOBAL_CONFIG` fallback chain.
///
/// 设计原则：
/// 1. 数据库优先，配置为空时回退到环境变量
/// 2. base_url 承担多种功能：OAuth 回调、Cookie Secure 判断、前端重定向等
pub struct SiteConfig;

impl SiteConfig {
    /// 获取站点 base_url
    ///
    /// # 优先级
    /// 1. 数据库 DynamicConfig.base_url
    /// 2. 环境变量 `BASE_URL`
    /// 3. 环境变量 `FRONTEND_URL`
    /// 4. 开发默认值 `http://localhost:1102`
    pub async fn get_base_url() -> String {
        use crate::GLOBAL_DYNAMIC_CONFIG;
        use std::env;

        // 1. 优先从数据库读取
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        if let Some(url) = &config.base_url {
            if !url.is_empty() {
                tracing::debug!("📍 Using base_url from database: {}", url);
                return url.trim_end_matches('/').to_string();
            }
        }
        drop(config); // 释放锁

        // 2. 回退到环境变量 BASE_URL
        if let Ok(url) = env::var("BASE_URL") {
            if !url.is_empty() {
                tracing::debug!("📍 Using BASE_URL from env: {}", url);
                return url.trim_end_matches('/').to_string();
            }
        }

        // 3. 回退到环境变量 FRONTEND_URL
        if let Ok(url) = env::var("FRONTEND_URL") {
            if !url.is_empty() {
                tracing::debug!("📍 Using FRONTEND_URL from env: {}", url);
                return url.trim_end_matches('/').to_string();
            }
        }

        // 4. 开发默认值
        tracing::warn!("⚠️ No base_url configured, using localhost default");
        "http://localhost:1102".to_string()
    }

    /// Whether Cookie `Secure` (and similar) should be set.
    ///
    /// Driven by the **public site URL**, not `ENVIRONMENT` alone:
    /// - `true` only when `base_url` is `https://` (real TLS to the browser).
    /// - Compose often sets `ENVIRONMENT=production` while still serving
    /// `http://host:port` during bring-up; marking cookies Secure there makes
    /// the browser drop login/guest cookies and drifts Tapp grant subjects.
    ///
    /// CORS uses [`AppConfig::is_production_environment`]. Analytics salt has
    /// its own `is_production_environment()` in `intake_helpers`.
    pub async fn is_production() -> bool {
        let base_url = Self::get_base_url().await;
        base_url.starts_with("https://")
    }
}

/// OAuth URL 构建器 — 启动时信息性检查
///
/// 真正的 OAuth provider 实例化走 [`crate::services::oauth::registry`]，
/// 此模块只保留启动日志辅助函数。
pub struct OAuthUrlBuilder;

impl OAuthUrlBuilder {
    /// 启动时检查 base_url 与已启用 OAuth provider 条数，仅日志，不阻塞启动。
    pub async fn validate_github_oauth_config() -> Result<(), String> {
        let base_url = SiteConfig::get_base_url().await;
        if base_url.contains("localhost") {
            tracing::debug!(
                "ℹ️  base_url is localhost: {} - configure BASE_URL for production",
                base_url
            );
        } else {
            tracing::info!("✅ Site base_url: {}", base_url);
        }

        let cfg = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        let entry_count = cfg.oauth_providers.iter().filter(|p| p.enabled).count();
        drop(cfg);

        if entry_count > 0 {
            tracing::info!("✅ OAuth providers configured: entries={}", entry_count);
        } else {
            tracing::debug!("ℹ️  No OAuth providers configured (can be set in Settings > OAuth)");
        }

        Ok(())
    }
}
