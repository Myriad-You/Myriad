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
/// GitHub 仍走 `github_client_id` / `github_client_secret` 平铺字段（内置 provider）。
/// 这里专门给 OIDC / 未来其他 provider 用。
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

    // OAuth 配置（GitHub 是内置 provider，仍用平铺字段；其他 provider 走 oauth_providers）
    pub github_client_id: Option<String>,
    pub github_client_secret: Option<String>,
    pub github_redirect_url: String,

    /// 通用 OIDC providers 列表（PR #3）
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
    // 基于 Tapp 系统的 elevated 级别权限（17 项可配置下放）
    // 这些权限默认只有管理员可用，可以配置下放给普通用户或游客
    // 注意：basic 级别权限默认可授予所有用户
    // 注意：privileged 级别权限始终只限管理员

    // 普通用户可使用的 elevated 权限（17 项）
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
    /// storage:write - 写入 Tapp 存储
    pub user_perm_storage_write: bool,
    /// federation:post - 发布联邦内容（publish/unpublish/createNote/uploadMedia/密钥轮换/投递队列）
    pub user_perm_federation_post: bool,
    /// federation:channel - 频道创建与治理
    pub user_perm_federation_channel: bool,
    /// federation:room - 房间创建/加入/治理
    pub user_perm_federation_room: bool,

    // 游客可使用的 elevated 权限（17 项）
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
    /// storage:write - 写入 Tapp 存储（游客）
    pub guest_perm_storage_write: bool,
    /// federation:post - 发布联邦内容（游客；联邦写路由要求持久登录主体，配置无效）
    pub guest_perm_federation_post: bool,
    /// federation:channel - 频道治理（游客；同上，配置无效）
    pub guest_perm_federation_channel: bool,
    /// federation:room - 房间治理（游客；同上，配置无效）
    pub guest_perm_federation_room: bool,

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

            github_client_id: None,
            github_client_secret: None,
            github_redirect_url: String::new(), // 自动从 base_url 生成

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

            // 普通用户 elevated 权限默认值（13 项）
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
            user_perm_storage_write: false,
            user_perm_federation_post: false,
            user_perm_federation_channel: false,
            user_perm_federation_room: false,

            // 游客 elevated 权限默认值
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
            guest_perm_storage_write: false,
            guest_perm_federation_post: false,
            guest_perm_federation_channel: false,
            guest_perm_federation_room: false,

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
    /// 根据模型层级解析 AI 配置
    ///
    /// Lite / Pro 仅在对应开关开启时使用独立配置；关闭或字段留空时回退到 Standard。
    pub fn resolve_ai_config(&self, tier: ModelTier) -> ResolvedAiConfig {
        if tier == ModelTier::Lite && self.lite_enabled {
            return self.resolve_secondary_tier(
                &self.lite_ai_provider,
                self.lite_gemini_api_key.clone(),
                &self.lite_gemini_model,
                self.lite_openai_api_key.clone(),
                &self.lite_openai_model,
                &self.lite_openai_base_url,
            );
        }
        if tier == ModelTier::Pro && self.pro_enabled {
            return self.resolve_secondary_tier(
                &self.pro_ai_provider,
                self.pro_gemini_api_key.clone(),
                &self.pro_gemini_model,
                self.pro_openai_api_key.clone(),
                &self.pro_openai_model,
                &self.pro_openai_base_url,
            );
        }

        // 标准层级（含 Lite/Pro 关闭时的回退）
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

    fn resolve_secondary_tier(
        &self,
        provider: &str,
        gemini_api_key: Option<String>,
        gemini_model: &str,
        openai_api_key: Option<String>,
        openai_model: &str,
        openai_base_url: &str,
    ) -> ResolvedAiConfig {
        if provider == "openai" {
            ResolvedAiConfig {
                provider: "openai".to_string(),
                api_key: openai_api_key
                    .filter(|key| !key.trim().is_empty())
                    .or_else(|| self.openai_api_key.clone()),
                model: if openai_model.trim().is_empty() {
                    self.openai_model.clone()
                } else {
                    openai_model.to_string()
                },
                base_url: if openai_base_url.trim().is_empty() {
                    self.openai_base_url.clone()
                } else {
                    openai_base_url.to_string()
                },
            }
        } else {
            ResolvedAiConfig {
                provider: "gemini".to_string(),
                api_key: gemini_api_key
                    .filter(|key| !key.trim().is_empty())
                    .or_else(|| self.gemini_api_key.clone()),
                model: if gemini_model.trim().is_empty() {
                    self.gemini_model.clone()
                } else {
                    gemini_model.to_string()
                },
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
}
