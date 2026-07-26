use serde::{Deserialize, Serialize};
use std::env;

/// AI 模型层级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ModelTier {
    /// 标准模型 - 用于日常任务
    #[default]
    Standard,
    /// Pro 模型 - 用于复杂任务
    Pro,
}

/// 单个 OAuth Provider 配置（OIDC / 其他）
///
/// GitHub 仍走 `github_client_id` / `github_client_secret` 平铺字段（内置 provider）。
/// 这里专门给 OIDC / 未来其他 provider 用。
///
/// 详见 [`docs/oauth-refactor-plan.md`](../../docs/oauth-refactor-plan.md) §5。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OAuthProviderEntry {
    /// 路由 slug：`/api/auth/oauth/<slug>/login`。须全局唯一。
    pub slug: String,
    /// "oidc" 或其他扩展 kind
    pub kind: String,
    /// UI 展示名
    pub display_name: String,
    /// 是否启用
    pub enabled: bool,
    pub client_id: String,
    pub client_secret: String,
    /// OAuth scopes (OIDC 至少需要 "openid")
    #[serde(default)]
    pub scopes: Vec<String>,
    /// OIDC discovery URL: `.../.well-known/openid-configuration`
    #[serde(default)]
    pub discovery_url: Option<String>,
    /// UI 图标 URL（可选；缺省时前端用默认 OIDC logo）
    #[serde(default)]
    pub icon_url: Option<String>,
}

/// 解析后的 AI 配置（已根据 tier 确定具体的 provider/key/model）
pub struct ResolvedAiConfig {
    pub provider: String,
    pub api_key: Option<String>,
    pub model: String,
    pub base_url: String,
}

/// 核心应用配置（从环境变量读取）
/// 这些是应用启动所必需的基础设施配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    /// 数据库连接URL
    pub database_url: String,
    /// 服务器监听地址
    pub server_host: String,
    /// 服务器监听端口
    pub server_port: u16,
    /// 前端静态文件路径
    pub frontend_dist_path: String,
    /// JWT密钥
    pub jwt_secret: String,
    /// CORS允许的源
    pub cors_origins: Vec<String>,
    /// 基础URL（用于自动生成OAuth回调等URL）
    pub base_url: Option<String>,
    /// 前端URL（用于OAuth成功后重定向）
    pub frontend_url: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            database_url: String::new(),
            server_host: "127.0.0.1".to_string(),
            server_port: 1103,
            frontend_dist_path: "../frontend/dist".to_string(),
            jwt_secret: String::new(),
            cors_origins: vec![
                "http://localhost:1102".to_string(),
                "http://localhost:1103".to_string(),
            ],
            base_url: None,
            frontend_url: None,
        }
    }
}

impl AppConfig {
    /// 从环境变量加载配置
    pub fn from_env() -> anyhow::Result<Self> {
        let jwt_secret = env::var("JWT_SECRET").unwrap_or_else(|_| String::new());

        // Validate JWT secret strength in production
        if !jwt_secret.is_empty() {
            Self::validate_jwt_secret(&jwt_secret)?;
        }

        Ok(Self {
            database_url: env::var("DATABASE_URL").unwrap_or_else(|_| String::new()),
            server_host: env::var("SERVER_HOST").unwrap_or_else(|_| "127.0.0.1".to_string()),
            server_port: env::var("SERVER_PORT")
                .ok()
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "1103".to_string())
                .parse()?,
            frontend_dist_path: env::var("FRONTEND_DIST_PATH")
                .unwrap_or_else(|_| "../frontend/dist".to_string()),
            jwt_secret,
            cors_origins: env::var("CORS_ORIGINS")
                .unwrap_or_else(|_| "http://localhost:1102,http://localhost:1103".to_string())
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            base_url: env::var("BASE_URL").ok().filter(|s| !s.is_empty()),
            frontend_url: env::var("FRONTEND_URL").ok().filter(|s| !s.is_empty()),
        })
    }

    /// 验证 JWT Secret 强度
    fn validate_jwt_secret(secret: &str) -> anyhow::Result<()> {
        // Minimum length check (32 characters recommended)
        if secret.len() < 32 {
            anyhow::bail!(
                "JWT_SECRET is too weak. Must be at least 32 characters. \
                Generate a strong secret with: openssl rand -base64 32"
            );
        }

        // Warn if using obvious weak values
        let weak_secrets = [
            "secret",
            "your-secret-key-here",
            "change-me",
            "changeme",
            "your_secret_key_here",
            "your-secret-key-here-change-in-production",
        ];

        let secret_lower = secret.to_lowercase();
        if weak_secrets.iter().any(|&weak| secret_lower.contains(weak)) {
            anyhow::bail!(
                "JWT_SECRET contains weak/default value. \
                Generate a strong secret with: openssl rand -base64 32"
            );
        }

        Ok(())
    }

    /// 验证配置是否完整
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.database_url.is_empty() {
            anyhow::bail!("DATABASE_URL is required");
        }
        if self.jwt_secret.is_empty() {
            anyhow::bail!("JWT_SECRET is required");
        }

        Self::validate_jwt_secret(&self.jwt_secret)?;

        Ok(())
    }
}

/// 应用级配置（从数据库读取）
/// 这些配置可以在运行时通过API修改
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicConfig {
    // AI 配置（标准模型）
    pub ai_provider: String,
    pub gemini_api_key: Option<String>,
    pub gemini_model: String,
    pub openai_api_key: Option<String>,
    pub openai_model: String,
    pub openai_base_url: String,
    pub openai_max_tokens: i32,
    // AI 配置（Pro 模型）
    pub pro_enabled: bool,
    pub pro_ai_provider: String,
    pub pro_gemini_api_key: Option<String>,
    pub pro_gemini_model: String,
    pub pro_openai_api_key: Option<String>,
    pub pro_openai_model: String,
    pub pro_openai_base_url: String,
    pub topic_style: String,

    // 平台配置
    pub github_enabled: Option<bool>,
    pub github_token: Option<String>,
    pub github_username: Option<String>,
    pub bilibili_enabled: Option<bool>,
    pub bilibili_uid: Option<String>,
    pub steam_enabled: Option<bool>,
    pub steam_api_key: Option<String>,
    pub steam_id: Option<String>,
    pub netease_enabled: Option<bool>,
    pub netease_user_id: Option<String>,
    pub bangumi_enabled: Option<bool>,
    pub bangumi_username: Option<String>,
    pub bangumi_access_token: Option<String>,
    /// 平台展示顺序（平台名称列表，如 ["Steam", "GitHub", ...]）
    /// 决定报告页平台卡片的出现顺序，未列出的平台按默认顺序排在最后
    pub platform_order: Option<Vec<String>>,
    pub bangumi_user_agent: Option<String>,
    /// X (Twitter) 平台
    pub x_enabled: Option<bool>,
    /// X 用户名（不含 @）
    pub x_username: Option<String>,
    /// App Bearer Token（developer.x.com 生成，只读同步用；分享走 Intent 不需要用户 OAuth）
    pub x_bearer_token: Option<String>,
    /// Discord 数据平台
    pub discord_enabled: Option<bool>,
    /// Discord OAuth user access token（scope: identify guilds connections）
    pub discord_access_token: Option<String>,
    /// Discord OAuth refresh token（建议填写，access token 会过期）
    pub discord_refresh_token: Option<String>,
    /// access token 过期时间（Unix 秒，字符串存储）
    pub discord_token_expires_at: Option<String>,
    /// Discord 用户 snowflake id（test/同步成功后回写）
    pub discord_user_id: Option<String>,
    /// MyAnimeList 数据平台
    pub mal_enabled: Option<bool>,
    /// MAL 用户名（必填即可启用；默认走公开 load.json）
    pub mal_username: Option<String>,
    /// MAL API Client ID（可选；填写后优先走官方 API v2 + X-MAL-CLIENT-ID）
    pub mal_client_id: Option<String>,
    /// Xbox 数据平台（成就向报告，Xbox Live 不提供游玩时长）
    pub xbox_enabled: Option<bool>,
    /// Xbox Gamertag（现代格式可含 #suffix，如 "染川瞳#6234"）
    pub xbox_gamertag: Option<String>,
    /// OpenXBL API Key（xbl.io 申请，服务端凭据）
    pub openxbl_api_key: Option<String>,
    /// PlayStation 数据平台（奖杯向报告）
    pub psn_enabled: Option<bool>,
    /// PSN Online ID
    pub psn_online_id: Option<String>,
    /// PSN NPSSO cookie（ca.account.sony.com 获取，服务端凭据）
    pub psn_npsso: Option<String>,

    // Tapp 外部 API 密钥（用于 Tapp API 声明系统）
    pub openweather_api_key: Option<String>,

    // 腾讯云语音服务配置（TTS 文本转语音 / ASR 语音转文本）
    pub tencent_secret_id: Option<String>,
    pub tencent_secret_key: Option<String>,
    pub tencent_region: Option<String>, // 默认 ap-guangzhou

    // UI 配置
    pub ui_wallpaper_url: Option<String>,
    pub ui_wallpaper_blur: i32,
    pub ui_wallpaper_parallax: bool,
    // Evocative 壁纸动效
    pub ui_evocative_parallax: bool,
    pub ui_evocative_dynamic_blur: bool,
    pub ui_evocative_ripple: bool,
    /// Evocative 动效帧率 (30 或 60)
    pub ui_evocative_fps: i32,
    /// 涟漪效果 Canvas 质量 (0.5-1.0, 默认 0.85)
    pub ui_evocative_ripple_quality: f64,
    pub ui_theme: Option<String>,
    pub ui_primary_color: Option<String>,
    pub ui_secondary_color: Option<String>,
    pub pet_enabled: bool,
    pub pet_image_url: Option<String>,

    // 站点元数据
    pub site_title: Option<String>,
    pub site_description: Option<String>,
    pub site_favicon: Option<String>,
    pub site_icp: Option<String>,       // ICP 备案号
    pub site_gongan: Option<String>,    // 公安备案号
    pub cloud_sponsors: Option<String>, // 云赞助商（cloudflare,edgeone,upyun 逗号分隔）

    // 音乐配置
    pub music_enabled: Option<String>,
    pub music_source: Option<String>,
    pub music_playlist_id: Option<String>,

    // AI 图片生成配置（从虚拟人设移动过来）
    pub ai_image_provider: String, // "pollinations" 或 "pixai"
    pub ai_image_model: String,
    pub ai_image_width: i32,
    pub ai_image_height: i32,
    pub pixai_api_key: Option<String>,

    // 自动数据获取配置
    pub enable_auto_fetch: bool,
    pub fetch_interval_hours: i32,

    // OAuth 配置（GitHub 是内置 provider，仍用平铺字段；其他 provider 走 oauth_providers）
    pub github_client_id: Option<String>,
    pub github_client_secret: Option<String>,
    pub github_redirect_url: String,

    /// 通用 OIDC providers 列表（PR #3）
    /// 详见 docs/oauth-refactor-plan.md §5
    pub oauth_providers: Vec<OAuthProviderEntry>,

    /// 是否允许公开本地账号注册（PR #4）
    pub allow_local_registration: bool,

    // 站点 URL 配置（用于自动生成 OAuth 回调等 URL）
    pub base_url: Option<String>,

    // 仪表盘配置
    pub dashboard_layout: Option<String>,
    pub dashboard_title: Option<String>,
    pub custom_platforms: Option<String>, // 自定义社交平台数据 (JSON)
    pub widget_theme: Option<String>,     // 小组件外观主题 (JSON: surface/glow)

    // 标题字体样式配置
    pub title_font: Option<String>,   // 标题字体 ID
    pub title_font_size: Option<f64>, // 标题字体大小倍率
    pub title_color: Option<String>,  // 标题颜色 ID

    // 控制面板小组件配置
    pub control_panel_layout: Option<String>,
    pub control_panel_rows: i32,

    // Tapp 多窗口方案配置
    pub tapp_window_schemes: Option<String>, // 窗口方案数据 (JSON)

    // ========== Tapp 权限下放配置 ==========
    // 基于 Tapp 系统的 elevated 级别权限（13 项可配置下放）
    // 这些权限默认只有管理员可用，可以配置下放给普通用户或游客
    // 注意：basic 级别权限默认可授予所有用户
    // 注意：privileged 级别权限始终只限管理员

    // ===== 普通用户可使用的 elevated 权限（13 项） =====
    /// ai:generate - AI 生成内容
    pub user_perm_ai_generate: bool,
    /// ai:analyze - AI 分析数据
    pub user_perm_ai_analyze: bool,
    /// ai:chat - AI 对话
    pub user_perm_ai_chat: bool,
    /// ai:image - AI 图片生成
    pub user_perm_ai_image: bool,
    /// report:write - 写入/生成报告
    pub user_perm_report_write: bool,
    /// network:fetch - 发起网络请求
    pub user_perm_network_fetch: bool,
    /// media:control - 已降为 basic，始终允许；字段保留供 DB/API 兼容
    pub user_perm_media_control: bool,
    /// component:theme - 注册主题组件
    pub user_perm_component_theme: bool,
    /// shortcut:register - 注册快捷键
    pub user_perm_shortcut_register: bool,
    /// event:publish - 发布事件
    pub user_perm_event_publish: bool,
    /// scheduler:register - 注册定时任务
    pub user_perm_scheduler_register: bool,
    /// speech:tts - 文本转语音
    pub user_perm_speech_tts: bool,
    /// speech:asr - 语音转文本
    pub user_perm_speech_asr: bool,

    // ===== 游客可使用的 elevated 权限（13 项） =====
    /// ai:generate - AI 生成内容（游客）
    pub guest_perm_ai_generate: bool,
    /// ai:analyze - AI 分析数据（游客）
    pub guest_perm_ai_analyze: bool,
    /// ai:chat - AI 对话（游客）
    pub guest_perm_ai_chat: bool,
    /// ai:image - AI 图片生成（游客）
    pub guest_perm_ai_image: bool,
    /// report:write - 写入/生成报告（游客）
    pub guest_perm_report_write: bool,
    /// network:fetch - 发起网络请求（游客）
    pub guest_perm_network_fetch: bool,
    /// media:control - 已降为 basic，始终允许；字段保留供 DB/API 兼容（游客）
    pub guest_perm_media_control: bool,
    /// component:theme - 注册主题组件（游客）
    pub guest_perm_component_theme: bool,
    /// shortcut:register - 注册快捷键（游客）
    pub guest_perm_shortcut_register: bool,
    /// event:publish - 发布事件（游客）
    pub guest_perm_event_publish: bool,
    /// scheduler:register - 注册定时任务（游客）
    pub guest_perm_scheduler_register: bool,
    /// speech:tts - 文本转语音（游客）
    pub guest_perm_speech_tts: bool,
    /// speech:asr - 语音转文本（游客）
    pub guest_perm_speech_asr: bool,

    // ===== AI 使用限额配置（当权限已下放时生效） =====
    // 这些限额只对非管理员用户生效，管理员无限制
    /// 普通用户每日 AI 调用次数限制（所有 AI 权限共享）
    pub user_ai_daily_calls: i32,
    /// 普通用户每日 AI Token 限制
    pub user_ai_daily_tokens: i32,
    /// 普通用户 AI 调用冷却时间（秒）
    pub user_ai_cooldown_seconds: i32,

    /// 游客每日 AI 调用次数限制
    pub guest_ai_daily_calls: i32,
    /// 游客每日 AI Token 限制
    pub guest_ai_daily_tokens: i32,
    /// 游客 AI 调用冷却时间（秒）
    pub guest_ai_cooldown_seconds: i32,

    // ===== 网络代理配置（用于中国大陆服务器访问外部API） =====
    /// 是否启用网络代理
    pub proxy_enabled: bool,
    /// HTTP/HTTPS 代理地址（如 http://127.0.0.1:7890 或 socks5://127.0.0.1:1080）
    pub proxy_url: Option<String>,
    /// 不使用代理的域名列表（逗号分隔，如 localhost,127.0.0.1,bilibili.com）
    pub proxy_bypass: Option<String>,
    /// Gemini API Base URL（用于使用第三方代理服务）
    pub gemini_base_url: Option<String>,
    /// GitHub API Base URL（用于使用 GitHub 镜像服务）
    pub github_api_base_url: Option<String>,
}

impl Default for DynamicConfig {
    fn default() -> Self {
        Self {
            // 默认使用 OpenRouter（OpenAI 兼容，provider 记为 openai + OpenRouter base_url）
            ai_provider: "openai".to_string(),
            gemini_api_key: None,
            gemini_model: "gemini-3.5-flash".to_string(),
            openai_api_key: None,
            openai_model: "minimax/minimax-m3".to_string(),
            openai_base_url: "https://openrouter.ai/api/v1".to_string(),
            openai_max_tokens: 2000,
            // Pro 模型默认配置
            pro_enabled: false,
            pro_ai_provider: "openai".to_string(),
            pro_gemini_api_key: None,
            pro_gemini_model: "gemini-3.1-pro-preview".to_string(),
            pro_openai_api_key: None,
            pro_openai_model: "anthropic/claude-opus-4.8".to_string(),
            pro_openai_base_url: "https://openrouter.ai/api/v1".to_string(),
            topic_style: "balanced".to_string(),

            github_enabled: None,
            github_token: None,
            github_username: None,
            bilibili_enabled: None,
            bilibili_uid: None,
            steam_enabled: None,
            steam_api_key: None,
            steam_id: None,
            netease_enabled: None,
            netease_user_id: None,
            bangumi_enabled: None,
            bangumi_username: None,
            bangumi_access_token: None,
            platform_order: None,
            bangumi_user_agent: Some("haru/Myriad".to_string()),
            x_enabled: None,
            x_username: None,
            x_bearer_token: None,
            discord_enabled: None,
            discord_access_token: None,
            discord_refresh_token: None,
            discord_token_expires_at: None,
            discord_user_id: None,
            mal_enabled: None,
            mal_username: None,
            mal_client_id: None,
            xbox_enabled: None,
            xbox_gamertag: None,
            openxbl_api_key: None,
            psn_enabled: None,
            psn_online_id: None,
            psn_npsso: None,

            // Tapp 外部 API 密钥
            openweather_api_key: None,

            // 腾讯云语音服务配置
            tencent_secret_id: None,
            tencent_secret_key: None,
            tencent_region: Some("ap-guangzhou".to_string()),

            ui_wallpaper_url: None,
            ui_wallpaper_blur: 3,
            ui_wallpaper_parallax: true,
            // Evocative 壁纸动效
            ui_evocative_parallax: true,
            ui_evocative_dynamic_blur: false,
            ui_evocative_ripple: false,
            ui_evocative_fps: 30,
            ui_evocative_ripple_quality: 0.85,
            ui_theme: None,
            ui_primary_color: None,
            ui_secondary_color: None,
            pet_enabled: true,
            pet_image_url: None,

            site_title: None,
            site_description: None,
            site_favicon: None,
            site_icp: None,
            site_gongan: None,
            cloud_sponsors: None,

            music_enabled: None,
            music_source: None,
            music_playlist_id: None,

            // AI 图片生成配置
            ai_image_provider: "pollinations".to_string(),
            ai_image_model: "1983308862240288769".to_string(),
            ai_image_width: 768,
            ai_image_height: 1280,
            pixai_api_key: None,
            enable_auto_fetch: false,
            fetch_interval_hours: 24,

            github_client_id: None,
            github_client_secret: None,
            github_redirect_url: String::new(), // 自动从 base_url 生成

            oauth_providers: Vec::new(),
            allow_local_registration: false,

            base_url: None,

            dashboard_layout: None,
            dashboard_title: None,
            custom_platforms: None,
            widget_theme: None,

            title_font: None,
            title_font_size: None,
            title_color: None,

            control_panel_layout: None,
            control_panel_rows: 2,

            tapp_window_schemes: None,

            // ===== 普通用户 elevated 权限默认值（13 项） =====
            // 默认全部关闭，管理员可选择性开放
            user_perm_ai_generate: false,
            user_perm_ai_analyze: false,
            user_perm_ai_chat: false,
            user_perm_ai_image: false,
            user_perm_report_write: false,
            user_perm_network_fetch: false,
            user_perm_media_control: false,
            user_perm_component_theme: false,
            user_perm_shortcut_register: false,
            user_perm_event_publish: false,
            user_perm_scheduler_register: false,
            user_perm_speech_tts: false,
            user_perm_speech_asr: false,

            // ===== 游客 elevated 权限默认值 =====
            // 默认全部关闭
            guest_perm_ai_generate: false,
            guest_perm_ai_analyze: false,
            guest_perm_ai_chat: false,
            guest_perm_ai_image: false,
            guest_perm_report_write: false,
            guest_perm_network_fetch: false,
            guest_perm_media_control: false,
            guest_perm_component_theme: false,
            guest_perm_shortcut_register: false,
            guest_perm_event_publish: false,
            guest_perm_scheduler_register: false,
            guest_perm_speech_tts: false,
            guest_perm_speech_asr: false,

            // ===== AI 使用限额默认值 =====
            // 普通用户: 每日 50 次调用, 20000 tokens, 5 秒冷却
            user_ai_daily_calls: 50,
            user_ai_daily_tokens: 20000,
            user_ai_cooldown_seconds: 5,

            // 游客: 每日 10 次调用, 5000 tokens, 10 秒冷却
            guest_ai_daily_calls: 10,
            guest_ai_daily_tokens: 5000,
            guest_ai_cooldown_seconds: 10,

            // ===== 网络代理配置默认值 =====
            proxy_enabled: false, // 默认关闭代理
            proxy_url: None,
            proxy_bypass: None,
            gemini_base_url: None,     // 默认使用官方 API
            github_api_base_url: None, // 默认使用官方 API
        }
    }
}

impl DynamicConfig {
    /// 根据模型层级解析 AI 配置
    ///
    /// Pro 层级：如果 pro_enabled 且 Pro 有独立 API Key，则使用 Pro 配置；
    /// 否则回退到标准配置。
    pub fn resolve_ai_config(&self, tier: ModelTier) -> ResolvedAiConfig {
        if tier == ModelTier::Pro && self.pro_enabled {
            let provider = &self.pro_ai_provider;
            let (api_key, model, base_url) = if provider == "openai" {
                let key = self
                    .pro_openai_api_key
                    .clone()
                    .filter(|k| !k.is_empty())
                    .or_else(|| self.openai_api_key.clone());
                let model = if self.pro_openai_model.is_empty() {
                    self.openai_model.clone()
                } else {
                    self.pro_openai_model.clone()
                };
                let base_url = if self.pro_openai_base_url.is_empty() {
                    self.openai_base_url.clone()
                } else {
                    self.pro_openai_base_url.clone()
                };
                (key, model, base_url)
            } else {
                // gemini
                let key = self
                    .pro_gemini_api_key
                    .clone()
                    .filter(|k| !k.is_empty())
                    .or_else(|| self.gemini_api_key.clone());
                let model = if self.pro_gemini_model.is_empty() {
                    self.gemini_model.clone()
                } else {
                    self.pro_gemini_model.clone()
                };
                (key, model, String::new())
            };
            return ResolvedAiConfig {
                provider: provider.clone(),
                api_key,
                model,
                base_url,
            };
        }

        // 标准层级
        let provider = &self.ai_provider;
        let (api_key, model, base_url) = if provider == "openai" {
            (
                self.openai_api_key.clone(),
                self.openai_model.clone(),
                self.openai_base_url.clone(),
            )
        } else {
            (
                self.gemini_api_key.clone(),
                self.gemini_model.clone(),
                String::new(),
            )
        };
        ResolvedAiConfig {
            provider: provider.clone(),
            api_key,
            model,
            base_url,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_config_defaults() {
        // Ensure DATABASE_URL is set for from_env(); preserve any existing value.
        let prev = std::env::var("DATABASE_URL").ok();
        std::env::set_var("DATABASE_URL", "postgres://test:test@localhost/test");
        let config = AppConfig::from_env().unwrap();
        assert!(!config.database_url.is_empty());
        assert_eq!(config.server_port, 1103);
        match prev {
            Some(v) => std::env::set_var("DATABASE_URL", v),
            None => std::env::remove_var("DATABASE_URL"),
        }
    }
}
