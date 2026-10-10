use serde::{Deserialize, Serialize};
use std::env;

/// The official OpenAI endpoint: a source of kind `openai` with no address
/// of its own, and the built-in `openai` slug, go here.
pub const OPENAI_API_BASE: &str = "https://api.openai.com/v1";
/// The same for Volcengine Ark.
pub const VOLCENGINE_API_BASE: &str = "https://ark.cn-beijing.volces.com/api/v3";

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
    /// UI 图标 URL（可选）
    #[serde(default)]
    pub icon_url: Option<String>,
}

/// 一个可复用的 AI 服务商源（同一 kind 可以有多条，用 slug 区分）。
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct AiVendorSource {
    pub slug: String,
    /// openrouter | openai | openai_compatible | gemini | volcengine | tencent | agora | minimax
    pub kind: String,
    pub display_name: String,
    pub enabled: bool,
    /// 前端预设 id（openrouter / deepseek / groq …），与 kind 独立。
    #[serde(default)]
    pub preset: String,
    /// Wire protocol for text generation. Kept separate from vendor branding.
    /// Values use the analyzer runtime ids (`openai`, `openai_responses`, `anthropic`, `gemini`).
    #[serde(default)]
    pub api_format: String,
    /// Credential ownership for this source: `own`, `shared`, or `none`.
    /// An empty value is a legacy source and is resolved conservatively.
    #[serde(default)]
    pub credential_mode: String,
    /// Shared key family used when `credential_mode` is `shared`.
    #[serde(default)]
    pub shared_key_ref: Option<String>,
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
    pub fn effective_api_format(&self) -> &str {
        let explicit = self.api_format.trim();
        if !explicit.is_empty() {
            return explicit;
        }
        if self.kind.trim().eq_ignore_ascii_case("gemini") {
            "gemini"
        } else {
            "openai"
        }
    }

    pub fn has_supported_api_format(&self) -> bool {
        matches!(
            self.effective_api_format(),
            "openai" | "openai_responses" | "anthropic" | "gemini"
        )
    }

    /// Give a source with no credential mode the one it always had: its own
    /// key when it has one, else the shared key of its vendor on that vendor's
    /// own endpoint, else none. Every writer settles a source before storing
    /// it, so reading never has to guess.
    pub fn settle_credential_mode(&mut self) {
        if !self.credential_mode.trim().is_empty() {
            return;
        }
        if self
            .api_key
            .as_deref()
            .is_some_and(|key| !key.trim().is_empty())
        {
            self.credential_mode = "own".to_string();
        } else if let Some(key_ref) = self.canonical_shared_key_ref() {
            self.credential_mode = "shared".to_string();
            self.shared_key_ref = Some(key_ref.to_string());
        } else {
            self.credential_mode = "none".to_string();
            self.shared_key_ref = None;
        }
    }

    fn canonical_shared_key_ref(&self) -> Option<&'static str> {
        let kind = self.kind.trim().to_ascii_lowercase();
        let base_url = self.base_url.trim().trim_end_matches('/');
        match kind.as_str() {
            "openai" if base_url.is_empty() || base_url == "https://api.openai.com/v1" => {
                Some("openai")
            }
            "openai_compatible" if base_url == "https://api.openai.com/v1" => Some("openai"),
            "openrouter" if base_url.is_empty() || base_url == "https://openrouter.ai/api/v1" => {
                Some("openrouter")
            }
            "gemini"
                if base_url.is_empty()
                    || base_url == "https://generativelanguage.googleapis.com" =>
            {
                Some("gemini")
            }
            "volcengine"
                if base_url.is_empty()
                    || base_url == "https://ark.cn-beijing.volces.com/api/v3" =>
            {
                Some("volcengine")
            }
            _ => None,
        }
    }

    pub fn is_agora(&self) -> bool {
        let slug = self.slug.trim().to_ascii_lowercase();
        self.kind.trim().eq_ignore_ascii_case("agora")
            || self.preset.trim().eq_ignore_ascii_case("agora")
            || slug == "agora"
            || slug.starts_with("agora-")
    }
}

/// Lite 档作为她的原声：DashScope 的 OpenAI 兼容端点、模型与音色。
/// 不派生 Debug：带着密钥，不该出现在任何日志里。
#[derive(Clone)]
pub struct OmniVoice {
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    /// 空：模型默认音色。
    pub voice: String,
}

/// 解析后的 AI 配置（已根据 tier 确定具体的 provider/key/model）
pub struct ResolvedAiConfig {
    pub provider: String,
    pub api_format: String,
    pub api_key: Option<String>,
    pub credential_origin: AiCredentialOrigin,
    pub model: String,
    pub base_url: String,
    pub source_enabled: bool,
    pub requires_explicit_endpoint: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiCredentialOrigin {
    Own,
    Shared(String),
    None,
    Invalid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedAiCredential {
    pub api_key: Option<String>,
    pub origin: AiCredentialOrigin,
}

impl ResolvedAiConfig {
    pub fn text_ready(&self) -> bool {
        let credential_ready = match &self.credential_origin {
            AiCredentialOrigin::None => true,
            AiCredentialOrigin::Own | AiCredentialOrigin::Shared(_) => self
                .api_key
                .as_ref()
                .is_some_and(|key| !key.trim().is_empty()),
            AiCredentialOrigin::Invalid => false,
        };
        let endpoint_ready = !self.requires_explicit_endpoint || !self.base_url.trim().is_empty();

        self.source_enabled
            && !self.model.trim().is_empty()
            && matches!(
                self.api_format.as_str(),
                "openai" | "openai_responses" | "anthropic" | "gemini"
            )
            && endpoint_ready
            && credential_ready
    }
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
    /// 环境变量 `BASE_URL`（`from_env`）
    pub base_url: Option<String>,
    /// 环境变量 `FRONTEND_URL`（`from_env`）
    pub frontend_url: Option<String>,
}

impl Default for AppConfig {
    fn default() -> Self {
        // Development-oriented scaffolding defaults (tests / first boot).
        // Production loads via `from_env` (no localhost injection).
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
    /// - **Production**: empty.
    /// - **Development**: `http://localhost:1102` and `http://localhost:1103`.
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

        // Non-empty JWT_SECRET is strength-checked in every environment.
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
        // Must be at least 32 bytes (`.len()`).
        if secret.len() < 32 {
            anyhow::bail!(
                "JWT_SECRET is too weak. Must be at least 32 characters. \
                Generate a strong secret with: openssl rand -base64 32"
            );
        }

        // Reject obvious weak/default values.
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
    // 文本模型：每一档是一个服务商源（slug）加一个模型名。
    /// Standard 的服务商源。留空按出厂的 OpenRouter。
    pub ai_source: String,
    pub ai_model: String,
    /// Lite 的服务商源。留空跟 Standard 用同一个。
    pub lite_ai_source: String,
    /// Lite 的模型。留空就不用 Lite，没有单独的开关。
    pub lite_ai_model: String,
    /// Optional model for small typed judgments (affect appraisal, event
    /// decisions, memory extraction, whether to look something up), on the
    /// judgment and embedding source. Blank: Lite's own model.
    pub aux_judge_model: String,
    /// Optional embedding model on the judgment and embedding source: recall
    /// then finds a memory by what it means, not only by its words. Blank:
    /// words only.
    pub aux_embedding_model: String,
    /// Where the judgment and embedding models run, their own source apart
    /// from Lite's (a vendor source slug): Lite can move (to DashScope, say)
    /// while they stay where they are. Blank (never chosen): Lite's source
    /// of now, which is also what the settings page shows chosen.
    pub aux_ai_source: String,
    // Pro 有开关；开着却没填模型时，Pro 的活交给 Standard。
    pub pro_enabled: bool,
    /// Pro 的服务商源。留空跟 Standard 用同一个。
    pub pro_ai_source: String,
    pub pro_ai_model: String,

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

    pub speech_stt_model: String,
    pub speech_tts_model: String,
    pub speech_tts_voice: String,

    /// 共享密钥：服务商源选「共享」时用的 key（文字 / 图片 / 语音都从这里取）。
    /// 地址在源上，不在这里。
    pub provider_openai_api_key: Option<String>,
    pub provider_openrouter_api_key: Option<String>,
    pub provider_gemini_api_key: Option<String>,
    /// TinyFish Search / Fetch host secret. Search is free; the key is still required.
    #[serde(default)]
    pub provider_tinyfish_api_key: Option<String>,
    pub provider_volcengine_api_key: Option<String>,
    /// 可添加的服务商源列表（OAuth providers 同款：可多家、可同 kind 多源）
    pub ai_vendor_sources: Vec<AiVendorSource>,
    /// 图片走的服务商源（slug）。留空按出厂的 OpenRouter。
    pub ai_image_source: String,
    /// 语音走的服务商源（slug）。留空按出厂的腾讯云。
    pub speech_source: String,

    /// QQ 机器人办事通道。默认关；凭证只写，不复用 Discord OAuth。
    pub qq_bot_enabled: bool,
    pub qq_bot_app_id: String,
    pub qq_bot_app_secret: Option<String>,

    /// Telegram 机器人办事通道。默认关；bot token 只写，不是 MTProto api_hash。
    pub telegram_bot_enabled: bool,
    pub telegram_bot_token: Option<String>,

    /// Discord 机器人办事通道。默认关；bot token 只写，不复用数据平台 OAuth。
    pub discord_bot_enabled: bool,
    pub discord_bot_token: Option<String>,

    /// 飞书机器人办事通道。默认关；app_secret 只写。企业自建应用 + 长连接。
    pub feishu_bot_enabled: bool,
    pub feishu_bot_app_id: String,
    pub feishu_bot_app_secret: Option<String>,

    /// OneBot 11 办事通道（正向 WebSocket，对接 NapCat 等协议端）。默认关。
    /// ws_url 是路由信息，可读回；access_token 只写。
    pub onebot_bot_enabled: bool,
    pub onebot_bot_ws_url: String,
    pub onebot_bot_access_token: Option<String>,
    /// 群聊默认关。个人 QQ 号已经在很多群里，不打开就不记录、不应答。
    pub onebot_bot_groups_enabled: bool,
    /// 群号白名单，逗号分隔（规范化后）。空：打开群聊时不限群。
    pub onebot_bot_group_ids: String,

    // UI 配置
    pub ui_wallpaper_url: Option<String>,
    pub ui_wallpaper_blur: i32,
    // Evocative 壁纸动效
    pub ui_evocative_parallax: bool,
    pub ui_evocative_dynamic_blur: bool,
    pub ui_evocative_ripple: bool,
    /// Evocative 动效帧率（默认 30；后端按 i32 存储）
    pub ui_evocative_fps: i32,
    /// 涟漪效果 Canvas 质量（默认 0.85；后端按 f64 存储）
    pub ui_evocative_ripple_quality: f64,

    /// 第一方访客统计（pageview / engagement / event）是否开启；关闭后服务端拒绝采集
    pub analytics_enabled: bool,

    /// PWA：是否启用 Service Worker 注册与可安装清单（默认 true）。
    pub pwa_enabled: bool,

    // 站点元数据
    pub site_title: Option<String>,
    pub site_description: Option<String>,
    pub site_favicon: Option<String>,
    /// SEO 关键词（逗号分隔，写入 meta keywords）
    pub site_keywords: Option<String>,
    /// 社交分享预览图（Open Graph / Twitter Card）
    pub site_og_image: Option<String>,
    /// Google Search Console HTML 标签验证码（写入 `google-site-verification` meta）
    #[serde(default)]
    pub google_site_verification: Option<String>,
    /// 禁止搜索引擎收录（true → robots: noindex, nofollow）
    /// 与 site_visibility_policy 联动：private 时为 true。
    pub site_noindex: bool,
    /// 站点可见性 / GEO 策略：private | search_only | ai_citation | ai_full
    pub site_visibility_policy: String,
    /// 面向 AI 引擎的站点简介（写入 llms.txt；可空则回退 site_description）
    pub site_ai_intro: Option<String>,
    /// Agent 对照自有公开内容检查 SEO/GEO 文案的节奏：off | daily | weekly
    #[serde(default)]
    pub site_seo_review_cadence: String,
    /// Google Analytics 4 Measurement ID（如 G-XXXXXXXXXX）；空则不加载 gtag
    pub ga_measurement_id: Option<String>,
    /// Umami website id（UUID）；空则不加载
    pub umami_website_id: Option<String>,
    /// Umami tracker 脚本完整 URL（如 https://cloud.umami.is/script.js）
    pub umami_script_url: Option<String>,
    pub site_icp: Option<String>,       // ICP 备案号
    pub site_gongan: Option<String>,    // 公安备案号
    pub cloud_sponsors: Option<String>, // 云赞助商（cloudflare,edgeone,upyun 逗号分隔）
    /// 页脚自定义项 JSON 字符串（`[{text, icon?, url?}]`；条数上限在 UI `FOOTER_CUSTOM_MAX`）
    pub site_footer_custom: Option<String>,

    // 音乐配置
    pub music_enabled: Option<String>,
    pub music_source: Option<String>,
    pub music_playlist_id: Option<String>,
    /// 播放器是否走同源全量音频代理。关闭后一律用 play-url 直连 CDN，
    /// 桌面端频谱因 CORS 不可用。缺省开。
    pub music_proxy_enabled: bool,
    /// 播放器是否预加载下一首。缺省开。
    pub music_preload_enabled: bool,

    /// 智能岛收缩态是否轮播问候 / 天气 / 一言 / 音乐 / Tapp。缺省全开。
    /// 通知进岛走通知偏好，不走这组开关。
    pub island_show_greeting: bool,
    pub island_show_weather: bool,
    pub island_show_quote: bool,
    pub island_show_music: bool,
    pub island_show_tapp: bool,

    // AI 图片生成：源是 `ai_image_source`，这里是模型。分辨率由调用方
    // （agent / tapp）在请求参数中决定，不设全局配置。
    pub ai_image_model: String,

    /// Merope：设定、状态、主动对话、事件开口。默认关。用户界面叫 Agent 人设。
    pub merope_enabled: bool,

    /// 人设形象是否开口朗读聊天回复。默认关；人设未生效时一律关。
    pub merope_speech_enabled: bool,
    /// 她的声音是哪一整套（见 `merope_voice_mode_resolved`）：
    /// `tts` 浏览器听写（ASR）→ Lite → 朗读（TTS）；`agora` 实时对话走声网（声网的 ASR 与
    /// TTS，中间是 Lite），打字聊天仍是 Lite + TTS；`omni` 全由 Lite 档的 Omni 听、想、说，
    /// 要求 Lite 档直连 DashScope（见 `merope_omni_voice`）。空：没选过，按旧行为——
    /// 配了声网就是 `agora`，否则 `tts`。
    pub merope_voice_mode: String,
    /// `omni` 时她的音色：预置音色名或复刻得到的音色 id；空为模型默认。
    pub merope_voice_voice: String,

    /// Agent 人设的页面形象：当前生效的 2.5D 图集包 id（sha256 hex）。
    /// None = 没有编译过的骨骼，浮动层只回退主立绘。站点级——全站一份形象。
    pub agent_rig_asset_id: Option<String>,

    /// Hugging Face token used only by the backend when invoking the remote
    /// See-through ZeroGPU Space. This is a write-only host credential.
    pub see_through_hf_token: Option<String>,
    /// The See-through Space (`owner/name`) to decompose with; None is
    /// Myriad's own. A Space serving `decompose` fits portraits on a canvas
    /// of their own shape; any other pads them to a square.
    pub see_through_space: Option<String>,

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

    /// 是否允许公开本地账号注册
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
    /// Site-wide home layout mode: `standard` or `free`.
    pub dashboard_layout_mode: Option<String>,
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

    // 下放字段：改授予权限。elevated 可配；privileged 只限管理员；
    // media:control 为 basic，授予始终允许。basic 对 User 默认开放；
    // 需持久登录主体的 basic 不向 Guest 授予。

    // 普通用户授予路径读取的配置字段（report:write / media:control 不读）
    /// ai:generate - AI 生成内容
    pub user_perm_ai_generate: bool,
    /// ai:analyze - AI 分析数据
    pub user_perm_ai_analyze: bool,
    /// ai:chat - AI 对话
    pub user_perm_ai_chat: bool,
    /// ai:image - AI 图片生成
    pub user_perm_ai_image: bool,
    /// ai:search - 联网搜索（TinyFish / Gemini grounding）
    pub user_perm_ai_search: bool,
    /// 3d:generate - Tripo 3D 模型生成
    pub user_perm_3d_generate: bool,
    /// network:fetch - 发起网络请求
    pub user_perm_network_fetch: bool,
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
    /// phantasi:commentWrite - 写 Phantasi 评论（需登录主体）
    pub user_perm_phantasi_comment_write: bool,

    // 游客授予路径读取的配置字段（若干恒 false，见各字段）
    /// ai:generate - AI 生成内容（游客）
    pub guest_perm_ai_generate: bool,
    /// ai:analyze - AI 分析数据（游客）
    pub guest_perm_ai_analyze: bool,
    /// ai:chat - AI 对话（游客）
    pub guest_perm_ai_chat: bool,
    /// ai:image - AI 图片生成（游客）
    pub guest_perm_ai_image: bool,
    /// ai:search - 联网搜索（游客）
    pub guest_perm_ai_search: bool,
    /// network:fetch - 发起网络请求（游客）
    pub guest_perm_network_fetch: bool,
    /// event:publish - 发布事件（游客）
    pub guest_perm_event_publish: bool,
    /// storage:write - 写入 Tapp 存储（游客）
    pub guest_perm_storage_write: bool,

    // AI 使用限额配置（非管理员生效；管理员无限制）
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
    /// 默认 false = 均衡档。`MYRIAD_MEMORY_PROFILE` env 可覆盖。
    pub memory_saver_enabled: bool,

    /// 天气是否向浏览器申请精确位置（高级设置）。默认 false：只走 IP，不弹定位许可。
    pub precise_location_enabled: bool,

    // 网络代理配置（用于中国大陆服务器访问外部API）
    /// 是否启用网络代理
    pub proxy_enabled: bool,
    /// HTTP/HTTPS 代理地址（如 http://127.0.0.1:7890 或 socks5://127.0.0.1:1080）
    pub proxy_url: Option<String>,
    /// 不使用代理的域名列表（逗号分隔，如 localhost,127.0.0.1,bilibili.com）
    pub proxy_bypass: Option<String>,
    /// 出站 SSRF 防护放行 DNS 解析到 198.18.0.0/15（RFC 2544）的结果。
    /// 给 Clash 等 fake-ip 模式的宿主用；默认关。URL 里直接写的 IP 不受影响。
    pub allow_rfc2544_benchmark_range: bool,
    /// Gemini API Base URL（用于使用第三方代理服务）
    pub gemini_base_url: Option<String>,
    /// GitHub API Base URL（用于使用 GitHub 镜像服务）
    pub github_api_base_url: Option<String>,
}

impl Default for DynamicConfig {
    fn default() -> Self {
        Self {
            // 出厂走 OpenRouter。
            ai_source: "openrouter".to_string(),
            ai_model: "minimax/minimax-m3".to_string(),
            // Lite 默认不用：模型留空；源跟 Standard。
            lite_ai_source: String::new(),
            lite_ai_model: String::new(),
            // 留空：判断也用 Lite 的模型，回忆只按字面。
            aux_judge_model: String::new(),
            aux_embedding_model: String::new(),
            aux_ai_source: String::new(),
            pro_enabled: false,
            pro_ai_source: String::new(),
            pro_ai_model: "anthropic/claude-opus-5.5".to_string(),

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

            speech_stt_model: String::new(),
            speech_tts_model: String::new(),
            speech_tts_voice: String::new(),
            provider_openai_api_key: None,
            provider_openrouter_api_key: None,
            provider_gemini_api_key: None,
            provider_tinyfish_api_key: None,
            provider_volcengine_api_key: None,
            ai_vendor_sources: Vec::new(),
            ai_image_source: "openrouter".to_string(),
            speech_source: "tencent".to_string(),
            qq_bot_enabled: false,
            qq_bot_app_id: String::new(),
            qq_bot_app_secret: None,
            telegram_bot_enabled: false,
            telegram_bot_token: None,
            discord_bot_enabled: false,
            discord_bot_token: None,
            feishu_bot_enabled: false,
            feishu_bot_app_id: String::new(),
            feishu_bot_app_secret: None,
            onebot_bot_enabled: false,
            onebot_bot_ws_url: String::new(),
            onebot_bot_access_token: None,
            onebot_bot_groups_enabled: false,
            onebot_bot_group_ids: String::new(),

            ui_wallpaper_url: None,
            ui_wallpaper_blur: 3,
            // Evocative 壁纸动效
            ui_evocative_parallax: true,
            ui_evocative_dynamic_blur: false,
            ui_evocative_ripple: false,
            ui_evocative_fps: 30,
            ui_evocative_ripple_quality: 0.85,

            analytics_enabled: true,

            pwa_enabled: true,

            site_title: None,
            site_description: None,
            site_favicon: None,
            site_keywords: None,
            site_og_image: None,
            google_site_verification: None,
            site_noindex: false,
            site_visibility_policy: String::new(),
            site_ai_intro: None,
            site_seo_review_cadence: String::new(),
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
            music_proxy_enabled: true,
            music_preload_enabled: true,

            island_show_greeting: true,
            island_show_weather: true,
            island_show_quote: true,
            island_show_music: true,
            island_show_tapp: true,

            ai_image_model: "openai/gpt-image-2.5-sunburst".to_string(),
            merope_enabled: false,
            merope_speech_enabled: false,
            merope_voice_mode: String::new(),
            merope_voice_voice: String::new(),
            agent_rig_asset_id: None,
            see_through_hf_token: None,
            see_through_space: None,
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
            dashboard_layout_mode: None,
            dashboard_title: None,
            custom_platforms: None,
            widget_theme: None,

            title_font: None,
            title_font_size: None,
            title_color: None,

            control_panel_layout: None,
            control_panel_rows: 2,

            tapp_window_schemes: None,

            // 普通用户下放字段默认 false（media:control 授予仍始终允许）
            user_perm_ai_generate: false,
            user_perm_ai_analyze: false,
            user_perm_ai_chat: false,
            user_perm_ai_image: false,
            user_perm_ai_search: false,
            user_perm_3d_generate: false,
            user_perm_network_fetch: false,
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
            user_perm_phantasi_comment_write: false,

            // 游客下放字段默认 false（若干授予路径恒 false，见字段注释）
            guest_perm_ai_generate: false,
            guest_perm_ai_analyze: false,
            guest_perm_ai_chat: false,
            guest_perm_ai_image: false,
            guest_perm_ai_search: false,
            guest_perm_network_fetch: false,
            guest_perm_event_publish: false,
            guest_perm_storage_write: false,

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

            memory_saver_enabled: false,
            precise_location_enabled: false,
            // 网络代理配置默认值
            proxy_enabled: false, // 默认关闭代理
            proxy_url: None,
            proxy_bypass: None,
            allow_rfc2544_benchmark_range: false,
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
    /// 开关开着且 Pro 开着。设定引导走 Pro；聊天人设是系统词，跟 Lite 无关。
    pub fn merope_enabled_resolved(&self) -> bool {
        self.merope_switch_on() && self.pro_enabled
    }

    /// 开关开着却缺 Lite。主动开口会走短句兜底，心情微调不会跑。
    pub fn merope_needs_lite(&self) -> bool {
        self.merope_switch_on() && self.pro_enabled && !self.lite_on()
    }

    /// 开关开着却缺 Pro。设定引导和开关生效都要这一档。
    pub fn merope_needs_pro(&self) -> bool {
        self.merope_switch_on() && !self.pro_enabled
    }

    /// 人设开口朗读。人设未生效时一律关。
    pub fn merope_speech_enabled_resolved(&self) -> bool {
        self.merope_enabled_resolved() && self.merope_speech_enabled
    }

    /// 给配置页看的事实：实际生效的是哪一套，与选的不同时为什么——
    /// `tts`、`omni`，或 `tts:lite_not_dashscope` 这样带原因。页面只显示，不自己推断。
    pub fn merope_voice_mode_effective(&self) -> String {
        let resolved = self.merope_voice_mode_resolved();
        let why = match self.merope_voice_mode.trim() {
            "omni" if resolved != "omni" => self.merope_omni_voice().err(),
            "agora" if resolved != "agora" => Some("agora_unconfigured"),
            _ => None,
        };
        match why {
            Some(why) => format!("{resolved}:{why}"),
            None => resolved.to_string(),
        }
    }

    /// 保存时的选择：认得的三种之一，其余一律当朗读。
    pub fn voice_mode_choice(value: &str) -> &'static str {
        match value.trim() {
            "omni" => "omni",
            "agora" => "agora",
            _ => "tts",
        }
    }

    /// 她的声音实际是哪一整套：选了声网却没配好，或选了 Omni 却不满足，退回 `tts`。
    pub fn merope_voice_mode_resolved(&self) -> &'static str {
        match self.merope_voice_mode.trim() {
            "omni" if self.merope_omni_voice().is_ok() => "omni",
            "agora" | "" if crate::services::agora_convo::convo_configured(self) => "agora",
            _ => "tts",
        }
    }

    /// 她用 Omni 原声说话时的出站：Lite 档本身（同一个源、同一个模型），
    /// 加上音色。不满足时是为什么，给配置页和日志看。
    pub fn merope_omni_voice(&self) -> Result<OmniVoice, &'static str> {
        if !self.merope_speech_enabled_resolved() {
            return Err("speech_off");
        }
        if self.merope_voice_mode.trim() != "omni" {
            return Err("mode_tts");
        }
        let lite = self
            .resolve_strict_lite_ai_config()
            .ok_or("lite_unconfigured")?;
        if !Self::is_dashscope_base(&lite.base_url) {
            return Err("lite_not_dashscope");
        }
        if !lite.model.to_ascii_lowercase().contains("omni") {
            return Err("lite_not_omni");
        }
        let api_key = lite
            .api_key
            .filter(|key| !key.trim().is_empty())
            .ok_or("lite_no_key")?;
        Ok(OmniVoice {
            base_url: lite.base_url.trim_end_matches('/').to_string(),
            api_key,
            model: lite.model,
            voice: self.merope_voice_voice.trim().to_string(),
        })
    }

    pub fn is_dashscope_base(url: &str) -> bool {
        url.to_ascii_lowercase().contains("dashscope")
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

    pub fn shared_openrouter_api_key(&self) -> Option<String> {
        Self::first_nonempty_key([self.provider_openrouter_api_key.clone()])
    }

    pub fn shared_openai_api_key(&self) -> Option<String> {
        Self::first_nonempty_key([self.provider_openai_api_key.clone()])
    }

    pub fn shared_gemini_api_key(&self) -> Option<String> {
        Self::first_nonempty_key([self.provider_gemini_api_key.clone()])
    }

    pub fn shared_tinyfish_api_key(&self) -> Option<String> {
        Self::first_nonempty_key([self.provider_tinyfish_api_key.clone()])
    }

    /// Standard 档解析后是否有可用文本端点；新 source 可以明确选择免鉴权。
    pub fn text_ai_available(&self) -> bool {
        self.resolve_ai_config(ModelTier::Standard).text_ready()
    }

    /// Google Search grounding 只能走 Gemini。
    ///
    /// key 用共享 Gemini 库；模型取第一档本身就在 Gemini 上的（Standard、
    /// Lite、Pro），都不是就用 grounding 默认型号。
    pub fn resolve_gemini_grounding(&self) -> Option<(String, String)> {
        let api_key = self.shared_gemini_api_key()?;
        let model = [ModelTier::Standard, ModelTier::Lite, ModelTier::Pro]
            .into_iter()
            .map(|tier| self.resolve_ai_config(tier))
            .find(|resolved| resolved.provider == "gemini" && !resolved.model.trim().is_empty())
            .map_or_else(|| "gemini-3.8-flash".to_string(), |resolved| resolved.model);
        Some((api_key, model))
    }

    pub fn shared_volcengine_api_key(&self) -> Option<String> {
        Self::first_nonempty_key([self.provider_volcengine_api_key.clone()])
    }

    fn shared_api_key_by_ref(&self, key_ref: &str) -> Option<String> {
        match key_ref.trim() {
            "openai" => self.shared_openai_api_key(),
            "openrouter" => self.shared_openrouter_api_key(),
            "gemini" => self.shared_gemini_api_key(),
            "volcengine" => self.shared_volcengine_api_key(),
            _ => None,
        }
    }

    /// Resolve a source credential without exposing it outside the backend.
    /// A source without a mode resolves to nothing: every writer settles it
    /// (`AiVendorSource::settle_credential_mode`).
    pub fn resolve_source_credential(&self, source: &AiVendorSource) -> ResolvedAiCredential {
        match source.credential_mode.trim() {
            "own" => ResolvedAiCredential {
                api_key: Self::nonempty_opt(source.api_key.as_ref()),
                origin: AiCredentialOrigin::Own,
            },
            "shared" => {
                let Some(key_ref) = source
                    .shared_key_ref
                    .as_deref()
                    .map(str::trim)
                    .filter(|value| !value.is_empty())
                else {
                    return ResolvedAiCredential {
                        api_key: None,
                        origin: AiCredentialOrigin::Invalid,
                    };
                };
                if !matches!(key_ref, "openai" | "openrouter" | "gemini" | "volcengine") {
                    return ResolvedAiCredential {
                        api_key: None,
                        origin: AiCredentialOrigin::Invalid,
                    };
                }
                let api_key = self.shared_api_key_by_ref(key_ref);
                ResolvedAiCredential {
                    api_key,
                    origin: AiCredentialOrigin::Shared(key_ref.to_string()),
                }
            }
            "none" => ResolvedAiCredential {
                api_key: None,
                origin: AiCredentialOrigin::None,
            },
            _ => ResolvedAiCredential {
                api_key: None,
                origin: AiCredentialOrigin::Invalid,
            },
        }
    }

    pub fn nonempty_opt(value: Option<&String>) -> Option<String> {
        value
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    }

    pub fn synthesize_vendor_sources(&self) -> Vec<AiVendorSource> {
        let mut sources = Vec::new();
        if self.shared_openrouter_api_key().is_some() {
            sources.push(AiVendorSource {
                slug: "openrouter".to_string(),
                kind: "openrouter".to_string(),
                display_name: "OpenRouter".to_string(),
                enabled: true,
                preset: "openrouter".to_string(),
                credential_mode: "shared".to_string(),
                shared_key_ref: Some("openrouter".to_string()),
                base_url: "https://openrouter.ai/api/v1".to_string(),
                ..AiVendorSource::default()
            });
        }
        if self.shared_openai_api_key().is_some() {
            sources.push(AiVendorSource {
                slug: "openai".to_string(),
                kind: "openai".to_string(),
                display_name: "OpenAI".to_string(),
                enabled: true,
                preset: "openai".to_string(),
                credential_mode: "shared".to_string(),
                shared_key_ref: Some("openai".to_string()),
                base_url: OPENAI_API_BASE.to_string(),
                ..AiVendorSource::default()
            });
        }
        if self.shared_gemini_api_key().is_some() {
            sources.push(AiVendorSource {
                slug: "gemini".to_string(),
                kind: "gemini".to_string(),
                display_name: "Gemini".to_string(),
                enabled: true,
                preset: "gemini".to_string(),
                api_format: "gemini".to_string(),
                credential_mode: "shared".to_string(),
                shared_key_ref: Some("gemini".to_string()),
                ..AiVendorSource::default()
            });
        }
        if self.shared_volcengine_api_key().is_some() {
            sources.push(AiVendorSource {
                slug: "volcengine".to_string(),
                kind: "volcengine".to_string(),
                display_name: "Volcengine".to_string(),
                enabled: true,
                preset: "volcengine".to_string(),
                credential_mode: "shared".to_string(),
                shared_key_ref: Some("volcengine".to_string()),
                base_url: VOLCENGINE_API_BASE.to_string(),
                ..AiVendorSource::default()
            });
        }
        sources
    }

    pub fn effective_vendor_sources(&self) -> Vec<AiVendorSource> {
        if self.ai_vendor_sources.is_empty() {
            self.synthesize_vendor_sources()
        } else {
            self.ai_vendor_sources.clone()
        }
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

    pub fn resolve_from_vendor_source(
        &self,
        source: &AiVendorSource,
        model: &str,
    ) -> ResolvedAiConfig {
        let credential = self.resolve_source_credential(source);
        match source.kind.trim().to_ascii_lowercase().as_str() {
            "gemini" => ResolvedAiConfig {
                provider: "gemini".to_string(),
                api_format: source.effective_api_format().to_string(),
                api_key: credential.api_key,
                credential_origin: credential.origin,
                model: model.to_string(),
                base_url: source.base_url.trim().to_string(),
                source_enabled: source.enabled,
                requires_explicit_endpoint: false,
            },
            "openrouter" => ResolvedAiConfig {
                provider: "openai".to_string(),
                api_format: source.effective_api_format().to_string(),
                api_key: credential.api_key,
                credential_origin: credential.origin,
                model: model.to_string(),
                base_url: if source.base_url.trim().is_empty() {
                    "https://openrouter.ai/api/v1".to_string()
                } else {
                    source.base_url.trim().to_string()
                },
                source_enabled: source.enabled,
                requires_explicit_endpoint: false,
            },
            "openai" => ResolvedAiConfig {
                provider: if source.effective_api_format() == "anthropic" {
                    "anthropic".to_string()
                } else {
                    "openai".to_string()
                },
                api_format: source.effective_api_format().to_string(),
                api_key: credential.api_key,
                credential_origin: credential.origin,
                model: model.to_string(),
                base_url: if source.base_url.trim().is_empty() {
                    OPENAI_API_BASE.to_string()
                } else {
                    source.base_url.trim().to_string()
                },
                source_enabled: source.enabled,
                requires_explicit_endpoint: false,
            },
            _ => ResolvedAiConfig {
                provider: if source.effective_api_format() == "anthropic" {
                    "anthropic".to_string()
                } else {
                    "openai".to_string()
                },
                api_format: source.effective_api_format().to_string(),
                api_key: credential.api_key,
                credential_origin: credential.origin,
                model: model.to_string(),
                base_url: source.base_url.trim().to_string(),
                source_enabled: source.enabled,
                requires_explicit_endpoint: true,
            },
        }
    }

    /// `model` on a built-in source that has no vendor source row: the
    /// shared key of that vendor. Any other slug (a source since deleted)
    /// resolves to nothing usable rather than to someone else's endpoint.
    fn resolve_builtin_source(&self, slug: &str, model: &str) -> ResolvedAiConfig {
        let shared =
            |provider: &str, api_format: &str, api_key, key_ref: &str, base_url| ResolvedAiConfig {
                provider: provider.to_string(),
                api_format: api_format.to_string(),
                api_key,
                credential_origin: AiCredentialOrigin::Shared(key_ref.to_string()),
                model: model.to_string(),
                base_url,
                source_enabled: true,
                requires_explicit_endpoint: false,
            };
        match slug {
            "openrouter" => shared(
                "openai",
                "openai",
                self.shared_openrouter_api_key(),
                "openrouter",
                "https://openrouter.ai/api/v1".to_string(),
            ),
            "openai" => shared(
                "openai",
                "openai",
                self.shared_openai_api_key(),
                "openai",
                OPENAI_API_BASE.to_string(),
            ),
            "gemini" => shared(
                "gemini",
                "gemini",
                self.shared_gemini_api_key(),
                "gemini",
                String::new(),
            ),
            _ => ResolvedAiConfig {
                provider: "openai".to_string(),
                api_format: "openai".to_string(),
                api_key: None,
                credential_origin: AiCredentialOrigin::Invalid,
                model: model.to_string(),
                base_url: String::new(),
                source_enabled: false,
                requires_explicit_endpoint: true,
            },
        }
    }

    /// The source a tier runs on: its own; else, for Lite and Pro,
    /// Standard's; else the factory OpenRouter.
    pub fn tier_source(&self, tier: ModelTier) -> String {
        let own = match tier {
            ModelTier::Standard => &self.ai_source,
            ModelTier::Lite => &self.lite_ai_source,
            ModelTier::Pro => &self.pro_ai_source,
        }
        .trim();
        if !own.is_empty() {
            own.to_string()
        } else if tier == ModelTier::Standard {
            "openrouter".to_string()
        } else {
            self.tier_source(ModelTier::Standard)
        }
    }

    /// The source images are made on: as chosen, else the factory OpenRouter.
    pub fn image_source(&self) -> String {
        Self::slug_or(&self.ai_image_source, "openrouter")
    }

    /// The source speech runs on: as chosen, else the factory Tencent Cloud.
    pub fn speech_source_slug(&self) -> String {
        Self::slug_or(&self.speech_source, "tencent")
    }

    /// `chosen` trimmed, or `factory` when it is blank.
    fn slug_or(chosen: &str, factory: &str) -> String {
        let chosen = chosen.trim();
        if chosen.is_empty() { factory } else { chosen }.to_string()
    }

    /// Lite is in use when its own model is filled in; there is no switch.
    pub fn lite_on(&self) -> bool {
        !self.lite_ai_model.trim().is_empty()
    }

    /// Resolve Lite only when its own model is filled in. Shared provider
    /// credentials remain valid, but Standard's model is never inherited.
    pub fn resolve_strict_lite_ai_config(&self) -> Option<ResolvedAiConfig> {
        self.lite_on()
            .then(|| self.resolve_ai_config(ModelTier::Lite))
    }

    /// Small typed judgments: `aux_judge_model` on the judgment and
    /// embedding source when both are set and that source can be used,
    /// whether Lite is on or not; otherwise Lite's own model on Lite's own
    /// source (a model name is never sent to a source it is not for).
    /// Speaking stays on Lite's own model.
    pub fn resolve_lite_judge_ai_config(&self) -> Option<ResolvedAiConfig> {
        let lite = self.resolve_strict_lite_ai_config();
        let judge = self.aux_judge_model.trim();
        if judge.is_empty() {
            return lite;
        }
        match lite {
            // Never chosen: on Lite's source, as before.
            Some(lite) if !self.aux_chosen() => Some(ResolvedAiConfig {
                model: judge.to_string(),
                ..lite
            }),
            lite => self.on_aux_source(judge).or(lite),
        }
    }

    /// The embedding model, when one is set, on the judgment and embedding
    /// source, whether Lite is on or not; `None` (recall by words alone)
    /// when either is missing or that source cannot be used.
    pub fn resolve_lite_embedding_ai_config(&self) -> Option<ResolvedAiConfig> {
        let model = self.aux_embedding_model.trim();
        if model.is_empty() {
            return None;
        }
        if !self.aux_chosen()
            && let Some(lite) = self.resolve_strict_lite_ai_config()
        {
            // Never chosen: on Lite's source, as before.
            return Some(ResolvedAiConfig {
                model: model.to_string(),
                ..lite
            });
        }
        self.on_aux_source(model)
    }

    fn aux_chosen(&self) -> bool {
        !self.aux_ai_source.trim().is_empty()
    }

    /// The judgment and embedding source: as chosen, or, never chosen,
    /// Lite's source of now.
    pub fn aux_source_slug(&self) -> String {
        let chosen = self.aux_ai_source.trim();
        if chosen.is_empty() {
            self.tier_source(ModelTier::Lite)
        } else {
            chosen.to_string()
        }
    }

    /// `model` on the judgment and embedding source, when that source is
    /// there and can be used (on, with its key).
    fn on_aux_source(&self, model: &str) -> Option<ResolvedAiConfig> {
        let resolved = self.resolve_on_source(&self.aux_source_slug(), model);
        resolved.text_ready().then_some(resolved)
    }

    /// 这一档要的模型没配、实际会落到 Standard 上吗？
    ///
    /// 只有 Pro 会：开关开着、模型留空时，Pro 的活整个交给 Standard（源和
    /// 模型都是 Standard 的）。配置上看是「Pro 已启用」，账单和延迟却按
    /// Standard 走。这个函数只负责认出这件事，喊出来是调用方的事。
    ///
    /// 关掉的 Pro 不算回退：那是明确的选择。Lite 没有开关，模型留空就是
    /// 不用，所以从不回退。
    pub fn tier_falls_back_to_standard(&self, tier: ModelTier) -> bool {
        tier == ModelTier::Pro && self.pro_enabled && self.pro_ai_model.trim().is_empty()
    }

    /// 按档解析 AI 配置：这一档的源加这一档的模型。Lite 没填模型、Pro 关着
    /// 或没填模型时，解析成 Standard。与 `resolve_strict_lite_ai_config` 不同。
    pub fn resolve_ai_config(&self, tier: ModelTier) -> ResolvedAiConfig {
        let model = match tier {
            ModelTier::Lite if self.lite_on() => self.lite_ai_model.trim(),
            ModelTier::Pro if self.pro_enabled && !self.pro_ai_model.trim().is_empty() => {
                self.pro_ai_model.trim()
            }
            _ => {
                return self
                    .resolve_on_source(&self.tier_source(ModelTier::Standard), &self.ai_model);
            }
        };
        self.resolve_on_source(&self.tier_source(tier), model)
    }

    /// `model` on the source named `slug`: its vendor source row, or a
    /// built-in vendor's shared key.
    pub fn resolve_on_source(&self, slug: &str, model: &str) -> ResolvedAiConfig {
        match self.find_vendor_source(slug) {
            Some(source) => self.resolve_from_vendor_source(&source, model),
            None => self.resolve_builtin_source(slug.trim(), model),
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
        unsafe { std::env::set_var("DATABASE_URL", "postgres://test:test@localhost/test") };
        let config = AppConfig::from_env().unwrap();
        assert!(!config.database_url.is_empty());
        assert_eq!(config.server_port, 1103);
        match prev {
            Some(v) => unsafe { std::env::set_var("DATABASE_URL", v) },
            None => unsafe { std::env::remove_var("DATABASE_URL") },
        }
    }

    #[test]
    fn vendor_source_api_format_uses_runtime_ids() {
        let defaulted: AiVendorSource = serde_json::from_value(serde_json::json!({
            "slug": "legacy",
            "kind": "custom",
            "display_name": "Legacy",
            "enabled": true
        }))
        .expect("source without api_format");
        assert_eq!(defaulted.effective_api_format(), "openai");
        assert_eq!(AiVendorSource::default().effective_api_format(), "openai");
        assert_eq!(
            AiVendorSource {
                kind: "gemini".to_string(),
                api_format: String::new(),
                ..AiVendorSource::default()
            }
            .effective_api_format(),
            "gemini"
        );

        let explicit: AiVendorSource = serde_json::from_value(serde_json::json!({
            "slug": "custom-responses",
            "kind": "custom",
            "display_name": "Custom Responses",
            "enabled": true,
            "api_format": "openai_responses",
            "base_url": "https://llm.example/v1"
        }))
        .expect("explicit source");
        assert_eq!(explicit.effective_api_format(), "openai_responses");
        let config = DynamicConfig {
            provider_openai_api_key: Some("must-not-leak".to_string()),
            ai_vendor_sources: vec![explicit.clone()],
            ..DynamicConfig::default()
        };
        let resolved = config.resolve_from_vendor_source(&explicit, "gpt-x");
        assert_eq!(
            resolved.api_key, None,
            "custom source must not inherit another source's key"
        );

        let chat: AiVendorSource = serde_json::from_value(serde_json::json!({
            "slug": "ollama",
            "kind": "openai_compatible",
            "display_name": "Ollama",
            "enabled": true,
            "api_format": "openai",
            "base_url": "http://127.0.0.1:11434/v1"
        }))
        .expect("chat-completions source");
        assert_eq!(chat.effective_api_format(), "openai");
        assert_eq!(
            crate::services::analyzer::AiProvider::from_str(chat.effective_api_format())
                .expect("valid provider"),
            crate::services::analyzer::AiProvider::OpenAI
        );

        let invalid = AiVendorSource {
            api_format: "future_protocol".to_string(),
            ..AiVendorSource::default()
        };
        assert_eq!(invalid.effective_api_format(), "future_protocol");
        assert!(!invalid.has_supported_api_format());
    }

    #[test]
    fn an_unsettled_source_settles_to_what_it_always_resolved_to() {
        let source: AiVendorSource = serde_json::from_value(serde_json::json!({
            "slug": "openai-work",
            "kind": "openai",
            "display_name": "Work OpenAI",
            "enabled": true,
            "preset": "openai",
            "api_format": "openai_responses",
            "base_url": "https://api.openai.com/v1"
        }))
        .expect("legacy official source");
        let config = DynamicConfig {
            provider_openai_api_key: Some("shared-openai-key".to_string()),
            ..DynamicConfig::default()
        };
        // Unsettled, it resolves to nothing: every writer settles first.
        assert_eq!(
            config.resolve_source_credential(&source).origin,
            AiCredentialOrigin::Invalid
        );
        let mut settled = source;
        settled.settle_credential_mode();
        assert_eq!(settled.credential_mode, "shared");
        assert_eq!(settled.shared_key_ref.as_deref(), Some("openai"));
        let resolved = config.resolve_from_vendor_source(&settled, "gpt-x");
        assert_eq!(resolved.api_key.as_deref(), Some("shared-openai-key"));
    }

    #[test]
    fn source_credential_mode_controls_shared_key_reuse() {
        let config = DynamicConfig {
            provider_openai_api_key: Some("shared-openai-key".to_string()),
            provider_openrouter_api_key: Some("shared-openrouter-key".to_string()),
            ..DynamicConfig::default()
        };
        let mut custom = AiVendorSource {
            kind: "custom".to_string(),
            base_url: "https://gateway.example/v1".to_string(),
            ..AiVendorSource::default()
        };
        custom.settle_credential_mode();
        assert_eq!(
            config.resolve_source_credential(&custom),
            ResolvedAiCredential {
                api_key: None,
                origin: AiCredentialOrigin::None,
            }
        );

        let mut customized_openai = AiVendorSource {
            kind: "openai".to_string(),
            preset: "openai".to_string(),
            base_url: "https://proxy.example/v1".to_string(),
            ..AiVendorSource::default()
        };
        customized_openai.settle_credential_mode();
        assert_eq!(
            config.resolve_source_credential(&customized_openai).api_key,
            None,
            "settling never sends the OpenAI key to a custom endpoint"
        );

        let explicitly_shared = AiVendorSource {
            credential_mode: "shared".to_string(),
            shared_key_ref: Some("openrouter".to_string()),
            ..customized_openai.clone()
        };
        assert_eq!(
            config
                .resolve_source_credential(&explicitly_shared)
                .api_key
                .as_deref(),
            Some("shared-openrouter-key")
        );

        let own = AiVendorSource {
            credential_mode: "own".to_string(),
            shared_key_ref: Some("openrouter".to_string()),
            api_key: Some("source-key".to_string()),
            ..customized_openai
        };
        assert_eq!(
            config.resolve_source_credential(&own).api_key.as_deref(),
            Some("source-key")
        );

        let keyless = AiVendorSource {
            credential_mode: "none".to_string(),
            api_key: Some("stale-source-key".to_string()),
            ..custom
        };
        assert_eq!(config.resolve_source_credential(&keyless).api_key, None);

        let invalid_ref = AiVendorSource {
            credential_mode: "shared".to_string(),
            shared_key_ref: Some("unknown".to_string()),
            ..AiVendorSource::default()
        };
        assert_eq!(
            config.resolve_source_credential(&invalid_ref).origin,
            AiCredentialOrigin::Invalid
        );
    }

    #[test]
    fn text_ai_readiness_requires_a_key_for_own_and_shared_credentials() {
        let source = AiVendorSource {
            slug: "custom".to_string(),
            kind: "custom".to_string(),
            display_name: "Custom".to_string(),
            enabled: true,
            credential_mode: "own".to_string(),
            base_url: "https://gateway.example/v1".to_string(),
            ..AiVendorSource::default()
        };
        let mut config = DynamicConfig {
            ai_source: source.slug.clone(),
            ai_model: "model".to_string(),
            ai_vendor_sources: vec![source],
            ..DynamicConfig::default()
        };

        let resolved = config.resolve_ai_config(ModelTier::Standard);
        assert_eq!(resolved.credential_origin, AiCredentialOrigin::Own);
        assert!(!resolved.text_ready());
        assert!(!config.text_ai_available());

        config.ai_vendor_sources[0].api_key = Some(" source-key ".to_string());
        assert!(config.resolve_ai_config(ModelTier::Standard).text_ready());

        config.ai_vendor_sources[0].credential_mode = "shared".to_string();
        config.ai_vendor_sources[0].shared_key_ref = Some("openai".to_string());
        config.ai_vendor_sources[0].api_key = None;
        assert!(!config.resolve_ai_config(ModelTier::Standard).text_ready());

        config.provider_openai_api_key = Some(" shared-key ".to_string());
        assert!(config.resolve_ai_config(ModelTier::Standard).text_ready());

        config.ai_vendor_sources[0].credential_mode = "none".to_string();
        config.ai_vendor_sources[0].shared_key_ref = None;
        config.provider_openai_api_key = None;
        let resolved = config.resolve_ai_config(ModelTier::Standard);
        assert_eq!(resolved.credential_origin, AiCredentialOrigin::None);
        assert!(resolved.text_ready());
        assert!(config.text_ai_available());
    }

    #[test]
    fn custom_text_sources_require_an_explicit_endpoint() {
        let config = DynamicConfig::default();
        let source = AiVendorSource {
            kind: "custom".to_string(),
            enabled: true,
            credential_mode: "own".to_string(),
            api_key: Some("custom-key".to_string()),
            ..AiVendorSource::default()
        };

        let resolved = config.resolve_from_vendor_source(&source, "model");
        assert!(resolved.base_url.is_empty());
        assert!(!resolved.text_ready());

        let custom_gemini = AiVendorSource {
            api_format: "gemini".to_string(),
            ..source.clone()
        };
        let resolved = config.resolve_from_vendor_source(&custom_gemini, "model");
        assert!(resolved.base_url.is_empty());
        assert!(!resolved.text_ready());

        let compatible = AiVendorSource {
            kind: "openai_compatible".to_string(),
            ..source.clone()
        };
        let resolved = config.resolve_from_vendor_source(&compatible, "model");
        assert!(resolved.base_url.is_empty());
        assert!(!resolved.text_ready());

        let official = AiVendorSource {
            kind: "openai".to_string(),
            ..source
        };
        let resolved = config.resolve_from_vendor_source(&official, "model");
        assert_eq!(resolved.base_url, OPENAI_API_BASE, "the official one");
        assert!(resolved.text_ready());
    }

    /// 「我明明开了 Lite」得能被认出来。
    #[test]
    fn an_enabled_tier_with_no_model_of_its_own_is_a_fallback() {
        let mut config = DynamicConfig::default();
        config.ai_model = "standard-model".into();
        // 出厂默认给 Pro 配了自己的模型，回退只在字段被清空后发生。
        config.pro_ai_model = String::new();

        // 关着不算回退：那是明确的选择。
        assert!(!config.tier_falls_back_to_standard(ModelTier::Pro));

        // Lite 没有开关：模型留空就是不用，不算回退。
        assert!(!config.lite_on());
        assert!(!config.tier_falls_back_to_standard(ModelTier::Lite));

        // 填上自己的模型就用上了。
        config.lite_ai_model = "lite-model".into();
        assert!(config.lite_on());
        assert!(!config.tier_falls_back_to_standard(ModelTier::Lite));
        assert_eq!(
            config.resolve_ai_config(ModelTier::Lite).model,
            "lite-model"
        );

        // Pro 开着但模型留空 —— 配置上写着 Pro，跑的是 Standard。
        config.pro_enabled = true;
        assert!(config.tier_falls_back_to_standard(ModelTier::Pro));
        config.pro_ai_model = "pro-model".into();
        assert!(!config.tier_falls_back_to_standard(ModelTier::Pro));

        // Standard 是回退的目的地，它自己不会回退。
        assert!(!config.tier_falls_back_to_standard(ModelTier::Standard));
    }

    /// Lite 没有开关：默认不用，填了模型才用。
    #[test]
    fn lite_is_on_exactly_when_its_own_model_is_filled() {
        let mut config = DynamicConfig::default();
        assert!(!config.lite_on());
        assert!(config.resolve_strict_lite_ai_config().is_none());

        config.lite_ai_model = " lite-model ".into();
        assert!(config.lite_on());
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
    fn shared_keys_are_the_vaults() {
        let config = DynamicConfig {
            provider_openrouter_api_key: Some("vault-or".to_string()),
            provider_openai_api_key: Some("vault-oa".to_string()),
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
    fn a_tier_is_its_source_and_its_model() {
        let config = DynamicConfig {
            ai_source: "openai".to_string(),
            ai_model: "std/model".to_string(),
            provider_openai_api_key: Some("oa-key".to_string()),
            provider_openrouter_api_key: Some("or-key".to_string()),
            lite_ai_model: "lite/model".to_string(),
            pro_enabled: true,
            pro_ai_source: "openrouter".to_string(),
            pro_ai_model: "pro/model".to_string(),
            ..DynamicConfig::default()
        };
        let standard = config.resolve_ai_config(ModelTier::Standard);
        assert_eq!(standard.api_key.as_deref(), Some("oa-key"));
        assert_eq!(standard.base_url, OPENAI_API_BASE);
        assert_eq!(standard.model, "std/model");
        // Lite without a source of its own runs on Standard's.
        assert_eq!(config.tier_source(ModelTier::Lite), "openai");
        let lite = config.resolve_ai_config(ModelTier::Lite);
        assert_eq!(lite.api_key.as_deref(), Some("oa-key"));
        assert_eq!(lite.model, "lite/model");
        let pro = config.resolve_ai_config(ModelTier::Pro);
        assert_eq!(pro.api_key.as_deref(), Some("or-key"));
        assert!(pro.base_url.contains("openrouter.ai"));
        assert_eq!(pro.model, "pro/model");
        // Pro on with no model: all of Standard, never Standard's model on
        // Pro's source.
        let blank_pro = DynamicConfig {
            pro_ai_model: " ".to_string(),
            ..config.clone()
        };
        let fallback = blank_pro.resolve_ai_config(ModelTier::Pro);
        assert_eq!(fallback.base_url, OPENAI_API_BASE);
        assert_eq!(fallback.model, "std/model");
        // Lite with no model: Standard (only for callers that allow it).
        let blank_lite = DynamicConfig {
            lite_ai_model: String::new(),
            ..config
        };
        assert_eq!(
            blank_lite.resolve_ai_config(ModelTier::Lite).model,
            "std/model"
        );
    }

    #[test]
    fn a_source_since_deleted_resolves_to_nothing_usable() {
        let config = DynamicConfig {
            ai_source: "deleted".to_string(),
            provider_openrouter_api_key: Some("or-key".to_string()),
            ..DynamicConfig::default()
        };
        let resolved = config.resolve_ai_config(ModelTier::Standard);
        assert!(!resolved.text_ready());
        assert_eq!(resolved.api_key, None);
        // A blank Standard source is the factory OpenRouter.
        let blank = DynamicConfig {
            ai_source: String::new(),
            ..config
        };
        assert!(blank.resolve_ai_config(ModelTier::Standard).text_ready());
    }

    #[test]
    fn strict_lite_resolution_never_inherits_the_standard_model() {
        let empty = DynamicConfig {
            lite_ai_model: String::new(),
            ai_model: "std/model".to_string(),
            ..DynamicConfig::default()
        };
        assert!(empty.resolve_strict_lite_ai_config().is_none());

        let configured = DynamicConfig {
            lite_ai_model: "lite/model".to_string(),
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
    fn judgments_use_the_judge_model_on_the_same_lite_credentials() {
        let lite = DynamicConfig {
            lite_ai_model: "google/gemini-3.8-flash".to_string(),
            ..DynamicConfig::default()
        };
        assert_eq!(
            lite.resolve_lite_judge_ai_config().map(|r| r.model),
            Some("google/gemini-3.8-flash".to_string()),
            "blank: judgments use Lite's own model"
        );
        let split = DynamicConfig {
            aux_judge_model: "  openai/gpt-6-luna ".to_string(),
            ..lite.clone()
        };
        let judge = split.resolve_lite_judge_ai_config().unwrap();
        let speak = split.resolve_strict_lite_ai_config().unwrap();
        assert_eq!(judge.model, "openai/gpt-6-luna");
        assert_eq!(speak.model, "google/gemini-3.8-flash");
        assert_eq!(judge.base_url, speak.base_url, "same provider");
        assert_eq!(judge.api_format, speak.api_format);
        let off = DynamicConfig {
            lite_ai_model: String::new(),
            ..split
        };
        assert!(
            off.resolve_lite_judge_ai_config().is_none(),
            "no Lite and no source of their own, no judge"
        );
    }

    #[test]
    fn embeddings_are_off_until_a_model_is_named_on_lite() {
        let lite = DynamicConfig {
            lite_ai_model: "google/gemini-3.8-flash".to_string(),
            ..DynamicConfig::default()
        };
        assert!(lite.resolve_lite_embedding_ai_config().is_none());
        let on = DynamicConfig {
            aux_embedding_model: " perplexity/pplx-embed-v1-0.6b ".to_string(),
            ..lite.clone()
        };
        let embedding = on.resolve_lite_embedding_ai_config().unwrap();
        assert_eq!(embedding.model, "perplexity/pplx-embed-v1-0.6b");
        assert_eq!(
            embedding.base_url,
            on.resolve_strict_lite_ai_config().unwrap().base_url
        );
        let off = DynamicConfig {
            lite_ai_model: String::new(),
            ..on
        };
        assert!(off.resolve_lite_embedding_ai_config().is_none());
    }

    #[test]
    fn judgments_and_embeddings_stay_on_their_own_source_when_lite_moves() {
        let dashscope = AiVendorSource {
            slug: "dashscope".into(),
            kind: "openai_compatible".into(),
            enabled: true,
            credential_mode: "own".into(),
            api_key: Some("ds-key".into()),
            base_url: "https://dashscope-intl.aliyuncs.com/compatible-mode/v1".into(),
            ..AiVendorSource::default()
        };
        let openrouter = AiVendorSource {
            slug: "openrouter".into(),
            kind: "openrouter".into(),
            enabled: true,
            credential_mode: "own".into(),
            api_key: Some("or-key".into()),
            base_url: "https://openrouter.ai/api/v1".into(),
            ..AiVendorSource::default()
        };
        let moved = DynamicConfig {
            lite_ai_model: "qwen3.8-omni-flash".to_string(),
            lite_ai_source: "dashscope".to_string(),
            aux_judge_model: "openai/gpt-6-luna".to_string(),
            aux_embedding_model: "perplexity/pplx-embed-v1-0.6b".to_string(),
            ai_vendor_sources: vec![dashscope, openrouter],
            ..DynamicConfig::default()
        };
        // Never chosen: Lite's source of now, as before.
        assert_eq!(moved.aux_source_slug(), "dashscope");
        let follows = moved.resolve_lite_judge_ai_config().unwrap();
        assert!(follows.base_url.contains("dashscope"));
        // Their own source: Lite moved, they did not.
        let own = DynamicConfig {
            aux_ai_source: "openrouter".to_string(),
            ..moved.clone()
        };
        let judge = own.resolve_lite_judge_ai_config().unwrap();
        assert!(judge.base_url.contains("openrouter"), "{}", judge.base_url);
        assert_eq!(judge.model, "openai/gpt-6-luna");
        let embedding = own.resolve_lite_embedding_ai_config().unwrap();
        assert!(embedding.base_url.contains("openrouter"));
        assert_eq!(embedding.model, "perplexity/pplx-embed-v1-0.6b");
        // Their source turned off or gone: judgments on Lite's own model
        // (never its name for the other provider), no embeddings.
        let mut off_source = own.clone();
        off_source.ai_vendor_sources[1].enabled = false;
        let fallback = off_source.resolve_lite_judge_ai_config().unwrap();
        assert_eq!(fallback.model, "qwen3.8-omni-flash");
        assert!(fallback.base_url.contains("dashscope"));
        assert!(off_source.resolve_lite_embedding_ai_config().is_none());
        let gone = DynamicConfig {
            aux_ai_source: "deleted".to_string(),
            ..own.clone()
        };
        assert_eq!(
            gone.resolve_lite_judge_ai_config().unwrap().model,
            "qwen3.8-omni-flash"
        );
        // Speaking stays on Lite; a blank judge model is Lite's own.
        assert!(
            own.resolve_strict_lite_ai_config()
                .unwrap()
                .base_url
                .contains("dashscope")
        );
        let blank = DynamicConfig {
            aux_judge_model: String::new(),
            ..own.clone()
        };
        assert_eq!(
            blank.resolve_lite_judge_ai_config().unwrap().model,
            "qwen3.8-omni-flash"
        );
        // Their own option, not Lite's: with Lite off they still run on
        // their source; only a judge model left blank (Lite's own) stops.
        let off = DynamicConfig {
            lite_ai_model: String::new(),
            ..own
        };
        let judge = off.resolve_lite_judge_ai_config().unwrap();
        assert!(judge.base_url.contains("openrouter"));
        assert_eq!(judge.model, "openai/gpt-6-luna");
        assert!(
            off.resolve_lite_embedding_ai_config()
                .unwrap()
                .base_url
                .contains("openrouter")
        );
        let blank_off = DynamicConfig {
            aux_judge_model: String::new(),
            ..off.clone()
        };
        assert!(blank_off.resolve_lite_judge_ai_config().is_none());
        // Never chosen with Lite off: the source the page shows, Lite's.
        let never_off = DynamicConfig {
            aux_ai_source: String::new(),
            ..off.clone()
        };
        assert!(
            never_off
                .resolve_lite_judge_ai_config()
                .unwrap()
                .base_url
                .contains("dashscope")
        );
        // Their source unusable and no Lite to fall back to: nothing.
        let mut nothing = off.clone();
        nothing.ai_vendor_sources[1].enabled = false;
        assert!(nothing.resolve_lite_judge_ai_config().is_none());
        assert!(nothing.resolve_lite_embedding_ai_config().is_none());
    }

    #[test]
    fn text_ai_available_follows_resolved_standard_not_raw_fields() {
        let missing = DynamicConfig::default();
        assert!(!missing.text_ai_available());

        let vault_only = DynamicConfig {
            ai_source: "openai".to_string(),
            provider_openai_api_key: Some("vault-oa".to_string()),
            ..DynamicConfig::default()
        };
        assert!(vault_only.text_ai_available());

        let keyless_custom = DynamicConfig {
            ai_source: "local".to_string(),
            ai_model: "local-model".to_string(),
            ai_vendor_sources: vec![AiVendorSource {
                slug: "local".to_string(),
                kind: "custom".to_string(),
                display_name: "Local".to_string(),
                enabled: true,
                api_format: String::new(),
                credential_mode: "none".to_string(),
                base_url: "http://127.0.0.1:11434/v1".to_string(),
                ..AiVendorSource::default()
            }],
            ..DynamicConfig::default()
        };
        assert!(keyless_custom.text_ai_available());

        let invalid_format = DynamicConfig {
            ai_source: "invalid".to_string(),
            ai_model: "model".to_string(),
            ai_vendor_sources: vec![AiVendorSource {
                slug: "invalid".to_string(),
                kind: "custom".to_string(),
                display_name: "Invalid".to_string(),
                enabled: true,
                api_format: "future_protocol".to_string(),
                base_url: "https://llm.example/v1".to_string(),
                ..AiVendorSource::default()
            }],
            ..DynamicConfig::default()
        };
        assert!(!invalid_format.text_ai_available());
    }

    #[test]
    fn gemini_grounding_uses_shared_key_even_when_text_tier_is_openai() {
        let config = DynamicConfig {
            ai_source: "openai".to_string(),
            provider_gemini_api_key: Some("vault-gm".to_string()),
            lite_ai_source: "gemini".to_string(),
            lite_ai_model: "gemini-lite".to_string(),
            ..DynamicConfig::default()
        };
        let (key, model) = config.resolve_gemini_grounding().expect("shared gemini");
        assert_eq!(key, "vault-gm");
        assert_eq!(model, "gemini-lite", "the first tier on Gemini");
        assert!(!config.text_ai_available());
        let none_on_gemini = DynamicConfig {
            lite_ai_model: String::new(),
            ..config
        };
        assert_eq!(
            none_on_gemini.resolve_gemini_grounding().unwrap().1,
            "gemini-3.8-flash"
        );
    }

    #[test]
    fn tinyfish_key_is_independent_of_gemini() {
        let config = DynamicConfig {
            provider_tinyfish_api_key: Some(" tf-key ".to_string()),
            ..DynamicConfig::default()
        };
        assert_eq!(config.shared_tinyfish_api_key().as_deref(), Some("tf-key"));
        assert!(config.resolve_gemini_grounding().is_none());
    }
}
