//! Public (unauthenticated) site metadata, platform cards, and UI runtime config.
use axum::{Json, http::StatusCode};
use serde_json::{Value, json};

use super::flags::{
    db_or_env_clearable, nonempty_db, nonempty_env, resolve_platform_enabled,
    sort_platforms_by_order,
};
use super::types::{ConfigField, PlatformConfig};

pub async fn get_site_metadata(
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    // 优先从数据库读取站点元数据配置
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let db_config = config_service.load_config().await.ok();

    // Branding fields: empty DB still falls through to env/default.
    let get_branding = |db_val: Option<String>, env_key: &str, default: &str| -> String {
        db_val
            .filter(|v| !v.is_empty())
            .or_else(|| std::env::var(env_key).ok().filter(|v| !v.is_empty()))
            .unwrap_or_else(|| default.to_string())
    };

    let site_noindex = db_config
        .as_ref()
        .map(|c| c.site_noindex)
        .unwrap_or_else(|| {
            std::env::var("SITE_NOINDEX")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false)
        });

    let site_visibility_policy = {
        let raw = db_config
            .as_ref()
            .map(|c| c.site_visibility_policy.clone())
            .filter(|s| !s.trim().is_empty())
            .or_else(|| std::env::var("SITE_VISIBILITY_POLICY").ok())
            .unwrap_or_default();
        crate::api::seo_policy::normalize_visibility_policy(&raw, site_noindex)
    };
    let site_ai_intro = db_or_env_clearable(
        db_config.as_ref().and_then(|c| c.site_ai_intro.clone()),
        "SITE_AI_INTRO",
        "",
    );

    let metadata = json!({
        "site_title": get_branding(
            db_config.as_ref().and_then(|c| c.site_title.clone()),
            "SITE_TITLE",
            "Myriad - A myriad of lights, in one place."
        ),
        "site_description": get_branding(
            db_config.as_ref().and_then(|c| c.site_description.clone()),
            "SITE_DESCRIPTION",
            "A myriad of lights, in one place."
        ),
        "site_favicon": get_branding(
            db_config.as_ref().and_then(|c| c.site_favicon.clone()),
            "SITE_FAVICON",
            "/favicon.webp"
        ),
        // Clearable SEO / third-party analytics: explicit empty DB disables (no env re-fill).
        "site_keywords": db_or_env_clearable(
            db_config.as_ref().and_then(|c| c.site_keywords.clone()),
            "SITE_KEYWORDS",
            ""
        ),
        "site_og_image": db_or_env_clearable(
            db_config.as_ref().and_then(|c| c.site_og_image.clone()),
            "SITE_OG_IMAGE",
            ""
        ),
        "google_site_verification": db_or_env_clearable(
            db_config
                .as_ref()
                .and_then(|c| c.google_site_verification.clone()),
            "GOOGLE_SITE_VERIFICATION",
            ""
        ),
        "site_noindex": site_noindex,
        "site_visibility_policy": site_visibility_policy,
        "site_ai_intro": site_ai_intro,
        "ga_measurement_id": db_or_env_clearable(
            db_config.as_ref().and_then(|c| c.ga_measurement_id.clone()),
            "GA_MEASUREMENT_ID",
            ""
        ),
        "umami_website_id": db_or_env_clearable(
            db_config.as_ref().and_then(|c| c.umami_website_id.clone()),
            "UMAMI_WEBSITE_ID",
            ""
        ),
        "umami_script_url": db_or_env_clearable(
            db_config.as_ref().and_then(|c| c.umami_script_url.clone()),
            "UMAMI_SCRIPT_URL",
            ""
        ),
    });

    (StatusCode::OK, Json(metadata))
}

/// Public platforms (no secrets) plus meropeEnabled / persona name / sticker avatar.
/// 公开端点 - 不需要认证
pub async fn get_public_config(
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    // 优先从数据库读取配置
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let db_config = config_service.load_config().await.ok();

    // Prefer DB when present (including intentional empty clear); else process env.
    let get_value = |db_val: Option<String>, env_key: &str| -> String {
        db_or_env_clearable(db_val, env_key, "")
    };

    // Match build_config: empty compose `${VAR:-}` still sets the key — gate on nonempty.
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
        nonempty_db(db_config.as_ref().and_then(|c| c.steam_id.as_ref()))
            || nonempty_env("STEAM_ID"),
    );
    let youtube_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.youtube_enabled),
        (nonempty_db(
            db_config
                .as_ref()
                .and_then(|c| c.youtube_channel_id.as_ref()),
        ) || nonempty_env("YOUTUBE_CHANNEL_ID"))
            && (nonempty_db(db_config.as_ref().and_then(|c| c.youtube_api_key.as_ref()))
                || nonempty_env("YOUTUBE_API_KEY")),
    );
    let netease_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.netease_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.netease_user_id.as_ref()))
            || nonempty_env("NETEASE_USER_ID"),
    );
    let bangumi_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.bangumi_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.bangumi_username.as_ref()))
            || nonempty_db(
                db_config
                    .as_ref()
                    .and_then(|c| c.bangumi_access_token.as_ref()),
            )
            || nonempty_env("BANGUMI_USERNAME")
            || nonempty_env("BANGUMI_ACCESS_TOKEN"),
    );
    let x_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.x_enabled),
        (nonempty_db(db_config.as_ref().and_then(|c| c.x_username.as_ref()))
            || nonempty_env("X_USERNAME"))
            && (nonempty_db(db_config.as_ref().and_then(|c| c.x_bearer_token.as_ref()))
                || nonempty_env("X_BEARER_TOKEN")),
    );
    let discord_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.discord_enabled),
        nonempty_db(
            db_config
                .as_ref()
                .and_then(|c| c.discord_access_token.as_ref()),
        ) || nonempty_env("DISCORD_ACCESS_TOKEN"),
    );
    let mal_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.mal_enabled),
        nonempty_db(db_config.as_ref().and_then(|c| c.mal_username.as_ref()))
            || nonempty_env("MAL_USERNAME"),
    );
    let xbox_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.xbox_enabled),
        (nonempty_db(db_config.as_ref().and_then(|c| c.xbox_gamertag.as_ref()))
            || nonempty_env("XBOX_GAMERTAG"))
            && (nonempty_db(db_config.as_ref().and_then(|c| c.openxbl_api_key.as_ref()))
                || nonempty_env("OPENXBL_API_KEY")
                || nonempty_env("XBL_API_KEY")),
    );
    let psn_enabled = resolve_platform_enabled(
        db_config.as_ref().and_then(|c| c.psn_enabled),
        (nonempty_db(db_config.as_ref().and_then(|c| c.psn_online_id.as_ref()))
            || nonempty_env("PSN_ONLINE_ID"))
            && (nonempty_db(db_config.as_ref().and_then(|c| c.psn_npsso.as_ref()))
                || nonempty_env("PSN_NPSSO")),
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
            name: "YouTube".to_string(),
            enabled: youtube_enabled,
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "channel_id".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: get_value(
                    db_config
                        .as_ref()
                        .and_then(|c| c.youtube_channel_id.clone()),
                    "YOUTUBE_CHANNEL_ID",
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

    let is_enabled = db_config
        .as_ref()
        .map(|config| config.merope_enabled_resolved())
        .unwrap_or_else(|| crate::config::DynamicConfig::default().merope_enabled_resolved());
    let stored_persona = if is_enabled {
        crate::services::agent::merope::get_persona(&db)
            .await
            .ok()
            .flatten()
    } else {
        None
    };
    let stored_name = stored_persona.as_ref().map(|persona| persona.name.clone());
    // Sticker avatar only when stored persona exists. Name still `"Agent"` if merope is off.
    let sticker_avatar = stored_persona
        .as_ref()
        .and_then(|persona| persona.avatar_asset_id.as_deref())
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let response = json!({
        "platforms": public_platforms,
        "aiAvailability": db_config.as_ref().map(ai_availability),
        "meropeEnabled": is_enabled,
        "agentPersonaName": crate::services::agent::merope::public_persona_name(
            is_enabled,
            stored_name.as_deref(),
        ),
        "agentPersonaAvatarUrl": sticker_avatar,
    });

    (StatusCode::OK, Json(response))
}

/// 获取公开的 UI 运行时配置（壁纸 / 动效 / 音乐 / 站点展示等）
/// 公开端点 - 不需要认证
/// 已剥离无前端消费的死字段：`pet_*`、`wallpaper_parallax`（动效改走 evocative_*）
pub async fn get_public_ui_config(
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    // 优先从数据库读取配置
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let db_config = config_service.load_config().await.ok();

    // Prefer DB when present (including intentional empty clear); else process env.
    // Must match admin bag / SEO clearable fields — empty wallpaper must NOT fall
    // through to a stale UI_WALLPAPER_URL still sitting in process env after clear.
    let get_value = |db_val: Option<String>, env_key: &str| -> String {
        db_or_env_clearable(db_val, env_key, "")
    };

    let ui_config = json!({
        "analytics_enabled": db_config.as_ref().map(|c| c.analytics_enabled).unwrap_or_else(||
            std::env::var("ANALYTICS_ENABLED").unwrap_or_else(|_| "true".to_string()).parse().unwrap_or(true)
        ),
        "pwa_enabled": db_config.as_ref().map(|c| c.pwa_enabled).unwrap_or_else(||
            std::env::var("PWA_ENABLED").unwrap_or_else(|_| "true".to_string()).parse().unwrap_or(true)
        ),
        "wallpaper_url": get_value(
            db_config.as_ref().and_then(|c| c.ui_wallpaper_url.clone()),
            "UI_WALLPAPER_URL"
        ),
        "wallpaper_blur": db_config.as_ref().map(|c| c.ui_wallpaper_blur as u32).unwrap_or_else(||
            std::env::var("UI_WALLPAPER_BLUR").unwrap_or_else(|_| "3".to_string()).parse::<u32>().unwrap_or(3)
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
        "island_show_greeting": db_config.as_ref().map(|c| c.island_show_greeting).unwrap_or(true),
        "island_show_weather": db_config.as_ref().map(|c| c.island_show_weather).unwrap_or(true),
        "island_show_quote": db_config.as_ref().map(|c| c.island_show_quote).unwrap_or(true),
        "island_show_music": db_config.as_ref().map(|c| c.island_show_music).unwrap_or(true),
        "island_show_tapp": db_config.as_ref().map(|c| c.island_show_tapp).unwrap_or(true),
        "precise_location_enabled": db_config.as_ref().map(|c| c.precise_location_enabled).unwrap_or(false),
        "dashboard_layout": db_config.as_ref().and_then(|c| c.dashboard_layout.clone()),
        "dashboard_layout_mode": db_config.as_ref().and_then(|c| c.dashboard_layout_mode.clone()),
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
        "site_footer_custom": db_or_env_clearable(
            db_config.as_ref().and_then(|c| c.site_footer_custom.clone()),
            "SITE_FOOTER_CUSTOM",
            ""
        ),
    });

    (StatusCode::OK, Json(ui_config))
}

/// Readiness only; provider credentials never leave the backend.
fn ai_availability(config: &crate::config::DynamicConfig) -> Value {
    use crate::config::ModelTier;
    let ready = |tier| {
        config
            .resolve_ai_config(tier)
            .api_key
            .is_some_and(|key| !key.trim().is_empty())
    };
    let standard = ready(ModelTier::Standard);
    let pro = ready(ModelTier::Pro);
    let strict_lite = config
        .resolve_strict_lite_ai_config()
        .and_then(|resolved| resolved.api_key)
        .is_some_and(|key| !key.trim().is_empty());
    json!({
        "standard": standard,
        "chat": strict_lite,
        "personaName": strict_lite,
        "pro": pro,
        "persona": config.pro_enabled && pro,
        "image": crate::services::image_generation::config_from_dynamic(config).is_ok(),
    })
}

#[cfg(test)]
mod ai_availability_tests {
    use super::*;

    #[test]
    fn missing_configuration_has_no_available_ai() {
        let value = ai_availability(&crate::config::DynamicConfig::default());
        assert!(
            value
                .as_object()
                .unwrap()
                .values()
                .all(|flag| flag == false)
        );
    }

    #[test]
    fn chat_and_naming_require_explicit_lite_without_pro_or_merope() {
        let mut config = crate::config::DynamicConfig {
            lite_enabled: true,
            lite_openai_model: "test-lite".into(),
            provider_openrouter_api_key: Some("test-key".into()),
            ..Default::default()
        };
        let value = ai_availability(&config);
        assert_eq!(value["chat"], true);
        assert_eq!(value["personaName"], true);
        assert_eq!(value["persona"], false);
        config.lite_openai_model.clear();
        assert_eq!(ai_availability(&config)["chat"], false);
        assert_eq!(ai_availability(&config)["personaName"], false);
    }

    #[test]
    fn shared_credentials_follow_runtime_resolution_without_exposing_keys() {
        let config = crate::config::DynamicConfig {
            provider_openrouter_api_key: Some("test-private-key".into()),
            ..Default::default()
        };
        let value = ai_availability(&config);
        assert!(config.text_ai_available());
        assert_eq!(value["standard"], true);
        assert_eq!(value["chat"], false);
        assert_eq!(value["personaName"], false);
        assert_eq!(value["pro"], true);
        assert_eq!(value["persona"], false);
        assert!(!value.to_string().contains("test-private-key"));
    }
}
