//! Build the admin config bag from DB (and reconcile platform auto-refresh).
use axum::{Json, http::StatusCode};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use super::flags::{admin_platform_enabled, nonempty_db, sort_platforms_by_order};
use super::secrets::mask_secret_display_value;
use super::types::{
    AiConfig, ConfigField, ConfigResponse, PlatformAutoFetchConfig, PlatformConfig, TripoConfig,
    UiConfig,
};
use crate::config::ModelTier;
use crate::services::platform_id::PlatformId;

/// Fails when stored configuration cannot be read (database or decryption).
/// Falling back to env here would hand the admin form empty secrets, and
/// saving that form writes every secret field as cleared.
pub(crate) async fn build_config(
    db: &DatabaseConnection,
    reveal_sensitive: bool,
) -> Result<ConfigResponse, String> {
    let db = db.clone();
    // 优先从数据库读取配置
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let stored = config_service.load_config().await.map_err(|error| {
        tracing::error!(error = %error, "stored configuration could not be read");
        "Stored configuration could not be read".to_string()
    })?;

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

    let has_bangumi_username = nonempty_db(stored.bangumi_username.as_ref());
    let has_bangumi_access_token = nonempty_db(stored.bangumi_access_token.as_ref());
    let has_bangumi_identity = has_bangumi_username || has_bangumi_access_token;
    let has_youtube_key = nonempty_db(stored.youtube_api_key.as_ref());
    let has_x_bearer = nonempty_db(stored.x_bearer_token.as_ref());
    let has_discord_token = nonempty_db(stored.discord_access_token.as_ref());
    let has_mal_username = nonempty_db(stored.mal_username.as_ref());
    let has_openxbl_key = nonempty_db(stored.openxbl_api_key.as_ref());
    let has_psn_npsso = nonempty_db(stored.psn_npsso.as_ref());
    // 开关显示的是「按表单现状保存之后」的启用：同一条 PlatformId 规则，作用在表单
    // 里显示（也会被保存回去）的凭据上。
    let enabled = {
        let admin = admin_platform_enabled(&stored);
        move |id: PlatformId| admin.iter().any(|(p, on)| *p == id && *on)
    };

    let config = ConfigResponse {
        platforms: vec![
            PlatformConfig {
                name: "GitHub".to_string(),
                enabled: enabled(PlatformId::Github),
                has_token: nonempty_db(stored.github_token.as_ref()),
                icon: "".to_string(),
                description: "Repos, stars, and contributions".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "GitHub Username".to_string(),
                        field_type: "text".to_string(),
                        value: stored.github_username.clone().unwrap_or_default(),
                        placeholder: "octocat".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "token".to_string(),
                        label: "Personal Access Token (Optional)".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.github_token.clone().unwrap_or_default()),
                        placeholder: "ghp_xxxxxxxxxxxx (Increases API rate limit)".to_string(),
                        required: false,
                    },
                ],
            },
            PlatformConfig {
                name: "Bilibili".to_string(),
                enabled: enabled(PlatformId::Bilibili),
                has_token: nonempty_db(stored.bilibili_uid.as_ref()),
                icon: "".to_string(),
                description: "Favorites, anime, and viewing history".to_string(),
                config_fields: vec![ConfigField {
                    key: "uid".to_string(),
                    label: "User ID (UID)".to_string(),
                    field_type: "number".to_string(),
                    value: stored.bilibili_uid.clone().unwrap_or_default(),
                    placeholder: "123456789".to_string(),
                    required: true,
                }],
            },
            PlatformConfig {
                name: "Steam".to_string(),
                enabled: enabled(PlatformId::Steam),
                has_token: nonempty_db(stored.steam_api_key.as_ref()),
                icon: "".to_string(),
                description: "Library, wishlist, and play stats".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "api_key".to_string(),
                        label: "Steam API Key".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.steam_api_key.clone().unwrap_or_default()),
                        placeholder: "Get from steamcommunity.com/dev/apikey".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "steam_id".to_string(),
                        label: "Steam ID".to_string(),
                        field_type: "text".to_string(),
                        value: stored.steam_id.clone().unwrap_or_default(),
                        placeholder: "76561198XXXXXXXXX".to_string(),
                        required: true,
                    },
                ],
            },
            PlatformConfig {
                name: "YouTube".to_string(),
                enabled: enabled(PlatformId::Youtube),
                has_token: has_youtube_key,
                icon: "".to_string(),
                description: "Public channel stats and recent uploads".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "api_key".to_string(),
                        label: "YouTube Data API Key".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.youtube_api_key.clone().unwrap_or_default()),
                        placeholder: "Google Cloud → YouTube Data API v3 key".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "channel_id".to_string(),
                        label: "Channel ID or @handle".to_string(),
                        field_type: "text".to_string(),
                        value: stored.youtube_channel_id.clone().unwrap_or_default(),
                        placeholder: "UCxxxxx or @GoogleDevelopers".to_string(),
                        required: true,
                    },
                ],
            },
            PlatformConfig {
                name: "Netease Music".to_string(),
                enabled: enabled(PlatformId::Netease),
                has_token: nonempty_db(stored.netease_user_id.as_ref()),
                icon: "".to_string(),
                description: "Liked songs and music taste".to_string(),
                config_fields: vec![ConfigField {
                    key: "user_id".to_string(),
                    label: "User ID".to_string(),
                    field_type: "number".to_string(),
                    value: stored.netease_user_id.clone().unwrap_or_default(),
                    placeholder: "Your Netease Cloud Music user ID".to_string(),
                    required: true,
                }],
            },
            PlatformConfig {
                name: "Bangumi".to_string(),
                enabled: enabled(PlatformId::Bangumi),
                has_token: has_bangumi_identity,
                icon: "".to_string(),
                description: "Collections, ratings, and watching status".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "Bangumi Username".to_string(),
                        field_type: "text".to_string(),
                        value: stored.bangumi_username.clone().unwrap_or_default(),
                        placeholder: "your Bangumi username".to_string(),
                        required: false,
                    },
                    ConfigField {
                        key: "access_token".to_string(),
                        label: "Access Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.bangumi_access_token.clone().unwrap_or_default()),
                        placeholder: "Bearer token for private collections".to_string(),
                        required: false,
                    },
                    ConfigField {
                        key: "user_agent".to_string(),
                        label: "User-Agent".to_string(),
                        field_type: "text".to_string(),
                        value: stored.bangumi_user_agent.clone().unwrap_or_default(),
                        placeholder: "myriad/Myriad".to_string(),
                        required: false,
                    },
                ],
            },
            PlatformConfig {
                name: "X".to_string(),
                enabled: enabled(PlatformId::X),
                has_token: has_x_bearer,
                icon: "".to_string(),
                description: "Profile and posts, with sharing".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "X Username".to_string(),
                        field_type: "text".to_string(),
                        value: stored.x_username.clone().unwrap_or_default(),
                        placeholder: String::new(),
                        required: true,
                    },
                    ConfigField {
                        key: "bearer_token".to_string(),
                        label: "Bearer Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.x_bearer_token.clone().unwrap_or_default()),
                        placeholder: "From developer.x.com App keys (read-only sync)".to_string(),
                        required: true,
                    },
                ],
            },
            PlatformConfig {
                name: "Discord".to_string(),
                enabled: enabled(PlatformId::Discord),
                has_token: has_discord_token,
                icon: "".to_string(),
                description: "Profile, servers, and linked accounts".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "access_token".to_string(),
                        label: "Access Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.discord_access_token.clone().unwrap_or_default()),
                        placeholder:
                            "OAuth user token (scopes: identify guilds connections)".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "refresh_token".to_string(),
                        label: "Refresh Token (Recommended)".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.discord_refresh_token.clone().unwrap_or_default()),
                        placeholder:
                            "Optional; enables auto-refresh when access token expires".to_string(),
                        required: false,
                    },
                    ConfigField {
                        key: "user_id".to_string(),
                        label: "User ID (auto-filled after test)".to_string(),
                        field_type: "text".to_string(),
                        value: stored.discord_user_id.clone().unwrap_or_default(),
                        placeholder: "Discord snowflake id".to_string(),
                        required: false,
                    },
                ],
            },
            PlatformConfig {
                name: "MyAnimeList".to_string(),
                enabled: enabled(PlatformId::Mal),
                has_token: has_mal_username,
                icon: "".to_string(),
                description: "Anime / manga lists and scores".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "username".to_string(),
                        label: "MyAnimeList Username".to_string(),
                        field_type: "text".to_string(),
                        value: stored.mal_username.clone().unwrap_or_default(),
                        placeholder: "your MAL username (required)".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "client_id".to_string(),
                        label: "Client ID (optional)".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.mal_client_id.clone().unwrap_or_default()),
                        placeholder:
                            "Optional — leave empty for public list (load.json); fill for official API (myanimelist.net/apiconfig)"
                                .to_string(),
                        required: false,
                    },
                ],
            },
            PlatformConfig {
                name: "Xbox".to_string(),
                enabled: enabled(PlatformId::Xbox),
                has_token: has_openxbl_key,
                icon: "".to_string(),
                description: "Achievements, Gamerscore, and recent games".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "gamertag".to_string(),
                        label: "Gamertag".to_string(),
                        field_type: "text".to_string(),
                        value: stored.xbox_gamertag.clone().unwrap_or_default(),
                        placeholder: "Major Nelson or Name#1234".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "openxbl_api_key".to_string(),
                        label: "OpenXBL API Key".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.openxbl_api_key.clone().unwrap_or_default()),
                        placeholder: "From xbl.io profile".to_string(),
                        required: true,
                    },
                ],
            },
            PlatformConfig {
                name: "PlayStation".to_string(),
                enabled: enabled(PlatformId::Psn),
                has_token: has_psn_npsso,
                icon: "".to_string(),
                description: "Trophies, trophy level, and recent games".to_string(),
                config_fields: vec![
                    ConfigField {
                        key: "online_id".to_string(),
                        label: "Online ID".to_string(),
                        field_type: "text".to_string(),
                        value: stored.psn_online_id.clone().unwrap_or_default(),
                        placeholder: "Your PSN Online ID".to_string(),
                        required: true,
                    },
                    ConfigField {
                        key: "npsso".to_string(),
                        label: "NPSSO Token".to_string(),
                        field_type: "password".to_string(),
                        value: mask_sensitive(stored.psn_npsso.clone().unwrap_or_default()),
                        placeholder: "64-char token from ca.account.sony.com".to_string(),
                        required: true,
                    },
                ],
            },
        ],
        auto_fetch: Some(PlatformAutoFetchConfig {
            enabled: stored.enable_auto_fetch,
            interval_hours: crate::services::platform_auto_refresh::clamp_interval_hours(
                stored.fetch_interval_hours,
            ),
        }),
        ai_config: AiConfig {
            config_fields: vec![
                // 文本模型：每档一个服务商源加一个模型名。源显示的是实际在用的那个
                // （Lite / Pro 没选过时是 Standard 的，判断与向量没选过时是 Lite 的）。
                ConfigField {
                    key: "ai_source".to_string(),
                    label: "Standard AI source".to_string(),
                    field_type: "text".to_string(),
                    value: stored.tier_source(ModelTier::Standard),
                    placeholder: String::new(),
                    required: false,
                },
                ConfigField {
                    key: "ai_model".to_string(),
                    label: "Standard model".to_string(),
                    field_type: "text".to_string(),
                    value: stored.ai_model.clone(),
                    placeholder: "minimax/minimax-m3, gpt-5.6-terra, etc.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "lite_ai_source".to_string(),
                    label: "Lite AI source".to_string(),
                    field_type: "text".to_string(),
                    value: stored.tier_source(ModelTier::Lite),
                    placeholder: String::new(),
                    required: false,
                },
                ConfigField {
                    key: "lite_ai_model".to_string(),
                    label: "Lite model".to_string(),
                    field_type: "text".to_string(),
                    value: stored.lite_ai_model.clone(),
                    placeholder: "Blank: Lite is not used".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pro_enabled".to_string(),
                    label: "Enable Pro Model".to_string(),
                    field_type: "boolean".to_string(),
                    value: stored.pro_enabled.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pro_ai_source".to_string(),
                    label: "Pro AI source".to_string(),
                    field_type: "text".to_string(),
                    value: stored.tier_source(ModelTier::Pro),
                    placeholder: String::new(),
                    required: false,
                },
                ConfigField {
                    key: "pro_ai_model".to_string(),
                    label: "Pro model".to_string(),
                    field_type: "text".to_string(),
                    value: stored.pro_ai_model.clone(),
                    placeholder: "anthropic/claude-opus-5.5, gpt-5.6-sol, etc.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "aux_ai_source".to_string(),
                    label: "Judgment and embedding source".to_string(),
                    field_type: "text".to_string(),
                    value: stored.aux_source_slug(),
                    placeholder: String::new(),
                    required: false,
                },
                ConfigField {
                    key: "aux_judge_model".to_string(),
                    label: "Judgment Model".to_string(),
                    field_type: "text".to_string(),
                    value: stored.aux_judge_model.clone(),
                    placeholder: "Blank: same as the Lite model".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "aux_embedding_model".to_string(),
                    label: "Embedding Model".to_string(),
                    field_type: "text".to_string(),
                    value: stored.aux_embedding_model.clone(),
                    placeholder: "Blank: recall by words only".to_string(),
                    required: false,
                },
                // AI 图片生成配置
                ConfigField {
                    key: "ai_image_model".to_string(),
                    label: "Model Name".to_string(),
                    field_type: "text".to_string(),
                    value: stored.ai_image_model.clone(),
                    placeholder: "openai/gpt-image-2.5-sunburst".to_string(),
                    required: false,
                },
                // 语音的模型和音色（源是 speech_source）
                ConfigField {
                    key: "speech_stt_model".to_string(),
                    label: "Speech-to-text model".to_string(),
                    field_type: "text".to_string(),
                    value: stored.speech_stt_model.clone(),
                    placeholder: "gpt-transcribe".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "speech_tts_model".to_string(),
                    label: "Text-to-speech model".to_string(),
                    field_type: "text".to_string(),
                    value: stored.speech_tts_model.clone(),
                    placeholder: "gpt-4o-mini-tts".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "speech_tts_voice".to_string(),
                    label: "TTS voice".to_string(),
                    field_type: "text".to_string(),
                    value: stored.speech_tts_voice.clone(),
                    placeholder: "marin".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "provider_openrouter_api_key".to_string(),
                    label: "OpenRouter API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        stored.shared_openrouter_api_key()
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
                        stored.shared_openai_api_key()
                            .unwrap_or_default(),
                    ),
                    placeholder: "sk-...".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "provider_gemini_api_key".to_string(),
                    label: "Gemini API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        stored.shared_gemini_api_key()
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
                        stored.shared_tinyfish_api_key()
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
                        stored.shared_volcengine_api_key()
                            .unwrap_or_default(),
                    ),
                    placeholder: "Ark API key".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_vendor_sources".to_string(),
                    label: "AI vendor sources".to_string(),
                    field_type: "text".to_string(),
                    value: {
                            let mut sources = stored.effective_vendor_sources();
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
                        },
                    placeholder: "[]".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ai_image_source".to_string(),
                    label: "Image AI source".to_string(),
                    field_type: "text".to_string(),
                    value: stored.image_source(),
                    placeholder: "".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "speech_source".to_string(),
                    label: "Speech source".to_string(),
                    field_type: "text".to_string(),
                    value: stored.speech_source_slug(),
                    placeholder: "".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "qq_bot_enabled".to_string(),
                    label: "QQ bot".to_string(),
                    field_type: "boolean".to_string(),
                    value: stored.qq_bot_enabled.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "qq_bot_app_id".to_string(),
                    label: "QQ bot AppID".to_string(),
                    field_type: "text".to_string(),
                    value: stored.qq_bot_app_id.clone(),
                    placeholder: "102...".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "qq_bot_app_secret".to_string(),
                    label: "QQ bot AppSecret".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        stored.qq_bot_app_secret.clone()
                            .unwrap_or_default(),
                    ),
                    placeholder: "".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "telegram_bot_enabled".to_string(),
                    label: "Telegram bot".to_string(),
                    field_type: "boolean".to_string(),
                    value: stored.telegram_bot_enabled.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "telegram_bot_token".to_string(),
                    label: "Telegram bot token".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        stored.telegram_bot_token.clone()
                            .unwrap_or_default(),
                    ),
                    placeholder: "".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "discord_bot_enabled".to_string(),
                    label: "Discord bot".to_string(),
                    field_type: "boolean".to_string(),
                    value: stored.discord_bot_enabled.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "discord_bot_token".to_string(),
                    label: "Discord bot token".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        stored.discord_bot_token.clone()
                            .unwrap_or_default(),
                    ),
                    placeholder: "".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "feishu_bot_enabled".to_string(),
                    label: "Feishu bot".to_string(),
                    field_type: "boolean".to_string(),
                    value: stored.feishu_bot_enabled.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "feishu_bot_app_id".to_string(),
                    label: "Feishu bot AppID".to_string(),
                    field_type: "text".to_string(),
                    value: stored.feishu_bot_app_id.clone(),
                    placeholder: "cli_...".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "feishu_bot_app_secret".to_string(),
                    label: "Feishu bot AppSecret".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        stored.feishu_bot_app_secret.clone()
                            .unwrap_or_default(),
                    ),
                    placeholder: "".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "onebot_bot_enabled".to_string(),
                    label: "OneBot bot".to_string(),
                    field_type: "boolean".to_string(),
                    value: stored.onebot_bot_enabled.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "onebot_bot_groups_enabled".to_string(),
                    label: "OneBot groups".to_string(),
                    field_type: "boolean".to_string(),
                    value: stored.onebot_bot_groups_enabled.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "onebot_bot_group_ids".to_string(),
                    label: "OneBot group allowlist".to_string(),
                    field_type: "text".to_string(),
                    value: stored.onebot_bot_group_ids.clone(),
                    placeholder: "".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "onebot_bot_ws_url".to_string(),
                    label: "OneBot WebSocket URL".to_string(),
                    field_type: "text".to_string(),
                    value: stored.onebot_bot_ws_url.clone(),
                    placeholder: "ws://127.0.0.1:3001".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "onebot_bot_access_token".to_string(),
                    label: "OneBot access token".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(
                        stored.onebot_bot_access_token.clone()
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
                    value: stored.tripo_enabled.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "tripo_api_key".to_string(),
                    label: "Tripo API Key".to_string(),
                    field_type: "password".to_string(),
                    value: mask_sensitive(stored.tripo_api_key.clone().unwrap_or_default()),
                    placeholder: "Get from platform.tripo3d.ai".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "tripo_base_url".to_string(),
                    label: "Tripo API Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: stored.tripo_base_url.clone(),
                    placeholder: "https://openapi.tripo3d.ai/v3".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "tripo_model".to_string(),
                    label: "Default low-poly model".to_string(),
                    field_type: "text".to_string(),
                    value: stored.tripo_model.clone(),
                    placeholder: "P1-20260311".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "tripo_face_limit".to_string(),
                    label: "Default face limit".to_string(),
                    field_type: "number".to_string(),
                    value: stored.tripo_face_limit.to_string(),
                    placeholder: "5000".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "tripo_poll_interval_seconds".to_string(),
                    label: "Polling interval (seconds)".to_string(),
                    field_type: "number".to_string(),
                    value: stored.tripo_poll_interval_seconds.to_string(),
                    placeholder: "2".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "tripo_task_timeout_seconds".to_string(),
                    label: "Task timeout (seconds)".to_string(),
                    field_type: "number".to_string(),
                    value: stored.tripo_task_timeout_seconds.to_string(),
                    placeholder: "900".to_string(),
                    required: true,
                },
                ConfigField {
                    key: "tripo_max_download_mb".to_string(),
                    label: "Maximum stored model size (MB)".to_string(),
                    field_type: "number".to_string(),
                    value: stored.tripo_max_download_mb.to_string(),
                    placeholder: "64".to_string(),
                    required: true,
                },
            ],
        },
        // 管理端 ui_config 仅 bag（见 UiConfig）
        ui_config: UiConfig {
            // ui_config.config_fields 跨页共享大袋子；按设置 Section 归属 emit。
            // 死字段（无设置页入口）勿再 emit：
            // pet_*、wallpaper_parallax（已下线，备份恢复会忽略，运行时也不再读）
            // github_client_*（走 OAuth 专用端点，勿进 admin bag）
            // 归属（uiBagOwnership）：
            // UI        → wallpaper_*, evocative_*, site_*, google_site_verification, cloud_sponsors, pwa_enabled（不含 base_url）
            // Platforms → analytics_enabled, ga_*, umami_*
            // Modules   → music_*, island_show_*
            // Advanced  → memory_saver_enabled, precise_location_enabled, proxy_*, gemini_base_url, github_api_base_url
            // AI        → merope_*
            // base_url 不进 RESET；OAuth 只读拼回调，编辑走 SiteUrlField
            config_fields: vec![
                ConfigField {
                    key: "wallpaper_url".to_string(),
                    label: "Wallpaper URL".to_string(),
                    field_type: "text".to_string(),
                    value: stored.ui_wallpaper_url.clone().unwrap_or_default(),
                    placeholder: "URL to wallpaper image or API endpoint".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "wallpaper_blur".to_string(),
                    label: "Wallpaper Blur (0-10)".to_string(),
                    field_type: "number".to_string(),
                    value: stored.ui_wallpaper_blur.to_string(),
                    placeholder: "3".to_string(),
                    required: false,
                },
                // Evocative 壁纸动效配置
                ConfigField {
                    key: "evocative_parallax".to_string(),
                    label: "Parallax effect".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.ui_evocative_parallax.to_string(),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "evocative_dynamic_blur".to_string(),
                    label: "Dynamic blur".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.ui_evocative_dynamic_blur.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "evocative_ripple".to_string(),
                    label: "Ripple effect".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.ui_evocative_ripple.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "evocative_fps".to_string(),
                    label: "Effect frame rate".to_string(),
                    field_type: "select".to_string(),
                    value: stored.ui_evocative_fps.to_string(),
                    placeholder: "30".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "evocative_ripple_quality".to_string(),
                    label: "Ripple quality".to_string(),
                    field_type: "select".to_string(),
                    value: stored.ui_evocative_ripple_quality.to_string(),
                    placeholder: "0.85".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "analytics_enabled".to_string(),
                    label: "Enable visitor analytics".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.analytics_enabled.to_string(),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "pwa_enabled".to_string(),
                    label: "Enable PWA".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.pwa_enabled.to_string(),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_title".to_string(),
                    label: "Site title".to_string(),
                    field_type: "text".to_string(),
                    value: stored.site_title.clone().unwrap_or_default(),
                    placeholder: "Myriad - A myriad of lights, in one place.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_description".to_string(),
                    label: "Site description".to_string(),
                    field_type: "text".to_string(),
                    value: stored.site_description.clone().unwrap_or_default(),
                    placeholder: "A myriad of lights, in one place.".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_favicon".to_string(),
                    label: "Favicon URL".to_string(),
                    field_type: "text".to_string(),
                    value: stored.site_favicon.clone().unwrap_or_default(),
                    placeholder: "/favicon.webp or https://example.com/icon.png (external URLs allowed)"
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_keywords".to_string(),
                    label: "SEO keywords".to_string(),
                    field_type: "text".to_string(),
                    value: stored.site_keywords.clone().unwrap_or_default(),
                    placeholder: "homepage, blog, digital life (comma-separated)".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_og_image".to_string(),
                    label: "Share preview image".to_string(),
                    field_type: "text".to_string(),
                    value: stored.site_og_image.clone().unwrap_or_default(),
                    placeholder: "https://example.com/og.png or upload a local image".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "google_site_verification".to_string(),
                    label: "Google Search Console verification".to_string(),
                    field_type: "text".to_string(),
                    value: stored.google_site_verification.clone().unwrap_or_default(),
                    placeholder: "Paste the verification code or a full meta tag".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_noindex".to_string(),
                    label: "Block search engine indexing".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.site_noindex.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_visibility_policy".to_string(),
                    label: "Search and AI visibility".to_string(),
                    field_type: "select".to_string(),
                    value: {
                        let noindex = stored.site_noindex;
                        crate::api::seo_policy::normalize_visibility_policy(
                            stored.site_visibility_policy.trim(),
                            noindex,
                        )
                        .to_string()
                    },
                    placeholder: "ai_citation".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_ai_intro".to_string(),
                    label: "AI site intro".to_string(),
                    field_type: "text".to_string(),
                    value: stored.site_ai_intro.clone().unwrap_or_default(),
                    placeholder: "2–4 sentences for AI about who this site is and what it contains (written to llms.txt)"
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_seo_review_cadence".to_string(),
                    label: "How often Agent checks public copy".to_string(),
                    field_type: "select".to_string(),
                    value: crate::api::seo_policy::normalize_seo_review_cadence(
                        stored.site_seo_review_cadence.as_str(),
                    )
                    .to_string(),
                    placeholder: "off".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "ga_measurement_id".to_string(),
                    label: "Google Analytics".to_string(),
                    field_type: "text".to_string(),
                    value: stored.ga_measurement_id.clone().unwrap_or_default(),
                    placeholder: "G-XXXXXXXXXX".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "umami_website_id".to_string(),
                    label: "Umami Website ID".to_string(),
                    field_type: "text".to_string(),
                    value: stored.umami_website_id.clone().unwrap_or_default(),
                    placeholder: "xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "umami_script_url".to_string(),
                    label: "Umami Script URL".to_string(),
                    field_type: "text".to_string(),
                    value: stored.umami_script_url.clone().unwrap_or_default(),
                    placeholder: "https://cloud.umami.is/script.js".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_icp".to_string(),
                    label: "ICP filing number".to_string(),
                    field_type: "text".to_string(),
                    value: stored.site_icp.clone()
                        .unwrap_or_default(),
                    placeholder: "e.g. 京ICP备12345678号".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_gongan".to_string(),
                    label: "Public security filing number".to_string(),
                    field_type: "text".to_string(),
                    value: stored.site_gongan.clone()
                        .unwrap_or_default(),
                    placeholder: "e.g. 京公网安备11010802012345号".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "cloud_sponsors".to_string(),
                    label: "Cloud sponsors".to_string(),
                    field_type: "text".to_string(),
                    value: stored.cloud_sponsors.clone()
                        .unwrap_or_default(),
                    placeholder: "cloudflare,edgeone,upyun (comma-separated)".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "site_footer_custom".to_string(),
                    label: "Footer custom items".to_string(),
                    field_type: "text".to_string(),
                    value: stored.site_footer_custom.clone().unwrap_or_default(),
                    placeholder: r#"[{"text":"示例","icon":"/logo.webp","url":"https://example.com"}]"#
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "base_url".to_string(),
                    label: "Site Base URL".to_string(),
                    field_type: "text".to_string(),
                    value: stored.base_url.clone()
                        .unwrap_or_default(),
                    placeholder: "https://yourdomain.com (used to build OAuth callback URLs)".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "music_enabled".to_string(),
                    label: "Enable Music Player".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.music_enabled.clone().unwrap_or_default(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "music_source".to_string(),
                    label: "Music Source".to_string(),
                    field_type: "select".to_string(),
                    value: stored.music_source.clone().unwrap_or_default(),
                    placeholder: "netease or qq".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "music_playlist_id".to_string(),
                    label: "Playlist ID".to_string(),
                    field_type: "text".to_string(),
                    value: stored.music_playlist_id.clone().unwrap_or_default(),
                    placeholder: "Playlist ID from music platform".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "music_proxy_enabled".to_string(),
                    label: "Music stream proxy".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.music_proxy_enabled.to_string(),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "music_preload_enabled".to_string(),
                    label: "Music preload".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.music_preload_enabled.to_string(),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "island_show_greeting".to_string(),
                    label: "Island greeting".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.island_show_greeting.to_string(),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "island_show_weather".to_string(),
                    label: "Island weather".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.island_show_weather.to_string(),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "island_show_quote".to_string(),
                    label: "Island quote".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.island_show_quote.to_string(),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "island_show_music".to_string(),
                    label: "Island music".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.island_show_music.to_string(),
                    placeholder: "true".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "island_show_tapp".to_string(),
                    label: "Island Tapp content".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.island_show_tapp.to_string(),
                    placeholder: "true".to_string(),
                    required: false,
                },
                // 内存节约 / 精确位置（高级设置）
                ConfigField {
                    key: "memory_saver_enabled".to_string(),
                    label: "Memory saver".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.memory_saver_enabled.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "precise_location_enabled".to_string(),
                    label: "Precise location".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.precise_location_enabled.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "merope_enabled".to_string(),
                    label: "Agent persona".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.merope_enabled.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "merope_speech_enabled".to_string(),
                    label: "Agent persona speech".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.merope_speech_enabled.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "merope_voice_mode".to_string(),
                    label: "Agent persona voice mode".to_string(),
                    field_type: "select".to_string(),
                    // What was chosen, even if it cannot be met now (the page
                    // says why); never chosen: what she does as before.
                    value: match stored.merope_voice_mode.trim() {
                        "" => stored.merope_voice_mode_resolved().to_string(),
                        chosen => chosen.to_string(),
                    },
                    placeholder: "tts".to_string(),
                    required: false,
                },
                // Read-only: what is in effect, as the server resolves it.
                ConfigField {
                    key: "merope_voice_mode_effective".to_string(),
                    label: "Agent persona voice in effect".to_string(),
                    field_type: "readonly".to_string(),
                    value: stored.merope_voice_mode_effective(),
                    placeholder: String::new(),
                    required: false,
                },
                ConfigField {
                    key: "merope_voice_voice".to_string(),
                    label: "Agent persona voice".to_string(),
                    field_type: "text".to_string(),
                    value: stored.merope_voice_voice.clone(),
                    placeholder: "Tina".to_string(),
                    required: false,
                },
                // 网络代理配置
                ConfigField {
                    key: "proxy_enabled".to_string(),
                    label: "Enable network proxy".to_string(),
                    field_type: "checkbox".to_string(),
                    value: stored.proxy_enabled.to_string(),
                    placeholder: "false".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "proxy_url".to_string(),
                    label: "Proxy server URL".to_string(),
                    field_type: "text".to_string(),
                    value: stored.proxy_url.clone()
                        .unwrap_or_default(),
                    placeholder: "http://127.0.0.1:7890 or socks5://127.0.0.1:1080".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "proxy_bypass".to_string(),
                    label: "Proxy bypass list".to_string(),
                    field_type: "text".to_string(),
                    value: stored.proxy_bypass.clone()
                        .unwrap_or_default(),
                    placeholder: "localhost,127.0.0.1,.local".to_string(),
                    required: false,
                },
                ConfigField {
                    key: "gemini_base_url".to_string(),
                    label: "Gemini API base URL".to_string(),
                    field_type: "text".to_string(),
                    value: stored.gemini_base_url.clone()
                        .unwrap_or_default(),
                    placeholder: "https://generativelanguage.googleapis.com (leave empty for default)"
                        .to_string(),
                    required: false,
                },
                ConfigField {
                    key: "github_api_base_url".to_string(),
                    label: "GitHub API base URL".to_string(),
                    field_type: "text".to_string(),
                    value: stored.github_api_base_url.clone()
                        .unwrap_or_default(),
                    placeholder: "https://api.github.com (leave empty for default)".to_string(),
                    required: false,
                },
            ],
        },
    };

    let mut config = config;
    sort_platforms_by_order(&mut config.platforms, stored.platform_order.as_ref());

    Ok(config)
}

pub async fn get_config(crate::extract::Db(db): crate::extract::Db) -> (StatusCode, Json<Value>) {
    match build_config(&db, false).await {
        Ok(config) => (StatusCode::OK, Json(json!(config))),
        Err(message) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": message, "code": "CONFIG_UNREADABLE" })),
        ),
    }
}

/// Rebuild core platform refresh tasks from persisted configuration.
/// Called on backend startup and after saves so the database configuration
/// remains the source of truth even after a restart or interrupted settings save.
///
/// 自动刷新覆盖凭据齐备的平台，与抓取同一条判断（`configured_platform_ids`）；
/// 启用开关不参与。
pub async fn reconcile_platform_auto_refresh(
    db: &DatabaseConnection,
) -> Result<crate::services::platform_auto_refresh::PlatformAutoRefreshSummary, String> {
    let config = crate::services::config_service::ConfigService::new(db.clone())
        .load_config()
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "stored configuration could not be read");
            "Stored configuration could not be read".to_string()
        })?;
    let user_id = crate::api::profile::site_owner_user_id(db).await?;
    let platforms: Vec<String> =
        crate::services::platform_refresh::configured_platform_ids(&config)
            .into_iter()
            .map(str::to_string)
            .collect();
    crate::services::platform_auto_refresh::reconcile_platform_auto_refresh(
        db,
        user_id,
        config.enable_auto_fetch,
        config.fetch_interval_hours,
        &platforms,
    )
    .await
}
