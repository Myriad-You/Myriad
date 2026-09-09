//! Build the admin config bag from DB (and reconcile platform auto-refresh).
use axum::{http::StatusCode, Json};
use sea_orm::DatabaseConnection;
use serde_json::{json, Value};

use super::flags::{
    db_or_env_clearable, nonempty_db, nonempty_env, platform_config_is_ready,
    resolve_platform_enabled, sort_platforms_by_order,
};
use super::secrets::mask_secret_display_value;
use super::types::{
    AiConfig, ConfigField, ConfigResponse, PlatformAutoFetchConfig, PlatformConfig, ReportConfig,
    TripoConfig, UiConfig,
};

pub(crate) async fn build_config(
    db: &DatabaseConnection,
    reveal_sensitive: bool,
) -> ConfigResponse {
    let db = db.clone();
    // 优先从数据库读取配置
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let db_config = config_service.load_config().await.ok();

    // Prefer DB when present (including intentional empty clear); else process env.
    // Same clearable semantics as SEO/analytics (`db_or_env_clearable`).
    let get_value = |db_val: Option<String>, env_key: &str| -> String {
        db_or_env_clearable(db_val, env_key, "")
    };

    // Helper to mask sensitive values (passwords, API keys, tokens).
    // Must stay aligned with [`is_masked_secret_value`] (save + platform_test).
    let mask_sensitive = |value: String| -> String {
        if value.trim().is_empty() || reveal_sensitive {
            if value.trim().is_empty() {
                String::new()
            } else {
                value
            }
        } else {
            mask_secret_display_value()
        }
    };

    let has_bangumi_username =
        nonempty_db(db_config.as_ref().and_then(|c| c.bangumi_username.as_ref()))
            || nonempty_env("BANGUMI_USERNAME");
    let has_bangumi_access_token = nonempty_db(
        db_config
            .as_ref()
            .and_then(|c| c.bangumi_access_token.as_ref()),
    ) || nonempty_env("BANGUMI_ACCESS_TOKEN");
    let has_bangumi_identity = has_bangumi_username || has_bangumi_access_token;
    let github_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.github_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.github_username.as_ref()))
            || nonempty_env("GITHUB_USERNAME"),
    );
    let bilibili_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.bilibili_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.bilibili_uid.as_ref()))
            || nonempty_env("BILIBILI_UID"),
    );
    let steam_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.steam_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.steam_api_key.as_ref()))
            || nonempty_env("STEAM_API_KEY"),
    );
    let has_youtube_key = nonempty_db(db_config.as_ref().and_then(|c| c.youtube_api_key.as_ref()))
        || nonempty_env("YOUTUBE_API_KEY");
    let has_youtube_channel = nonempty_db(
        db_config
            .as_ref()
            .and_then(|c| c.youtube_channel_id.as_ref()),
    ) || nonempty_env("YOUTUBE_CHANNEL_ID");
    let youtube_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.youtube_enabled),
        has_youtube_key && has_youtube_channel,
    );
    let netease_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.netease_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.netease_user_id.as_ref()))
            || nonempty_env("NETEASE_USER_ID"),
    );
    let bangumi_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.bangumi_enabled),
        has_bangumi_identity,
    );
    let has_x_username = nonempty_db(db_config.as_ref().and_then(|c| c.x_username.as_ref()))
        || nonempty_env("X_USERNAME");
    let has_x_bearer = nonempty_db(db_config.as_ref().and_then(|c| c.x_bearer_token.as_ref()))
        || nonempty_env("X_BEARER_TOKEN");
    let x_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.x_enabled),
        has_x_username && has_x_bearer,
    );
    let has_discord_token = nonempty_db(
        db_config
            .as_ref()
            .and_then(|c| c.discord_access_token.as_ref()),
    ) || nonempty_env("DISCORD_ACCESS_TOKEN");
    let discord_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.discord_enabled),
        has_discord_token,
    );
    let has_mal_username = nonempty_db(db_config.as_ref().and_then(|c| c.mal_username.as_ref()))
        || nonempty_env("MAL_USERNAME");
    let mal_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.mal_enabled),
        has_mal_username,
    );
    let has_openxbl_key = nonempty_db(db_config.as_ref().and_then(|c| c.openxbl_api_key.as_ref()))
        || nonempty_env("OPENXBL_API_KEY")
        || nonempty_env("XBL_API_KEY");
    let has_xbox_gamertag = nonempty_db(db_config.as_ref().and_then(|c| c.xbox_gamertag.as_ref()))
        || nonempty_env("XBOX_GAMERTAG");
    let xbox_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.xbox_enabled),
        has_openxbl_key && has_xbox_gamertag,
    );
    let has_psn_npsso = nonempty_db(db_config.as_ref().and_then(|c| c.psn_npsso.as_ref()))
        || nonempty_env("PSN_NPSSO");
    let has_psn_online_id = nonempty_db(db_config.as_ref().and_then(|c| c.psn_online_id.as_ref()))
        || nonempty_env("PSN_ONLINE_ID");
    let psn_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.psn_enabled),
        has_psn_npsso && has_psn_online_id,
    );

    let config = ConfigResponse {
        platforms: vec![
            PlatformConfig {
                name: "GitHub".to_string(),
                enabled: github_enabled,
                has_token: nonempty_db(db_config.as_ref().and_then(|c| c.github_token.as_ref()))
                    || nonempty_env("GITHUB_TOKEN"),
                icon: "".to_string(),
                description: "Repos, stars, and contributions".to_string(),
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
                has_token: nonempty_db(db_config.as_ref().and_then(|c| c.bilibili_uid.as_ref()))
                    || nonempty_env("BILIBILI_UID"),
                icon: "".to_string(),
                description: "Favorites, anime, and viewing history".to_string(),
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
                has_token: nonempty_db(db_config.as_ref().and_then(|c| c.steam_api_key.as_ref()))
                    || nonempty_env("STEAM_API_KEY"),
                icon: "".to_string(),
                description: "Library, wishlist, and play stats".to_string(),
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
                name: "YouTube".to_string(),
                enabled: youtube_enabled,
                has_token: has_youtube_key,
                icon: "".to_string(),
                description: "Public channel stats and recent uploads".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "api_key".to_string(),
                        label: "YouTube Data API Key".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(get_value(
                            db_config.as_ref().and_then(|c| c.youtube_api_key.clone()),
                            "YOUTUBE_API_KEY",
                        )),
                        placeholder: "Google Cloud → YouTube Data API v3 key".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "channel_id".to_string(),
                        label: "Channel ID or @handle".to_string(),
                        field_type: "text".to_string(),
                        value: get_value(
                            db_config.as_ref().and_then(|c| c.youtube_channel_id.clone()),
                            "YOUTUBE_CHANNEL_ID",
                        ),
                        placeholder: "UCxxxxx or @GoogleDevelopers".to_string(),
                        required: true,
                    },
                ],
            },
            PlatformConfig {
                name: "Netease Music".to_string(),
                enabled: netease_enabled,
                has_token: nonempty_db(db_config.as_ref().and_then(|c| c.netease_user_id.as_ref()))
                    || nonempty_env("NETEASE_USER_ID"),
                icon: "".to_string(),
                description: "Liked songs and music taste".to_string(),
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
                description: "Collections, ratings, and watching status".to_string(),
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
                        placeholder: "myriad/Myriad".to_string(),
                        required: false,
                    },
                ],
            },
            PlatformConfig {
                name: "X".to_string(),
                enabled: x_enabled,
                has_token: has_x_bearer,
                icon: "".to_string(),
                description: "Profile and posts, with sharing".to_string(),
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
                description: "Profile, servers, and linked accounts".to_string(),
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
                description: "Anime / manga lists and scores".to_string(),
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
                description: "Achievements, Gamerscore, and recent games".to_string(),
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
                description: "Trophies, trophy level, and recent games".to_string(),
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
                    label: "Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.gemini_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("GEMINI_MODEL")
                                .unwrap_or_else(|_| "gemini-3.6-flash".to_string())
                        }),
                    placeholder: "gemini-3.6-flash, gemini-3.1-pro-preview, gemini-2.5-flash, etc."
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
                    label: "Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.openai_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("OPENAI_MODEL")
                                .unwrap_or_else(|_| "minimax/minimax-m3".to_string())
                        }),
                    placeholder: "minimax/minimax-m3, gpt-5.6-terra, etc.".to_string(),
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
                    label: "Model Name".to_string(),
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
                    label: "Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.pro_openai_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("PRO_OPENAI_MODEL")
                                .unwrap_or_else(|_| "anthropic/claude-opus-5".to_string())
                        }),
                    placeholder: "anthropic/claude-opus-5, gpt-5.6-sol, etc.".to_string(),
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
                                .unwrap_or_else(|_| "openrouter".to_string())
                        }),
                    placeholder: "openai (compatible), openrouter, or volcengine"
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_model".to_string(),
                    label: "Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ai_image_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("AI_IMAGE_MODEL")
                                .unwrap_or_else(|_| "openai/gpt-image-2".to_string())
                        }),
                    placeholder: "openai/gpt-image-2".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_openai_api_key".to_string(),
                    label: "OpenAI API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.ai_image_openai_api_key.clone()),
                        "AI_IMAGE_OPENAI_API_KEY",
                    )),
                    placeholder: "Falls back to the standard OpenAI key when empty".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_openai_base_url".to_string(),
                    label: "OpenAI Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ai_image_openai_base_url.clone())
                        .unwrap_or_else(|| {
                            std::env::var("AI_IMAGE_OPENAI_BASE_URL")
                                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string())
                        }),
                    placeholder: "https://api.openai.com/v1".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_openrouter_api_key".to_string(),
                    label: "OpenAI API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.ai_image_openrouter_api_key.clone()),
                        "AI_IMAGE_OPENROUTER_API_KEY",
                    )),
                    placeholder: "Falls back to the standard OpenRouter key when compatible".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_volcengine_api_key".to_string(),
                    label: "Volcengine Ark API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.ai_image_volcengine_api_key.clone()),
                        "AI_IMAGE_VOLCENGINE_API_KEY",
                    )),
                    placeholder: "Ark API key".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_volcengine_base_url".to_string(),
                    label: "Volcengine Ark Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ai_image_volcengine_base_url.clone())
                        .unwrap_or_else(|| {
                            std::env::var("AI_IMAGE_VOLCENGINE_BASE_URL").unwrap_or_else(|_| {
                                "https://ark.cn-beijing.volces.com/api/v3".to_string()
                            })
                        }),
                    placeholder: "https://ark.cn-beijing.volces.com/api/v3".to_string(),
                    required: false,
                },
                // Lite 模型配置（与 Pro 使用同一字段协议：开关 + 字段留空回退 Standard）
                ConfigField {
                    key: "lite_enabled".to_string(),
                    label: "Enable Lite Model".to_string(),
                    field_type: "boolean".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.lite_enabled.to_string())
                        .unwrap_or_else(|| "false".to_string()),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "lite_provider".to_string(),
                    label: "【Lite Model】AI Provider".to_string(),
                    field_type: "select".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.lite_ai_provider.clone())
                        .unwrap_or_else(|| {
                            std::env::var("LITE_AI_PROVIDER")
                                .unwrap_or_else(|_| "openai".to_string())
                        }),
                    placeholder: "openai".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "lite_gemini_api_key".to_string(),
                    label: "【Lite Model】Gemini API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.lite_gemini_api_key.clone()),
                        "LITE_GEMINI_API_KEY",
                    )),
                    placeholder: "Leave empty to reuse Standard".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "lite_gemini_model".to_string(),
                    label: "Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.lite_gemini_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("LITE_GEMINI_MODEL")
                                .unwrap_or_else(|_| "gemini-3.5-flash-lite".to_string())
                        }),
                    placeholder: "gemini-3.5-flash-lite".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "lite_openai_api_key".to_string(),
                    label: "【Lite Model】OpenAI API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.lite_openai_api_key.clone()),
                        "LITE_OPENAI_API_KEY",
                    )),
                    placeholder: "Leave empty to reuse Standard".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "lite_openai_model".to_string(),
                    label: "Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.lite_openai_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("LITE_OPENAI_MODEL")
                                .unwrap_or_else(|_| "openai/gpt-oss-20b:free".to_string())
                        }),
                    placeholder: "openai/gpt-oss-20b:free, gpt-5.6-luna, etc.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "lite_openai_base_url".to_string(),
                    label: "【Lite Model】OpenAI Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.lite_openai_base_url.clone())
                        .unwrap_or_else(|| {
                            std::env::var("LITE_OPENAI_BASE_URL").unwrap_or_else(|_| {
                                "https://openrouter.ai/api/v1".to_string()
                            })
                        }),
                    placeholder: "https://openrouter.ai/api/v1".to_string(),
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
                ConfigField {
                    key: "speech_provider".to_string(),
                    label: "Speech Provider".to_string(),
                    field_type: "select".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.speech_provider.clone())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "tencent".to_string()),
                    placeholder: "tencent".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "speech_stt_model".to_string(),
                    label: "Speech-to-text model".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.speech_stt_model.clone())
                        .unwrap_or_default(),
                    placeholder: "gpt-transcribe".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "speech_tts_model".to_string(),
                    label: "Text-to-speech model".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.speech_tts_model.clone())
                        .unwrap_or_default(),
                    placeholder: "gpt-4o-mini-tts".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "speech_tts_voice".to_string(),
                    label: "TTS voice".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.speech_tts_voice.clone())
                        .unwrap_or_default(),
                    placeholder: "marin".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "speech_openai_api_key".to_string(),
                    label: "OpenAI API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.speech_openai_api_key.clone()),
                        "SPEECH_OPENAI_API_KEY",
                    )),
                    placeholder: "sk-...".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "speech_openai_base_url".to_string(),
                    label: "OpenAI Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.speech_openai_base_url.clone())
                        .filter(|s| !s.is_empty())
                        .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
                    placeholder: "https://api.openai.com/v1".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "speech_openrouter_api_key".to_string(),
                    label: "OpenRouter API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config
                            .as_ref()
                            .and_then(|c| c.speech_openrouter_api_key.clone()),
                        "SPEECH_OPENROUTER_API_KEY",
                    )),
                    placeholder: "sk-or-v1-...".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "provider_openrouter_api_key".to_string(),
                    label: "OpenRouter API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        db_config
                            .as_ref()
                            .and_then(|c| c.shared_openrouter_api_key())
                            .unwrap_or_default(),
                    ),
                    placeholder: "sk-or-v1-...".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "provider_openai_api_key".to_string(),
                    label: "OpenAI API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        db_config
                            .as_ref()
                            .and_then(|c| c.shared_openai_api_key())
                            .unwrap_or_default(),
                    ),
                    placeholder: "sk-...".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "provider_openai_base_url".to_string(),
                    label: "OpenAI Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.shared_openai_base_url())
                        .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
                    placeholder: "https://api.openai.com/v1".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "provider_gemini_api_key".to_string(),
                    label: "Gemini API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        db_config
                            .as_ref()
                            .and_then(|c| c.shared_gemini_api_key())
                            .unwrap_or_default(),
                    ),
                    placeholder: "AIza...".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "provider_tinyfish_api_key".to_string(),
                    label: "TinyFish API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        db_config
                            .as_ref()
                            .and_then(|c| c.shared_tinyfish_api_key())
                            .unwrap_or_default(),
                    ),
                    placeholder: "Get from https://agent.tinyfish.ai/api-keys".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "provider_volcengine_api_key".to_string(),
                    label: "Volcengine Ark API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        db_config
                            .as_ref()
                            .and_then(|c| c.shared_volcengine_api_key())
                            .unwrap_or_default(),
                    ),
                    placeholder: "Ark API key".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "provider_volcengine_base_url".to_string(),
                    label: "Volcengine Ark Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.shared_volcengine_base_url())
                        .unwrap_or_else(|| {
                            "https://ark.cn-beijing.volces.com/api/v3".to_string()
                        }),
                    placeholder: "https://ark.cn-beijing.volces.com/api/v3".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_vendor_sources".to_string(),
                    label: "AI vendor sources".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| {
                            let mut sources = c.effective_vendor_sources();
                            for source in &mut sources {
                                if source
                                    .api_key
                                    .as_ref()
                                    .is_some_and(|key| !key.trim().is_empty())
                                {
                                    source.api_key = Some(mask_secret_display_value());
                                }
                                if source
                                    .secret_id
                                    .as_ref()
                                    .is_some_and(|key| !key.trim().is_empty())
                                {
                                    source.secret_id = Some(mask_secret_display_value());
                                }
                                if source
                                    .secret_key
                                    .as_ref()
                                    .is_some_and(|key| !key.trim().is_empty())
                                {
                                    source.secret_key = Some(mask_secret_display_value());
                                }
                            }
                            serde_json::to_string(&sources).unwrap_or_else(|_| "[]".to_string())
                        })
                        .unwrap_or_else(|| "[]".to_string()),
                    placeholder: "[]".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_source".to_string(),
                    label: "Standard AI source".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ai_source.clone())
                        .unwrap_or_default(),
                    placeholder: "".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "lite_ai_source".to_string(),
                    label: "Lite AI source".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.lite_ai_source.clone())
                        .unwrap_or_default(),
                    placeholder: "".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pro_ai_source".to_string(),
                    label: "Pro AI source".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.pro_ai_source.clone())
                        .unwrap_or_default(),
                    placeholder: "".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_source".to_string(),
                    label: "Image AI source".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.ai_image_source.clone())
                        .unwrap_or_default(),
                    placeholder: "".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "speech_source".to_string(),
                    label: "Speech source".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.speech_source.clone())
                        .unwrap_or_default(),
                    placeholder: "".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "qq_bot_enabled".to_string(),
                    label: "QQ bot".to_string(),
                    field_type: "boolean".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.qq_bot_enabled.to_string())
                        .unwrap_or_else(|| "false".to_string()),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "qq_bot_app_id".to_string(),
                    label: "QQ bot AppID".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.qq_bot_app_id.clone())
                        .unwrap_or_default(),
                    placeholder: "102...".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "qq_bot_app_secret".to_string(),
                    label: "QQ bot AppSecret".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        db_config
                            .as_ref()
                            .and_then(|c| c.qq_bot_app_secret.clone())
                            .unwrap_or_default(),
                    ),
                    placeholder: "".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "telegram_bot_enabled".to_string(),
                    label: "Telegram bot".to_string(),
                    field_type: "boolean".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.telegram_bot_enabled.to_string())
                        .unwrap_or_else(|| "false".to_string()),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "telegram_bot_token".to_string(),
                    label: "Telegram bot token".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        db_config
                            .as_ref()
                            .and_then(|c| c.telegram_bot_token.clone())
                            .unwrap_or_default(),
                    ),
                    placeholder: "".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "discord_bot_enabled".to_string(),
                    label: "Discord bot".to_string(),
                    field_type: "boolean".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.discord_bot_enabled.to_string())
                        .unwrap_or_else(|| "false".to_string()),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "discord_bot_token".to_string(),
                    label: "Discord bot token".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        db_config
                            .as_ref()
                            .and_then(|c| c.discord_bot_token.clone())
                            .unwrap_or_default(),
                    ),
                    placeholder: "".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "feishu_bot_enabled".to_string(),
                    label: "Feishu bot".to_string(),
                    field_type: "boolean".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.feishu_bot_enabled.to_string())
                        .unwrap_or_else(|| "false".to_string()),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "feishu_bot_app_id".to_string(),
                    label: "Feishu bot AppID".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.feishu_bot_app_id.clone())
                        .unwrap_or_default(),
                    placeholder: "cli_...".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "feishu_bot_app_secret".to_string(),
                    label: "Feishu bot AppSecret".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        db_config
                            .as_ref()
                            .and_then(|c| c.feishu_bot_app_secret.clone())
                            .unwrap_or_default(),
                    ),
                    placeholder: "".to_string(),
                    required: false,
                },
            ],
        },
        tripo_config: TripoConfig {
            config_fields: vec![
                ConfigField {
                    key: "tripo_enabled".to_string(),
                    label: "Enable Tripo 3D".to_string(),
                    field_type: "boolean".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.tripo_enabled.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("TRIPO_ENABLED")
                                .unwrap_or_else(|_| "false".to_string())
                        }),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "tripo_api_key".to_string(),
                    label: "Tripo API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(get_value(
                        db_config.as_ref().and_then(|c| c.tripo_api_key.clone()),
                        "TRIPO_API_KEY",
                    )),
                    placeholder: "Get from platform.tripo3d.ai".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "tripo_base_url".to_string(),
                    label: "Tripo API Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.tripo_base_url.clone())
                        .unwrap_or_else(|| {
                            std::env::var("TRIPO_BASE_URL").unwrap_or_else(|_| {
                                "https://openapi.tripo3d.ai/v3".to_string()
                            })
                        }),
                    placeholder: "https://openapi.tripo3d.ai/v3".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "tripo_model".to_string(),
                    label: "Default low-poly model".to_string(),
                    field_type: "text".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.tripo_model.clone())
                        .unwrap_or_else(|| {
                            std::env::var("TRIPO_MODEL")
                                .unwrap_or_else(|_| "P1-20260311".to_string())
                        }),
                    placeholder: "P1-20260311".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "tripo_face_limit".to_string(),
                    label: "Default face limit".to_string(),
                    field_type: "number".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.tripo_face_limit.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("TRIPO_FACE_LIMIT")
                                .unwrap_or_else(|_| "5000".to_string())
                        }),
                    placeholder: "5000".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "tripo_poll_interval_seconds".to_string(),
                    label: "Polling interval (seconds)".to_string(),
                    field_type: "number".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.tripo_poll_interval_seconds.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("TRIPO_POLL_INTERVAL_SECONDS")
                                .unwrap_or_else(|_| "2".to_string())
                        }),
                    placeholder: "2".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "tripo_task_timeout_seconds".to_string(),
                    label: "Task timeout (seconds)".to_string(),
                    field_type: "number".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.tripo_task_timeout_seconds.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("TRIPO_TASK_TIMEOUT_SECONDS")
                                .unwrap_or_else(|_| "900".to_string())
                        }),
                    placeholder: "900".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "tripo_max_download_mb".to_string(),
                    label: "Maximum stored model size (MB)".to_string(),
                    field_type: "number".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.tripo_max_download_mb.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("TRIPO_MAX_DOWNLOAD_MB")
                                .unwrap_or_else(|_| "64".to_string())
                        }),
                    placeholder: "64".to_string(),
                    required: true,
                },
            ],
        },
        report_config: ReportConfig {
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
        // 管理端 ui_config 仅 bag；typed 镜像已删除（见 UiConfig 注释）
        ui_config: UiConfig {
            // ui_config.config_fields 跨页共享大袋子；按设置 Section 归属 emit。
            // 死字段（无设置页入口）勿再 emit：
            // pet_*、wallpaper_parallax（已下线，备份恢复会忽略，运行时也不再读）
            // github_client_*（走 OAuth 专用端点，勿进 admin bag）
            // 归属：
            // UI        → wallpaper_*, evocative_*, site_*, cloud_sponsors, pwa_enabled, base_url
            // Platforms → analytics_enabled
            // Modules   → music_*
            // Advanced  → proxy_*, gemini_base_url, github_api_base_url
            // OAuth     → 只读 base_url（编辑走 SiteUrlField 独立 API）
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
                    key: "analytics_enabled".to_string(),
                    label: "Enable visitor analytics".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.analytics_enabled.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("ANALYTICS_ENABLED")
                                .unwrap_or_else(|_| "true".to_string())
                        }),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pwa_enabled".to_string(),
                    label: "Enable PWA".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.pwa_enabled.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("PWA_ENABLED")
                                .unwrap_or_else(|_| "true".to_string())
                        }),
                    placeholder: "true".to_string(),
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
                    key: "site_keywords".to_string(),
                    label: "SEO 关键词".to_string(),
                    field_type: "text".to_string(),
                    // Clearable: empty DB wins over env (see db_or_env_clearable).
                    value: db_or_env_clearable(
                        db_config.as_ref().and_then(|c| c.site_keywords.clone()),
                        "SITE_KEYWORDS",
                        "",
                    ),
                    placeholder: "个人主页, 博客, 数字生活（逗号分隔）".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_og_image".to_string(),
                    label: "分享预览图".to_string(),
                    field_type: "text".to_string(),
                    value: db_or_env_clearable(
                        db_config.as_ref().and_then(|c| c.site_og_image.clone()),
                        "SITE_OG_IMAGE",
                        "",
                    ),
                    placeholder: "https://example.com/og.png 或上传本地图片".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "google_site_verification".to_string(),
                    label: "Google Search Console 验证".to_string(),
                    field_type: "text".to_string(),
                    value: db_or_env_clearable(
                        db_config
                            .as_ref()
                            .and_then(|c| c.google_site_verification.clone()),
                        "GOOGLE_SITE_VERIFICATION",
                        "",
                    ),
                    placeholder: "粘贴验证码或整段 meta 标签".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_noindex".to_string(),
                    label: "禁止搜索引擎收录".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.site_noindex.to_string())
                        .unwrap_or_else(|| {
                            std::env::var("SITE_NOINDEX")
                                .unwrap_or_else(|_| "false".to_string())
                        }),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_visibility_policy".to_string(),
                    label: "搜索与 AI 可见性".to_string(),
                    field_type: "select".to_string(),
                    value: {
                        let noindex = db_config
                            .as_ref()
                            .map(|c| c.site_noindex)
                            .unwrap_or_else(|| {
                                std::env::var("SITE_NOINDEX")
                                    .map(|v| v == "true" || v == "1")
                                    .unwrap_or(false)
                            });
                        let raw = db_config
                            .as_ref()
                            .map(|c| c.site_visibility_policy.clone())
                            .filter(|s| !s.trim().is_empty())
                            .or_else(|| std::env::var("SITE_VISIBILITY_POLICY").ok())
                            .unwrap_or_default();
                        crate::api::seo_policy::normalize_visibility_policy(&raw, noindex).to_string()
                    },
                    placeholder: "ai_citation".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_ai_intro".to_string(),
                    label: "AI 站点简介".to_string(),
                    field_type: "text".to_string(),
                    value: db_or_env_clearable(
                        db_config.as_ref().and_then(|c| c.site_ai_intro.clone()),
                        "SITE_AI_INTRO",
                        "",
                    ),
                    placeholder: "用 2～4 句话向 AI 说明本站是谁、有什么内容（写入 llms.txt）"
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ga_measurement_id".to_string(),
                    label: "Google Analytics".to_string(),
                    field_type: "text".to_string(),
                    value: db_or_env_clearable(
                        db_config
                            .as_ref()
                            .and_then(|c| c.ga_measurement_id.clone()),
                        "GA_MEASUREMENT_ID",
                        "",
                    ),
                    placeholder: "G-XXXXXXXXXX".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "umami_website_id".to_string(),
                    label: "Umami Website ID".to_string(),
                    field_type: "text".to_string(),
                    value: db_or_env_clearable(
                        db_config
                            .as_ref()
                            .and_then(|c| c.umami_website_id.clone()),
                        "UMAMI_WEBSITE_ID",
                        "",
                    ),
                    placeholder: "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "umami_script_url".to_string(),
                    label: "Umami Script URL".to_string(),
                    field_type: "text".to_string(),
                    value: db_or_env_clearable(
                        db_config
                            .as_ref()
                            .and_then(|c| c.umami_script_url.clone()),
                        "UMAMI_SCRIPT_URL",
                        "",
                    ),
                    placeholder: "https://cloud.umami.is/script.js".to_string(),
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
                    key: "site_footer_custom".to_string(),
                    label: "页脚自定义项".to_string(),
                    field_type: "text".to_string(),
                    value: db_or_env_clearable(
                        db_config
                            .as_ref()
                            .and_then(|c| c.site_footer_custom.clone()),
                        "SITE_FOOTER_CUSTOM",
                        "",
                    ),
                    placeholder: r#"[{"text":"示例","icon":"/logo.webp","url":"https://example.com"}]"#
                        .to_string(),
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
                ConfigField {
                    key: "island_show_greeting".to_string(),
                    label: "Island greeting".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.island_show_greeting.to_string())
                        .unwrap_or_else(|| "true".to_string()),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "island_show_weather".to_string(),
                    label: "Island weather".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.island_show_weather.to_string())
                        .unwrap_or_else(|| "true".to_string()),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "island_show_quote".to_string(),
                    label: "Island quote".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.island_show_quote.to_string())
                        .unwrap_or_else(|| "true".to_string()),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "island_show_music".to_string(),
                    label: "Island music".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.island_show_music.to_string())
                        .unwrap_or_else(|| "true".to_string()),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "island_show_tapp".to_string(),
                    label: "Island Tapp content".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.island_show_tapp.to_string())
                        .unwrap_or_else(|| "true".to_string()),
                    placeholder: "true".to_string(),
                    required: false,
                },
                // 内存节约（高级设置）
                ConfigField {
                    key: "memory_saver_enabled".to_string(),
                    label: "内存节约".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.memory_saver_enabled.to_string())
                        .unwrap_or_else(|| "false".to_string()),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "merope_enabled".to_string(),
                    label: "Agent 人设".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.merope_enabled.to_string())
                        .unwrap_or_else(|| "false".to_string()),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "merope_speech_enabled".to_string(),
                    label: "Agent 人设说话".to_string(),
                    field_type: "checkbox".to_string(),
                    value: db_config
                        .as_ref()
                        .map(|c| c.merope_speech_enabled.to_string())
                        .unwrap_or_else(|| "false".to_string()),
                    placeholder: "false".to_string(),
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

pub async fn get_config(crate::extract::Db(db): crate::extract::Db) -> (StatusCode, Json<Value>) {
    let config = build_config(&db, false).await;
    (StatusCode::OK, Json(json!(config)))
}

pub(crate) async fn reconcile_platform_auto_refresh_with_config(
    db: &DatabaseConnection,
    config: &ConfigResponse,
) -> Result<crate::services::platform_auto_refresh::PlatformAutoRefreshSummary, String> {
    let auto_fetch = config.auto_fetch.clone().unwrap_or_default();
    let user_id = crate::api::profile::site_owner_user_id(db).await?;
    // 自动刷新覆盖所有「已配置」平台；报告页 enabled 不参与
    let platforms: Vec<String> = config
        .platforms
        .iter()
        .filter(|platform| platform_config_is_ready(platform))
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
pub async fn reconcile_platform_auto_refresh(
    db: &DatabaseConnection,
) -> Result<crate::services::platform_auto_refresh::PlatformAutoRefreshSummary, String> {
    let config = build_config(db, false).await;
    reconcile_platform_auto_refresh_with_config(db, &config).await
}
