use axum::{extract::State, http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigResponse {
    pub platforms: Vec<PlatformConfig>,
    pub auto_fetch: Option<PlatformAutoFetchConfig>,
    pub ai_config: AiConfig,
    pub report_config: ReportConfig,
    pub ui_config: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PlatformAutoFetchConfig {
    pub enabled: bool,
    pub interval_hours: i32,
}

impl Default for PlatformAutoFetchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_hours: 24,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct PlatformConfig {
    pub name: String,
    pub enabled: bool,
    pub has_token: bool,
    pub config_fields: Vec<ConfigField>,
    pub description: String,
    pub icon: String,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct ConfigField {
    pub key: String,
    pub label: String,
    pub field_type: String,
    pub value: String,
    pub placeholder: String,
    pub required: bool,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    pub provider: String,
    pub model: String,
    pub api_key: String,
    pub enabled: bool,
    // AI 图片生成配置
    pub image_provider: String,
    pub config_fields: Vec<ConfigField>,
}
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ReportConfig {
    pub topic_style: String,
    pub config_fields: Vec<ConfigField>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub wallpaper_url: String,
    pub wallpaper_blur: u32,
    pub wallpaper_parallax: bool,
    // Evocative 壁纸动效
    pub evocative_parallax: bool,
    pub evocative_dynamic_blur: bool,
    pub evocative_ripple: bool,
    pub evocative_fps: u32,
    pub evocative_ripple_quality: f64,
    pub theme: String,
    pub primary_color: String,
    pub secondary_color: String,
    pub pet_enabled: bool,
    pub pet_image_url: String,
    // 网络代理配置
    pub proxy_enabled: bool,
    pub proxy_url: String,
    pub proxy_bypass: String,
    pub gemini_base_url: String,
    pub github_api_base_url: String,
    pub config_fields: Vec<ConfigField>,
}

fn resolve_platform_enabled(explicit_enabled: Option<bool>, fallback_enabled: bool) -> bool {
    explicit_enabled.unwrap_or(fallback_enabled)
}

/// 按管理员配置的平台顺序对平台列表排序。
/// `order` 中的平台按其顺序排在前面，未列出的平台保持原有默认顺序排在最后。
fn sort_platforms_by_order(platforms: &mut [PlatformConfig], order: Option<&Vec<String>>) {
    let Some(order) = order else { return };
    let rank = |name: &str| -> usize {
        order
            .iter()
            .position(|n| n.eq_ignore_ascii_case(name))
            .unwrap_or(usize::MAX)
    };
    // 稳定排序：未列出的平台（rank == MAX）保持彼此间的默认相对顺序
    platforms.sort_by_key(|p| rank(&p.name));
}

async fn build_config(db: &DatabaseConnection, reveal_sensitive: bool) -> ConfigResponse {
    let db = db.clone();
    // 优先从数据库读取配置
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let db_config = config_service.load_config().await.ok();

    // 辅助函数：优先使用数据库值，否则使用环境变量
    // Helper to get string value from database or environment
    let get_value = |db_val: Option<String>, env_key: &str| -> String {
        db_val
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| std::env::var(env_key).unwrap_or_default())
    };

    // Helper to mask sensitive values (passwords, API keys, tokens)
    // SECURITY: Do not expose any real characters to prevent key type detection
    let mask_sensitive = |value: String| -> String {
        if value.is_empty() || reveal_sensitive {
            value
        } else {
            // Show only fixed-length mask without exposing real characters
            "••••••••".to_string()
        }
    };

    let has_bangumi_username = db_config
        .as_ref()
        .and_then(|c| c.bangumi_username.as_ref())
        .is_some()
        || std::env::var("BANGUMI_USERNAME").is_ok();
    let has_bangumi_access_token = db_config
        .as_ref()
        .and_then(|c| c.bangumi_access_token.as_ref())
        .is_some()
        || std::env::var("BANGUMI_ACCESS_TOKEN").is_ok();
    let has_bangumi_identity = has_bangumi_username || has_bangumi_access_token;
    let github_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.github_enabled),
        db_config
            .as_ref()
            .and_then(|c| c.github_username.as_ref())
            .is_some()
            || std::env::var("GITHUB_USERNAME").is_ok(),
    );
    let bilibili_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.bilibili_enabled),
        db_config
            .as_ref()
            .and_then(|c| c.bilibili_uid.as_ref())
            .is_some()
            || std::env::var("BILIBILI_UID").is_ok(),
    );
    let steam_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.steam_enabled),
        db_config
            .as_ref()
            .and_then(|c| c.steam_api_key.as_ref())
            .is_some()
            || std::env::var("STEAM_API_KEY").is_ok(),
    );
    let netease_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.netease_enabled),
        db_config
            .as_ref()
            .and_then(|c| c.netease_user_id.as_ref())
            .is_some()
            || std::env::var("NETEASE_USER_ID").is_ok(),
    );
    let bangumi_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.bangumi_enabled),
        has_bangumi_identity,
    );
    let has_x_username = db_config
        .as_ref()
        .and_then(|c| c.x_username.as_ref())
        .is_some()
        || std::env::var("X_USERNAME").is_ok();
    let has_x_bearer = db_config
        .as_ref()
        .and_then(|c| c.x_bearer_token.as_ref())
        .is_some()
        || std::env::var("X_BEARER_TOKEN").is_ok();
    let x_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.x_enabled),
        has_x_username && has_x_bearer,
    );
    let has_discord_token = db_config
        .as_ref()
        .and_then(|c| c.discord_access_token.as_ref())
        .is_some()
        || std::env::var("DISCORD_ACCESS_TOKEN").is_ok();
    let discord_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.discord_enabled),
        has_discord_token,
    );
    let has_mal_username = db_config
        .as_ref()
        .and_then(|c| c.mal_username.as_ref())
        .is_some()
        || std::env::var("MAL_USERNAME").is_ok();
    let mal_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.mal_enabled),
        has_mal_username,
    );
    let has_openxbl_key = db_config
        .as_ref()
        .and_then(|c| c.openxbl_api_key.as_ref())
        .is_some()
        || std::env::var("OPENXBL_API_KEY").is_ok()
        || std::env::var("XBL_API_KEY").is_ok();
    let has_xbox_gamertag = db_config
        .as_ref()
        .and_then(|c| c.xbox_gamertag.as_ref())
        .is_some()
        || std::env::var("XBOX_GAMERTAG").is_ok();
    let xbox_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.xbox_enabled),
        has_openxbl_key && has_xbox_gamertag,
    );
    let has_psn_npsso = db_config
        .as_ref()
        .and_then(|c| c.psn_npsso.as_ref())
        .is_some()
        || std::env::var("PSN_NPSSO").is_ok();
    let has_psn_online_id = db_config
        .as_ref()
        .and_then(|c| c.psn_online_id.as_ref())
        .is_some()
        || std::env::var("PSN_ONLINE_ID").is_ok();
    let psn_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.psn_enabled),
        has_psn_npsso && has_psn_online_id,
    );

    let config = ConfigResponse {
        platforms: vec![
            PlatformConfig {
                name: "GitHub".to_string(),
                enabled: github_enabled,
                has_token: db_config
                    .as_ref()
                    .and_then(|c| c.github_token.as_ref())
                    .is_some()
                    || std::env::var("GITHUB_TOKEN").is_ok(),
                icon: "".to_string(),
                description: "Track repositories, stars, and contributions".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "GitHub Username".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.github_username.clone()),
                            "GITHUB_USERNAME",
                        ),
                        placeholder: "octocat".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "token".to_string(),
                        label: "Personal Access Token (Optional)".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config.as_ref().and_then(|c| c.github_token.clone()),
                            "GITHUB_TOKEN",
                        )),
                        placeholder: "ghp_xxxxxxxxxxxx (Increases API rate limit)".to_string(),
                        required: false,
                    },
                ],
            },
            PlatformConfig {
                name: "Bilibili".to_string(),
                enabled: bilibili_enabled,
                has_token: db_config
                    .as_ref()
                    .and_then(|c| c.bilibili_uid.as_ref())
                    .is_some()
                    || std::env::var("BILIBILI_UID").is_ok(),
                icon: "".to_string(),
                description: "Track your Bilibili favorites, anime, and viewing history"
                    .to_string(),
                config_fields: vec![ConfigField {
                    key: "uid".to_string(),
                    label: "User ID (UID)".to_string(),
                    field_type: "number".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.bilibili_uid.clone()),
                        "BILIBILI_UID",
                    ),
                    placeholder: "123456789".to_string(),
                    required: true,
                }],
            },
            PlatformConfig {
                name: "Steam".to_string(),
                enabled: steam_enabled,
                has_token: db_config
                    .as_ref()
                    .and_then(|c| c.steam_api_key.as_ref())
                    .is_some()
                    || std::env::var("STEAM_API_KEY").is_ok(),
                icon: "".to_string(),
                description: "Sync your Steam library, wishlist, and gaming stats".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "api_key".to_string(),
                        label: "Steam API Key".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config.as_ref().and_then(|c| c.steam_api_key.clone()),
                            "STEAM_API_KEY",
                        )),
                        placeholder: "Get from steamcommunity.com/dev/apikey".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "steam_id".to_string(),
                        label: "Steam ID".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.steam_id.clone()),
                            "STEAM_ID",
                        ),
                        placeholder: "76561198XXXXXXXXX".to_string(),
                        required: true,
                    },
                ],
            },
            PlatformConfig {
                name: "Netease Music".to_string(),
                enabled: netease_enabled,
                has_token: db_config
                    .as_ref()
                    .and_then(|c| c.netease_user_id.as_ref())
                    .is_some()
                    || std::env::var("NETEASE_USER_ID").is_ok(),
                icon: "".to_string(),
                description: "Sync your liked songs and music taste from Netease Cloud Music"
                    .to_string(),
                config_fields: vec![ConfigField {
                    key: "user_id".to_string(),
                    label: "User ID".to_string(),
                    field_type: "number".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.netease_user_id.clone()),
                        "NETEASE_USER_ID",
                    ),
                    placeholder: "Your Netease Cloud Music user ID".to_string(),
                    required: true,
                }],
            },
            PlatformConfig {
                name: "Bangumi".to_string(),
                enabled: bangumi_enabled,
                has_token: has_bangumi_identity,
                icon: "".to_string(),
                description: "Sync your Bangumi collection, ratings, and watching status"
                    .to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "Bangumi Username".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.bangumi_username.clone()),
                            "BANGUMI_USERNAME",
                        ),
                        placeholder: "your Bangumi username".to_string(),
                        required: false,
                    },
                    ConfigField {
                        key: "access_token".to_string(),
                        label: "Access Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config
                                .as_ref()
                                .and_then(|c| c.bangumi_access_token.clone()),
                            "BANGUMI_ACCESS_TOKEN",
                        )),
                        placeholder: "Bearer token for private collections".to_string(),
                        required: false,
                    },
                    ConfigField {
                        key: "user_agent".to_string(),
                        label: "User-Agent".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config
                                .as_ref()
                                .and_then(|c| c.bangumi_user_agent.clone()),
                            "BANGUMI_USER_AGENT",
                        ),
                        placeholder: "haru/Myriad".to_string(),
                        required: false,
                    },
                ],
            },
            PlatformConfig {
                name: "X".to_string(),
                enabled: x_enabled,
                has_token: has_x_bearer,
                icon: "".to_string(),
                description: "Sync your X profile and posts (read-only); share via Web Intent"
                    .to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "X Username".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.x_username.clone()),
                            "X_USERNAME",
                        ),
                        placeholder: String::new(),
                        required: true,
                    },
                    ConfigField {
                        key: "bearer_token".to_string(),
                        label: "Bearer Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config.as_ref().and_then(|c| c.x_bearer_token.clone()),
                            "X_BEARER_TOKEN",
                        )),
                        placeholder: "From developer.x.com App keys (read-only sync)".to_string(),
                        required: true,
                    },
                ],
            },
            PlatformConfig {
                name: "Discord".to_string(),
                enabled: discord_enabled,
                has_token: has_discord_token,
                icon: "".to_string(),
                description:
                    "Sync your Discord profile, server footprint, and linked accounts (Steam/GitHub/…)"
                        .to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "access_token".to_string(),
                        label: "Access Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config
                                .as_ref()
                                .and_then(|c| c.discord_access_token.clone()),
                            "DISCORD_ACCESS_TOKEN",
                        )),
                        placeholder:
                            "OAuth user token (scopes: identify guilds connections)".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "refresh_token".to_string(),
                        label: "Refresh Token (Recommended)".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config
                                .as_ref()
                                .and_then(|c| c.discord_refresh_token.clone()),
                            "DISCORD_REFRESH_TOKEN",
                        )),
                        placeholder:
                            "Optional; enables auto-refresh when access token expires".to_string(),
                        required: false,
                    },
                    ConfigField {
                        key: "user_id".to_string(),
                        label: "User ID (auto-filled after test)".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.discord_user_id.clone()),
                            "DISCORD_USER_ID",
                        ),
                        placeholder: "Discord snowflake id".to_string(),
                        required: false,
                    },
                ],
            },
            PlatformConfig {
                name: "MyAnimeList".to_string(),
                enabled: mal_enabled,
                has_token: has_mal_username,
                icon: "".to_string(),
                description: "Username required; optional Client ID uses official API (else public load.json)"
                    .to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "MyAnimeList Username".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.mal_username.clone()),
                            "MAL_USERNAME",
                        ),
                        placeholder: "your MAL username (required)".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "client_id".to_string(),
                        label: "Client ID (optional)".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config.as_ref().and_then(|c| c.mal_client_id.clone()),
                            "MAL_CLIENT_ID",
                        )),
                        placeholder:
                            "Optional — leave empty for public list (load.json); fill for official API (myanimelist.net/apiconfig)"
                                .to_string(),
                        required: false,
                    },
                ],
            },
            PlatformConfig {
                name: "Xbox".to_string(),
                enabled: xbox_enabled,
                has_token: has_openxbl_key,
                icon: "".to_string(),
                description:
                    "Sync your Xbox achievements, Gamerscore, and recently played titles"
                        .to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "gamertag".to_string(),
                        label: "Gamertag".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.xbox_gamertag.clone()),
                            "XBOX_GAMERTAG",
                        ),
                        placeholder: "Major Nelson 或 名字#1234".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "openxbl_api_key".to_string(),
                        label: "OpenXBL API Key".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config.as_ref().and_then(|c| c.openxbl_api_key.clone()),
                            "OPENXBL_API_KEY",
                        )),
                        placeholder: "From xbl.io profile".to_string(),
                        required: true,
                    },
                ],
            },
            PlatformConfig {
                name: "PlayStation".to_string(),
                enabled: psn_enabled,
                has_token: has_psn_npsso,
                icon: "".to_string(),
                description: "Sync your PSN trophies, trophy level, and recently played titles"
                    .to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "online_id".to_string(),
                        label: "Online ID".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.psn_online_id.clone()),
                            "PSN_ONLINE_ID",
                        ),
                        placeholder: "Your PSN Online ID".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "npsso".to_string(),
                        label: "NPSSO Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config.as_ref().and_then(|c| c.psn_npsso.clone()),
                            "PSN_NPSSO",
                        )),
                        placeholder: "64-char token from ca.account.sony.com".to_string(),
                        required: true,
                    },
                ],
            },
        ],
        auto_fetch: Some(PlatformAutoFetchConfig {
            enabled: db_config
                .as_ref()
                .is_some_and(|config| config.enable_auto_fetch),
            interval_hours: crate::services::platform_auto_refresh::clamp_interval_hours(
                db_config
                    .as_ref()
                    .map(|config| config.fetch_interval_hours)
                    .unwrap_or(24),
            ),
        }),
        ai_config: AiConfig {
            provider: db_config
                .as_ref()
                .map(|c| c.ai_provider.clone())
                .unwrap_or_else(|| {
                    std::env::var("AI_PROVIDER").unwrap_or_else(|_| "gemini".to_string())
                }),
            model: db_config
                .as_ref()
                .map(|c| c.gemini_model.clone())
                .unwrap_or_else(|| {
                    std::env::var("GEMINI_MODEL")
                        .unwrap_or_else(|_| "gemini-3-flash-preview".to_string())
                }),
            api_key: get_value(
                db_config.as_ref().and_then(|c| c.gemini_api_key.clone()),
                "GEMINI_API_KEY",
            ),
            enabled: db_config
                .as_ref()
                .and_then(|c| c.gemini_api_key.as_ref())
                .is_some()
                || db_config
                    .as_ref()
                    .and_then(|c| c.openai_api_key.as_ref())
                    .is_some()
                || std::env::var("GEMINI_API_KEY").is_ok()
                || std::env::var("OPENAI_API_KEY").is_ok(),
            config_fields: vec![
                ConfigField {
                    key: "provider".to_string(),
                    label: "AI Provider".to_string(),
                    field_type: "select".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ai_provider.clone())
                        .unwrap_or_else(|| {
                            std::env::var("AI_PROVIDER").unwrap_or_else(|_| "openai".to_string())
                        }),
                    placeholder: "openai".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "gemini_api_key".to_string(),
                    label: "Gemini API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config.as_ref().and_then(|c| c.gemini_api_key.clone()),
                        "GEMINI_API_KEY",
                    )),
                    placeholder: "Get from https://makersuite.google.com/app/apikey".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "gemini_model".to_string(),
                    label: "Gemini Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.gemini_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("GEMINI_MODEL")
                                .unwrap_or_else(|_| "gemini-3.5-flash".to_string())
                        }),
                    placeholder: "gemini-3.5-flash, gemini-3.1-pro-preview, gemini-2.5-flash, etc."
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "openai_api_key".to_string(),
                    label: "OpenAI API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config.as_ref().and_then(|c| c.openai_api_key.clone()),
                        "OPENAI_API_KEY",
                    )),
                    placeholder: "OpenAI API Key or compatible service key".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "openai_model".to_string(),
                    label: "OpenAI Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.openai_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("OPENAI_MODEL")
                                .unwrap_or_else(|_| "minimax/minimax-m3".to_string())
                        }),
                    placeholder: "minimax/minimax-m3, gpt-5.5, etc.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "openai_base_url".to_string(),
                    label: "OpenAI Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.openai_base_url.clone())
                        .unwrap_or_else(|| {
                            std::env::var("OPENAI_BASE_URL")
                                .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string())
                        }),
                    placeholder:
                        "https://openrouter.ai/api/v1 (base URL only, no /chat/completions)"
                            .to_string(),
                    required: false,
                },
                // Pro 模型配置
                ConfigField {
                    key: "pro_enabled".to_string(),
                    label: "Enable Pro Model".to_string(),
                    field_type: "boolean".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.pro_enabled.to_string())
                        .unwrap_or_else(|| "false".to_string()),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pro_provider".to_string(),
                    label: "【Pro Model】AI Provider".to_string(),
                    field_type: "select".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.pro_ai_provider.clone())
                        .unwrap_or_else(|| {
                            std::env::var("PRO_AI_PROVIDER")
                                .unwrap_or_else(|_| "openai".to_string())
                        }),
                    placeholder: "openai".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "pro_gemini_api_key".to_string(),
                    label: "【Pro Model】Gemini API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.pro_gemini_api_key.clone()),
                        "PRO_GEMINI_API_KEY",
                    )),
                    placeholder: "Pro model Gemini API Key (leave empty to reuse standard)"
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pro_gemini_model".to_string(),
                    label: "【Pro Model】Gemini Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.pro_gemini_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("PRO_GEMINI_MODEL")
                                .unwrap_or_else(|_| "gemini-3.1-pro-preview".to_string())
                        }),
                    placeholder: "gemini-3.1-pro-preview, gemini-3-flash-preview, etc.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pro_openai_api_key".to_string(),
                    label: "【Pro Model】OpenAI API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.pro_openai_api_key.clone()),
                        "PRO_OPENAI_API_KEY",
                    )),
                    placeholder: "Pro model OpenAI API Key (leave empty to reuse standard)"
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pro_openai_model".to_string(),
                    label: "【Pro Model】OpenAI Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.pro_openai_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("PRO_OPENAI_MODEL")
                                .unwrap_or_else(|_| "anthropic/claude-opus-4.8".to_string())
                        }),
                    placeholder: "anthropic/claude-opus-4.8, gpt-5.5, etc.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pro_openai_base_url".to_string(),
                    label: "【Pro Model】OpenAI Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.pro_openai_base_url.clone())
                        .unwrap_or_else(|| {
                            std::env::var("PRO_OPENAI_BASE_URL")
                                .unwrap_or_else(|_| "https://openrouter.ai/api/v1".to_string())
                        }),
                    placeholder: "https://api.openai.com/v1 (leave empty to reuse standard)"
                        .to_string(),
                    required: false,
                },
                // AI 图片生成配置
                ConfigField {
                    key: "ai_image_provider".to_string(),
                    label: "Image Provider".to_string(),
                    field_type: "select".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ai_image_provider.clone())
                        .unwrap_or_else(|| {
                            std::env::var("AI_IMAGE_PROVIDER")
                                .unwrap_or_else(|_| "pollinations".to_string())
                        }),
                    placeholder: "pollinations or pixai".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_model".to_string(),
                    label: "Image Model (Pollinations)".to_string(),
                    field_type: "select".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ai_image_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("AI_IMAGE_MODEL")
                                .unwrap_or_else(|_| "1983308862240288769".to_string())
                        }),
                    placeholder: "1983308862240288769".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_width".to_string(),
                    label: "Image Width".to_string(),
                    field_type: "number".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ai_image_width.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("AI_IMAGE_WIDTH").unwrap_or_else(|_| "768".to_string())
                        }),
                    placeholder: "768".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_height".to_string(),
                    label: "Image Height".to_string(),
                    field_type: "number".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ai_image_height.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("AI_IMAGE_HEIGHT").unwrap_or_else(|_| "1280".to_string())
                        }),
                    placeholder: "1280".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pixai_api_key".to_string(),
                    label: "PixAI API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config.as_ref().and_then(|c| c.pixai_api_key.clone()),
                        "PIXAI_API_KEY",
                    )),
                    placeholder: "Get from platform.pixai.art".to_string(),
                    required: false,
                },
                // 腾讯云语音服务配置 (TTS/ASR)
                ConfigField {
                    key: "tencent_secret_id".to_string(),
                    label: "Tencent Cloud Secret ID".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config.as_ref().and_then(|c| c.tencent_secret_id.clone()),
                        "TENCENT_SECRET_ID",
                    )),
                    placeholder: "Get from https://console.cloud.tencent.com/cam/capi".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "tencent_secret_key".to_string(),
                    label: "Tencent Cloud Secret Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.tencent_secret_key.clone()),
                        "TENCENT_SECRET_KEY",
                    )),
                    placeholder: "Keep this secret secure".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "tencent_region".to_string(),
                    label: "Tencent Cloud Region".to_string(),
                    field_type: "select".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.tencent_region.clone()),
                        "TENCENT_REGION",
                    ),
                    placeholder: "ap-guangzhou".to_string(),
                    required: false,
                },
            ],
            image_provider: db_config
                .as_ref()
                .map(|c| c.ai_image_provider.clone())
                .unwrap_or_else(|| {
                    std::env::var("AI_IMAGE_PROVIDER")
                        .unwrap_or_else(|_| "pollinations".to_string())
                }),
        },
        report_config: ReportConfig {
            topic_style: db_config
                .as_ref()
                .map(|c| c.topic_style.clone())
                .unwrap_or_else(|| {
                    std::env::var("TOPIC_STYLE").unwrap_or_else(|_| "balanced".to_string())
                }),
            config_fields: vec![ConfigField {
                key: "topic_style".to_string(),
                label: "Report Topic Style".to_string(),
                field_type: "select".to_string(),
                value: db_config
                    .as_ref()
                    .map(|c| c.topic_style.clone())
                    .unwrap_or_else(|| {
                        std::env::var("TOPIC_STYLE").unwrap_or_else(|_| "balanced".to_string())
                    }),
                placeholder: "balanced".to_string(),
                required: true,
            }],
        },
        ui_config: UiConfig {
            wallpaper_url: get_value(
                db_config.as_ref().and_then(|c| c.ui_wallpaper_url.clone()),
                "UI_WALLPAPER_URL",
            ),
            wallpaper_blur: db_config
                .as_ref()
                .map(|c| c.ui_wallpaper_blur as u32)
                .unwrap_or_else(|| {
                    std::env::var("UI_WALLPAPER_BLUR")
                        .unwrap_or_else(|_| "3".to_string())
                        .parse::<u32>()
                        .unwrap_or(3)
                }),
            wallpaper_parallax: db_config
                .as_ref()
                .map(|c| c.ui_wallpaper_parallax)
                .unwrap_or_else(|| {
                    std::env::var("UI_WALLPAPER_PARALLAX")
                        .unwrap_or_else(|_| "true".to_string())
                        .parse()
                        .unwrap_or(true)
                }),
            // Evocative 壁纸动效
            evocative_parallax: db_config
                .as_ref()
                .map(|c| c.ui_evocative_parallax)
                .unwrap_or_else(|| {
                    std::env::var("UI_EVOCATIVE_PARALLAX")
                        .unwrap_or_else(|_| "true".to_string())
                        .parse()
                        .unwrap_or(true)
                }),
            evocative_dynamic_blur: db_config
                .as_ref()
                .map(|c| c.ui_evocative_dynamic_blur)
                .unwrap_or_else(|| {
                    std::env::var("UI_EVOCATIVE_DYNAMIC_BLUR")
                        .unwrap_or_else(|_| "false".to_string())
                        .parse()
                        .unwrap_or(false)
                }),
            evocative_ripple: db_config
                .as_ref()
                .map(|c| c.ui_evocative_ripple)
                .unwrap_or_else(|| {
                    std::env::var("UI_EVOCATIVE_RIPPLE")
                        .unwrap_or_else(|_| "false".to_string())
                        .parse()
                        .unwrap_or(false)
                }),
            evocative_fps: db_config
                .as_ref()
                .map(|c| c.ui_evocative_fps as u32)
                .unwrap_or_else(|| {
                    std::env::var("UI_EVOCATIVE_FPS")
                        .unwrap_or_else(|_| "30".to_string())
                        .parse()
                        .unwrap_or(30)
                }),
            evocative_ripple_quality: db_config
                .as_ref()
                .map(|c| c.ui_evocative_ripple_quality)
                .unwrap_or_else(|| {
                    std::env::var("UI_EVOCATIVE_RIPPLE_QUALITY")
                        .unwrap_or_else(|_| "0.85".to_string())
                        .parse()
                        .unwrap_or(0.85)
                }),
            theme: db_config
                .as_ref()
                .and_then(|c| c.ui_theme.clone())
                .unwrap_or_else(|| {
                    std::env::var("UI_THEME").unwrap_or_else(|_| "dark".to_string())
                }),
            primary_color: db_config
                .as_ref()
                .and_then(|c| c.ui_primary_color.clone())
                .unwrap_or_else(|| {
                    std::env::var("UI_PRIMARY_COLOR").unwrap_or_else(|_| "#6366f1".to_string())
                }),
            secondary_color: db_config
                .as_ref()
                .and_then(|c| c.ui_secondary_color.clone())
                .unwrap_or_else(|| {
                    std::env::var("UI_SECONDARY_COLOR").unwrap_or_else(|_| "#8b5cf6".to_string())
                }),
            pet_enabled: db_config
                .as_ref()
                .map(|c| c.pet_enabled)
                .unwrap_or_else(|| {
                    std::env::var("PET_ENABLED")
                        .unwrap_or_else(|_| "true".to_string())
                        .parse()
                        .unwrap_or(true)
                }),
            pet_image_url: get_value(
                db_config.as_ref().and_then(|c| c.pet_image_url.clone()),
                "PET_IMAGE_URL",
            ),
            // 网络代理配置
            proxy_enabled: db_config.as_ref().map(|c| c.proxy_enabled).unwrap_or(false),
            proxy_url: db_config
                .as_ref()
                .and_then(|c| c.proxy_url.clone())
                .unwrap_or_default(),
            proxy_bypass: db_config
                .as_ref()
                .and_then(|c| c.proxy_bypass.clone())
                .unwrap_or_default(),
            gemini_base_url: db_config
                .as_ref()
                .and_then(|c| c.gemini_base_url.clone())
                .unwrap_or_default(),
            github_api_base_url: db_config
                .as_ref()
                .and_then(|c| c.github_api_base_url.clone())
                .unwrap_or_default(),
            config_fields: vec![
                ConfigField {
                    key: "wallpaper_url".to_string(),
                    label: "Wallpaper URL".to_string(),
                    field_type: "text".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.ui_wallpaper_url.clone()),
                        "UI_WALLPAPER_URL",
                    ),
                    placeholder: "URL to wallpaper image or API endpoint".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "wallpaper_blur".to_string(),
                    label: "Wallpaper Blur (0-10)".to_string(),
                    field_type: "number".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ui_wallpaper_blur.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("UI_WALLPAPER_BLUR").unwrap_or_else(|_| "3".to_string())
                        }),
                    placeholder: "3".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "wallpaper_parallax".to_string(),
                    label: "壁纸视差效果".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ui_wallpaper_parallax.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("UI_WALLPAPER_PARALLAX")
                                .unwrap_or_else(|_| "true".to_string())
                        }),
                    placeholder: "true".to_string(),
                    required: false,
                },
                // Evocative 壁纸动效配置
                ConfigField {
                    key: "evocative_parallax".to_string(),
                    label: "微动效果".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ui_evocative_parallax.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("UI_EVOCATIVE_PARALLAX")
                                .unwrap_or_else(|_| "true".to_string())
                        }),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "evocative_dynamic_blur".to_string(),
                    label: "动态模糊".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ui_evocative_dynamic_blur.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("UI_EVOCATIVE_DYNAMIC_BLUR")
                                .unwrap_or_else(|_| "false".to_string())
                        }),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "evocative_ripple".to_string(),
                    label: "涟漪效果".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ui_evocative_ripple.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("UI_EVOCATIVE_RIPPLE")
                                .unwrap_or_else(|_| "false".to_string())
                        }),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "evocative_fps".to_string(),
                    label: "动效帧率".to_string(),
                    field_type: "select".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ui_evocative_fps.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("UI_EVOCATIVE_FPS").unwrap_or_else(|_| "30".to_string())
                        }),
                    placeholder: "30".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "evocative_ripple_quality".to_string(),
                    label: "涟漪画质".to_string(),
                    field_type: "select".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ui_evocative_ripple_quality.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("UI_EVOCATIVE_RIPPLE_QUALITY")
                                .unwrap_or_else(|_| "0.85".to_string())
                        }),
                    placeholder: "0.85".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pet_enabled".to_string(),
                    label: "Enable Pet Mascot".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.pet_enabled.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("PET_ENABLED").unwrap_or_else(|_| "true".to_string())
                        }),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pet_image_url".to_string(),
                    label: "Pet Image URL".to_string(),
                    field_type: "text".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.pet_image_url.clone()),
                        "PET_IMAGE_URL",
                    ),
                    placeholder: "URL to pet character image".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_title".to_string(),
                    label: "网站标题".to_string(),
                    field_type: "text".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.site_title.clone()),
                        "SITE_TITLE",
                    ),
                    placeholder: "Myriad - A myriad of lights, in one place.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_description".to_string(),
                    label: "网站描述".to_string(),
                    field_type: "text".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.site_description.clone()),
                        "SITE_DESCRIPTION",
                    ),
                    placeholder: "A myriad of lights, in one place.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_favicon".to_string(),
                    label: "网站图标 URL".to_string(),
                    field_type: "text".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.site_favicon.clone()),
                        "SITE_FAVICON",
                    ),
                    placeholder: "/favicon.webp 或 https://example.com/icon.png（支持站外链接）"
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_icp".to_string(),
                    label: "ICP 备案号".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.site_icp.clone())
                        .unwrap_or_default(),
                    placeholder: "如：京ICP备12345678号".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_gongan".to_string(),
                    label: "公安备案号".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.site_gongan.clone())
                        .unwrap_or_default(),
                    placeholder: "如：京公网安备11010802012345号".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "cloud_sponsors".to_string(),
                    label: "云赞助商".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.cloud_sponsors.clone())
                        .unwrap_or_default(),
                    placeholder: "cloudflare,edgeone,upyun（多个用逗号分隔）".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "github_client_id".to_string(),
                    label: "GitHub OAuth Client ID".to_string(),
                    field_type: "text".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.github_client_id.clone()),
                        "GITHUB_CLIENT_ID",
                    ),
                    placeholder: "GitHub OAuth Application Client ID".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "github_client_secret".to_string(),
                    label: "GitHub OAuth Client Secret".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.github_client_secret.clone()),
                        "GITHUB_CLIENT_SECRET",
                    )),
                    placeholder: "GitHub OAuth Application Client Secret".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "base_url".to_string(),
                    label: "Site Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.base_url.clone())
                        .unwrap_or_default(),
                    placeholder: "https://yourdomain.com (用于生成 OAuth 回调 URL)".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "music_enabled".to_string(),
                    label: "Enable Music Player".to_string(),
                    field_type: "checkbox".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.music_enabled.clone()),
                        "MUSIC_ENABLED",
                    ),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "music_source".to_string(),
                    label: "Music Source".to_string(),
                    field_type: "select".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.music_source.clone()),
                        "MUSIC_SOURCE",
                    ),
                    placeholder: "netease or qq".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "music_playlist_id".to_string(),
                    label: "Playlist ID".to_string(),
                    field_type: "text".to_string(),
                    value: get_value(
                        db_config.as_ref().and_then(|c| c.music_playlist_id.clone()),
                        "MUSIC_PLAYLIST_ID",
                    ),
                    placeholder: "Playlist ID from music platform".to_string(),
                    required: false,
                },
                // 网络代理配置
                ConfigField {
                    key: "proxy_enabled".to_string(),
                    label: "启用网络代理".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.proxy_enabled.to_string())
                        .unwrap_or_else(|| "false".to_string()),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "proxy_url".to_string(),
                    label: "代理服务器地址".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.proxy_url.clone())
                        .unwrap_or_default(),
                    placeholder: "http://127.0.0.1:7890 或 socks5://127.0.0.1:1080".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "proxy_bypass".to_string(),
                    label: "代理绕过列表".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.proxy_bypass.clone())
                        .unwrap_or_default(),
                    placeholder: "localhost,127.0.0.1,.local".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "gemini_base_url".to_string(),
                    label: "Gemini API 基础 URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.gemini_base_url.clone())
                        .unwrap_or_default(),
                    placeholder: "https://generativelanguage.googleapis.com (留空使用默认)"
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "github_api_base_url".to_string(),
                    label: "GitHub API 基础 URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .and_then(|c| c.github_api_base_url.clone())
                        .unwrap_or_default(),
                    placeholder: "https://api.github.com (留空使用默认)".to_string(),
                    required: false,
                },
            ],
        },
    };

    let mut config = config;
    sort_platforms_by_order(
        &mut config.platforms,
        db_config.as_ref().and_then(|c| c.platform_order.as_ref()),
    );

    config
}

pub async fn get_config(State(db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    let config = build_config(&db, false).await;
    (StatusCode::OK, Json(json!(config)))
}

async fn reconcile_platform_auto_refresh_with_config(
    db: &DatabaseConnection,
    config: &ConfigResponse,
) -> Result<crate::services::platform_auto_refresh::PlatformAutoRefreshSummary, String> {
    let auto_fetch = config.auto_fetch.clone().unwrap_or_default();
    let user_id = crate::api::profile::site_owner_user_id(db).await?;
    let platforms: Vec<String> = config
        .platforms
        .iter()
        .filter(|platform| platform.enabled)
        .map(|platform| platform.name.clone())
        .collect();
    crate::services::platform_auto_refresh::reconcile_platform_auto_refresh(
        db,
        user_id,
        auto_fetch.enabled,
        auto_fetch.interval_hours,
        &platforms,
    )
    .await
}

/// Rebuild core platform refresh tasks from persisted configuration.
/// Called on backend startup so the database configuration remains the source
/// of truth even after a restart or interrupted settings save.
pub(crate) async fn reconcile_platform_auto_refresh(
    db: &DatabaseConnection,
) -> Result<crate::services::platform_auto_refresh::PlatformAutoRefreshSummary, String> {
    let config = build_config(db, false).await;
    reconcile_platform_auto_refresh_with_config(db, &config).await
}

const SETTINGS_BACKUP_FORMAT: &str = "myriad-settings-backup";
const SETTINGS_BACKUP_VERSION: u32 = 2;
const MIN_SETTINGS_BACKUP_VERSION: u32 = 1;
const MAX_SETTINGS_BACKUP_ENTRIES: usize = 10_000;

fn default_setting_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Copy)]
struct SettingDescriptor {
    schema_version: u32,
    introduced_in_backup_version: u32,
}

// 这是配置备份唯一的后端注册表。新增或删除非 ConfigResponse 设置时只需要改这里；
// 恢复、预检和导出过滤全部从该注册表派生。
const REGISTERED_CONFIGURATION_KEYS_V1: &[&str] = &[
    "ai_image_height",
    "ai_image_model",
    "ai_image_provider",
    "ai_image_width",
    "ai_provider",
    "allow_local_registration",
    "bangumi_access_token",
    "bangumi_enabled",
    "bangumi_user_agent",
    "bangumi_username",
    "base_url",
    "bilibili_enabled",
    "bilibili_uid",
    "cloud_sponsors",
    "control_panel_layout",
    "control_panel_rows",
    "custom_platforms",
    "dashboard_layout",
    "dashboard_title",
    "discord_access_token",
    "discord_enabled",
    "discord_refresh_token",
    "discord_token_expires_at",
    "discord_user_id",
    "enable_auto_fetch",
    "fetch_interval_hours",
    "gemini_api_key",
    "gemini_base_url",
    "gemini_model",
    "github_api_base_url",
    "github_client_id",
    "github_client_secret",
    "github_enabled",
    "github_redirect_url",
    "github_token",
    "github_username",
    "guest_ai_cooldown_seconds",
    "guest_ai_daily_calls",
    "guest_ai_daily_tokens",
    "guest_perm_ai_analyze",
    "guest_perm_ai_chat",
    "guest_perm_ai_generate",
    "guest_perm_ai_image",
    "guest_perm_component_theme",
    "guest_perm_event_publish",
    "guest_perm_media_control",
    "guest_perm_network_fetch",
    "guest_perm_report_write",
    "guest_perm_scheduler_register",
    "guest_perm_shortcut_register",
    "guest_perm_speech_asr",
    "guest_perm_speech_tts",
    "hitokoto_config",
    "library_source_preferences",
    "mal_client_id",
    "mal_enabled",
    "mal_username",
    "module_visibility_preferences",
    "music_enabled",
    "music_playlist_id",
    "music_source",
    "netease_enabled",
    "netease_user_id",
    "oauth_providers",
    "openai_api_key",
    "openai_base_url",
    "openai_max_tokens",
    "openai_model",
    "openxbl_api_key",
    "pet_enabled",
    "pet_image_url",
    "pixai_api_key",
    "platform_order",
    "pro_ai_provider",
    "pro_enabled",
    "pro_gemini_api_key",
    "pro_gemini_model",
    "pro_openai_api_key",
    "pro_openai_base_url",
    "pro_openai_model",
    "proxy_bypass",
    "proxy_enabled",
    "proxy_url",
    "psn_enabled",
    "psn_npsso",
    "psn_online_id",
    "report_settings",
    "site_description",
    "site_favicon",
    "site_gongan",
    "site_icp",
    "site_title",
    "steam_api_key",
    "steam_enabled",
    "steam_id",
    "tapp_window_schemes",
    "tencent_region",
    "tencent_secret_id",
    "tencent_secret_key",
    "title_color",
    "title_font",
    "title_font_size",
    "topic_style",
    "ui_evocative_dynamic_blur",
    "ui_evocative_fps",
    "ui_evocative_parallax",
    "ui_evocative_ripple",
    "ui_evocative_ripple_quality",
    "ui_primary_color",
    "ui_secondary_color",
    "ui_theme",
    "ui_wallpaper_blur",
    "ui_wallpaper_parallax",
    "ui_wallpaper_url",
    "user_ai_cooldown_seconds",
    "user_ai_daily_calls",
    "user_ai_daily_tokens",
    "user_perm_ai_analyze",
    "user_perm_ai_chat",
    "user_perm_ai_generate",
    "user_perm_ai_image",
    "user_perm_component_theme",
    "user_perm_event_publish",
    "user_perm_media_control",
    "user_perm_network_fetch",
    "user_perm_report_write",
    "user_perm_scheduler_register",
    "user_perm_shortcut_register",
    "user_perm_speech_asr",
    "user_perm_speech_tts",
    "widget_theme",
    "x_bearer_token",
    "x_enabled",
    "x_username",
    "xbox_enabled",
    "xbox_gamertag",
];

fn settings_registry() -> std::collections::HashMap<&'static str, SettingDescriptor> {
    REGISTERED_CONFIGURATION_KEYS_V1
        .iter()
        .map(|key| {
            (
                *key,
                SettingDescriptor {
                    schema_version: 1,
                    introduced_in_backup_version: 1,
                },
            )
        })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SettingsBackupEntry {
    pub key: String,
    pub value: Value,
    #[serde(default = "default_setting_schema_version")]
    pub schema_version: u32,
    pub description: Option<String>,
    pub category: Option<String>,
    pub is_encrypted: Option<bool>,
    pub is_public: Option<bool>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SettingsBackupUserPreferences {
    pub notification_preferences:
        crate::services::agent::notification_preferences::NotificationPreferences,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SettingsBackup {
    pub format: String,
    pub version: u32,
    pub exported_at: String,
    pub contains_secrets: bool,
    pub configurations: Vec<SettingsBackupEntry>,
    pub effective_config: ConfigResponse,
    pub user_preferences: SettingsBackupUserPreferences,
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingsRestorePreview {
    pub backup_version: u32,
    pub current_version: u32,
    pub restore_count: usize,
    pub preserve_count: usize,
    pub ignored_count: usize,
    pub migrated_count: usize,
    pub invalid_count: usize,
    pub ignored_keys: Vec<String>,
    pub invalid_keys: Vec<String>,
}

struct SettingsRestorePlan {
    entries: Vec<SettingsBackupEntry>,
    preview: SettingsRestorePreview,
}

fn validate_settings_backup(backup: &SettingsBackup) -> Result<(), String> {
    if backup.format != SETTINGS_BACKUP_FORMAT {
        return Err("Unsupported settings backup format".to_string());
    }
    if !(MIN_SETTINGS_BACKUP_VERSION..=SETTINGS_BACKUP_VERSION).contains(&backup.version) {
        return Err(format!(
            "Unsupported settings backup version: {}",
            backup.version
        ));
    }
    if backup.configurations.len() > MAX_SETTINGS_BACKUP_ENTRIES {
        return Err("Settings backup contains too many configuration entries".to_string());
    }

    let mut keys = std::collections::HashSet::new();
    for entry in &backup.configurations {
        if entry.key.is_empty() || entry.key.len() > 255 {
            return Err("Settings backup contains an invalid configuration key".to_string());
        }
        if !keys.insert(entry.key.as_str()) {
            return Err(format!(
                "Settings backup contains duplicate key: {}",
                entry.key
            ));
        }
    }

    Ok(())
}

fn is_sensitive_configuration_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.contains("api_key")
        || key.contains("token")
        || key.contains("secret")
        || key.contains("password")
        || key.contains("npsso")
}

fn merge_settings_backup_entries(
    backup: &SettingsBackup,
) -> std::collections::HashMap<String, SettingsBackupEntry> {
    let registry = settings_registry();
    let mut entries: std::collections::HashMap<String, SettingsBackupEntry> = backup
        .configurations
        .iter()
        .cloned()
        .map(|entry| (entry.key.clone(), entry))
        .collect();

    // v1 deployments may source values from environment variables. Only settings which already
    // existed in that backup version may be filled from its effective legacy snapshot; settings
    // introduced later must keep the current installation's value/default.
    for (key, value) in collect_database_updates(&backup.effective_config) {
        let Some(descriptor) = registry.get(key.as_str()) else {
            continue;
        };
        if descriptor.introduced_in_backup_version > backup.version {
            continue;
        }
        let is_encrypted = is_sensitive_configuration_key(&key);
        entries.entry(key.clone()).or_insert(SettingsBackupEntry {
            key,
            value,
            schema_version: 1,
            description: None,
            category: Some("general".to_string()),
            is_encrypted: Some(is_encrypted),
            is_public: Some(false),
        });
    }

    entries
}

fn migrate_setting_entry(
    mut entry: SettingsBackupEntry,
    descriptor: SettingDescriptor,
) -> Result<(SettingsBackupEntry, bool), String> {
    if entry.schema_version == 0 || entry.schema_version > descriptor.schema_version {
        return Err(format!(
            "unsupported schema version {} (current {})",
            entry.schema_version, descriptor.schema_version
        ));
    }

    let migrated = entry.schema_version < descriptor.schema_version;
    if migrated {
        // Per-setting migrations are intentionally centralized here. Add explicit transforms
        // before increasing a descriptor's schema_version; silent shape guessing is forbidden.
        return Err(format!(
            "missing migration from schema version {} to {}",
            entry.schema_version, descriptor.schema_version
        ));
    }

    entry.value = normalize_registered_setting_value(&entry.key, entry.value)?;
    entry.schema_version = descriptor.schema_version;
    Ok((entry, migrated))
}

fn normalize_registered_setting_value(key: &str, value: Value) -> Result<Value, String> {
    fn normalize<T: serde::de::DeserializeOwned + Serialize>(
        value: Value,
        transform: impl FnOnce(T) -> T,
    ) -> Result<Value, String> {
        let parsed = serde_json::from_value::<T>(value).map_err(|error| error.to_string())?;
        serde_json::to_value(transform(parsed)).map_err(|error| error.to_string())
    }

    match key {
        MODULE_VISIBILITY_PREFERENCES_KEY => {
            normalize::<ModuleVisibilityPreferences>(value, ModuleVisibilityPreferences::normalized)
        }
        HITOKOTO_CONFIG_KEY => normalize::<HitokotoConfig>(value, HitokotoConfig::normalized),
        REPORT_SETTINGS_KEY => normalize::<ReportSettings>(value, ReportSettings::normalized),
        "library_source_preferences" => normalize::<crate::api::profile::LibrarySourcePreferences>(
            value,
            crate::api::profile::LibrarySourcePreferences::normalized,
        ),
        "oauth_providers" => {
            normalize::<Vec<crate::config::OAuthProviderEntry>>(value, |providers| providers)
        }
        _ => Ok(value),
    }
}

fn build_settings_restore_plan(backup: &SettingsBackup) -> SettingsRestorePlan {
    let registry = settings_registry();
    let merged = merge_settings_backup_entries(backup);
    let mut entries = Vec::new();
    let mut ignored_keys = Vec::new();
    let mut invalid_keys = Vec::new();
    let mut migrated_count = 0;

    for (_, entry) in merged {
        let Some(descriptor) = registry.get(entry.key.as_str()).copied() else {
            ignored_keys.push(entry.key);
            continue;
        };
        let entry_key = entry.key.clone();
        match migrate_setting_entry(entry, descriptor) {
            Ok((entry, migrated)) => {
                migrated_count += usize::from(migrated);
                entries.push(entry);
            }
            Err(_) => invalid_keys.push(entry_key),
        }
    }

    entries.sort_by(|left, right| left.key.cmp(&right.key));
    ignored_keys.sort();
    invalid_keys.sort();
    let preserve_count = registry.len().saturating_sub(entries.len());
    let preview = SettingsRestorePreview {
        backup_version: backup.version,
        current_version: SETTINGS_BACKUP_VERSION,
        // Notification preferences are normalized and restored as one registered user setting.
        restore_count: entries.len() + 1,
        preserve_count,
        ignored_count: ignored_keys.len(),
        migrated_count,
        invalid_count: invalid_keys.len(),
        ignored_keys,
        invalid_keys,
    };

    SettingsRestorePlan { entries, preview }
}

pub async fn export_settings(
    State(db): State<DatabaseConnection>,
    user_id: i32,
) -> (StatusCode, Json<Value>) {
    let registry = settings_registry();
    let effective_config = build_config(&db, true).await;
    let rows = match db
        .query_all(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT key, value, description, category, is_encrypted, is_public FROM configurations ORDER BY key"
                .to_string(),
        ))
        .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::error!("Failed to export settings: {}", error);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to read settings"})),
            );
        }
    };

    let mut configurations = Vec::with_capacity(rows.len());
    for row in rows {
        let key: String = match row.try_get("", "key") {
            Ok(value) => value,
            Err(error) => {
                tracing::error!("Failed to decode configuration key: {}", error);
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(json!({"error": "Failed to decode settings"})),
                );
            }
        };
        let Some(descriptor) = registry.get(key.as_str()) else {
            // Stale rows from removed settings are intentionally not re-exported.
            continue;
        };
        let entry = SettingsBackupEntry {
            key,
            value: match row.try_get("", "value") {
                Ok(value) => value,
                Err(error) => {
                    tracing::error!("Failed to decode configuration value: {}", error);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": "Failed to decode settings"})),
                    );
                }
            },
            schema_version: descriptor.schema_version,
            description: row.try_get("", "description").ok().flatten(),
            category: row.try_get("", "category").ok().flatten(),
            is_encrypted: row.try_get("", "is_encrypted").ok().flatten(),
            is_public: row.try_get("", "is_public").ok().flatten(),
        };
        configurations.push(entry);
    }

    let mut exported_keys: std::collections::HashSet<String> = configurations
        .iter()
        .map(|entry| entry.key.clone())
        .collect();
    for (key, value) in collect_database_updates(&effective_config) {
        let Some(descriptor) = registry.get(key.as_str()) else {
            continue;
        };
        if !exported_keys.insert(key.clone()) {
            continue;
        }
        configurations.push(SettingsBackupEntry {
            schema_version: descriptor.schema_version,
            description: None,
            category: Some("general".to_string()),
            is_encrypted: Some(is_sensitive_configuration_key(&key)),
            is_public: Some(false),
            key,
            value,
        });
    }
    configurations.sort_by(|left, right| left.key.cmp(&right.key));

    let notification_preferences =
        crate::services::agent::notification_preferences::load(Some(&db), user_id).await;
    let backup = SettingsBackup {
        format: SETTINGS_BACKUP_FORMAT.to_string(),
        version: SETTINGS_BACKUP_VERSION,
        exported_at: chrono::Utc::now().to_rfc3339(),
        contains_secrets: true,
        configurations,
        effective_config,
        user_preferences: SettingsBackupUserPreferences {
            notification_preferences,
        },
    };

    (StatusCode::OK, Json(json!(backup)))
}

pub async fn preview_settings_restore(
    Json(backup): Json<SettingsBackup>,
) -> (StatusCode, Json<Value>) {
    if let Err(message) = validate_settings_backup(&backup) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": message})));
    }

    let plan = build_settings_restore_plan(&backup);
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "preview": plan.preview,
        })),
    )
}

pub async fn restore_settings(
    State(db): State<DatabaseConnection>,
    user_id: i32,
    Json(backup): Json<SettingsBackup>,
) -> (StatusCode, Json<Value>) {
    if let Err(message) = validate_settings_backup(&backup) {
        return (StatusCode::BAD_REQUEST, Json(json!({"error": message})));
    }

    let plan = build_settings_restore_plan(&backup);
    let preview = plan.preview.clone();
    let entries = plan.entries;
    let notification_preferences = backup
        .user_preferences
        .notification_preferences
        .normalized();

    let transaction = match db.begin().await {
        Ok(transaction) => transaction,
        Err(error) => {
            tracing::error!("Failed to start settings restore transaction: {}", error);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Failed to start settings restore"})),
            );
        }
    };

    let restore_result: Result<(), sea_orm::DbErr> = async {
        for entry in entries {
            transaction
                .execute(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"
                        INSERT INTO configurations
                            (key, value, description, category, is_encrypted, is_public, created_at, updated_at)
                        VALUES ($1, $2, $3, $4, $5, $6, CURRENT_TIMESTAMP, CURRENT_TIMESTAMP)
                        ON CONFLICT (key) DO UPDATE SET
                            value = EXCLUDED.value,
                            description = COALESCE(EXCLUDED.description, configurations.description),
                            category = COALESCE(EXCLUDED.category, configurations.category),
                            is_encrypted = COALESCE(EXCLUDED.is_encrypted, configurations.is_encrypted),
                            is_public = COALESCE(EXCLUDED.is_public, configurations.is_public),
                            updated_at = CURRENT_TIMESTAMP
                    "#,
                    vec![
                        entry.key.into(),
                        entry.value.into(),
                        entry.description.into(),
                        entry.category.into(),
                        entry.is_encrypted.into(),
                        entry.is_public.into(),
                    ],
                ))
                .await?;
        }

        let notification_value = serde_json::to_value(&notification_preferences)
            .map_err(|error| sea_orm::DbErr::Custom(error.to_string()))?;
        let update_result = transaction
            .execute(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE users SET notification_preferences = $1, updated_at = CURRENT_TIMESTAMP WHERE id = $2",
                vec![notification_value.into(), user_id.into()],
            ))
            .await?;
        if update_result.rows_affected() == 0 {
            return Err(sea_orm::DbErr::Custom(
                "Authenticated user no longer exists".to_string(),
            ));
        }

        transaction.commit().await?;
        Ok(())
    }
    .await;

    if let Err(error) = restore_result {
        tracing::error!("Failed to restore settings: {}", error);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("Failed to restore settings: {}", error)})),
        );
    }

    crate::services::agent::notification_preferences::cache_restored(
        user_id,
        notification_preferences,
    )
    .await;

    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    match config_service.load_config().await {
        Ok(dynamic_config) => {
            *crate::GLOBAL_DYNAMIC_CONFIG.write().await = dynamic_config;
            crate::services::http_client::reload_global_client().await;
            crate::services::oauth::registry::REGISTRY.reload().await;
        }
        Err(error) => {
            tracing::error!("Settings restored but runtime reload failed: {}", error);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Settings restored, but runtime reload failed"})),
            );
        }
    }

    if let Err(error) = reconcile_platform_auto_refresh(&db).await {
        tracing::error!(
            "Settings restored but platform auto-refresh reconciliation failed: {}",
            error
        );
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Settings restored, but platform auto-refresh could not be updated"
            })),
        );
    }

    crate::api::system::CONFIG_RELOAD_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "Settings restored successfully",
            "requires_reload": true,
            "preview": preview
        })),
    )
}

#[cfg(test)]
mod settings_backup_tests {
    use super::*;

    fn empty_config() -> ConfigResponse {
        ConfigResponse::default()
    }

    fn backup_with_entries(configurations: Vec<SettingsBackupEntry>) -> SettingsBackup {
        SettingsBackup {
            format: SETTINGS_BACKUP_FORMAT.to_string(),
            version: SETTINGS_BACKUP_VERSION,
            exported_at: "2026-01-01T00:00:00Z".to_string(),
            contains_secrets: true,
            configurations,
            effective_config: empty_config(),
            user_preferences: SettingsBackupUserPreferences {
                notification_preferences: Default::default(),
            },
        }
    }

    fn entry(key: &str) -> SettingsBackupEntry {
        SettingsBackupEntry {
            key: key.to_string(),
            value: json!(true),
            schema_version: 1,
            description: None,
            category: None,
            is_encrypted: None,
            is_public: None,
        }
    }

    #[test]
    fn validates_versioned_backup_and_rejects_duplicate_keys() {
        let valid = backup_with_entries(vec![entry("oauth_providers"), entry("report_settings")]);
        assert!(validate_settings_backup(&valid).is_ok());

        let duplicate =
            backup_with_entries(vec![entry("report_settings"), entry("report_settings")]);
        assert!(validate_settings_backup(&duplicate)
            .unwrap_err()
            .contains("duplicate key"));
    }

    #[test]
    fn rejects_unknown_format_and_version() {
        let mut backup = backup_with_entries(Vec::new());
        backup.format = "legacy".to_string();
        assert!(validate_settings_backup(&backup).is_err());

        backup.format = SETTINGS_BACKUP_FORMAT.to_string();
        backup.version = MIN_SETTINGS_BACKUP_VERSION;
        assert!(validate_settings_backup(&backup).is_ok());

        backup.version = SETTINGS_BACKUP_VERSION + 1;
        assert!(validate_settings_backup(&backup).is_err());
    }

    #[test]
    fn deserializes_v1_entries_without_per_setting_schema_version() {
        let mut value = serde_json::to_value(backup_with_entries(vec![entry("github_enabled")]))
            .expect("backup should serialize");
        value["version"] = json!(MIN_SETTINGS_BACKUP_VERSION);
        value["configurations"][0]
            .as_object_mut()
            .expect("entry should be an object")
            .remove("schema_version");

        let backup: SettingsBackup =
            serde_json::from_value(value).expect("v1 backup should deserialize");
        assert_eq!(backup.configurations[0].schema_version, 1);
        assert!(validate_settings_backup(&backup).is_ok());
    }

    #[test]
    fn effective_config_collects_unmasked_credentials_for_migration() {
        let mut config = empty_config();
        config.platforms.push(PlatformConfig {
            name: "GitHub".to_string(),
            enabled: true,
            has_token: true,
            config_fields: vec![ConfigField {
                key: "token".to_string(),
                label: String::new(),
                field_type: "password".to_string(),
                value: "secret-token".to_string(),
                placeholder: String::new(),
                required: false,
            }],
            description: String::new(),
            icon: String::new(),
        });

        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("github_token"), Some(&json!("secret-token")));
        assert_eq!(updates.get("github_enabled"), Some(&json!(true)));

        let raw_entry = SettingsBackupEntry {
            key: "github_token".to_string(),
            value: json!("database-token"),
            schema_version: 1,
            description: Some("credential".to_string()),
            category: Some("platform".to_string()),
            is_encrypted: Some(true),
            is_public: Some(false),
        };
        let mut backup = backup_with_entries(vec![raw_entry]);
        backup.version = MIN_SETTINGS_BACKUP_VERSION;
        backup.effective_config = config;
        let merged = merge_settings_backup_entries(&backup);
        assert_eq!(
            merged.get("github_token").map(|entry| &entry.value),
            Some(&json!("database-token"))
        );
        assert_eq!(
            merged
                .get("github_enabled")
                .and_then(|entry| entry.is_encrypted),
            Some(false)
        );
    }

    #[test]
    fn restore_plan_ignores_removed_keys_and_preserves_missing_current_keys() {
        let backup = backup_with_entries(vec![entry("github_enabled"), entry("removed_setting")]);
        let plan = build_settings_restore_plan(&backup);

        assert_eq!(plan.entries.len(), 1);
        assert_eq!(plan.entries[0].key, "github_enabled");
        assert_eq!(plan.preview.ignored_keys, vec!["removed_setting"]);
        assert!(plan.preview.preserve_count > 0);
    }

    #[test]
    fn restore_plan_rejects_future_per_setting_schema_without_failing_file() {
        let mut future = entry("report_settings");
        future.schema_version = 99;
        let plan = build_settings_restore_plan(&backup_with_entries(vec![future]));

        assert!(plan.entries.is_empty());
        assert_eq!(plan.preview.invalid_keys, vec!["report_settings"]);
    }

    #[test]
    fn restore_plan_normalizes_structured_options_against_current_schema() {
        let mut module_entry = entry(MODULE_VISIBILITY_PREFERENCES_KEY);
        module_entry.value = json!({
            "modules": {
                "library": "admin",
                "removed_module": "all"
            }
        });

        let plan = build_settings_restore_plan(&backup_with_entries(vec![module_entry]));
        let modules = plan.entries[0]
            .value
            .get("modules")
            .and_then(Value::as_object)
            .unwrap();
        assert_eq!(modules.get("library"), Some(&json!("admin")));
        assert!(modules.contains_key("brew"));
        assert!(!modules.contains_key("removed_module"));
    }

    #[test]
    fn auto_fetch_settings_are_persisted_and_interval_is_clamped() {
        let mut config = empty_config();
        config.auto_fetch = Some(PlatformAutoFetchConfig {
            enabled: true,
            interval_hours: 0,
        });

        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("enable_auto_fetch"), Some(&json!(true)));
        assert_eq!(updates.get("fetch_interval_hours"), Some(&json!(1)));
    }

    #[test]
    fn missing_auto_fetch_settings_preserve_existing_values() {
        let config = empty_config();
        let updates = collect_database_updates(&config);

        assert!(!updates.contains_key("enable_auto_fetch"));
        assert!(!updates.contains_key("fetch_interval_hours"));
    }
}

pub async fn update_config(
    State(db): State<DatabaseConnection>,
    Json(payload): Json<ConfigResponse>,
) -> (StatusCode, Json<Value>) {
    tracing::info!("Updating configuration");

    // 1. 保存到数据库
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    if let Err(e) = save_to_database(&config_service, &payload).await {
        tracing::error!("Failed to save configuration to database: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": format!("Failed to save configuration to database: {}", e)
            })),
        );
    }
    tracing::info!("✅ Configuration saved to database");

    // 2. 保存到 .env 文件（向后兼容）
    let response = match save_all_configs(&payload).await {
        Ok(_) => {
            tracing::info!("✅ Configuration saved to .env file");
            (
                StatusCode::OK,
                Json(json!({
                    "success": true,
                    "message": "Configuration saved successfully! Changes will be applied automatically within a few seconds."
                })),
            )
        }
        Err(e) => {
            tracing::error!("Failed to save configuration to .env: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": format!("Failed to save configuration: {}", e)
                })),
            )
        }
    };

    // 3. 更新全局动态配置缓存
    match config_service.load_config().await {
        Ok(dynamic_config) => {
            *crate::GLOBAL_DYNAMIC_CONFIG.write().await = dynamic_config;
            tracing::info!("✅ Global dynamic configuration cache updated");

            // 3.1 重载全局 HTTP 客户端（以应用新的代理配置）
            crate::services::http_client::reload_global_client().await;
            tracing::info!("✅ Global HTTP client reloaded with new proxy settings");

            // 3.2 兼容旧配置保存路径：如果 github_client_id/secret 仍由
            // /api/config 写入，也要让 OAuth provider 列表立即生效。
            crate::services::oauth::registry::REGISTRY.reload().await;
            tracing::info!("✅ OAuth provider registry reloaded");
        }
        Err(e) => {
            tracing::warn!("⚠️ Failed to reload dynamic config into cache: {}", e);
        }
    }

    // 4. The platform settings are the source of truth for Core scheduler
    // tasks. Reconcile immediately so the saved switch/frequency takes effect
    // without waiting for a backend restart.
    if let Err(error) = reconcile_platform_auto_refresh(&db).await {
        tracing::error!("Failed to reconcile platform auto-refresh tasks: {}", error);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": format!(
                    "Configuration was saved, but platform auto-refresh could not be updated: {}",
                    error
                )
            })),
        );
    }

    // 5. 触发配置重载标志(虽然数据库连接可能不变,但确保其他服务知道配置已更新)
    crate::api::system::CONFIG_RELOAD_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);

    tracing::info!(
        "🔄 Configuration reload flag set - changes will be picked up within 2-3 seconds"
    );

    response
}

/// 保存配置到数据库
async fn save_to_database(
    config_service: &crate::services::config_service::ConfigService,
    config: &ConfigResponse,
) -> Result<(), Box<dyn std::error::Error>> {
    config_service
        .update_configs(collect_database_updates(config))
        .await?;
    Ok(())
}

fn collect_database_updates(config: &ConfigResponse) -> std::collections::HashMap<String, Value> {
    use serde_json::Value as JsonValue;
    use std::collections::HashMap;

    let mut updates: HashMap<String, JsonValue> = HashMap::new();

    if let Some(auto_fetch) = &config.auto_fetch {
        updates.insert(
            "enable_auto_fetch".to_string(),
            JsonValue::Bool(auto_fetch.enabled),
        );
        updates.insert(
            "fetch_interval_hours".to_string(),
            JsonValue::Number(
                crate::services::platform_auto_refresh::clamp_interval_hours(
                    auto_fetch.interval_hours,
                )
                .into(),
            ),
        );
    }

    // Helper to check if a value is masked
    let is_masked = |value: &str| -> bool {
        value.starts_with("••") || value.starts_with("**") || value == "********"
    };

    // 保存平台配置
    for platform in &config.platforms {
        match platform.name.as_str() {
            "GitHub" => {
                updates.insert(
                    "github_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "username" => "github_username",
                        "token" => "github_token",
                        _ => continue,
                    };
                    // 🔒 忽略屏蔽值（前端返回的掩码）
                    if !field.value.is_empty() && !is_masked(&field.value) {
                        updates.insert(key.to_string(), JsonValue::String(field.value.clone()));
                    }
                }
            }
            "Bilibili" => {
                updates.insert(
                    "bilibili_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    if field.key == "uid" && !field.value.is_empty() {
                        updates.insert(
                            "bilibili_uid".to_string(),
                            JsonValue::String(field.value.clone()),
                        );
                    }
                }
            }
            "Steam" => {
                updates.insert(
                    "steam_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "api_key" => "steam_api_key",
                        "steam_id" => "steam_id",
                        _ => continue,
                    };
                    // 🔒 忽略屏蔽值（前端返回的掩码）
                    if !field.value.is_empty() && !is_masked(&field.value) {
                        updates.insert(key.to_string(), JsonValue::String(field.value.clone()));
                    }
                }
            }
            "Netease Music" => {
                updates.insert(
                    "netease_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    if field.key == "user_id" && !field.value.is_empty() {
                        updates.insert(
                            "netease_user_id".to_string(),
                            JsonValue::String(field.value.clone()),
                        );
                    }
                }
            }
            "Bangumi" => {
                updates.insert(
                    "bangumi_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "username" => "bangumi_username",
                        "access_token" => "bangumi_access_token",
                        "user_agent" => "bangumi_user_agent",
                        _ => continue,
                    };
                    if !field.value.is_empty() && !is_masked(&field.value) {
                        updates.insert(key.to_string(), JsonValue::String(field.value.clone()));
                    }
                }
            }
            "X" => {
                updates.insert("x_enabled".to_string(), JsonValue::Bool(platform.enabled));
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "username" => "x_username",
                        "bearer_token" => "x_bearer_token",
                        _ => continue,
                    };
                    if !field.value.is_empty() && !is_masked(&field.value) {
                        updates.insert(key.to_string(), JsonValue::String(field.value.clone()));
                    }
                }
            }
            "Discord" => {
                updates.insert(
                    "discord_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "access_token" => "discord_access_token",
                        "refresh_token" => "discord_refresh_token",
                        "user_id" => "discord_user_id",
                        _ => continue,
                    };
                    if !field.value.is_empty() && !is_masked(&field.value) {
                        updates.insert(key.to_string(), JsonValue::String(field.value.clone()));
                    }
                }
            }
            "MyAnimeList" => {
                updates.insert("mal_enabled".to_string(), JsonValue::Bool(platform.enabled));
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "username" => "mal_username",
                        "client_id" => "mal_client_id",
                        _ => continue,
                    };
                    if !field.value.is_empty() && !is_masked(&field.value) {
                        updates.insert(key.to_string(), JsonValue::String(field.value.clone()));
                    }
                }
            }
            "Xbox" => {
                updates.insert(
                    "xbox_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "gamertag" => "xbox_gamertag",
                        "openxbl_api_key" => "openxbl_api_key",
                        _ => continue,
                    };
                    if !field.value.is_empty() && !is_masked(&field.value) {
                        updates.insert(key.to_string(), JsonValue::String(field.value.clone()));
                    }
                }
            }
            "PlayStation" => {
                updates.insert("psn_enabled".to_string(), JsonValue::Bool(platform.enabled));
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "online_id" => "psn_online_id",
                        "npsso" => "psn_npsso",
                        _ => continue,
                    };
                    if !field.value.is_empty() && !is_masked(&field.value) {
                        updates.insert(key.to_string(), JsonValue::String(field.value.clone()));
                    }
                }
            }
            _ => {}
        }
    }

    // 保存平台展示顺序（按前端提交的平台数组顺序）
    if !config.platforms.is_empty() {
        let platform_order: Vec<String> = config.platforms.iter().map(|p| p.name.clone()).collect();
        if let Ok(order_value) = serde_json::to_value(&platform_order) {
            updates.insert("platform_order".to_string(), order_value);
        }
    }

    // 保存 AI 配置
    for field in &config.ai_config.config_fields {
        let (key, json_value) = match field.key.as_str() {
            "provider" => ("ai_provider", JsonValue::String(field.value.clone())),
            "gemini_api_key" => ("gemini_api_key", JsonValue::String(field.value.clone())),
            "gemini_model" => ("gemini_model", JsonValue::String(field.value.clone())),
            "openai_api_key" => ("openai_api_key", JsonValue::String(field.value.clone())),
            "openai_model" => ("openai_model", JsonValue::String(field.value.clone())),
            "openai_base_url" => ("openai_base_url", JsonValue::String(field.value.clone())),
            // Pro 模型配置
            "pro_enabled" => ("pro_enabled", JsonValue::Bool(field.value == "true")),
            "pro_provider" => ("pro_ai_provider", JsonValue::String(field.value.clone())),
            "pro_gemini_api_key" => ("pro_gemini_api_key", JsonValue::String(field.value.clone())),
            "pro_gemini_model" => ("pro_gemini_model", JsonValue::String(field.value.clone())),
            "pro_openai_api_key" => ("pro_openai_api_key", JsonValue::String(field.value.clone())),
            "pro_openai_model" => ("pro_openai_model", JsonValue::String(field.value.clone())),
            "pro_openai_base_url" => (
                "pro_openai_base_url",
                JsonValue::String(field.value.clone()),
            ),
            // AI 图片生成配置
            "ai_image_provider" => ("ai_image_provider", JsonValue::String(field.value.clone())),
            "ai_image_model" => ("ai_image_model", JsonValue::String(field.value.clone())),
            "ai_image_width" => {
                if let Ok(n) = field.value.parse::<i64>() {
                    ("ai_image_width", JsonValue::Number(n.into()))
                } else {
                    continue;
                }
            }
            "ai_image_height" => {
                if let Ok(n) = field.value.parse::<i64>() {
                    ("ai_image_height", JsonValue::Number(n.into()))
                } else {
                    continue;
                }
            }
            "pixai_api_key" => ("pixai_api_key", JsonValue::String(field.value.clone())),
            // 腾讯云语音服务配置 (TTS/ASR)
            "tencent_secret_id" => ("tencent_secret_id", JsonValue::String(field.value.clone())),
            "tencent_secret_key" => ("tencent_secret_key", JsonValue::String(field.value.clone())),
            "tencent_region" => ("tencent_region", JsonValue::String(field.value.clone())),
            _ => continue,
        };
        // 🔒 忽略屏蔽值（前端返回的掩码）- 保持数据库原值不变
        if !field.value.is_empty() && !is_masked(&field.value) {
            updates.insert(key.to_string(), json_value);
        }
    }

    // 保存报告配置
    for field in &config.report_config.config_fields {
        if field.key == "topic_style" && !field.value.is_empty() {
            updates.insert(
                "topic_style".to_string(),
                JsonValue::String(field.value.clone()),
            );
        }
    }

    // 保存 UI 配置
    for field in &config.ui_config.config_fields {
        let (key, json_value) = match field.key.as_str() {
            "wallpaper_url" => ("ui_wallpaper_url", JsonValue::String(field.value.clone())),
            "wallpaper_blur" => {
                if let Ok(n) = field.value.parse::<i64>() {
                    ("ui_wallpaper_blur", JsonValue::Number(n.into()))
                } else {
                    continue;
                }
            }
            "wallpaper_parallax" => {
                let enabled = field.value == "true";
                ("ui_wallpaper_parallax", JsonValue::Bool(enabled))
            }
            // Evocative 壁纸动效
            "evocative_parallax" => {
                let enabled = field.value == "true";
                ("ui_evocative_parallax", JsonValue::Bool(enabled))
            }
            "evocative_dynamic_blur" => {
                let enabled = field.value == "true";
                ("ui_evocative_dynamic_blur", JsonValue::Bool(enabled))
            }
            "evocative_ripple" => {
                let enabled = field.value == "true";
                ("ui_evocative_ripple", JsonValue::Bool(enabled))
            }
            "evocative_fps" => {
                if let Ok(n) = field.value.parse::<i64>() {
                    ("ui_evocative_fps", JsonValue::Number(n.into()))
                } else {
                    continue;
                }
            }
            "evocative_ripple_quality" => {
                if let Ok(n) = field.value.parse::<f64>() {
                    ("ui_evocative_ripple_quality", JsonValue::from(n))
                } else {
                    continue;
                }
            }
            "pet_enabled" => {
                let enabled = field.value == "true";
                ("pet_enabled", JsonValue::Bool(enabled))
            }
            "pet_image_url" => ("pet_image_url", JsonValue::String(field.value.clone())),
            "site_title" => ("site_title", JsonValue::String(field.value.clone())),
            "site_description" => ("site_description", JsonValue::String(field.value.clone())),
            "site_favicon" => ("site_favicon", JsonValue::String(field.value.clone())),
            "github_client_id" => ("github_client_id", JsonValue::String(field.value.clone())),
            "github_client_secret" => (
                "github_client_secret",
                JsonValue::String(field.value.clone()),
            ),
            "base_url" => ("base_url", JsonValue::String(field.value.clone())),
            "music_enabled" => {
                let enabled = field.value == "true";
                ("music_enabled", JsonValue::Bool(enabled))
            }
            "music_source" => ("music_source", JsonValue::String(field.value.clone())),
            "music_playlist_id" => ("music_playlist_id", JsonValue::String(field.value.clone())),
            // 网络代理配置
            "proxy_enabled" => {
                let enabled = field.value == "true";
                ("proxy_enabled", JsonValue::Bool(enabled))
            }
            "proxy_url" => ("proxy_url", JsonValue::String(field.value.clone())),
            "proxy_bypass" => ("proxy_bypass", JsonValue::String(field.value.clone())),
            "gemini_base_url" => ("gemini_base_url", JsonValue::String(field.value.clone())),
            "github_api_base_url" => (
                "github_api_base_url",
                JsonValue::String(field.value.clone()),
            ),
            // 站点备案和云赞助商（允许清空）
            "site_icp" | "site_gongan" | "cloud_sponsors" => {
                updates.insert(field.key.clone(), JsonValue::String(field.value.clone()));
                continue;
            }
            _ => continue,
        };
        // 🔒 忽略屏蔽值（前端返回的掩码）- github_client_secret 是敏感字段
        if !field.value.is_empty() && !is_masked(&field.value) {
            updates.insert(key.to_string(), json_value);
        }
    }

    updates
}

/// 保存所有配置到 .env 文件
async fn save_all_configs(config: &ConfigResponse) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs;
    use std::path::Path;

    // 读取现有的 .env 文件（如果存在）
    let env_path = Path::new(".env");
    let mut env_content = if env_path.exists() {
        // 尝试读取文件，如果失败则从字节读取并替换非 UTF-8 字符
        match fs::read_to_string(env_path) {
            Ok(content) => content,
            Err(e) => {
                tracing::warn!("Failed to read .env as UTF-8: {}, attempting to recover", e);
                // 读取字节并尝试转换，替换无效字符
                let bytes = fs::read(env_path)?;
                String::from_utf8_lossy(&bytes).into_owned()
            }
        }
    } else {
        String::new()
    };

    // 保存平台配置
    for platform in &config.platforms {
        match platform.name.as_str() {
            "GitHub" => {
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "username" => "GITHUB_USERNAME",
                        "token" => "GITHUB_TOKEN",
                        _ => continue,
                    };
                    env_content = update_env_var(&env_content, key, &field.value);
                }
            }
            "Bilibili" => {
                for field in &platform.config_fields {
                    if field.key == "uid" {
                        env_content = update_env_var(&env_content, "BILIBILI_UID", &field.value);
                    }
                }
            }
            "Steam" => {
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "api_key" => "STEAM_API_KEY",
                        "steam_id" => "STEAM_ID",
                        _ => continue,
                    };
                    env_content = update_env_var(&env_content, key, &field.value);
                }
            }
            "Netease Music" => {
                for field in &platform.config_fields {
                    if field.key == "user_id" {
                        env_content = update_env_var(&env_content, "NETEASE_USER_ID", &field.value);
                    }
                }
            }
            "Bangumi" => {
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "username" => "BANGUMI_USERNAME",
                        "access_token" => "BANGUMI_ACCESS_TOKEN",
                        "user_agent" => "BANGUMI_USER_AGENT",
                        _ => continue,
                    };
                    env_content = update_env_var(&env_content, key, &field.value);
                }
            }
            "X" => {
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "username" => "X_USERNAME",
                        "bearer_token" => "X_BEARER_TOKEN",
                        _ => continue,
                    };
                    // 跳过掩码值，避免把 •••• 写进 .env
                    if field.value.is_empty()
                        || field.value.starts_with('•')
                        || field.value.starts_with('*')
                    {
                        continue;
                    }
                    env_content = update_env_var(&env_content, key, &field.value);
                }
            }
            "MyAnimeList" => {
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "username" => "MAL_USERNAME",
                        "client_id" => "MAL_CLIENT_ID",
                        _ => continue,
                    };
                    if field.value.is_empty()
                        || field.value.starts_with('•')
                        || field.value.starts_with('*')
                    {
                        continue;
                    }
                    env_content = update_env_var(&env_content, key, &field.value);
                }
            }
            "Xbox" => {
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "gamertag" => "XBOX_GAMERTAG",
                        "openxbl_api_key" => "OPENXBL_API_KEY",
                        _ => continue,
                    };
                    if field.value.is_empty()
                        || field.value.starts_with('•')
                        || field.value.starts_with('*')
                    {
                        continue;
                    }
                    env_content = update_env_var(&env_content, key, &field.value);
                }
            }
            "PlayStation" => {
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "online_id" => "PSN_ONLINE_ID",
                        "npsso" => "PSN_NPSSO",
                        _ => continue,
                    };
                    if field.value.is_empty()
                        || field.value.starts_with('•')
                        || field.value.starts_with('*')
                    {
                        continue;
                    }
                    env_content = update_env_var(&env_content, key, &field.value);
                }
            }
            _ => {}
        }
    }

    // 保存 AI 配置
    for field in &config.ai_config.config_fields {
        let key = match field.key.as_str() {
            "provider" => "AI_PROVIDER",
            "gemini_api_key" => "GEMINI_API_KEY",
            "gemini_model" => "GEMINI_MODEL",
            "openai_api_key" => "OPENAI_API_KEY",
            "openai_model" => "OPENAI_MODEL",
            "openai_base_url" => "OPENAI_BASE_URL",
            // Pro 模型配置
            "pro_enabled" => "PRO_ENABLED",
            "pro_provider" => "PRO_AI_PROVIDER",
            "pro_gemini_api_key" => "PRO_GEMINI_API_KEY",
            "pro_gemini_model" => "PRO_GEMINI_MODEL",
            "pro_openai_api_key" => "PRO_OPENAI_API_KEY",
            "pro_openai_model" => "PRO_OPENAI_MODEL",
            "pro_openai_base_url" => "PRO_OPENAI_BASE_URL",
            // AI 图片生成配置
            "ai_image_provider" => "AI_IMAGE_PROVIDER",
            "ai_image_model" => "AI_IMAGE_MODEL",
            "ai_image_width" => "AI_IMAGE_WIDTH",
            "ai_image_height" => "AI_IMAGE_HEIGHT",
            "pixai_api_key" => "PIXAI_API_KEY",
            _ => continue,
        };
        env_content = update_env_var(&env_content, key, &field.value);
    }

    // 保存报告配置
    for field in &config.report_config.config_fields {
        let key = match field.key.as_str() {
            "topic_style" => "TOPIC_STYLE",
            _ => continue,
        };
        env_content = update_env_var(&env_content, key, &field.value);
    }

    // Capture previous public origin before rewriting BASE_URL so CORS replace
    // can swap the old entry instead of treating the new value as previous.
    let previous_base_url = crate::api::site_domain::read_env_key(&env_content, "BASE_URL")
        .or_else(|| std::env::var("BASE_URL").ok().filter(|s| !s.is_empty()));

    // 保存 UI 配置
    let mut saved_base_url: Option<String> = None;
    for field in &config.ui_config.config_fields {
        let key = match field.key.as_str() {
            "wallpaper_url" => "UI_WALLPAPER_URL",
            "wallpaper_blur" => "UI_WALLPAPER_BLUR",
            "wallpaper_parallax" => "UI_WALLPAPER_PARALLAX",
            // Evocative 壁纸动效
            "evocative_parallax" => "UI_EVOCATIVE_PARALLAX",
            "evocative_dynamic_blur" => "UI_EVOCATIVE_DYNAMIC_BLUR",
            "evocative_ripple" => "UI_EVOCATIVE_RIPPLE",
            "evocative_fps" => "UI_EVOCATIVE_FPS",
            "evocative_ripple_quality" => "UI_EVOCATIVE_RIPPLE_QUALITY",
            "image_gen_enabled" => "IMAGE_GEN_ENABLED",
            "image_gen_model" => "IMAGE_GEN_MODEL",
            "image_gen_width" => "IMAGE_GEN_WIDTH",
            "image_gen_height" => "IMAGE_GEN_HEIGHT",
            "pet_enabled" => "PET_ENABLED",
            "pet_image_url" => "PET_IMAGE_URL",
            "site_title" => "SITE_TITLE",
            "site_description" => "SITE_DESCRIPTION",
            "site_favicon" => "SITE_FAVICON",
            "github_client_id" => "GITHUB_CLIENT_ID",
            "github_client_secret" => "GITHUB_CLIENT_SECRET",
            "base_url" => "BASE_URL",
            "music_enabled" => "MUSIC_ENABLED",
            "music_source" => "MUSIC_SOURCE",
            "music_playlist_id" => "MUSIC_PLAYLIST_ID",
            // 网络代理配置
            "proxy_enabled" => "PROXY_ENABLED",
            "proxy_url" => "PROXY_URL",
            "proxy_bypass" => "PROXY_BYPASS",
            "gemini_base_url" => "GEMINI_BASE_URL",
            "github_api_base_url" => "GITHUB_API_BASE_URL",
            _ => continue,
        };
        if field.key == "base_url" {
            saved_base_url = Some(field.value.clone());
        }
        env_content = update_env_var(&env_content, key, &field.value);
    }

    // When site base_url changes to a valid origin, also adapt FRONTEND_URL +
    // CORS_ORIGINS for the same origin (site access only — not federation Move).
    // Empty base_url must not wipe CORS_ORIGINS (production panics without it).
    if let Some(ref base) = saved_base_url {
        let base_trim = base.trim();
        if !base_trim.is_empty() {
            match crate::api::site_domain::apply_site_domain_to_env_content(
                &env_content,
                base_trim,
                previous_base_url.as_deref(),
            ) {
                Ok((adapted, _)) => env_content = adapted,
                Err(e) => {
                    // Invalid base_url: leave generic BASE_URL write as-is.
                    tracing::warn!(
                        "Skipping FRONTEND_URL/CORS_ORIGINS adapt for base_url={:?}: {}",
                        base_trim,
                        e
                    );
                }
            }
        }
    }

    // 写回 .env 文件，确保使用 UTF-8 编码
    // 在 Windows 上，确保换行符为 LF，避免编码问题
    let env_content_normalized = env_content.replace("\r\n", "\n");

    // 验证内容是否为有效的 UTF-8
    if !env_content_normalized.is_ascii() {
        tracing::debug!("Config contains non-ASCII characters, ensuring UTF-8 validity");
    }

    fs::write(env_path, env_content_normalized.as_bytes())?;
    tracing::info!("✅ Configuration saved to .env file");

    // 重新加载环境变量
    if let Err(e) = dotenvy::from_path_override(env_path) {
        tracing::warn!("⚠️ Failed to reload .env file after saving config: {}", e);
    } else {
        tracing::info!("♻️ Environment variables reloaded after config save");
    }

    // 触发配置重载标志(虽然数据库连接可能不变,但确保其他服务知道配置已更新)
    crate::api::system::CONFIG_RELOAD_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);

    tracing::info!(
        "🔄 Configuration reload flag set - changes will be picked up within 2-3 seconds"
    );

    Ok(())
}

/// 更新或添加环境变量
///
/// `pub(crate)` so site-domain migration can rewrite BASE_URL / FRONTEND_URL /
/// CORS_ORIGINS with the same quoting rules as the general config save path.
pub(crate) fn update_env_var(content: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let key_prefix = format!("{}=", key);

    // 处理值：如果包含空格、特殊字符或中文，用引号包裹
    let sanitized_value = if value.is_empty() {
        String::new()
    } else if value.contains(' ')
        || value.contains('#')
        || value.contains('\n')
        || value.chars().any(|c| c > '\u{007F}')
    // 包含非 ASCII 字符
    {
        // 转义内部的引号和反斜杠
        let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{}\"", escaped)
    } else {
        value.to_string()
    };

    let new_line = if value.is_empty() {
        format!("# {}=", key) // 空值时注释掉
    } else {
        format!("{}={}", key, sanitized_value)
    };

    // 查找是否已存在该键
    let mut found = false;
    for line in &mut lines {
        if line.starts_with(&key_prefix) || line.starts_with(&format!("# {}", key_prefix)) {
            *line = new_line.clone();
            found = true;
            break;
        }
    }

    // 如果不存在，添加到末尾
    if !found {
        lines.push(new_line);
    }

    lines.join("\n") + "\n"
}

pub async fn test_platform(
    State(_db): State<DatabaseConnection>,
    Json(payload): Json<Value>,
) -> (StatusCode, Json<Value>) {
    let platform = payload["platform"].as_str().unwrap_or("");
    let config = &payload["config"];

    match platform {
        "GitHub" => {
            let username = config["username"].as_str().unwrap_or("");
            if username.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"success": false, "message": "Username is required"})),
                );
            }

            let token = config["token"].as_str().filter(|s| !s.is_empty());

            // 调用 GitHub API 验证
            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher.fetch_github_user(username, token).await {
                Ok(user_info) => {
                    let name = user_info["name"].as_str().unwrap_or(username);
                    let followers = user_info["followers"].as_i64().unwrap_or(0);
                    let repos = user_info["public_repos"].as_i64().unwrap_or(0);
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!("✓ GitHub user '{}' verified. {} followers, {} repos", name, followers, repos)
                        })),
                    )
                }
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": format!("✗ Failed to verify GitHub user: {}", e)
                    })),
                ),
            }
        }
        "Bilibili" => {
            let uid = config["uid"].as_str().unwrap_or("");
            if uid.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"success": false, "message": "UID is required"})),
                );
            }

            // 尝试解析 UID 为数字
            let uid_i64 = match uid.parse::<i64>() {
                Ok(n) => n,
                Err(_) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"success": false, "message": "Invalid UID format"})),
                    );
                }
            };

            // 实际调用 Bilibili API 验证
            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher.fetch_bilibili_user(uid_i64).await {
                Ok(user_info) => (
                    StatusCode::OK,
                    Json(json!({
                        "success": true,
                        "message": format!("✓ Bilibili UID {} is valid. User: {}", uid, user_info.name)
                    })),
                ),
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": format!("✗ Failed to verify Bilibili UID: {}", e)
                    })),
                ),
            }
        }
        "Steam" => {
            let api_key = config["api_key"].as_str().unwrap_or("");
            let steam_id = config["steam_id"].as_str().unwrap_or("");
            if api_key.is_empty() || steam_id.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"success": false, "message": "API Key and Steam ID are required"})),
                );
            }

            // 调用 Steam API 验证
            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher.fetch_steam_user(api_key, steam_id).await {
                Ok(user_info) => (
                    StatusCode::OK,
                    Json(json!({
                        "success": true,
                        "message": format!("✓ Steam user '{}' verified", user_info.personaname)
                    })),
                ),
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": format!("✗ Failed to verify Steam: {}", e)
                    })),
                ),
            }
        }
        "Netease Music" => {
            let user_id = config["user_id"].as_str().unwrap_or("");
            if user_id.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"success": false, "message": "User ID is required"})),
                );
            }

            // 尝试解析 User ID 为数字
            let user_id_i64 = match user_id.parse::<i64>() {
                Ok(n) => n,
                Err(_) => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({"success": false, "message": "Invalid User ID format"})),
                    );
                }
            };

            // 调用网易云音乐 API 验证
            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher.fetch_netease_user(user_id_i64).await {
                Ok(user_info) => {
                    let nickname = user_info["profile"]["nickname"]
                        .as_str()
                        .unwrap_or("Unknown");
                    let playlist_count =
                        user_info["profile"]["playlistCount"].as_i64().unwrap_or(0);
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!("✓ Netease Music user '{}' verified. {} playlists", nickname, playlist_count)
                        })),
                    )
                }
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": format!("✗ Failed to verify Netease Music user: {}", e)
                    })),
                ),
            }
        }
        "Bangumi" => {
            let username = config["username"].as_str().unwrap_or("");
            let access_token = config["access_token"]
                .as_str()
                .filter(|s| !s.is_empty() && !s.contains('•') && !s.contains('*'));
            let user_agent = config["user_agent"].as_str().filter(|s| !s.is_empty());
            if username.is_empty() && access_token.is_none() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(
                        json!({"success": false, "message": "Username or access token is required"}),
                    ),
                );
            }

            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            let result = if username.is_empty() {
                fetcher
                    .fetch_bangumi_me(access_token.unwrap_or_default(), user_agent)
                    .await
            } else {
                fetcher
                    .fetch_bangumi_user(username, access_token, user_agent)
                    .await
            };

            match result {
                Ok(user_info) => {
                    let display_name = user_info["nickname"]
                        .as_str()
                        .or_else(|| user_info["username"].as_str())
                        .unwrap_or(username);
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!("✓ Bangumi user '{}' verified", display_name)
                        })),
                    )
                }
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": format!("✗ Failed to verify Bangumi user: {}", e)
                    })),
                ),
            }
        }
        "Discord" => {
            let form_token = config["access_token"]
                .as_str()
                .filter(|s| !s.is_empty() && !s.contains('•') && !s.contains('*'))
                .map(|s| s.to_string());
            // 一键授权后表单多为掩码：回退到已保存的 token
            let access_token = if let Some(t) = form_token {
                t
            } else {
                let cfg = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
                cfg.discord_access_token
                    .clone()
                    .filter(|s| !s.is_empty())
                    .unwrap_or_default()
            };
            if access_token.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": "Access Token is required. Use Connect Discord or paste a token."
                    })),
                );
            }

            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher.fetch_discord_me(&access_token).await {
                Ok(user_info) => {
                    let username = user_info["username"].as_str().unwrap_or("unknown");
                    let global_name = user_info["global_name"].as_str().unwrap_or(username);
                    let user_id = user_info["id"].as_str().unwrap_or("");
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!(
                                "✓ Discord user '{}' verified ({}). id={}",
                                global_name, username, user_id
                            ),
                            "user_id": user_id,
                        })),
                    )
                }
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": format!("✗ Failed to verify Discord token: {}", e)
                    })),
                ),
            }
        }
        "X" => {
            let username = config["username"]
                .as_str()
                .unwrap_or("")
                .trim()
                .trim_start_matches('@');
            let bearer_token = config["bearer_token"]
                .as_str()
                .filter(|s| !s.is_empty() && !s.contains('•') && !s.contains('*'))
                .unwrap_or("");
            if username.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"success": false, "message": "Username is required"})),
                );
            }
            if bearer_token.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": "Bearer Token is required (or re-enter it if the form shows a masked value)"
                    })),
                );
            }

            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher
                .fetch_x_user_by_username(username, bearer_token)
                .await
            {
                Ok(user_info) => {
                    let display = user_info["name"]
                        .as_str()
                        .or_else(|| user_info["username"].as_str())
                        .unwrap_or(username);
                    let followers = user_info
                        .pointer("/public_metrics/followers_count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let tweets = user_info
                        .pointer("/public_metrics/tweet_count")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!(
                                "✓ X user '@{}' verified ({}). {} followers, {} posts",
                                username, display, followers, tweets
                            )
                        })),
                    )
                }
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": format!("✗ Failed to verify X user: {}", e)
                    })),
                ),
            }
        }
        "MyAnimeList" => {
            let username = config["username"].as_str().unwrap_or("").trim();
            // Client ID optional: form value if not masked; else fall back to saved config/env
            let form_client_id = config["client_id"]
                .as_str()
                .filter(|s| !s.is_empty() && !s.contains('•') && !s.contains('*'))
                .map(|s| s.to_string());
            let cfg = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
            let client_id = form_client_id.or_else(|| {
                cfg.mal_client_id
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| std::env::var("MAL_CLIENT_ID").ok())
                    .filter(|s| !s.trim().is_empty())
            });
            drop(cfg);
            if username.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"success": false, "message": "Username is required"})),
                );
            }

            let mode = if client_id.as_ref().is_some_and(|s| !s.trim().is_empty()) {
                "official API"
            } else {
                "public load.json"
            };
            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher
                .fetch_mal_user(username, client_id.as_deref())
                .await
            {
                Ok(user_info) => {
                    let display = user_info["name"].as_str().unwrap_or(username);
                    let anime_completed = user_info
                        .pointer("/anime_statistics/num_items_completed")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    let anime_watching = user_info
                        .pointer("/anime_statistics/num_items_watching")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!(
                                "✓ MAL user '{}' verified ({}) via {}. {} completed, {} watching",
                                username, display, mode, anime_completed, anime_watching
                            )
                        })),
                    )
                }
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": format!("✗ Failed to verify MyAnimeList user: {}", e)
                    })),
                ),
            }
        }
        "Xbox" => {
            let form_gamertag = config["gamertag"].as_str().unwrap_or("").trim();
            let form_key = config["openxbl_api_key"]
                .as_str()
                .filter(|s| !s.is_empty() && !s.contains('•') && !s.contains('*'))
                .map(|s| s.to_string());

            let cfg = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
            let gamertag = if !form_gamertag.is_empty() {
                form_gamertag.to_string()
            } else {
                cfg.xbox_gamertag
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| std::env::var("XBOX_GAMERTAG").ok())
                    .unwrap_or_default()
            };
            let api_key = form_key.unwrap_or_else(|| {
                cfg.openxbl_api_key
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| std::env::var("OPENXBL_API_KEY").ok())
                    .or_else(|| std::env::var("XBL_API_KEY").ok())
                    .unwrap_or_default()
            });
            drop(cfg);

            if gamertag.trim().is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"success": false, "message": "Gamertag is required"})),
                );
            }
            if api_key.trim().is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": "OpenXBL API Key is required (or re-enter it if the form shows a masked value)"
                    })),
                );
            }

            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher.fetch_xbox_profile_bundle(&gamertag, &api_key).await {
                Ok(bundle) => {
                    let titles = bundle
                        .pointer("/achievements/titles")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    let display = bundle
                        .get("gamertag")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&gamertag);
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!(
                                "✓ Xbox player '{}' verified. {} titles with achievement data",
                                display, titles
                            )
                        })),
                    )
                }
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": format!("✗ Failed to verify Xbox player: {}", e)
                    })),
                ),
            }
        }
        "PlayStation" => {
            let form_online_id = config["online_id"].as_str().unwrap_or("").trim();
            let form_npsso = config["npsso"]
                .as_str()
                .filter(|s| !s.is_empty() && !s.contains('•') && !s.contains('*'))
                .map(|s| s.to_string());

            let cfg = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
            let online_id = if !form_online_id.is_empty() {
                form_online_id.to_string()
            } else {
                cfg.psn_online_id
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| std::env::var("PSN_ONLINE_ID").ok())
                    .unwrap_or_default()
            };
            let npsso = form_npsso.unwrap_or_else(|| {
                cfg.psn_npsso
                    .clone()
                    .filter(|s| !s.trim().is_empty())
                    .or_else(|| std::env::var("PSN_NPSSO").ok())
                    .unwrap_or_default()
            });
            drop(cfg);

            if online_id.trim().is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"success": false, "message": "Online ID is required"})),
                );
            }
            if npsso.trim().is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": "NPSSO Token is required (or re-enter it if the form shows a masked value)"
                    })),
                );
            }

            let fetcher = crate::services::fetcher::PlatformFetcher::new().await;
            match fetcher.fetch_psn_profile_bundle(&online_id, &npsso).await {
                Ok(bundle) => {
                    let titles = bundle
                        .get("trophy_titles")
                        .and_then(|v| v.as_array())
                        .map(|a| a.len())
                        .unwrap_or(0);
                    let level = bundle
                        .pointer("/trophy_summary/trophyLevel")
                        .and_then(|v| {
                            v.as_i64()
                                .or_else(|| v.as_str().and_then(|s| s.parse::<i64>().ok()))
                        })
                        .unwrap_or(0);
                    let display = bundle
                        .get("online_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or(&online_id);
                    (
                        StatusCode::OK,
                        Json(json!({
                            "success": true,
                            "message": format!(
                                "✓ PSN player '{}' verified (Lv.{}). {} trophy titles",
                                display, level, titles
                            )
                        })),
                    )
                }
                Err(e) => (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "success": false,
                        "message": format!("✗ Failed to verify PSN player: {}", e)
                    })),
                ),
            }
        }
        _ => (
            StatusCode::OK,
            Json(json!({"success": false, "message": "Platform test not implemented yet"})),
        ),
    }
}

/// 获取公开的网站元数据（不需要认证）
/// 直接从环境变量读取（配置保存时已经写入 .env 并重新加载）
/// 优先级：.env 文件（通过 save_all_configs 保存） > 默认值
pub async fn get_site_metadata(State(db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    // 优先从数据库读取站点元数据配置
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let db_config = config_service.load_config().await.ok();

    let get_value = |db_val: Option<String>, env_key: &str, default: &str| -> String {
        let from_db = db_val.filter(|v| !v.is_empty());
        let from_env = std::env::var(env_key).ok();
        let has_db = from_db.is_some();
        let has_env = from_env.is_some();
        let result = from_db.or(from_env).unwrap_or_else(|| default.to_string());

        tracing::debug!(
            "[元数据] {}: db={}, env={}, result={}",
            env_key,
            has_db,
            has_env,
            result
        );

        result
    };

    let metadata = json!({
        "site_title": get_value(
            db_config.as_ref().and_then(|c| c.site_title.clone()),
            "SITE_TITLE",
            "Myriad - A myriad of lights, in one place."
        ),
        "site_description": get_value(
            db_config.as_ref().and_then(|c| c.site_description.clone()),
            "SITE_DESCRIPTION",
            "A myriad of lights, in one place."
        ),
        "site_favicon": get_value(
            db_config.as_ref().and_then(|c| c.site_favicon.clone()),
            "SITE_FAVICON",
            "/favicon.webp"
        ),
    });

    (StatusCode::OK, Json(metadata))
}

/// 获取公开的平台配置（不包含敏感信息，仅用于社交链接显示）
/// 🔓 公开端点 - 不需要认证
pub async fn get_public_config(State(db): State<DatabaseConnection>) -> (StatusCode, Json<Value>) {
    // 优先从数据库读取配置
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let db_config = config_service.load_config().await.ok();

    // 辅助函数：优先使用数据库值，否则使用环境变量
    let get_value = |db_val: Option<String>, env_key: &str| -> String {
        db_val
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| std::env::var(env_key).unwrap_or_default())
    };

    let github_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.github_enabled),
        db_config
            .as_ref()
            .and_then(|c| c.github_username.as_ref())
            .is_some()
            || std::env::var("GITHUB_USERNAME").is_ok(),
    );
    let bilibili_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.bilibili_enabled),
        db_config
            .as_ref()
            .and_then(|c| c.bilibili_uid.as_ref())
            .is_some()
            || std::env::var("BILIBILI_UID").is_ok(),
    );
    let steam_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.steam_enabled),
        db_config
            .as_ref()
            .and_then(|c| c.steam_id.as_ref())
            .is_some()
            || std::env::var("STEAM_ID").is_ok(),
    );
    let netease_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.netease_enabled),
        db_config
            .as_ref()
            .and_then(|c| c.netease_user_id.as_ref())
            .is_some()
            || std::env::var("NETEASE_USER_ID").is_ok(),
    );
    let bangumi_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.bangumi_enabled),
        db_config
            .as_ref()
            .and_then(|c| c.bangumi_username.as_ref())
            .is_some()
            || db_config
                .as_ref()
                .and_then(|c| c.bangumi_access_token.as_ref())
                .is_some()
            || std::env::var("BANGUMI_USERNAME").is_ok()
            || std::env::var("BANGUMI_ACCESS_TOKEN").is_ok(),
    );
    let x_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.x_enabled),
        (db_config
            .as_ref()
            .and_then(|c| c.x_username.as_ref())
            .is_some()
            || std::env::var("X_USERNAME").is_ok())
            && (db_config
                .as_ref()
                .and_then(|c| c.x_bearer_token.as_ref())
                .is_some()
                || std::env::var("X_BEARER_TOKEN").is_ok()),
    );
    let discord_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.discord_enabled),
        db_config
            .as_ref()
            .and_then(|c| c.discord_access_token.as_ref())
            .is_some()
            || std::env::var("DISCORD_ACCESS_TOKEN").is_ok(),
    );
    let mal_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.mal_enabled),
        db_config
            .as_ref()
            .and_then(|c| c.mal_username.as_ref())
            .is_some()
            || std::env::var("MAL_USERNAME").is_ok(),
    );
    let xbox_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.xbox_enabled),
        (db_config
            .as_ref()
            .and_then(|c| c.xbox_gamertag.as_ref())
            .is_some()
            || std::env::var("XBOX_GAMERTAG").is_ok())
            && (db_config
                .as_ref()
                .and_then(|c| c.openxbl_api_key.as_ref())
                .is_some()
                || std::env::var("OPENXBL_API_KEY").is_ok()
                || std::env::var("XBL_API_KEY").is_ok()),
    );
    let psn_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.psn_enabled),
        (db_config
            .as_ref()
            .and_then(|c| c.psn_online_id.as_ref())
            .is_some()
            || std::env::var("PSN_ONLINE_ID").is_ok())
            && (db_config
                .as_ref()
                .and_then(|c| c.psn_npsso.as_ref())
                .is_some()
                || std::env::var("PSN_NPSSO").is_ok()),
    );

    // 只返回公开可见的平台配置字段（不包含 API 密钥等敏感信息）
    let public_platforms = vec![
        PlatformConfig {
            name: "GitHub".to_string(),
            enabled: github_enabled,
            has_token: false, // 不暴露是否有 token
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "username".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.github_username.clone()),
                    "GITHUB_USERNAME",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Bilibili".to_string(),
            enabled: bilibili_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "uid".to_string(),
                label: "".to_string(),
                field_type: "number".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.bilibili_uid.clone()),
                    "BILIBILI_UID",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Steam".to_string(),
            enabled: steam_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "steam_id".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.steam_id.clone()),
                    "STEAM_ID",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Netease Music".to_string(),
            enabled: netease_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "user_id".to_string(),
                label: "".to_string(),
                field_type: "number".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.netease_user_id.clone()),
                    "NETEASE_USER_ID",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Bangumi".to_string(),
            enabled: bangumi_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "username".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.bangumi_username.clone()),
                    "BANGUMI_USERNAME",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "X".to_string(),
            enabled: x_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "username".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.x_username.clone()),
                    "X_USERNAME",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Discord".to_string(),
            enabled: discord_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            // 不暴露 token；仅返回 user_id 便于公开名片展示
            config_fields: vec![ConfigField {
                key: "user_id".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.discord_user_id.clone()),
                    "DISCORD_USER_ID",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "MyAnimeList".to_string(),
            enabled: mal_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "username".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.mal_username.clone()),
                    "MAL_USERNAME",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Xbox".to_string(),
            enabled: xbox_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "gamertag".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.xbox_gamertag.clone()),
                    "XBOX_GAMERTAG",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "PlayStation".to_string(),
            enabled: psn_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "online_id".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config.as_ref().and_then(|c| c.psn_online_id.clone()),
                    "PSN_ONLINE_ID",
                ),
                placeholder: "".to_string(),
                required: false,
            }],
        },
    ];

    let mut public_platforms = public_platforms;
    sort_platforms_by_order(
        &mut public_platforms,
        db_config.as_ref().and_then(|c| c.platform_order.as_ref()),
    );

    let response = json!({
        "platforms": public_platforms
    });

    (StatusCode::OK, Json(response))
}

/// 获取公开的 UI 配置（萌宠、虚拟人设等）
/// 🔓 公开端点 - 不需要认证
pub async fn get_public_ui_config(
    State(db): State<DatabaseConnection>,
) -> (StatusCode, Json<Value>) {
    // 优先从数据库读取配置
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let db_config = config_service.load_config().await.ok();

    let get_value = |db_val: Option<String>, env_key: &str| -> String {
        db_val
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| std::env::var(env_key).unwrap_or_default())
    };

    let ui_config = json!({
        "pet_enabled": db_config.as_ref().map(|c| c.pet_enabled).unwrap_or_else(||
            std::env::var("PET_ENABLED").unwrap_or_else(|_| "true".to_string()).parse().unwrap_or(true)
        ),
        "pet_image_url": get_value(
            db_config.as_ref().and_then(|c| c.pet_image_url.clone()),
            "PET_IMAGE_URL"
        ),
        "wallpaper_url": get_value(
            db_config.as_ref().and_then(|c| c.ui_wallpaper_url.clone()),
            "UI_WALLPAPER_URL"
        ),
        "wallpaper_blur": db_config.as_ref().map(|c| c.ui_wallpaper_blur as u32).unwrap_or_else(||
            std::env::var("UI_WALLPAPER_BLUR").unwrap_or_else(|_| "3".to_string()).parse::<u32>().unwrap_or(3)
        ),
        "wallpaper_parallax": db_config.as_ref().map(|c| c.ui_wallpaper_parallax).unwrap_or_else(||
            std::env::var("UI_WALLPAPER_PARALLAX").unwrap_or_else(|_| "true".to_string()).parse().unwrap_or(true)
        ),
        // Evocative 壁纸动效
        "evocative_parallax": db_config.as_ref().map(|c| c.ui_evocative_parallax).unwrap_or_else(||
            std::env::var("UI_EVOCATIVE_PARALLAX").unwrap_or_else(|_| "true".to_string()).parse().unwrap_or(true)
        ),
        "evocative_dynamic_blur": db_config.as_ref().map(|c| c.ui_evocative_dynamic_blur).unwrap_or_else(||
            std::env::var("UI_EVOCATIVE_DYNAMIC_BLUR").unwrap_or_else(|_| "false".to_string()).parse().unwrap_or(false)
        ),
        "evocative_ripple": db_config.as_ref().map(|c| c.ui_evocative_ripple).unwrap_or_else(||
            std::env::var("UI_EVOCATIVE_RIPPLE").unwrap_or_else(|_| "false".to_string()).parse().unwrap_or(false)
        ),
        "evocative_fps": db_config.as_ref().map(|c| c.ui_evocative_fps as u32).unwrap_or_else(||
            std::env::var("UI_EVOCATIVE_FPS").unwrap_or_else(|_| "30".to_string()).parse::<u32>().unwrap_or(30)
        ),
        "evocative_ripple_quality": db_config.as_ref().map(|c| c.ui_evocative_ripple_quality).unwrap_or_else(||
            std::env::var("UI_EVOCATIVE_RIPPLE_QUALITY").unwrap_or_else(|_| "0.85".to_string()).parse::<f64>().unwrap_or(0.85)
        ),
        "music_enabled": get_value(
            db_config.as_ref().and_then(|c| c.music_enabled.clone()),
            "MUSIC_ENABLED"
        ),
        "music_source": get_value(
            db_config.as_ref().and_then(|c| c.music_source.clone()),
            "MUSIC_SOURCE"
        ),
        "music_playlist_id": get_value(
            db_config.as_ref().and_then(|c| c.music_playlist_id.clone()),
            "MUSIC_PLAYLIST_ID"
        ),
        "dashboard_layout": db_config.as_ref().and_then(|c| c.dashboard_layout.clone()),
        "dashboard_title": db_config.as_ref().and_then(|c| c.dashboard_title.clone()),
        "custom_platforms": db_config.as_ref().and_then(|c| c.custom_platforms.clone()),
        "widget_theme": db_config.as_ref().and_then(|c| c.widget_theme.clone()),
        "control_panel_layout": db_config.as_ref().and_then(|c| c.control_panel_layout.clone()),
        "control_panel_rows": db_config.as_ref().map(|c| c.control_panel_rows).unwrap_or(2),
        "tapp_window_schemes": db_config.as_ref().and_then(|c| c.tapp_window_schemes.clone()),
        // 标题字体样式设置
        "title_font": db_config.as_ref().and_then(|c| c.title_font.clone()),
        "title_font_size": db_config.as_ref().and_then(|c| c.title_font_size),
        "title_color": db_config.as_ref().and_then(|c| c.title_color.clone()),
        // 站点信息（用于底部显示）
        "site_icp": db_config.as_ref().and_then(|c| c.site_icp.clone()),
        "site_gongan": db_config.as_ref().and_then(|c| c.site_gongan.clone()),
        "cloud_sponsors": db_config.as_ref().and_then(|c| c.cloud_sponsors.clone()),
    });

    (StatusCode::OK, Json(ui_config))
}

#[derive(Debug, Deserialize)]
pub struct DashboardConfigPayload {
    pub layout: Option<String>,
    pub title: Option<String>,
    pub custom_platforms: Option<String>,
    pub title_font: Option<String>,
    pub title_font_size: Option<f64>,
    pub title_color: Option<String>,
    pub widget_theme: Option<String>,
}

pub async fn update_dashboard_config(
    State(db): State<DatabaseConnection>,
    Json(payload): Json<DashboardConfigPayload>,
) -> (StatusCode, Json<Value>) {
    let config_service = crate::services::config_service::ConfigService::new(db);
    let mut updates = std::collections::HashMap::new();

    if let Some(layout) = payload.layout {
        updates.insert("dashboard_layout".to_string(), json!(layout));
    }

    if let Some(title) = payload.title {
        updates.insert("dashboard_title".to_string(), json!(title));
    }

    if let Some(custom_platforms) = payload.custom_platforms {
        updates.insert("custom_platforms".to_string(), json!(custom_platforms));
    }

    if let Some(title_font) = payload.title_font {
        updates.insert("title_font".to_string(), json!(title_font));
    }

    if let Some(title_font_size) = payload.title_font_size {
        updates.insert("title_font_size".to_string(), json!(title_font_size));
    }

    if let Some(title_color) = payload.title_color {
        updates.insert("title_color".to_string(), json!(title_color));
    }

    if let Some(widget_theme) = payload.widget_theme {
        updates.insert("widget_theme".to_string(), json!(widget_theme));
    }

    if let Err(e) = config_service.update_configs(updates).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": format!("Failed to update dashboard config: {}", e)
            })),
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "Dashboard configuration updated successfully"
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct ControlPanelConfigPayload {
    pub control_panel_layout: Option<String>,
    pub control_panel_rows: Option<i32>,
}

pub async fn update_control_panel_config(
    State(db): State<DatabaseConnection>,
    Json(payload): Json<ControlPanelConfigPayload>,
) -> (StatusCode, Json<Value>) {
    let config_service = crate::services::config_service::ConfigService::new(db);
    let mut updates = std::collections::HashMap::new();

    if let Some(layout) = payload.control_panel_layout {
        updates.insert("control_panel_layout".to_string(), json!(layout));
    }

    if let Some(rows) = payload.control_panel_rows {
        updates.insert("control_panel_rows".to_string(), json!(rows));
    }

    if let Err(e) = config_service.update_configs(updates).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": format!("Failed to update control panel config: {}", e)
            })),
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "Control panel configuration updated successfully"
        })),
    )
}

// ========== Tapp 窗口方案 API ==========

#[derive(Debug, Deserialize)]
pub struct TappWindowSchemesPayload {
    pub schemes: String,
}

pub async fn update_tapp_window_schemes(
    State(db): State<DatabaseConnection>,
    Json(payload): Json<TappWindowSchemesPayload>,
) -> (StatusCode, Json<Value>) {
    let config_service = crate::services::config_service::ConfigService::new(db);
    let mut updates = std::collections::HashMap::new();

    updates.insert("tapp_window_schemes".to_string(), json!(payload.schemes));

    if let Err(e) = config_service.update_configs(updates).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": format!("Failed to update tapp window schemes: {}", e)
            })),
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "Tapp window schemes updated successfully"
        })),
    )
}

const MODULE_VISIBILITY_PREFERENCES_KEY: &str = "module_visibility_preferences";
const MODULE_VISIBILITY_KEYS: [&str; 5] = ["library", "brew", "reports", "tapp", "agent"];
const MODULE_VISIBILITY_LEVELS: [&str; 3] = ["all", "authenticated", "admin"];
/// 兼容旧配置字段（能力已迁至 Tapp 权限预设；读写仍规范化但不参与鉴权）
const AGENT_GUEST_USAGE_LEVELS: [&str; 2] = ["none", "visible"];
const AGENT_USER_USAGE_LEVELS: [&str; 4] = ["none", "chat", "standard", "elevated"];

/// 旧版 Agent 使用档位（已弃用：运行时以 Tapp `user_perm_*` / 预设模板为准）
#[derive(Debug, Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AgentUsagePreferences {
    #[serde(default = "default_agent_guest_usage")]
    pub guest: String,
    #[serde(default = "default_agent_user_usage")]
    pub user: String,
}

fn default_agent_guest_usage() -> String {
    "none".to_string()
}
fn default_agent_user_usage() -> String {
    "standard".to_string()
}

impl Default for AgentUsagePreferences {
    fn default() -> Self {
        Self {
            guest: default_agent_guest_usage(),
            user: default_agent_user_usage(),
        }
    }
}

impl AgentUsagePreferences {
    pub fn normalized(mut self) -> Self {
        if !AGENT_GUEST_USAGE_LEVELS.contains(&self.guest.as_str()) {
            self.guest = default_agent_guest_usage();
        }
        if !AGENT_USER_USAGE_LEVELS.contains(&self.user.as_str()) {
            self.user = default_agent_user_usage();
        }
        self
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ModuleVisibilityPreferences {
    #[serde(default = "default_module_visibility_modules")]
    pub modules: std::collections::HashMap<String, String>,
    /// 旧版 Agent 档位（兼容存储；鉴权请用 Tapp 权限）
    #[serde(default)]
    pub agent_usage: AgentUsagePreferences,
}

fn default_module_visibility_modules() -> std::collections::HashMap<String, String> {
    std::collections::HashMap::from([
        ("library".to_string(), "all".to_string()),
        ("brew".to_string(), "all".to_string()),
        ("reports".to_string(), "all".to_string()),
        ("tapp".to_string(), "all".to_string()),
        ("agent".to_string(), "all".to_string()),
    ])
}

impl Default for ModuleVisibilityPreferences {
    fn default() -> Self {
        Self {
            modules: default_module_visibility_modules(),
            agent_usage: AgentUsagePreferences::default(),
        }
    }
}

impl ModuleVisibilityPreferences {
    pub fn normalized(mut self) -> Self {
        let defaults = default_module_visibility_modules();
        let mut normalized = std::collections::HashMap::new();

        for key in MODULE_VISIBILITY_KEYS {
            let value = self
                .modules
                .remove(key)
                .unwrap_or_else(|| defaults.get(key).cloned().unwrap_or_else(|| "all".into()));
            let value = if MODULE_VISIBILITY_LEVELS.contains(&value.as_str()) {
                value
            } else {
                defaults.get(key).cloned().unwrap_or_else(|| "all".into())
            };
            normalized.insert(key.to_string(), value);
        }

        self.modules = normalized;
        self.agent_usage = self.agent_usage.normalized();
        self
    }

    /// Agent 模块页面可见级别
    pub fn agent_visibility(&self) -> &str {
        self.modules
            .get("agent")
            .map(String::as_str)
            .unwrap_or("all")
    }
}

/// 供 Agent 服务读取模块可见性与使用权限（公开给 agent 模块）
pub async fn load_module_visibility_preferences_for_agent(
    db: &DatabaseConnection,
) -> ModuleVisibilityPreferences {
    load_module_visibility_preferences(db).await
}

async fn load_module_visibility_preferences(
    db: &DatabaseConnection,
) -> ModuleVisibilityPreferences {
    let sql = "SELECT value FROM configurations WHERE key = $1";
    let result = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![MODULE_VISIBILITY_PREFERENCES_KEY.into()],
        ))
        .await;

    match result {
        Ok(Some(row)) => match row.try_get::<Value>("", "value") {
            Ok(value) => serde_json::from_value::<ModuleVisibilityPreferences>(value)
                .map(ModuleVisibilityPreferences::normalized)
                .unwrap_or_else(|e| {
                    tracing::warn!(
                        "Invalid module visibility preferences, using defaults: {}",
                        e
                    );
                    ModuleVisibilityPreferences::default()
                }),
            Err(e) => {
                tracing::warn!("Failed to read module visibility preferences: {}", e);
                ModuleVisibilityPreferences::default()
            }
        },
        Ok(None) => ModuleVisibilityPreferences::default(),
        Err(e) => {
            tracing::warn!("Failed to load module visibility preferences: {}", e);
            ModuleVisibilityPreferences::default()
        }
    }
}

pub async fn get_module_visibility_preferences(
    State(db): State<DatabaseConnection>,
) -> (StatusCode, Json<Value>) {
    let preferences = load_module_visibility_preferences(&db).await;
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "preferences": preferences
        })),
    )
}

pub async fn update_module_visibility_preferences(
    State(db): State<DatabaseConnection>,
    Json(payload): Json<ModuleVisibilityPreferences>,
) -> (StatusCode, Json<Value>) {
    let preferences = payload.normalized();
    let config_service = crate::services::config_service::ConfigService::new(db);

    match config_service
        .update_config(
            MODULE_VISIBILITY_PREFERENCES_KEY,
            serde_json::to_value(&preferences).unwrap_or_else(|_| json!({})),
        )
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "preferences": preferences
            })),
        ),
        Err(e) => {
            tracing::error!("Failed to save module visibility preferences: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Failed to save module visibility preferences"
                })),
            )
        }
    }
}

// ========== 一言（Hitokoto）配置 API ==========

const HITOKOTO_CONFIG_KEY: &str = "hitokoto_config";
const HITOKOTO_SOURCE_IDS: [&str; 5] = [
    "hitokoto-cn",
    "hitokoto-anime",
    "quotable-en",
    "meigen-ja",
    "custom",
];

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
pub struct HitokotoConfig {
    #[serde(default = "default_hitokoto_source_id")]
    pub source_id: String,
    #[serde(default)]
    pub custom_url: Option<String>,
    #[serde(default)]
    pub custom_text_field: Option<String>,
    #[serde(default)]
    pub custom_author_field: Option<String>,
}

fn default_hitokoto_source_id() -> String {
    "hitokoto-cn".to_string()
}

impl Default for HitokotoConfig {
    fn default() -> Self {
        Self {
            source_id: default_hitokoto_source_id(),
            custom_url: None,
            custom_text_field: None,
            custom_author_field: None,
        }
    }
}

impl HitokotoConfig {
    fn normalized(mut self) -> Self {
        if !HITOKOTO_SOURCE_IDS.contains(&self.source_id.as_str()) {
            self.source_id = default_hitokoto_source_id();
        }
        self.custom_url = self.custom_url.filter(|s| !s.trim().is_empty());
        self.custom_text_field = self.custom_text_field.filter(|s| !s.trim().is_empty());
        self.custom_author_field = self.custom_author_field.filter(|s| !s.trim().is_empty());
        self
    }
}

async fn load_hitokoto_config(db: &DatabaseConnection) -> HitokotoConfig {
    let sql = "SELECT value FROM configurations WHERE key = $1";
    let result = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![HITOKOTO_CONFIG_KEY.into()],
        ))
        .await;

    match result {
        Ok(Some(row)) => match row.try_get::<Value>("", "value") {
            Ok(value) => serde_json::from_value::<HitokotoConfig>(value)
                .map(HitokotoConfig::normalized)
                .unwrap_or_else(|e| {
                    tracing::warn!("Invalid hitokoto config, using defaults: {}", e);
                    HitokotoConfig::default()
                }),
            Err(e) => {
                tracing::warn!("Failed to read hitokoto config: {}", e);
                HitokotoConfig::default()
            }
        },
        Ok(None) => HitokotoConfig::default(),
        Err(e) => {
            tracing::warn!("Failed to load hitokoto config: {}", e);
            HitokotoConfig::default()
        }
    }
}

pub async fn get_hitokoto_config(
    State(db): State<DatabaseConnection>,
) -> (StatusCode, Json<Value>) {
    let config = load_hitokoto_config(&db).await;
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "config": config
        })),
    )
}

pub async fn update_hitokoto_config(
    State(db): State<DatabaseConnection>,
    Json(payload): Json<HitokotoConfig>,
) -> (StatusCode, Json<Value>) {
    let config = payload.normalized();
    let config_service = crate::services::config_service::ConfigService::new(db);

    match config_service
        .update_config(
            HITOKOTO_CONFIG_KEY,
            serde_json::to_value(&config).unwrap_or_else(|_| json!({})),
        )
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "config": config
            })),
        ),
        Err(e) => {
            tracing::error!("Failed to save hitokoto config: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Failed to save hitokoto config"
                })),
            )
        }
    }
}

// ========== 报告过期设置 ==========

const REPORT_SETTINGS_KEY: &str = "report_settings";

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReportSettings {
    /// 是否启用报告过期（关闭时报告永不过期，保持历史行为）
    #[serde(default)]
    pub expiry_enabled: bool,
    /// 过期后读取时自动后台重新生成（只消耗 AI 调用，不重新抓平台数据）
    #[serde(default)]
    pub auto_regenerate: bool,
    #[serde(default = "default_report_expiry_days")]
    pub expiry_days: i64,
}

fn default_report_expiry_days() -> i64 {
    7
}

impl Default for ReportSettings {
    fn default() -> Self {
        Self {
            expiry_enabled: false,
            auto_regenerate: false,
            expiry_days: default_report_expiry_days(),
        }
    }
}

impl ReportSettings {
    fn normalized(mut self) -> Self {
        self.expiry_days = self.expiry_days.clamp(1, 365);
        self
    }
}

pub async fn load_report_settings(db: &DatabaseConnection) -> ReportSettings {
    let sql = "SELECT value FROM configurations WHERE key = $1";
    let result = db
        .query_one(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            sql,
            vec![REPORT_SETTINGS_KEY.into()],
        ))
        .await;

    match result {
        Ok(Some(row)) => match row.try_get::<Value>("", "value") {
            Ok(value) => serde_json::from_value::<ReportSettings>(value)
                .map(ReportSettings::normalized)
                .unwrap_or_else(|e| {
                    tracing::warn!("Invalid report settings, using defaults: {}", e);
                    ReportSettings::default()
                }),
            Err(e) => {
                tracing::warn!("Failed to read report settings: {}", e);
                ReportSettings::default()
            }
        },
        Ok(None) => ReportSettings::default(),
        Err(e) => {
            tracing::warn!("Failed to load report settings: {}", e);
            ReportSettings::default()
        }
    }
}

pub async fn get_report_settings(
    State(db): State<DatabaseConnection>,
) -> (StatusCode, Json<Value>) {
    let settings = load_report_settings(&db).await;
    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "config": settings
        })),
    )
}

pub async fn update_report_settings(
    State(db): State<DatabaseConnection>,
    Json(payload): Json<ReportSettings>,
) -> (StatusCode, Json<Value>) {
    let settings = payload.normalized();
    let config_service = crate::services::config_service::ConfigService::new(db);

    match config_service
        .update_config(
            REPORT_SETTINGS_KEY,
            serde_json::to_value(&settings).unwrap_or_else(|_| json!({})),
        )
        .await
    {
        Ok(_) => (
            StatusCode::OK,
            Json(json!({
                "success": true,
                "config": settings
            })),
        ),
        Err(e) => {
            tracing::error!("Failed to save report settings: {}", e);
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "message": "Failed to save report settings"
                })),
            )
        }
    }
}

// ========== 权限配置 API ==========

use crate::middleware::auth::extract_optional_claims;
use crate::services::permission_service::{TappPermissionService, UserRole};
use axum::http::HeaderMap;

/// 获取 Tapp 权限配置（公开端点）
/// 返回当前用户的权限等级和系统权限下放配置
#[axum::debug_handler]
pub async fn get_permissions(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
) -> (StatusCode, Json<Value>) {
    let config_service = crate::services::config_service::ConfigService::new(db);
    let config = match config_service.load_config().await {
        Ok(c) => c,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": format!("Failed to load config: {}", e)
                })),
            );
        }
    };

    // 获取当前用户角色
    let claims = extract_optional_claims(&headers);
    let role = match &claims {
        Some(c) if c.is_admin => UserRole::Admin,
        Some(c) => {
            // 检查是否为游客（负数 ID）
            if let Ok(user_id) = c.sub.parse::<i32>() {
                if user_id < 0 {
                    UserRole::Guest
                } else {
                    UserRole::User
                }
            } else {
                UserRole::Guest
            }
        }
        None => UserRole::Guest,
    };

    // 获取用户可用的权限等级
    let allowed_levels: Vec<String> = TappPermissionService::get_allowed_levels(&config, role)
        .iter()
        .map(|l| format!("{:?}", l).to_lowercase())
        .collect();

    // 获取权限下放配置
    let perm_config = TappPermissionService::get_permission_config(&config);

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "role": role.as_str(),
            "allowed_levels": allowed_levels,
            "config": perm_config
        })),
    )
}

/// 更新 Tapp 权限下放配置（仅管理员）
#[derive(Debug, Deserialize)]
pub struct UpdatePermissionsPayload {
    // 普通用户可下放的 elevated 权限（13 项）
    pub user_perm_ai_generate: Option<bool>,
    pub user_perm_ai_analyze: Option<bool>,
    pub user_perm_ai_chat: Option<bool>,
    pub user_perm_ai_image: Option<bool>,
    #[allow(dead_code)]
    pub user_perm_report_write: Option<bool>, // 忽略：强制 false
    pub user_perm_network_fetch: Option<bool>,
    pub user_perm_media_control: Option<bool>,
    pub user_perm_component_theme: Option<bool>,
    pub user_perm_shortcut_register: Option<bool>,
    pub user_perm_event_publish: Option<bool>,
    pub user_perm_scheduler_register: Option<bool>,
    pub user_perm_speech_tts: Option<bool>,
    pub user_perm_speech_asr: Option<bool>,
    // 游客 elevated 配置；认证绑定字段仅为兼容旧请求，实际强制关闭
    pub guest_perm_ai_generate: Option<bool>,
    pub guest_perm_ai_analyze: Option<bool>,
    pub guest_perm_ai_chat: Option<bool>,
    pub guest_perm_ai_image: Option<bool>,
    #[allow(dead_code)]
    pub guest_perm_report_write: Option<bool>, // 忽略：强制 false
    pub guest_perm_network_fetch: Option<bool>,
    pub guest_perm_media_control: Option<bool>,
    #[allow(dead_code)] // accepted for compatibility; update endpoint forces false
    pub guest_perm_component_theme: Option<bool>,
    #[allow(dead_code)] // accepted for compatibility; update endpoint forces false
    pub guest_perm_shortcut_register: Option<bool>,
    pub guest_perm_event_publish: Option<bool>,
    #[allow(dead_code)] // accepted for compatibility; update endpoint forces false
    pub guest_perm_scheduler_register: Option<bool>,
    #[allow(dead_code)] // accepted for compatibility; update endpoint forces false
    pub guest_perm_speech_tts: Option<bool>,
    #[allow(dead_code)] // accepted for compatibility; update endpoint forces false
    pub guest_perm_speech_asr: Option<bool>,
    // AI 使用限额配置
    pub user_ai_daily_calls: Option<i32>,
    pub user_ai_daily_tokens: Option<i32>,
    pub user_ai_cooldown_seconds: Option<i32>,
    pub guest_ai_daily_calls: Option<i32>,
    pub guest_ai_daily_tokens: Option<i32>,
    pub guest_ai_cooldown_seconds: Option<i32>,
}

#[cfg(test)]
mod tapp_permission_payload_tests {
    use super::UpdatePermissionsPayload;

    #[test]
    fn accepts_speech_permission_delegation_fields() {
        let payload: UpdatePermissionsPayload = serde_json::from_value(serde_json::json!({
            "user_perm_speech_tts": true,
            "user_perm_speech_asr": false,
            "guest_perm_speech_tts": false,
            "guest_perm_speech_asr": true
        }))
        .unwrap();

        assert_eq!(payload.user_perm_speech_tts, Some(true));
        assert_eq!(payload.user_perm_speech_asr, Some(false));
        assert_eq!(payload.guest_perm_speech_tts, Some(false));
        assert_eq!(payload.guest_perm_speech_asr, Some(true));
    }
}

#[axum::debug_handler]
pub async fn update_permissions(
    State(db): State<DatabaseConnection>,
    Json(payload): Json<UpdatePermissionsPayload>,
) -> (StatusCode, Json<Value>) {
    let config_service = crate::services::config_service::ConfigService::new(db);
    let mut updates = std::collections::HashMap::new();

    // 普通用户权限（13 项 elevated）
    if let Some(v) = payload.user_perm_ai_generate {
        updates.insert("user_perm_ai_generate".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_ai_analyze {
        updates.insert("user_perm_ai_analyze".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_ai_chat {
        updates.insert("user_perm_ai_chat".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_ai_image {
        updates.insert("user_perm_ai_image".to_string(), json!(v));
    }
    // report:write 不再下放：强制写入 false
    updates.insert("user_perm_report_write".to_string(), json!(false));
    if let Some(v) = payload.user_perm_network_fetch {
        updates.insert("user_perm_network_fetch".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_media_control {
        updates.insert("user_perm_media_control".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_component_theme {
        updates.insert("user_perm_component_theme".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_shortcut_register {
        updates.insert("user_perm_shortcut_register".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_event_publish {
        updates.insert("user_perm_event_publish".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_scheduler_register {
        updates.insert("user_perm_scheduler_register".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_speech_tts {
        updates.insert("user_perm_speech_tts".to_string(), json!(v));
    }
    if let Some(v) = payload.user_perm_speech_asr {
        updates.insert("user_perm_speech_asr".to_string(), json!(v));
    }

    // 游客权限。需要持久登录主体的能力保留兼容字段，但强制关闭。
    if let Some(v) = payload.guest_perm_ai_generate {
        updates.insert("guest_perm_ai_generate".to_string(), json!(v));
    }
    if let Some(v) = payload.guest_perm_ai_analyze {
        updates.insert("guest_perm_ai_analyze".to_string(), json!(v));
    }
    if let Some(v) = payload.guest_perm_ai_chat {
        updates.insert("guest_perm_ai_chat".to_string(), json!(v));
    }
    if let Some(v) = payload.guest_perm_ai_image {
        updates.insert("guest_perm_ai_image".to_string(), json!(v));
    }
    // report:write 不再下放：强制写入 false
    updates.insert("guest_perm_report_write".to_string(), json!(false));
    if let Some(v) = payload.guest_perm_network_fetch {
        updates.insert("guest_perm_network_fetch".to_string(), json!(v));
    }
    if let Some(v) = payload.guest_perm_media_control {
        updates.insert("guest_perm_media_control".to_string(), json!(v));
    }
    updates.insert("guest_perm_component_theme".to_string(), json!(false));
    updates.insert("guest_perm_shortcut_register".to_string(), json!(false));
    if let Some(v) = payload.guest_perm_event_publish {
        updates.insert("guest_perm_event_publish".to_string(), json!(v));
    }
    updates.insert("guest_perm_scheduler_register".to_string(), json!(false));
    updates.insert("guest_perm_speech_tts".to_string(), json!(false));
    updates.insert("guest_perm_speech_asr".to_string(), json!(false));

    // AI 使用限额配置
    if let Some(v) = payload.user_ai_daily_calls {
        updates.insert("user_ai_daily_calls".to_string(), json!(v));
    }
    if let Some(v) = payload.user_ai_daily_tokens {
        updates.insert("user_ai_daily_tokens".to_string(), json!(v));
    }
    if let Some(v) = payload.user_ai_cooldown_seconds {
        updates.insert("user_ai_cooldown_seconds".to_string(), json!(v));
    }
    if let Some(v) = payload.guest_ai_daily_calls {
        updates.insert("guest_ai_daily_calls".to_string(), json!(v));
    }
    if let Some(v) = payload.guest_ai_daily_tokens {
        updates.insert("guest_ai_daily_tokens".to_string(), json!(v));
    }
    if let Some(v) = payload.guest_ai_cooldown_seconds {
        updates.insert("guest_ai_cooldown_seconds".to_string(), json!(v));
    }

    if updates.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "message": "No permission settings provided"
            })),
        );
    }

    if let Err(e) = config_service.update_configs(updates).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "message": format!("Failed to update permissions: {}", e)
            })),
        );
    }

    // 刷新全局配置缓存
    match config_service.load_config().await {
        Ok(new_config) => {
            *crate::GLOBAL_DYNAMIC_CONFIG.write().await = new_config;
            tracing::info!("✅ Global dynamic config refreshed after permission update");
        }
        Err(e) => {
            tracing::warn!("⚠️ Failed to refresh global config: {}", e);
        }
    }

    tracing::info!("✅ Tapp permission delegation settings updated by admin");

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "Permission settings updated successfully"
        })),
    )
}

// ============================================================================
// PR #6: OAuth Providers + 本地注册开关 — 专用端点
// ============================================================================
// 详见 docs/oauth-refactor-plan.md §5、§7
//
// GitHub 可以作为 kind="github" 的 provider entry 配置；旧的
// github_client_id/github_client_secret 字段保留为兼容镜像。
// 这里集中处理 provider 列表 + 注册开关。

/// GET /api/config/oauth-providers
///
/// 返回 OIDC providers 列表 + 本地注册开关。
/// `client_secret` 字段在响应中被掩码（仅在数据库已设置时返回 `***`），
/// 前端不应展示明文；保存时若收到 `***` 表示用户没改，沿用旧值。
pub async fn get_oauth_providers() -> (StatusCode, Json<Value>) {
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;

    let mut providers: Vec<Value> = config
        .oauth_providers
        .iter()
        .map(|p| {
            json!({
                "slug": p.slug,
                "kind": p.kind,
                "display_name": p.display_name,
                "enabled": p.enabled,
                "client_id": p.client_id,
                "client_secret": if p.client_secret.is_empty() { "" } else { "***" },
                "scopes": p.scopes,
                "discovery_url": p.discovery_url,
                "icon_url": p.icon_url,
            })
        })
        .collect();

    // 自动迁移：若 legacy github_client_id 有值但 entries 里没有 slug="github"，
    // 合成一条只读 entry 展示给前端。客户端首次保存时会写到 oauth_providers。
    let has_github_entry = config.oauth_providers.iter().any(|p| p.slug == "github");
    if !has_github_entry {
        if let (Some(cid), Some(_csec)) = (
            config.github_client_id.as_ref().filter(|s| !s.is_empty()),
            config
                .github_client_secret
                .as_ref()
                .filter(|s| !s.is_empty()),
        ) {
            providers.insert(
                0,
                json!({
                    "slug": "github",
                    "kind": "github",
                    "display_name": "GitHub",
                    "enabled": true,
                    "client_id": cid,
                    "client_secret": "***",
                    "scopes": Vec::<String>::new(),
                    "discovery_url": null,
                    "icon_url": null,
                }),
            );
        }
    }

    (
        StatusCode::OK,
        Json(json!({
            "providers": providers,
            "allow_local_registration": config.allow_local_registration,
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct UpdateOAuthProvidersPayload {
    pub providers: Vec<crate::config::OAuthProviderEntry>,
    pub allow_local_registration: bool,
}

/// PUT /api/config/oauth-providers
///
/// 全量覆盖 providers 列表 + 注册开关。
/// 校验：
/// 1. slug 必填、URL-safe、不能重复
/// 2. kind="oidc" 时 discovery_url 必填
/// 3. client_secret 若为掩码 `***`，沿用现有 secret
///
/// 保存后触发 [`ProviderRegistry::reload`]。
pub async fn update_oauth_providers(
    State(db): State<DatabaseConnection>,
    Json(mut payload): Json<UpdateOAuthProvidersPayload>,
) -> (StatusCode, Json<Value>) {
    // 校验 + secret 回填
    let mut seen = std::collections::HashSet::new();
    {
        let current = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        for p in payload.providers.iter_mut() {
            let slug = p.slug.trim();
            if slug.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": "Provider slug is required"})),
                );
            }
            // slug 必须 URL-safe（路由参数）：字母数字 + 连字符/下划线，2-32 字符
            if slug.len() > 32
                || !slug
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!("invalid slug '{}': only ASCII letters, digits, '-' and '_' allowed (max 32 chars)", slug)
                    })),
                );
            }
            p.slug = slug.to_string();
            if !seen.insert(p.slug.clone()) {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({"error": format!("duplicate provider slug: {}", p.slug)})),
                );
            }
            if p.enabled && p.client_id.trim().is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!("provider '{}' requires client_id (disable it if not ready)", p.slug)
                    })),
                );
            }
            match p.kind.as_str() {
                "github" => { /* no extra requirements */ }
                "oidc" => {
                    if p.discovery_url.as_deref().unwrap_or("").trim().is_empty() {
                        return (
                            StatusCode::BAD_REQUEST,
                            Json(json!({
                                "error": format!("OIDC provider '{}' requires discovery_url", p.slug)
                            })),
                        );
                    }
                }
                other => {
                    return (
                        StatusCode::BAD_REQUEST,
                        Json(json!({
                            "error": format!("unsupported provider kind '{}'", other),
                            "message": "kind must be 'github' or 'oidc'"
                        })),
                    );
                }
            }
            // secret 回填：前端送 "***" 表示沿用
            if p.client_secret == "***" || p.client_secret.is_empty() {
                if let Some(existing) = current.oauth_providers.iter().find(|e| e.slug == p.slug) {
                    p.client_secret = existing.client_secret.clone();
                } else if p.kind == "github" && p.slug == "github" {
                    // 从 legacy 字段拿一次作为初值
                    if let Some(legacy) = current
                        .github_client_secret
                        .as_ref()
                        .filter(|s| !s.is_empty())
                    {
                        p.client_secret = legacy.clone();
                    } else {
                        p.client_secret.clear();
                    }
                } else {
                    p.client_secret.clear();
                }
            }

            // 启用的 provider 必须有 secret（兜底检查，回填后仍为空才报错）
            if p.enabled && p.client_secret.is_empty() {
                return (
                    StatusCode::BAD_REQUEST,
                    Json(json!({
                        "error": format!("provider '{}' requires client_secret (disable it if not ready)", p.slug)
                    })),
                );
            }
        }
    }

    let config_service = crate::services::config_service::ConfigService::new(db);
    let mut updates = std::collections::HashMap::new();
    let providers_json = match serde_json::to_value(&payload.providers) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": format!("Failed to serialize providers: {}", e)})),
            );
        }
    };
    updates.insert("oauth_providers".to_string(), providers_json);
    updates.insert(
        "allow_local_registration".to_string(),
        json!(payload.allow_local_registration),
    );

    // 兼容镜像：若 entries 里有 slug="github"，同时写到 legacy 平铺字段；
    // 反之则清空它们，让 registry 不会同时拿到两份冲突的凭证。
    if let Some(gh) = payload
        .providers
        .iter()
        .find(|p| p.slug == "github" && p.kind == "github")
    {
        updates.insert("github_client_id".to_string(), json!(gh.client_id));
        updates.insert("github_client_secret".to_string(), json!(gh.client_secret));
    } else {
        updates.insert("github_client_id".to_string(), json!(""));
        updates.insert("github_client_secret".to_string(), json!(""));
    }

    if let Err(e) = config_service.update_configs(updates).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": format!("Failed to save: {}", e)})),
        );
    }

    // 刷新全局配置缓存
    match config_service.load_config().await {
        Ok(new_config) => {
            *crate::GLOBAL_DYNAMIC_CONFIG.write().await = new_config;
            tracing::info!("✅ Global dynamic config refreshed (oauth providers)");
        }
        Err(e) => {
            tracing::warn!("⚠️ Failed to refresh global config: {}", e);
        }
    }

    // 热重载 OAuth 注册中心
    crate::services::oauth::registry::REGISTRY.reload().await;

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "providers_count": payload.providers.len(),
            "allow_local_registration": payload.allow_local_registration,
        })),
    )
}
