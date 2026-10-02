//! Public (unauthenticated) site metadata, platform cards, and UI runtime config.
use axum::{Json, http::StatusCode};
use serde_json::{Value, json};

use super::flags::sort_platforms_by_order;
use super::types::{ConfigField, PlatformConfig};
use crate::services::platform_id::PlatformId;

pub async fn get_site_metadata(
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    // 优先从数据库读取站点元数据配置
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let stored = config_service.load_config().await.unwrap_or_else(|error| {
        tracing::warn!(%error, "stored configuration could not be read; using defaults");
        crate::config::DynamicConfig::default()
    });

    // Branding fields: an empty value falls through to the default.
    let get_branding = |db_val: Option<String>, default: &str| -> String {
        db_val
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| default.to_string())
    };

    let site_noindex = stored.site_noindex;

    let site_visibility_policy = {
        let raw = stored.site_visibility_policy.trim().to_string();
        crate::api::seo_policy::normalize_visibility_policy(&raw, site_noindex)
    };
    let site_ai_intro = stored.site_ai_intro.clone().unwrap_or_default();

    let metadata = json!({
        "site_title": get_branding(stored.site_title.clone(), "Myriad - A myriad of lights, in one place."),
        "site_description": get_branding(stored.site_description.clone(), "A myriad of lights, in one place."),
        "site_favicon": get_branding(stored.site_favicon.clone(), "/favicon.webp"),
        // Clearable SEO / third-party analytics: explicit empty DB disables (no env re-fill).
        "site_keywords": stored.site_keywords.clone().unwrap_or_default(),
        "site_og_image": stored.site_og_image.clone().unwrap_or_default(),
        "google_site_verification": stored.google_site_verification.clone().unwrap_or_default(),
        "site_noindex": site_noindex,
        "site_visibility_policy": site_visibility_policy,
        "site_ai_intro": site_ai_intro,
        "ga_measurement_id": stored.ga_measurement_id.clone().unwrap_or_default(),
        "umami_website_id": stored.umami_website_id.clone().unwrap_or_default(),
        "umami_script_url": stored.umami_script_url.clone().unwrap_or_default(),
    });

    (StatusCode::OK, Json(metadata))
}

/// 公开平台卡片（不含密钥）。`enabled` 与报告、Agent、刷新同源：[`PlatformId::enabled`]
/// ——抓取拿不到数据的平台不在公开页上占位。env 只对 Xbox / PSN 生效，
/// 因为只有这两个平台的抓取会回落到 env（见 `platform_id` 的解析函数）。
pub(crate) fn public_platform_cards(
    db_config: Option<&crate::config::DynamicConfig>,
) -> Vec<PlatformConfig> {
    let stored = db_config.cloned().unwrap_or_default();
    let enabled = |id: PlatformId| id.enabled(&stored);

    // 只返回公开可见的平台配置字段（不包含 API 密钥等敏感信息）
    vec![
        PlatformConfig {
            name: "GitHub".to_string(),
            enabled: enabled(PlatformId::Github),
            has_token: false, // 不暴露是否有 token
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "username".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: stored.github_username.clone().unwrap_or_default(),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Bilibili".to_string(),
            enabled: enabled(PlatformId::Bilibili),
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "uid".to_string(),
                label: "".to_string(),
                field_type: "number".to_string(),
                value: stored.bilibili_uid.clone().unwrap_or_default(),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Steam".to_string(),
            enabled: enabled(PlatformId::Steam),
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "steam_id".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: stored.steam_id.clone().unwrap_or_default(),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "YouTube".to_string(),
            enabled: enabled(PlatformId::Youtube),
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "channel_id".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: stored.youtube_channel_id.clone().unwrap_or_default(),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Netease Music".to_string(),
            enabled: enabled(PlatformId::Netease),
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "user_id".to_string(),
                label: "".to_string(),
                field_type: "number".to_string(),
                value: stored.netease_user_id.clone().unwrap_or_default(),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Bangumi".to_string(),
            enabled: enabled(PlatformId::Bangumi),
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "username".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: stored.bangumi_username.clone().unwrap_or_default(),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "X".to_string(),
            enabled: enabled(PlatformId::X),
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "username".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: stored.x_username.clone().unwrap_or_default(),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Discord".to_string(),
            enabled: enabled(PlatformId::Discord),
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            // 不暴露 token；仅返回 user_id 便于公开名片展示
            config_fields: vec![ConfigField {
                key: "user_id".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: stored.discord_user_id.clone().unwrap_or_default(),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "MyAnimeList".to_string(),
            enabled: enabled(PlatformId::Mal),
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "username".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: stored.mal_username.clone().unwrap_or_default(),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "Xbox".to_string(),
            enabled: enabled(PlatformId::Xbox),
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "gamertag".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: stored.xbox_gamertag.clone().unwrap_or_default(),
                placeholder: "".to_string(),
                required: false,
            }],
        },
        PlatformConfig {
            name: "PlayStation".to_string(),
            enabled: enabled(PlatformId::Psn),
            has_token: false,
            icon: "".to_string(),
            description: "".to_string(),
            config_fields: vec![ConfigField {
                key: "online_id".to_string(),
                label: "".to_string(),
                field_type: "text".to_string(),
                value: stored.psn_online_id.clone().unwrap_or_default(),
                placeholder: "".to_string(),
                required: false,
            }],
        },
    ]
}

/// Public platforms (no secrets) plus meropeEnabled / persona name / sticker avatar.
/// 公开端点 - 不需要认证
pub async fn get_public_config(
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    // 优先从数据库读取配置
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let stored = config_service.load_config().await.unwrap_or_else(|error| {
        tracing::warn!(%error, "stored configuration could not be read; using defaults");
        crate::config::DynamicConfig::default()
    });

    let public_platforms = public_platform_cards(Some(&stored));

    let mut public_platforms = public_platforms;
    sort_platforms_by_order(&mut public_platforms, stored.platform_order.as_ref());

    let is_enabled = stored.merope_enabled_resolved();
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
        "aiAvailability": Some(ai_availability(&stored)),
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
    let stored = config_service.load_config().await.unwrap_or_else(|error| {
        tracing::warn!(%error, "stored configuration could not be read; using defaults");
        crate::config::DynamicConfig::default()
    });

    let ui_config = json!({
        "analytics_enabled": stored.analytics_enabled,
        "pwa_enabled": stored.pwa_enabled,
        "wallpaper_url": stored.ui_wallpaper_url.clone().unwrap_or_default(),
        "wallpaper_blur": stored.ui_wallpaper_blur as u32,
        // Evocative 壁纸动效
        "evocative_parallax": stored.ui_evocative_parallax,
        "evocative_dynamic_blur": stored.ui_evocative_dynamic_blur,
        "evocative_ripple": stored.ui_evocative_ripple,
        "evocative_fps": stored.ui_evocative_fps as u32,
        "evocative_ripple_quality": stored.ui_evocative_ripple_quality,
        "music_enabled": stored.music_enabled.clone().unwrap_or_default(),
        "music_source": stored.music_source.clone().unwrap_or_default(),
        "music_playlist_id": stored.music_playlist_id.clone().unwrap_or_default(),
        "music_proxy_enabled": stored.music_proxy_enabled,
        "music_preload_enabled": stored.music_preload_enabled,
        "island_show_greeting": stored.island_show_greeting,
        "island_show_weather": stored.island_show_weather,
        "island_show_quote": stored.island_show_quote,
        "island_show_music": stored.island_show_music,
        "island_show_tapp": stored.island_show_tapp,
        "precise_location_enabled": stored.precise_location_enabled,
        "dashboard_layout": stored.dashboard_layout.clone(),
        "dashboard_layout_mode": stored.dashboard_layout_mode.clone(),
        "dashboard_title": stored.dashboard_title.clone(),
        "custom_platforms": stored.custom_platforms.clone(),
        "widget_theme": stored.widget_theme.clone(),
        "control_panel_layout": stored.control_panel_layout.clone(),
        "control_panel_rows": stored.control_panel_rows,
        "tapp_window_schemes": stored.tapp_window_schemes.clone(),
        // 标题字体样式设置
        "title_font": stored.title_font.clone(),
        "title_font_size": stored.title_font_size,
        "title_color": stored.title_color.clone(),
        // 站点信息（用于底部显示）
        "site_icp": stored.site_icp.clone(),
        "site_gongan": stored.site_gongan.clone(),
        "cloud_sponsors": stored.cloud_sponsors.clone(),
        "site_footer_custom": stored.site_footer_custom.clone().unwrap_or_default(),
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
            lite_ai_model: "test-lite".into(),
            provider_openrouter_api_key: Some("test-key".into()),
            ..Default::default()
        };
        let value = ai_availability(&config);
        assert_eq!(value["chat"], true);
        assert_eq!(value["personaName"], true);
        assert_eq!(value["persona"], false);
        config.lite_ai_model.clear();
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
