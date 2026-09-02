use serde::{Deserialize, Serialize};
use std::env;

/// AI 模型层级
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ModelTier {
    /// Lite 模型 - 用于高频、低成本、短输出任务
    Lite,
    /// 标准模型 - 用于日常任务
    #[default]
    Standard,
    /// Pro 模型 - 用于复杂任务
    Pro,
}

/// 单个 OAuth Provider 配置（OIDC / 其他）
///
/// GitHub 也作为 kind="github" 的条目放进这个列表。
///
/// 详见 [`docs/development/OAUTH.md`](../../docs/development/OAUTH.md)。
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OAuthProviderEntry {
    /// 路由 slug：`/api/auth/oauth/<slug>/login`。须全局唯一。
    pub slug: String,
    /// "oidc" 或其他扩展 kind
    pub kind: String,
    /// UI 展示名
    pub display_name: String,
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

/// 一个可复用的 AI 服务商源（同一 kind 可以有多条，用 slug 区分）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AiVendorSource {
    pub slug: String,
    /// openrouter | openai | openai_compatible | gemini | volcengine | tencent
    pub kind: String,
    pub display_name: String,
    pub enabled: bool,
    /// 前端预设 id（openrouter / deepseek / groq …），与 kind 独立。
    #[serde(default)]
    pub preset: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub secret_id: Option<String>,
    #[serde(default)]
    pub secret_key: Option<String>,
    #[serde(default)]
    pub region: Option<String>,
    /// Agora / Shengwang App ID (not the REST customer id).
    #[serde(default)]
    pub app_id: Option<String>,
}

impl AiVendorSource {
    pub fn is_agora(&self) -> bool {
        let slug = self.slug.trim().to_ascii_lowercase();
        self.kind.trim().eq_ignore_ascii_case("agora")
            || self.preset.trim().eq_ignore_ascii_case("agora")
            || slug == "agora"
            || slug.starts_with("agora-")
    }
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
        // Development-oriented scaffolding defaults (tests / first boot).
        // Production loads via `from_env`, which keeps CORS empty when unset so
        // the router panic path fires — never silently inject localhost there.
        Self {
            database_url: String::new(),
            server_host: "127.0.0.1".to_string(),
            server_port: 1103,
            frontend_dist_path: "../frontend/dist".to_string(),
            jwt_secret: String::new(),
            cors_origins: Self::default_cors_origins_for_env(false),
            base_url: None,
            frontend_url: None,
        }
    }
}

impl AppConfig {
    /// Same production gate as router CORS: `ENVIRONMENT=production`.
    pub fn is_production_environment() -> bool {
        env::var("ENVIRONMENT")
            .map(|s| s.trim() == "production")
            .unwrap_or(false)
    }

    /// Default CORS origins when `CORS_ORIGINS` is unset.
    ///
    /// - **Production**: empty (router panics if still empty — fail closed).
    /// - **Development**: localhost SPA/API ports only.
    pub fn default_cors_origins_for_env(is_production: bool) -> Vec<String> {
        if is_production {
            Vec::new()
        } else {
            vec![
                "http://localhost:1102".to_string(),
                "http://localhost:1103".to_string(),
            ]
        }
    }

    /// 从环境变量加载配置
    pub fn from_env() -> anyhow::Result<Self> {
        let jwt_secret = env::var("JWT_SECRET").unwrap_or_else(|_| String::new());

        // Validate JWT secret strength in production
        if !jwt_secret.is_empty() {
            Self::validate_jwt_secret(&jwt_secret)?;
        }

        let production = Self::is_production_environment();
        // Unset → env-aware defaults. Explicit empty string is treated like unset
        // for the purpose of recovering FRONTEND_URL/BASE_URL below.
        let mut cors_origins: Vec<String> = match env::var("CORS_ORIGINS") {
            Ok(s) if !s.trim().is_empty() => s
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
            _ => Self::default_cors_origins_for_env(production),
        };

        // Production with empty CORS (unset / blank): fall back to FRONTEND_URL
        // then BASE_URL so compose can boot without a hard panic when operators
        // only configured the public site URL.
        if production && cors_origins.is_empty() {
            for key in ["FRONTEND_URL", "BASE_URL"] {
                if let Ok(raw) = env::var(key) {
                    let origin = raw.trim().trim_end_matches('/').to_string();
                    if !origin.is_empty() && !cors_origins.iter().any(|o| o == &origin) {
                        cors_origins.push(origin);
                    }
                }
            }
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
            cors_origins,
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
    // AI 配置（Lite 模型）
    pub lite_enabled: bool,
    pub lite_ai_provider: String,
    pub lite_gemini_api_key: Option<String>,
    pub lite_gemini_model: String,
    pub lite_openai_api_key: Option<String>,
    pub lite_openai_model: String,
    pub lite_openai_base_url: String,
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
    /// YouTube Data API v3（公开频道；API key only，无 OAuth）
    pub youtube_enabled: Option<bool>,
    /// Google Cloud YouTube Data API key
    pub youtube_api_key: Option<String>,
    /// 频道身份：UC… channel id、@handle、或 customUrl（不含 OAuth mine）
    pub youtube_channel_id: Option<String>,
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
    pub psn_online_id: Option<String>,
    /// PSN NPSSO cookie（ca.account.sony.com 获取，服务端凭据）
    pub psn_npsso: Option<String>,

    // Tapp 外部 API 密钥（用于 Tapp API 声明系统）
    pub openweather_api_key: Option<String>,

    // 腾讯云语音服务配置（TTS 文本转语音 / ASR 语音转文本）
    pub tencent_secret_id: Option<String>,
    pub tencent_secret_key: Option<String>,
    pub tencent_region: Option<String>, // 默认 ap-guangzhou
    /// 语音服务商：tencent | openai | openrouter | gemini | minimax
    pub speech_provider: String,
    /// 为 true 且专用密钥为空时，沿用 Standard 档文字模型的 OpenAI/OpenRouter 密钥
    pub speech_reuse_text_credentials: bool,
    pub speech_openai_api_key: Option<String>,
    pub speech_openai_base_url: String,
    pub speech_openrouter_api_key: Option<String>,
    pub speech_stt_model: String,
    pub speech_tts_model: String,
    pub speech_tts_voice: String,

    /// 全站共用的服务商凭据（文字 / 图片 / 语音都从这里取）
    pub provider_openai_api_key: Option<String>,
    pub provider_openai_base_url: String,
    pub provider_openrouter_api_key: Option<String>,
    pub provider_gemini_api_key: Option<String>,
    pub provider_volcengine_api_key: Option<String>,
    pub provider_volcengine_base_url: String,
    /// 可添加的服务商源列表（OAuth providers 同款：可多家、可同 kind 多源）
    pub ai_vendor_sources: Vec<AiVendorSource>,
    pub ai_source: String,
    pub lite_ai_source: String,
    pub pro_ai_source: String,
    pub ai_image_source: String,
    pub speech_source: String,

    /// 声网 Conversational AI（实时对话通道）。默认关。
    pub agora_convo_enabled: bool,
    pub agora_app_id: String,
    pub agora_app_certificate: String,
    pub agora_customer_id: String,
    pub agora_customer_secret: Option<String>,
    /// 默认中国区 `https://api.agora.io/cn`
    pub agora_api_base: String,

    // UI 配置
    pub ui_wallpaper_url: Option<String>,
    pub ui_wallpaper_blur: i32,
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

    /// 第一方访客统计（pageview / engagement / event）是否开启；关闭后服务端拒绝采集
    pub analytics_enabled: bool,

    /// PWA：是否启用 Service Worker 注册与可安装清单（默认开启，与历史行为一致）
    pub pwa_enabled: bool,

    // 站点元数据
    pub site_title: Option<String>,
    pub site_description: Option<String>,
    pub site_favicon: Option<String>,
    /// SEO 关键词（逗号分隔，写入 meta keywords）
    pub site_keywords: Option<String>,
    /// 社交分享预览图（Open Graph / Twitter Card）
    pub site_og_image: Option<String>,
    /// 禁止搜索引擎收录（true → robots: noindex, nofollow）
    /// 与 site_visibility_policy 联动：private 时为 true。
    pub site_noindex: bool,
    /// 站点可见性 / GEO 策略：private | search_only | ai_citation | ai_full
    pub site_visibility_policy: String,
    /// 面向 AI 引擎的站点简介（写入 llms.txt；可空则回退 site_description）
    pub site_ai_intro: Option<String>,
    /// Google Analytics 4 Measurement ID（如 G-XXXXXXXXXX）；空则不加载 gtag
    pub ga_measurement_id: Option<String>,
    /// Umami website id（UUID）；空则不加载
    pub umami_website_id: Option<String>,
    /// Umami tracker 脚本完整 URL（如 https://cloud.umami.is/script.js）
    pub umami_script_url: Option<String>,
    pub site_icp: Option<String>,       // ICP 备案号
    pub site_gongan: Option<String>,    // 公安备案号
    pub cloud_sponsors: Option<String>, // 云赞助商（cloudflare,edgeone,upyun 逗号分隔）
    /// 页脚自定义项 JSON 数组，最多 2 条：[{text, icon?, url?}]
    pub site_footer_custom: Option<String>,

    // 音乐配置
    pub music_enabled: Option<String>,
    pub music_source: Option<String>,
    pub music_playlist_id: Option<String>,

    // AI 图片生成配置（统一服务：OpenAI 兼容 / OpenRouter / Volcengine）
    // 分辨率由调用方（agent / tapp）在请求参数中决定，不设全局配置
    pub ai_image_provider: String,
    pub ai_image_model: String,
    pub ai_image_openai_api_key: Option<String>,
    /// OpenAI 兼容图片接口 Base URL（如官方 /v1 或第三方代理）
    pub ai_image_openai_base_url: String,
    pub ai_image_openrouter_api_key: Option<String>,
    pub ai_image_volcengine_api_key: Option<String>,
    pub ai_image_volcengine_base_url: String,

    /// Merope：设定、状态、主动对话、事件开口。默认关。用户界面叫 Agent 人设。
    pub merope_enabled: bool,

    /// 人设形象是否开口朗读聊天回复。默认关；人设未生效时一律关。
    pub merope_speech_enabled: bool,

    /// Agent 人设的页面形象：当前生效的 2.5D 图集包 id（sha256 hex）。
    /// None = 没有编译过的骨骼，浮动层只回退主立绘。站点级——全站一份形象。
    pub agent_rig_asset_id: Option<String>,

    /// Hugging Face token used only by the backend when invoking the remote
    /// See-through ZeroGPU Space. This is a write-only host credential.
    pub see_through_hf_token: Option<String>,

    // 3D 模型生成配置（独立于 AI 图片 Provider）
    pub tripo_enabled: bool,
    pub tripo_api_key: Option<String>,
    pub tripo_base_url: String,
    pub tripo_model: String,
    pub tripo_face_limit: i32,
    pub tripo_poll_interval_seconds: i32,
    pub tripo_task_timeout_seconds: i32,
    pub tripo_max_download_mb: i32,

    // 自动数据获取配置
    pub enable_auto_fetch: bool,
    pub fetch_interval_hours: i32,

    /// OAuth providers（GitHub 为 kind="github"，其余为 OIDC）
    /// 详见 docs/development/OAUTH.md
    pub oauth_providers: Vec<OAuthProviderEntry>,

    /// 是否允许公开本地账号注册（PR #4）
    pub allow_local_registration: bool,

    /// Private (non-admin) Tapp install cleanup:
    /// - `"logout"`: wipe subject's private installs on logout
    /// - `"inactivity"`: prune after `tapp_private_install_inactivity_days` without login/seen
    pub tapp_private_install_cleanup: String,
    /// Days of inactivity before pruning private installs (when mode is inactivity).
    pub tapp_private_install_inactivity_days: i32,

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

    // Tapp 权限下放配置
    // 基于 Tapp 系统的 elevated 级别权限（18 项可配置下放）
    // 这些权限默认只有管理员可用，可以配置下放给普通用户或游客
    // 注意：basic 级别权限默认可授予所有用户
    // 注意：privileged 级别权限始终只限管理员

    // 普通用户可使用的 elevated 权限（18 项）
    /// ai:generate - AI 生成内容
    pub user_perm_ai_generate: bool,
    /// ai:analyze - AI 分析数据
    pub user_perm_ai_analyze: bool,
    /// ai:chat - AI 对话
    pub user_perm_ai_chat: bool,
    /// ai:image - AI 图片生成
    pub user_perm_ai_image: bool,
    /// 3d:generate - Tripo 3D 模型生成
    pub user_perm_3d_generate: bool,
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
    /// storage:write - 写入 Tapp 存储
    pub user_perm_storage_write: bool,
    /// federation:post - 发布联邦内容（publish/unpublish/createNote/uploadMedia/密钥轮换/投递队列）
    pub user_perm_federation_post: bool,
    /// federation:channel - 频道创建与治理
    pub user_perm_federation_channel: bool,
    /// federation:room - 房间创建/加入/治理
    pub user_perm_federation_room: bool,
    /// brew:commentWrite - 写 Brew 评论（需登录主体）
    pub user_perm_brew_comment_write: bool,

    // 游客可使用的 elevated 权限（18 项）
    /// ai:generate - AI 生成内容（游客）
    pub guest_perm_ai_generate: bool,
    /// ai:analyze - AI 分析数据（游客）
    pub guest_perm_ai_analyze: bool,
    /// ai:chat - AI 对话（游客）
    pub guest_perm_ai_chat: bool,
    /// ai:image - AI 图片生成（游客）
    pub guest_perm_ai_image: bool,
    /// 3d:generate - Tripo 3D 模型生成（游客）
    pub guest_perm_3d_generate: bool,
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
    /// storage:write - 写入 Tapp 存储（游客）
    pub guest_perm_storage_write: bool,
    /// federation:post - 发布联邦内容（游客；联邦写路由要求持久登录主体，配置无效）
    pub guest_perm_federation_post: bool,
    /// federation:channel - 频道治理（游客；同上，配置无效）
    pub guest_perm_federation_channel: bool,
    /// federation:room - 房间治理（游客；同上，配置无效）
    pub guest_perm_federation_room: bool,
    /// brew:commentWrite - 写 Brew 评论（游客；路由要求登录主体，实际恒为关闭）
    pub guest_perm_brew_comment_write: bool,

    // AI 使用限额配置（当权限已下放时生效）
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

    // 沙箱生命周期配额
    /// 暂存池允许保留的隐藏实例数
    pub stash_hidden_capacity: i32,
    /// 隐藏实例空闲淘汰时间（秒）
    pub stash_hidden_idle_seconds: i32,
    /// 每个应用可占用的常驻名额
    pub resident_quota_per_app: i32,
    /// 全站常驻名额上限
    pub resident_quota_site_total: i32,

    /// 内存节约模式（高级设置）：在当前有界均衡档上再收一档，适合 ~1 GiB 主机。
    /// 默认 false = 均衡档，不是旧版无界高水位。`MYRIAD_MEMORY_PROFILE` env 可覆盖。
    pub memory_saver_enabled: bool,

    // 网络代理配置（用于中国大陆服务器访问外部API）
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
            gemini_model: "gemini-3.6-flash".to_string(),
            openai_api_key: None,
            openai_model: "minimax/minimax-m3".to_string(),
            openai_base_url: "https://openrouter.ai/api/v1".to_string(),
            openai_max_tokens: 2000,
            // Lite 模型默认配置（关闭时 Lite 任务回退 Standard，与 Pro 同协议）
            lite_enabled: false,
            lite_ai_provider: "openai".to_string(),
            lite_gemini_api_key: None,
            lite_gemini_model: "gemini-3.5-flash-lite".to_string(),
            lite_openai_api_key: None,
            lite_openai_model: "openai/gpt-oss-20b:free".to_string(),
            lite_openai_base_url: "https://openrouter.ai/api/v1".to_string(),
            // Pro 模型默认配置
            pro_enabled: false,
            pro_ai_provider: "openai".to_string(),
            pro_gemini_api_key: None,
            pro_gemini_model: "gemini-3.1-pro-preview".to_string(),
            pro_openai_api_key: None,
            pro_openai_model: "anthropic/claude-opus-5".to_string(),
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
            youtube_enabled: None,
            youtube_api_key: None,
            youtube_channel_id: None,
            netease_enabled: None,
            netease_user_id: None,
            bangumi_enabled: None,
            bangumi_username: None,
            bangumi_access_token: None,
            platform_order: None,
            bangumi_user_agent: Some("myriad/Myriad".to_string()),
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
            speech_provider: "tencent".to_string(),
            speech_reuse_text_credentials: true,
            speech_openai_api_key: None,
            speech_openai_base_url: "https://api.openai.com/v1".to_string(),
            speech_openrouter_api_key: None,
            speech_stt_model: String::new(),
            speech_tts_model: String::new(),
            speech_tts_voice: String::new(),
            provider_openai_api_key: None,
            provider_openai_base_url: "https://api.openai.com/v1".to_string(),
            provider_openrouter_api_key: None,
            provider_gemini_api_key: None,
            provider_volcengine_api_key: None,
            provider_volcengine_base_url: "https://ark.cn-beijing.volces.com/api/v3".to_string(),
            ai_vendor_sources: Vec::new(),
            ai_source: String::new(),
            lite_ai_source: String::new(),
            pro_ai_source: String::new(),
            ai_image_source: String::new(),
            speech_source: String::new(),
            agora_convo_enabled: false,
            agora_app_id: String::new(),
            agora_app_certificate: String::new(),
            agora_customer_id: String::new(),
            agora_customer_secret: None,
            agora_api_base: String::new(),

            ui_wallpaper_url: None,
            ui_wallpaper_blur: 3,
            // Evocative 壁纸动效
            ui_evocative_parallax: true,
            ui_evocative_dynamic_blur: false,
            ui_evocative_ripple: false,
            ui_evocative_fps: 30,
            ui_evocative_ripple_quality: 0.85,
            ui_theme: None,
            ui_primary_color: None,
            ui_secondary_color: None,

            analytics_enabled: true,

            pwa_enabled: true,

            site_title: None,
            site_description: None,
            site_favicon: None,
            site_keywords: None,
            site_og_image: None,
            site_noindex: false,
            site_visibility_policy: String::new(),
            site_ai_intro: None,
            ga_measurement_id: None,
            umami_website_id: None,
            umami_script_url: None,
            site_icp: None,
            site_gongan: None,
            cloud_sponsors: None,
            site_footer_custom: None,

            music_enabled: None,
            music_source: None,
            music_playlist_id: None,

            // AI 图片生成配置
            ai_image_provider: "openrouter".to_string(),
            ai_image_model: "openai/gpt-image-2".to_string(),
            ai_image_openai_api_key: None,
            ai_image_openai_base_url: "https://api.openai.com/v1".to_string(),
            ai_image_openrouter_api_key: None,
            ai_image_volcengine_api_key: None,
            ai_image_volcengine_base_url: "https://ark.cn-beijing.volces.com/api/v3".to_string(),
            merope_enabled: false,
            merope_speech_enabled: false,
            agent_rig_asset_id: None,
            see_through_hf_token: None,
            // Tripo 3D（低模 Web 角色默认预算）
            tripo_enabled: false,
            tripo_api_key: None,
            tripo_base_url: "https://openapi.tripo3d.ai/v3".to_string(),
            tripo_model: "P1-20260311".to_string(),
            tripo_face_limit: 5_000,
            tripo_poll_interval_seconds: 2,
            tripo_task_timeout_seconds: 900,
            tripo_max_download_mb: 64,
            enable_auto_fetch: false,
            fetch_interval_hours: 24,

            oauth_providers: Vec::new(),
            allow_local_registration: false,
            // Default: keep private installs until 14 days inactive (not wipe on logout)
            tapp_private_install_cleanup: "inactivity".to_string(),
            tapp_private_install_inactivity_days: 14,

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

            // 普通用户 elevated 权限默认值
            // 默认全部关闭，管理员可选择性开放
            user_perm_ai_generate: false,
            user_perm_ai_analyze: false,
            user_perm_ai_chat: false,
            user_perm_ai_image: false,
            user_perm_3d_generate: false,
            user_perm_report_write: false,
            user_perm_network_fetch: false,
            user_perm_media_control: false,
            user_perm_component_theme: false,
            user_perm_shortcut_register: false,
            user_perm_event_publish: false,
            user_perm_scheduler_register: false,
            user_perm_speech_tts: false,
            user_perm_speech_asr: false,
            user_perm_storage_write: false,
            user_perm_federation_post: false,
            user_perm_federation_channel: false,
            user_perm_federation_room: false,
            user_perm_brew_comment_write: false,

            // 游客 elevated 权限默认值
            // 默认全部关闭
            guest_perm_ai_generate: false,
            guest_perm_ai_analyze: false,
            guest_perm_ai_chat: false,
            guest_perm_ai_image: false,
            guest_perm_3d_generate: false,
            guest_perm_report_write: false,
            guest_perm_network_fetch: false,
            guest_perm_media_control: false,
            guest_perm_component_theme: false,
            guest_perm_shortcut_register: false,
            guest_perm_event_publish: false,
            guest_perm_scheduler_register: false,
            guest_perm_speech_tts: false,
            guest_perm_speech_asr: false,
            guest_perm_storage_write: false,
            guest_perm_federation_post: false,
            guest_perm_federation_channel: false,
            guest_perm_federation_room: false,
            guest_perm_brew_comment_write: false,

            // AI 使用限额默认值
            // 普通用户: 每日 50 次调用, 20000 tokens, 5 秒冷却
            user_ai_daily_calls: 50,
            user_ai_daily_tokens: 20000,
            user_ai_cooldown_seconds: 5,

            // 游客: 每日 10 次调用, 5000 tokens, 10 秒冷却
            guest_ai_daily_calls: 10,
            guest_ai_daily_tokens: 5000,
            guest_ai_cooldown_seconds: 10,

            // 沙箱生命周期配额默认值
            stash_hidden_capacity: 8,
            stash_hidden_idle_seconds: 300,
            resident_quota_per_app: 1,
            resident_quota_site_total: 3,

            // 网络代理配置默认值
            memory_saver_enabled: false,
            proxy_enabled: false, // 默认关闭代理
            proxy_url: None,
            proxy_bypass: None,
            gemini_base_url: None,     // 默认使用官方 API
            github_api_base_url: None, // 默认使用官方 API
        }
    }
}

impl DynamicConfig {
    /// 开关本身：环境变量 `MEROPE_ENABLED` 覆盖库里的 `merope_enabled`。
    pub fn merope_switch_on(&self) -> bool {
        match std::env::var("MEROPE_ENABLED") {
            Ok(value) => matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            ),
            Err(_) => self.merope_enabled,
        }
    }

    /// Merope 是否真的生效。
    ///
    /// 设定引导走 Pro；聊天里的人设只是系统词，跟 Lite 无关。
    /// Lite 只写主动开口和心情微调——档关着时这两处直接停，不回落到标准模型。
    pub fn merope_enabled_resolved(&self) -> bool {
        self.merope_switch_on() && self.pro_enabled
    }

    /// 开关开着却缺 Lite。主动开口会走短句兜底，心情微调不会跑。
    pub fn merope_needs_lite(&self) -> bool {
        self.merope_switch_on() && self.pro_enabled && !self.lite_enabled
    }

    /// 开关开着却缺 Pro。设定引导和开关生效都要这一档。
    pub fn merope_needs_pro(&self) -> bool {
        self.merope_switch_on() && !self.pro_enabled
    }

    /// 人设开口朗读。人设未生效时一律关。
    pub fn merope_speech_enabled_resolved(&self) -> bool {
        self.merope_enabled_resolved() && self.merope_speech_enabled
    }

    pub fn is_openrouter_base(url: &str) -> bool {
        url.to_ascii_lowercase().contains("openrouter.ai")
    }

    fn first_nonempty_key(candidates: impl IntoIterator<Item = Option<String>>) -> Option<String> {
        candidates.into_iter().find_map(|value| {
            value
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        })
    }

    fn first_nonempty_url<'a>(candidates: impl IntoIterator<Item = &'a str>) -> String {
        candidates
            .into_iter()
            .map(str::trim)
            .find(|s| !s.is_empty())
            .unwrap_or("")
            .to_string()
    }

    pub fn shared_openrouter_api_key(&self) -> Option<String> {
        Self::first_nonempty_key([
            self.provider_openrouter_api_key.clone(),
            self.speech_openrouter_api_key.clone(),
            self.ai_image_openrouter_api_key.clone(),
            Self::is_openrouter_base(&self.openai_base_url)
                .then(|| self.openai_api_key.clone())
                .flatten(),
            Self::is_openrouter_base(&self.lite_openai_base_url)
                .then(|| self.lite_openai_api_key.clone())
                .flatten(),
            Self::is_openrouter_base(&self.pro_openai_base_url)
                .then(|| self.pro_openai_api_key.clone())
                .flatten(),
        ])
    }

    pub fn shared_openai_api_key(&self) -> Option<String> {
        Self::first_nonempty_key([
            self.provider_openai_api_key.clone(),
            self.speech_openai_api_key.clone(),
            self.ai_image_openai_api_key.clone(),
            (!Self::is_openrouter_base(&self.openai_base_url))
                .then(|| self.openai_api_key.clone())
                .flatten(),
            (!Self::is_openrouter_base(&self.lite_openai_base_url))
                .then(|| self.lite_openai_api_key.clone())
                .flatten(),
            (!Self::is_openrouter_base(&self.pro_openai_base_url))
                .then(|| self.pro_openai_api_key.clone())
                .flatten(),
        ])
    }

    pub fn shared_openai_base_url(&self) -> String {
        let from_shared = self.provider_openai_base_url.trim();
        if !from_shared.is_empty() && !Self::is_openrouter_base(from_shared) {
            return from_shared.to_string();
        }
        for url in [
            self.speech_openai_base_url.as_str(),
            self.ai_image_openai_base_url.as_str(),
            self.openai_base_url.as_str(),
        ] {
            if !url.trim().is_empty() && !Self::is_openrouter_base(url) {
                return url.trim().to_string();
            }
        }
        "https://api.openai.com/v1".to_string()
    }

    pub fn shared_gemini_api_key(&self) -> Option<String> {
        Self::first_nonempty_key([
            self.provider_gemini_api_key.clone(),
            self.gemini_api_key.clone(),
            self.lite_gemini_api_key.clone(),
            self.pro_gemini_api_key.clone(),
        ])
    }

    /// Standard 档解析后是否有可用文本 key（含 vendor / 共享库）。
    pub fn text_ai_available(&self) -> bool {
        self.resolve_ai_config(ModelTier::Standard)
            .api_key
            .as_ref()
            .is_some_and(|key| !key.trim().is_empty())
    }

    /// Google Search grounding 只能走 Gemini。
    ///
    /// key 用共享 Gemini 库；模型优先当前 Standard 若本身就是 Gemini，
    /// 否则借各档 `gemini_model`，再落到 grounding 默认型号。
    pub fn resolve_gemini_grounding(&self) -> Option<(String, String)> {
        let api_key = self.shared_gemini_api_key()?;
        let resolved = self.resolve_ai_config(ModelTier::Standard);
        let model = if resolved.provider == "gemini" && !resolved.model.trim().is_empty() {
            resolved.model
        } else {
            let borrowed = Self::first_nonempty_url([
                self.gemini_model.as_str(),
                self.lite_gemini_model.as_str(),
                self.pro_gemini_model.as_str(),
            ]);
            if borrowed.is_empty() {
                "gemini-3.6-flash".to_string()
            } else {
                borrowed
            }
        };
        Some((api_key, model))
    }

    pub fn shared_volcengine_api_key(&self) -> Option<String> {
        Self::first_nonempty_key([
            self.provider_volcengine_api_key.clone(),
            self.ai_image_volcengine_api_key.clone(),
        ])
    }

    pub fn shared_volcengine_base_url(&self) -> String {
        let url = Self::first_nonempty_url([
            self.provider_volcengine_base_url.as_str(),
            self.ai_image_volcengine_base_url.as_str(),
        ]);
        if url.is_empty() {
            "https://ark.cn-beijing.volces.com/api/v3".to_string()
        } else {
            url
        }
    }

    pub fn vendor_kind_supports(kind: &str, capability: &str) -> bool {
        match (kind, capability) {
            ("openrouter" | "openai" | "openai_compatible", "text" | "image" | "speech") => true,
            ("gemini", "text" | "image" | "speech") => true,
            ("volcengine", "text" | "image") => true,
            ("tencent", "speech") => true,
            ("agora", "realtime") => true,
            _ => false,
        }
    }

    pub fn nonempty_opt(value: Option<&String>) -> Option<String> {
        value
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn synthesize_vendor_sources(&self) -> Vec<AiVendorSource> {
        let mut sources = Vec::new();
        if let Some(api_key) = self.shared_openrouter_api_key() {
            sources.push(AiVendorSource {
                slug: "openrouter".to_string(),
                kind: "openrouter".to_string(),
                display_name: "OpenRouter".to_string(),
                enabled: true,
                preset: "openrouter".to_string(),
                api_key: Some(api_key),
                base_url: "https://openrouter.ai/api/v1".to_string(),
                ..AiVendorSource::default()
            });
        }
        if let Some(api_key) = self.shared_openai_api_key() {
            sources.push(AiVendorSource {
                slug: "openai".to_string(),
                kind: "openai".to_string(),
                display_name: "OpenAI".to_string(),
                enabled: true,
                preset: "openai".to_string(),
                api_key: Some(api_key),
                base_url: self.shared_openai_base_url(),
                ..AiVendorSource::default()
            });
        }
        if let Some(api_key) = self.shared_gemini_api_key() {
            sources.push(AiVendorSource {
                slug: "gemini".to_string(),
                kind: "gemini".to_string(),
                display_name: "Gemini".to_string(),
                enabled: true,
                preset: "gemini".to_string(),
                api_key: Some(api_key),
                ..AiVendorSource::default()
            });
        }
        if let Some(api_key) = self.shared_volcengine_api_key() {
            sources.push(AiVendorSource {
                slug: "volcengine".to_string(),
                kind: "volcengine".to_string(),
                display_name: "Volcengine".to_string(),
                enabled: true,
                preset: "volcengine".to_string(),
                api_key: Some(api_key),
                base_url: self.shared_volcengine_base_url(),
                ..AiVendorSource::default()
            });
        }
        let tencent_id = Self::nonempty_opt(self.tencent_secret_id.as_ref());
        let tencent_key = Self::nonempty_opt(self.tencent_secret_key.as_ref());
        if tencent_id.is_some() || tencent_key.is_some() {
            sources.push(AiVendorSource {
                slug: "tencent".to_string(),
                kind: "tencent".to_string(),
                display_name: "Tencent Cloud".to_string(),
                enabled: true,
                preset: "tencent".to_string(),
                secret_id: tencent_id,
                secret_key: tencent_key,
                region: self.tencent_region.clone(),
                ..AiVendorSource::default()
            });
        }
        sources
    }

    fn synthesize_agora_source(&self) -> Option<AiVendorSource> {
        let app_id = Self::nonempty_opt(Some(&self.agora_app_id));
        let certificate = Self::nonempty_opt(Some(&self.agora_app_certificate));
        let customer_id = Self::nonempty_opt(Some(&self.agora_customer_id));
        let customer_secret = Self::nonempty_opt(self.agora_customer_secret.as_ref());
        if app_id.is_none()
            && certificate.is_none()
            && customer_id.is_none()
            && customer_secret.is_none()
        {
            return None;
        }
        Some(AiVendorSource {
            slug: "agora".to_string(),
            kind: "agora".to_string(),
            display_name: "Shengwang / Agora".to_string(),
            enabled: self.agora_convo_enabled,
            preset: "agora".to_string(),
            api_key: certificate,
            secret_id: customer_id,
            secret_key: customer_secret,
            app_id,
            base_url: if self.agora_api_base.trim().is_empty() {
                "https://api.agora.io/cn".to_string()
            } else {
                self.agora_api_base.trim().to_string()
            },
            ..AiVendorSource::default()
        })
    }

    pub fn effective_vendor_sources(&self) -> Vec<AiVendorSource> {
        let mut sources = if self.ai_vendor_sources.is_empty() {
            self.synthesize_vendor_sources()
        } else {
            self.ai_vendor_sources.clone()
        };
        if !sources.iter().any(AiVendorSource::is_agora) {
            if let Some(agora) = self.synthesize_agora_source() {
                sources.push(agora);
            }
        }
        sources
    }

    pub fn find_vendor_source(&self, slug: &str) -> Option<AiVendorSource> {
        let slug = slug.trim();
        if slug.is_empty() {
            return None;
        }
        self.effective_vendor_sources()
            .into_iter()
            .find(|source| source.slug == slug)
    }

    fn inferred_text_source_slug(&self, provider: &str, base_url: &str) -> String {
        if provider == "gemini" {
            "gemini".to_string()
        } else if Self::is_openrouter_base(base_url) {
            "openrouter".to_string()
        } else {
            "openai".to_string()
        }
    }

    pub fn resolve_from_vendor_source(
        &self,
        source: &AiVendorSource,
        model: &str,
    ) -> ResolvedAiConfig {
        match source.kind.as_str() {
            "gemini" => ResolvedAiConfig {
                provider: "gemini".to_string(),
                api_key: Self::nonempty_opt(source.api_key.as_ref())
                    .or_else(|| self.shared_gemini_api_key()),
                model: model.to_string(),
                base_url: String::new(),
            },
            "openrouter" => ResolvedAiConfig {
                provider: "openai".to_string(),
                api_key: Self::nonempty_opt(source.api_key.as_ref())
                    .or_else(|| self.shared_openrouter_api_key()),
                model: model.to_string(),
                base_url: if source.base_url.trim().is_empty() {
                    "https://openrouter.ai/api/v1".to_string()
                } else {
                    source.base_url.trim().to_string()
                },
            },
            _ => ResolvedAiConfig {
                provider: "openai".to_string(),
                api_key: Self::nonempty_opt(source.api_key.as_ref())
                    .or_else(|| self.shared_openai_api_key()),
                model: model.to_string(),
                base_url: if source.base_url.trim().is_empty() {
                    self.shared_openai_base_url()
                } else {
                    source.base_url.trim().to_string()
                },
            },
        }
    }

    fn resolve_openai_compatible(&self, model: &str, selected_base: &str) -> ResolvedAiConfig {
        if Self::is_openrouter_base(selected_base) {
            ResolvedAiConfig {
                provider: "openai".to_string(),
                api_key: self.shared_openrouter_api_key(),
                model: model.to_string(),
                base_url: "https://openrouter.ai/api/v1".to_string(),
            }
        } else {
            ResolvedAiConfig {
                provider: "openai".to_string(),
                api_key: self.shared_openai_api_key(),
                model: model.to_string(),
                base_url: self.shared_openai_base_url(),
            }
        }
    }

    /// Resolve Lite only when its own model is explicit. Shared provider
    /// credentials remain valid, but Standard's model is never inherited.
    pub fn resolve_strict_lite_ai_config(&self) -> Option<ResolvedAiConfig> {
        if !self.lite_enabled {
            return None;
        }
        let configured_model = if self.lite_ai_provider == "gemini" {
            self.lite_gemini_model.trim()
        } else {
            self.lite_openai_model.trim()
        };
        if configured_model.is_empty() {
            return None;
        }
        Some(self.resolve_ai_config(ModelTier::Lite))
    }

    /// 这一档要的模型没配、实际会落到 Standard 上吗？
    ///
    /// 开关打开但模型字段留空时，`resolve_tier` 会取 Standard 的模型。配置上
    /// 看是「Lite 已启用」，跑的却是 Standard——账单和延迟都按 Standard 走，
    /// 而日志里记的是 Lite。这个函数只负责认出这件事，喊出来是调用方的事。
    ///
    /// 关掉的档不算回退：那是明确的选择，不是没说清楚。
    pub fn tier_falls_back_to_standard(&self, tier: ModelTier) -> bool {
        let (enabled, provider, gemini_model, openai_model) = match tier {
            ModelTier::Lite => (
                self.lite_enabled,
                &self.lite_ai_provider,
                &self.lite_gemini_model,
                &self.lite_openai_model,
            ),
            ModelTier::Pro => (
                self.pro_enabled,
                &self.pro_ai_provider,
                &self.pro_gemini_model,
                &self.pro_openai_model,
            ),
            ModelTier::Standard => return false,
        };
        if !enabled {
            return false;
        }
        let configured = if provider == "gemini" {
            gemini_model.trim()
        } else {
            openai_model.trim()
        };
        configured.is_empty()
    }

    /// 根据模型层级解析 AI 配置。与严格 Lite 解析不同，这里保留平台级的
    /// 兼容行为：Lite / Pro 关闭或对应模型留空时可回退到 Standard。
    ///
    /// 回退是静默的，所以调用方应当先问一次 [`Self::tier_falls_back_to_standard`]
    /// 并把它记下来——否则「我明明开了 Lite」会变成一个查不出来的问题。
    pub fn resolve_ai_config(&self, tier: ModelTier) -> ResolvedAiConfig {
        if tier == ModelTier::Lite && self.lite_enabled {
            return self.resolve_tier(
                &self.lite_ai_source,
                &self.lite_ai_provider,
                &self.lite_gemini_model,
                &self.lite_openai_model,
                &self.lite_openai_base_url,
            );
        }
        if tier == ModelTier::Pro && self.pro_enabled {
            return self.resolve_tier(
                &self.pro_ai_source,
                &self.pro_ai_provider,
                &self.pro_gemini_model,
                &self.pro_openai_model,
                &self.pro_openai_base_url,
            );
        }

        self.resolve_tier(
            &self.ai_source,
            &self.ai_provider,
            &self.gemini_model,
            &self.openai_model,
            &self.openai_base_url,
        )
    }

    fn resolve_tier(
        &self,
        source_slug: &str,
        provider: &str,
        gemini_model: &str,
        openai_model: &str,
        openai_base_url: &str,
    ) -> ResolvedAiConfig {
        let base = if openai_base_url.trim().is_empty() {
            self.openai_base_url.as_str()
        } else {
            openai_base_url
        };
        let slug = if source_slug.trim().is_empty() {
            self.inferred_text_source_slug(provider, base)
        } else {
            source_slug.trim().to_string()
        };
        let model = if provider == "gemini" {
            if gemini_model.trim().is_empty() {
                self.gemini_model.clone()
            } else {
                gemini_model.to_string()
            }
        } else if openai_model.trim().is_empty() {
            self.openai_model.clone()
        } else {
            openai_model.to_string()
        };
        if let Some(source) = self.find_vendor_source(&slug) {
            return self.resolve_from_vendor_source(&source, &model);
        }
        if provider == "openai" {
            self.resolve_openai_compatible(&model, base)
        } else {
            ResolvedAiConfig {
                provider: "gemini".to_string(),
                api_key: self.shared_gemini_api_key(),
                model,
                base_url: String::new(),
            }
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

    /// 「我明明开了 Lite」得能被认出来。
    #[test]
    fn an_enabled_tier_with_no_model_of_its_own_is_a_fallback() {
        let mut config = DynamicConfig::default();
        config.openai_model = "standard-model".into();
        // 出厂默认给 Lite / Pro 各配了自己的模型，回退只在字段被清空后发生。
        config.lite_openai_model = String::new();
        config.pro_openai_model = String::new();

        // 关着不算回退：那是明确的选择。
        assert!(!config.tier_falls_back_to_standard(ModelTier::Lite));
        assert!(!config.tier_falls_back_to_standard(ModelTier::Pro));

        // 开着但模型留空 —— 配置上写着 Lite，跑的是 Standard。
        config.lite_enabled = true;
        assert!(config.tier_falls_back_to_standard(ModelTier::Lite));
        assert_eq!(
            config.resolve_ai_config(ModelTier::Lite).model,
            "standard-model",
            "这正是那句警告要说的事实"
        );

        // 填上自己的模型就不再是回退。
        config.lite_openai_model = "lite-model".into();
        assert!(!config.tier_falls_back_to_standard(ModelTier::Lite));
        assert_eq!(
            config.resolve_ai_config(ModelTier::Lite).model,
            "lite-model"
        );

        // Pro 同一套形状。
        config.pro_enabled = true;
        assert!(config.tier_falls_back_to_standard(ModelTier::Pro));
        config.pro_openai_model = "pro-model".into();
        assert!(!config.tier_falls_back_to_standard(ModelTier::Pro));

        // Standard 是回退的目的地，它自己不会回退。
        assert!(!config.tier_falls_back_to_standard(ModelTier::Standard));
    }

    /// 严格 Lite 拒绝的，正好就是会静默回退的那一档。
    #[test]
    fn strict_lite_refuses_exactly_what_would_have_fallen_back() {
        let mut config = DynamicConfig::default();
        config.lite_enabled = true;
        config.lite_openai_model = String::new();
        assert!(config.tier_falls_back_to_standard(ModelTier::Lite));
        assert!(config.resolve_strict_lite_ai_config().is_none());

        config.lite_openai_model = "lite-model".into();
        assert!(!config.tier_falls_back_to_standard(ModelTier::Lite));
        assert!(config.resolve_strict_lite_ai_config().is_some());
    }

    #[test]
    fn default_cors_origins_empty_in_production_localhost_in_dev() {
        assert!(AppConfig::default_cors_origins_for_env(true).is_empty());
        let dev = AppConfig::default_cors_origins_for_env(false);
        assert!(dev.iter().any(|o| o.contains("localhost:1102")));
        assert!(dev.iter().any(|o| o.contains("localhost:1103")));
    }

    #[test]
    fn app_config_default_still_has_dev_cors() {
        // Default is development-oriented (tests / local scaffolding).
        let cfg = AppConfig::default();
        assert!(!cfg.cors_origins.is_empty());
        assert!(cfg.cors_origins.iter().all(|o| o.contains("localhost")));
    }

    #[test]
    fn shared_provider_keys_win_over_legacy_fields() {
        let config = DynamicConfig {
            provider_openrouter_api_key: Some("vault-or".to_string()),
            openai_api_key: Some("legacy-or".to_string()),
            openai_base_url: "https://openrouter.ai/api/v1".to_string(),
            provider_openai_api_key: Some("vault-oa".to_string()),
            ai_image_openai_api_key: Some("image-oa".to_string()),
            ..DynamicConfig::default()
        };
        assert_eq!(
            config.shared_openrouter_api_key().as_deref(),
            Some("vault-or")
        );
        assert_eq!(config.shared_openai_api_key().as_deref(), Some("vault-oa"));
        let resolved = config.resolve_ai_config(ModelTier::Standard);
        assert_eq!(resolved.api_key.as_deref(), Some("vault-or"));
        assert!(resolved.base_url.contains("openrouter.ai"));
    }

    #[test]
    fn resolves_lite_with_the_same_provider_contract_as_pro() {
        let config = DynamicConfig {
            lite_enabled: true,
            lite_ai_provider: "openai".to_string(),
            lite_openai_api_key: Some("lite-key".to_string()),
            lite_openai_model: "cheap/model".to_string(),
            ..DynamicConfig::default()
        };
        let resolved = config.resolve_ai_config(ModelTier::Lite);
        assert_eq!(resolved.provider, "openai");
        assert_eq!(resolved.api_key.as_deref(), Some("lite-key"));
        assert_eq!(resolved.model, "cheap/model");
        assert!(resolved.base_url.contains("openrouter.ai"));
    }

    #[test]
    fn lite_falls_back_to_standard_when_disabled() {
        let config = DynamicConfig {
            lite_enabled: false,
            lite_openai_api_key: Some("lite-key".to_string()),
            lite_openai_model: "cheap/model".to_string(),
            openai_api_key: Some("std-key".to_string()),
            openai_model: "std/model".to_string(),
            ..DynamicConfig::default()
        };
        let resolved = config.resolve_ai_config(ModelTier::Lite);
        assert_eq!(resolved.api_key.as_deref(), Some("std-key"));
        assert_eq!(resolved.model, "std/model");
    }

    #[test]
    fn lite_reuses_standard_credentials_when_enabled_and_empty() {
        let config = DynamicConfig {
            lite_enabled: true,
            lite_ai_provider: "openai".to_string(),
            lite_openai_api_key: None,
            lite_openai_model: String::new(),
            lite_openai_base_url: String::new(),
            openai_api_key: Some("std-key".to_string()),
            openai_model: "std/model".to_string(),
            openai_base_url: "https://api.openai.com/v1".to_string(),
            ..DynamicConfig::default()
        };
        let resolved = config.resolve_ai_config(ModelTier::Lite);
        assert_eq!(resolved.api_key.as_deref(), Some("std-key"));
        assert_eq!(resolved.model, "std/model");
        assert_eq!(resolved.base_url, "https://api.openai.com/v1");
    }

    #[test]
    fn strict_lite_resolution_never_inherits_the_standard_model() {
        let disabled = DynamicConfig {
            lite_enabled: false,
            openai_model: "std/model".to_string(),
            ..DynamicConfig::default()
        };
        assert!(disabled.resolve_strict_lite_ai_config().is_none());

        let empty = DynamicConfig {
            lite_enabled: true,
            lite_ai_provider: "openai".to_string(),
            lite_openai_model: String::new(),
            openai_model: "std/model".to_string(),
            ..DynamicConfig::default()
        };
        assert!(empty.resolve_strict_lite_ai_config().is_none());

        let configured = DynamicConfig {
            lite_enabled: true,
            lite_ai_provider: "openai".to_string(),
            lite_openai_model: "lite/model".to_string(),
            ..DynamicConfig::default()
        };
        assert_eq!(
            configured
                .resolve_strict_lite_ai_config()
                .map(|resolved| resolved.model),
            Some("lite/model".to_string())
        );
    }

    #[test]
    fn text_ai_available_follows_resolved_standard_not_raw_fields() {
        let missing = DynamicConfig::default();
        assert!(!missing.text_ai_available());

        let vault_only = DynamicConfig {
            ai_provider: "openai".to_string(),
            openai_base_url: "https://api.openai.com/v1".to_string(),
            provider_openai_api_key: Some("vault-oa".to_string()),
            ..DynamicConfig::default()
        };
        assert!(vault_only.text_ai_available());
    }

    #[test]
    fn gemini_grounding_uses_shared_key_even_when_text_tier_is_openai() {
        let config = DynamicConfig {
            ai_provider: "openai".to_string(),
            openai_base_url: "https://api.openai.com/v1".to_string(),
            gemini_api_key: None,
            provider_gemini_api_key: Some("vault-gm".to_string()),
            gemini_model: String::new(),
            lite_gemini_model: "gemini-lite".to_string(),
            ..DynamicConfig::default()
        };
        let (key, model) = config.resolve_gemini_grounding().expect("shared gemini");
        assert_eq!(key, "vault-gm");
        assert_eq!(model, "gemini-lite");
        assert!(!config.text_ai_available());
    }
}
