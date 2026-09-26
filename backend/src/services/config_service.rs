use crate::config::DynamicConfig;
use anyhow::{Context, Result};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// Optional config string: null / "" / whitespace → None.
/// Empty `Some("")` must not reach outbound auth headers.
fn opt_nonempty_string(v: &JsonValue) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
}

/// Keys whose stored value maps onto the `DynamicConfig` field of the same
/// name by one standard rule. A key that needs its own parsing stays in
/// `parse_config`. Either way, every key the settings form saves must be read
/// back; `every_saved_setting_is_read_back` checks that.
macro_rules! standard_fields {
    ($($rule:ident: [$($field:ident),* $(,)?]),* $(,)?) => {
        /// Agent `config.get` ui 段：与公开 UI 运行时同类的非密钥字段。
pub(crate) fn public_ui_config_value(config: &crate::config::DynamicConfig) -> JsonValue {
    serde_json::json!({
        "analytics_enabled": config.analytics_enabled,
        "pwa_enabled": config.pwa_enabled,
        "wallpaper_url": config.ui_wallpaper_url,
        "wallpaper_blur": config.ui_wallpaper_blur,
        "evocative_parallax": config.ui_evocative_parallax,
        "evocative_dynamic_blur": config.ui_evocative_dynamic_blur,
        "evocative_ripple": config.ui_evocative_ripple,
        "evocative_fps": config.ui_evocative_fps,
        "evocative_ripple_quality": config.ui_evocative_ripple_quality,
        "music_enabled": config.music_enabled,
        "music_source": config.music_source,
        "site_title": config.site_title,
        "site_description": config.site_description,
    })
}

#[cfg(test)]
        const STANDARD_KEYS: &[&str] = &[$($(stringify!($field)),*),*];

        fn read_standard_fields(config: &mut DynamicConfig, map: &HashMap<String, JsonValue>) {
            $($(
                if let Some(v) = map.get(stringify!($field)) {
                    read_rule!($rule, config.$field, v);
                }
            )*)*
        }
    };
}

macro_rules! read_rule {
    (text, $slot:expr, $v:ident) => {
        if let Some(s) = $v.as_str() {
            $slot = s.to_string();
        }
    };
    (opt_text, $slot:expr, $v:ident) => {
        $slot = $v.as_str().map(str::to_string);
    };
    (opt_nonempty, $slot:expr, $v:ident) => {
        $slot = opt_nonempty_string($v);
    };
    (flag, $slot:expr, $v:ident) => {
        if let Some(b) = $v.as_bool() {
            $slot = b;
        }
    };
    (flag_or_text, $slot:expr, $v:ident) => {
        if let Some(b) = $v.as_bool() {
            $slot = b;
        } else if let Some(s) = $v.as_str() {
            $slot = s == "true";
        }
    };
    (opt_flag_or_text, $slot:expr, $v:ident) => {
        if let Some(b) = $v.as_bool() {
            $slot = Some(b);
        } else if let Some(s) = $v.as_str() {
            $slot = Some(s == "true");
        }
    };
    (int32, $slot:expr, $v:ident) => {
        if let Some(n) = $v.as_i64() {
            $slot = n as i32;
        }
    };
}

standard_fields! {
    text: [
        ai_provider,
        gemini_model,
        openai_model,
        openai_base_url,
        topic_style,
        lite_ai_provider,
        lite_gemini_model,
        lite_openai_model,
        lite_openai_base_url,
        lite_judge_model,
        pro_ai_provider,
        pro_gemini_model,
        pro_openai_model,
        pro_openai_base_url,
        ai_image_provider,
        ai_image_model,
        ai_image_volcengine_base_url,
        speech_provider,
        speech_openai_base_url,
        provider_openai_base_url,
        provider_volcengine_base_url,
        ai_source,
        lite_ai_source,
        pro_ai_source,
        ai_image_source,
        speech_source,
        speech_stt_model,
        speech_tts_model,
        speech_tts_voice,
        agora_app_id,
        agora_app_certificate,
        agora_customer_id,
        agora_api_base,
        site_visibility_policy,
    ],
    opt_text: [
        gemini_api_key,
        openai_api_key,
        openweather_api_key,
        lite_gemini_api_key,
        lite_openai_api_key,
        pro_gemini_api_key,
        pro_openai_api_key,
        ui_wallpaper_url,
        ui_theme,
        ui_primary_color,
        ui_secondary_color,
        ai_image_openai_api_key,
        ai_image_openrouter_api_key,
        ai_image_volcengine_api_key,
        tripo_api_key,
        tencent_secret_id,
        tencent_secret_key,
        tencent_region,
        speech_openai_api_key,
        speech_openrouter_api_key,
        provider_openai_api_key,
        provider_openrouter_api_key,
        provider_gemini_api_key,
        provider_tinyfish_api_key,
        provider_volcengine_api_key,
        agora_customer_secret,
        base_url,
        site_title,
        site_description,
        site_favicon,
        site_keywords,
        site_og_image,
        google_site_verification,
        site_ai_intro,
        ga_measurement_id,
        umami_website_id,
        umami_script_url,
        site_icp,
        site_gongan,
        cloud_sponsors,
        site_footer_custom,
        music_source,
        music_playlist_id,
        proxy_url,
        proxy_bypass,
        gemini_base_url,
        github_api_base_url,
    ],
    opt_nonempty: [
        github_token,
        github_username,
        bilibili_uid,
        steam_api_key,
        steam_id,
        youtube_api_key,
        youtube_channel_id,
        netease_user_id,
        bangumi_username,
        bangumi_access_token,
        bangumi_user_agent,
        x_username,
        x_bearer_token,
        discord_access_token,
        discord_refresh_token,
        mal_username,
        mal_client_id,
        xbox_gamertag,
        openxbl_api_key,
        psn_online_id,
        psn_npsso,
        see_through_hf_token,
        qq_bot_app_secret,
        telegram_bot_token,
        discord_bot_token,
        feishu_bot_app_secret,
    ],
    flag: [
        ui_evocative_parallax,
        ui_evocative_dynamic_blur,
        ui_evocative_ripple,
        enable_auto_fetch,
        user_perm_ai_generate,
        user_perm_ai_analyze,
        user_perm_ai_chat,
        user_perm_ai_search,
        user_perm_ai_image,
        user_perm_3d_generate,
        user_perm_report_write,
        user_perm_network_fetch,
        user_perm_media_control,
        user_perm_component_theme,
        user_perm_shortcut_register,
        user_perm_event_publish,
        user_perm_scheduler_register,
        user_perm_speech_tts,
        user_perm_speech_asr,
        user_perm_storage_write,
        user_perm_federation_post,
        user_perm_federation_channel,
        user_perm_federation_room,
        user_perm_phantasi_comment_write,
        guest_perm_ai_generate,
        guest_perm_ai_analyze,
        guest_perm_ai_chat,
        guest_perm_ai_search,
        guest_perm_ai_image,
        guest_perm_3d_generate,
        guest_perm_report_write,
        guest_perm_network_fetch,
        guest_perm_media_control,
        guest_perm_component_theme,
        guest_perm_shortcut_register,
        guest_perm_event_publish,
        guest_perm_scheduler_register,
        guest_perm_speech_tts,
        guest_perm_speech_asr,
        guest_perm_storage_write,
        guest_perm_federation_post,
        guest_perm_federation_channel,
        guest_perm_federation_room,
        guest_perm_phantasi_comment_write,
        proxy_enabled,
    ],
    flag_or_text: [
        lite_enabled,
        pro_enabled,
        site_noindex,
    ],
    opt_flag_or_text: [
        github_enabled,
        bilibili_enabled,
        steam_enabled,
        youtube_enabled,
        netease_enabled,
        bangumi_enabled,
        x_enabled,
        discord_enabled,
        mal_enabled,
        xbox_enabled,
        psn_enabled,
    ],
    int32: [
        openai_max_tokens,
        ui_wallpaper_blur,
        ui_evocative_fps,
        fetch_interval_hours,
        control_panel_rows,
        user_ai_daily_calls,
        user_ai_daily_tokens,
        user_ai_cooldown_seconds,
        guest_ai_daily_calls,
        guest_ai_daily_tokens,
        guest_ai_cooldown_seconds,
        stash_hidden_capacity,
        stash_hidden_idle_seconds,
        resident_quota_per_app,
        resident_quota_site_total,
    ],
}

/// 配置服务 - 用于从数据库读写动态配置
pub struct ConfigService {
    db: DatabaseConnection,
}

impl ConfigService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// Fresh permission policy for grant revalidation across processes. This is
    /// a permission-only snapshot, never a replacement for the full config cache.
    /// Reuse the authoritative parser/defaults without reading/decrypting secrets.
    pub async fn load_permission_config_on(db: &impl ConnectionTrait) -> Result<DynamicConfig> {
        let rows = db.query_all_raw(Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT key, value FROM configurations WHERE LEFT(key, 10) = 'user_perm_' OR LEFT(key, 11) = 'guest_perm_'".to_string(),
        )).await.context("Failed to load current permission policy")?;
        let mut map = HashMap::new();
        for row in rows {
            let key: String = row.try_get("", "key")?;
            let value: JsonValue = row.try_get("", "value")?;
            anyhow::ensure!(
                value.is_boolean(),
                "permission policy contains a non-boolean value"
            );
            map.insert(key, value);
        }
        Ok(Self::parse_config(map))
    }

    /// Audio relay switch only; the full load decrypts every secret, too heavy
    /// for a per-request guard. Same parser, so the default cannot drift.
    pub async fn load_music_proxy_enabled_on(db: &impl ConnectionTrait) -> Result<bool> {
        let rows = db
            .query_all_raw(Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT key, value FROM configurations WHERE key = 'music_proxy_enabled'"
                    .to_string(),
            ))
            .await
            .context("Failed to load music proxy switch")?;
        let mut map = HashMap::new();
        for row in rows {
            map.insert(
                row.try_get::<String>("", "key")?,
                row.try_get::<JsonValue>("", "value")?,
            );
        }
        Ok(Self::parse_config(map).music_proxy_enabled)
    }

    /// 从数据库加载所有配置
    pub async fn load_config(&self) -> Result<DynamicConfig> {
        Self::load_config_on(&self.db).await
    }

    /// Full configuration as seen by `db`, which may be an open transaction:
    /// a writer can prove what it wrote still loads before committing it.
    pub async fn load_config_on(db: &impl ConnectionTrait) -> Result<DynamicConfig> {
        let sql = "SELECT key, value FROM configurations";
        let rows = db
            .query_all_raw(Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                sql.to_string(),
            ))
            .await
            .context("Failed to load configurations from database")?;

        let mut config_map: HashMap<String, JsonValue> = HashMap::new();
        for row in rows {
            if let (Ok(key), Ok(value)) = (
                row.try_get::<String>("", "key"),
                row.try_get::<JsonValue>("", "value"),
            ) {
                // 敏感值在库里是密文；这里解封成明文供运行时使用。
                // 未迁移的遗留明文原样通过。解密失败不得当成未配置。
                let value = crate::services::data_key::open_config_value(&key, value)
                    .with_context(|| format!("Failed to decrypt configuration {key}"))?;
                config_map.insert(key, value);
            }
        }

        Ok(Self::parse_config(config_map))
    }

    /// 从配置映射解析为 DynamicConfig
    fn parse_config(map: HashMap<String, JsonValue>) -> DynamicConfig {
        let mut config = DynamicConfig::default();
        read_standard_fields(&mut config, &map);

        if let Some(v) = map.get("discord_token_expires_at") {
            config.discord_token_expires_at = v.as_str().map(|s| s.to_string()).or_else(|| {
                v.as_i64()
                    .map(|n| n.to_string())
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            });
        }

        if let Some(v) = map.get("discord_user_id") {
            config.discord_user_id = v.as_str().map(|s| s.to_string()).or_else(|| {
                v.as_i64()
                    .map(|n| n.to_string())
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            });
        }

        // 平台展示顺序：JSON 数组，或 JSON 编码的字符串数组
        if let Some(v) = map.get("platform_order") {
            let order = if let Some(arr) = v.as_array() {
                Some(
                    arr.iter()
                        .filter_map(|item| item.as_str().map(|s| s.to_string()))
                        .collect::<Vec<String>>(),
                )
            } else {
                v.as_str()
                    .and_then(|s| serde_json::from_str::<Vec<String>>(s).ok())
            };
            if let Some(order) = order {
                if !order.is_empty() {
                    config.platform_order = Some(order);
                }
            }
        }

        // UI 配置
        // Evocative 壁纸动效
        if let Some(v) = map.get("ui_evocative_ripple_quality") {
            if let Some(n) = v.as_f64() {
                config.ui_evocative_ripple_quality = n;
            }
        }
        if let Some(v) = map.get("analytics_enabled") {
            if let Some(b) = v.as_bool() {
                config.analytics_enabled = b;
            } else if let Some(s) = v.as_str() {
                config.analytics_enabled = s != "false" && s != "0";
            }
        }
        if let Some(v) = map.get("pwa_enabled") {
            if let Some(b) = v.as_bool() {
                config.pwa_enabled = b;
            } else if let Some(s) = v.as_str() {
                config.pwa_enabled = s != "false" && s != "0";
            }
        }

        // AI 图片生成配置
        if let Some(v) = map.get("ai_image_openai_base_url") {
            if let Some(s) = v.as_str() {
                if !s.trim().is_empty() {
                    config.ai_image_openai_base_url = s.to_string();
                }
            }
        }

        if let Some(v) = map.get("merope_enabled") {
            config.merope_enabled = v
                .as_bool()
                .or_else(|| v.as_str().map(|s| s == "true" || s == "1"))
                .unwrap_or(config.merope_enabled);
        }
        if let Some(v) = map.get("merope_speech_enabled") {
            config.merope_speech_enabled = v
                .as_bool()
                .or_else(|| v.as_str().map(|s| s == "true" || s == "1"))
                .unwrap_or(config.merope_speech_enabled);
        }
        if let Some(v) = map.get("agent_rig_asset_id") {
            config.agent_rig_asset_id = v
                .as_str()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .and_then(crate::services::merope_rig::normalize_asset_id);
        }

        // Tripo 3D 独立配置
        if let Some(v) = map.get("tripo_enabled") {
            config.tripo_enabled = v
                .as_bool()
                .or_else(|| v.as_str().map(|s| s == "true"))
                .unwrap_or(config.tripo_enabled);
        }
        if let Some(v) = map.get("tripo_base_url").and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                config.tripo_base_url = v.to_string();
            }
        }
        if let Some(v) = map.get("tripo_model").and_then(|v| v.as_str()) {
            if !v.trim().is_empty() {
                config.tripo_model = v.to_string();
            }
        }
        if let Some(v) = map.get("tripo_face_limit").and_then(|v| v.as_i64()) {
            config.tripo_face_limit = v as i32;
        }
        if let Some(v) = map
            .get("tripo_poll_interval_seconds")
            .and_then(|v| v.as_i64())
        {
            config.tripo_poll_interval_seconds = v as i32;
        }
        if let Some(v) = map
            .get("tripo_task_timeout_seconds")
            .and_then(|v| v.as_i64())
        {
            config.tripo_task_timeout_seconds = v as i32;
        }
        if let Some(v) = map.get("tripo_max_download_mb").and_then(|v| v.as_i64()) {
            config.tripo_max_download_mb = v as i32;
        }
        // 腾讯云语音服务配置 (TTS/ASR)
        if let Some(v) = map.get("speech_reuse_text_credentials") {
            if let Some(b) = v.as_bool() {
                config.speech_reuse_text_credentials = b;
            } else if let Some(s) = v.as_str() {
                config.speech_reuse_text_credentials = s == "true" || s == "1";
            }
        }
        if let Some(v) = map.get("ai_vendor_sources") {
            if let Ok(parsed) =
                serde_json::from_value::<Vec<crate::config::AiVendorSource>>(v.clone())
            {
                config.ai_vendor_sources = parsed;
            } else if let Some(s) = v.as_str() {
                if let Ok(parsed) = serde_json::from_str::<Vec<crate::config::AiVendorSource>>(s) {
                    config.ai_vendor_sources = parsed;
                }
            }
        }
        if let Some(v) = map.get("agora_convo_enabled") {
            config.agora_convo_enabled = v
                .as_bool()
                .or_else(|| v.as_str().map(|s| s == "true" || s == "1"))
                .unwrap_or(config.agora_convo_enabled);
        }
        if let Some(v) = map.get("qq_bot_enabled") {
            config.qq_bot_enabled = v
                .as_bool()
                .or_else(|| v.as_str().map(|s| s == "true" || s == "1"))
                .unwrap_or(config.qq_bot_enabled);
        }
        if let Some(v) = map.get("qq_bot_app_id") {
            config.qq_bot_app_id = v.as_str().map(str::trim).unwrap_or("").to_string();
        }
        if let Some(v) = map.get("telegram_bot_enabled") {
            config.telegram_bot_enabled = v
                .as_bool()
                .or_else(|| v.as_str().map(|s| s == "true" || s == "1"))
                .unwrap_or(config.telegram_bot_enabled);
        }
        if let Some(v) = map.get("discord_bot_enabled") {
            config.discord_bot_enabled = v
                .as_bool()
                .or_else(|| v.as_str().map(|s| s == "true" || s == "1"))
                .unwrap_or(config.discord_bot_enabled);
        }
        if let Some(v) = map.get("feishu_bot_enabled") {
            config.feishu_bot_enabled = v
                .as_bool()
                .or_else(|| v.as_str().map(|s| s == "true" || s == "1"))
                .unwrap_or(config.feishu_bot_enabled);
        }
        if let Some(v) = map.get("feishu_bot_app_id") {
            config.feishu_bot_app_id = v.as_str().map(str::trim).unwrap_or("").to_string();
        }

        // OAuth providers 列表
        if let Some(v) = map.get("oauth_providers") {
            if let Ok(parsed) =
                serde_json::from_value::<Vec<crate::config::OAuthProviderEntry>>(v.clone())
            {
                config.oauth_providers = parsed;
            } else {
                tracing::warn!(
                    "oauth_providers config exists but failed to parse as Vec<OAuthProviderEntry>"
                );
            }
        }

        // 本地注册开关
        if let Some(v) = map.get("allow_local_registration") {
            config.allow_local_registration = v.as_bool().unwrap_or(false);
        }

        // Private Tapp install retention (users section)
        if let Some(v) = map.get("tapp_private_install_cleanup") {
            let mode = v
                .as_str()
                .unwrap_or("inactivity")
                .trim()
                .to_ascii_lowercase();
            config.tapp_private_install_cleanup = if mode == "logout" {
                "logout".to_string()
            } else {
                "inactivity".to_string()
            };
        }
        if let Some(v) = map.get("tapp_private_install_inactivity_days") {
            let days = v
                .as_i64()
                .or_else(|| v.as_u64().map(|n| n as i64))
                .unwrap_or(14);
            config.tapp_private_install_inactivity_days = days.clamp(1, 365) as i32;
        }

        // 仪表盘配置
        if let Some(v) = map.get("dashboard_layout") {
            // 如果是字符串直接使用，如果是对象/数组则转为字符串
            if let Some(s) = v.as_str() {
                config.dashboard_layout = Some(s.to_string());
            } else {
                config.dashboard_layout = Some(v.to_string());
            }
        }
        if let Some(v) = map.get("dashboard_layout_mode") {
            if let Some(s) = v.as_str() {
                config.dashboard_layout_mode = Some(if s.trim() == "free" {
                    "free".to_string()
                } else {
                    "standard".to_string()
                });
            }
        }
        if let Some(v) = map.get("dashboard_title") {
            if let Some(s) = v.as_str() {
                config.dashboard_title = Some(s.to_string());
            }
        }
        if let Some(v) = map.get("custom_platforms") {
            if let Some(s) = v.as_str() {
                config.custom_platforms = Some(s.to_string());
            } else {
                config.custom_platforms = Some(v.to_string());
            }
        }
        if let Some(v) = map.get("widget_theme") {
            if let Some(s) = v.as_str() {
                config.widget_theme = Some(s.to_string());
            } else {
                config.widget_theme = Some(v.to_string());
            }
        }

        // 标题字体样式配置
        if let Some(v) = map.get("title_font") {
            if let Some(s) = v.as_str() {
                config.title_font = Some(s.to_string());
            }
        }
        if let Some(v) = map.get("title_font_size") {
            if let Some(n) = v.as_f64() {
                config.title_font_size = Some(n);
            }
        }
        if let Some(v) = map.get("title_color") {
            if let Some(s) = v.as_str() {
                config.title_color = Some(s.to_string());
            }
        }

        // 控制面板小组件配置
        if let Some(v) = map.get("control_panel_layout") {
            if v.is_null() {
                config.control_panel_layout = None;
            } else if let Some(s) = v.as_str() {
                config.control_panel_layout = Some(s.to_string());
            } else {
                config.control_panel_layout = Some(v.to_string());
            }
        }

        // Tapp 多窗口方案配置
        if let Some(v) = map.get("tapp_window_schemes") {
            if let Some(s) = v.as_str() {
                config.tapp_window_schemes = Some(s.to_string());
            } else {
                config.tapp_window_schemes = Some(v.to_string());
            }
        }

        // 网站元数据配置
        if let Some(v) = map.get("site_seo_review_cadence") {
            config.site_seo_review_cadence =
                crate::services::seo_policy::normalize_seo_review_cadence(v.as_str().unwrap_or(""))
                    .to_string();
        } else {
            config.site_seo_review_cadence =
                crate::services::seo_policy::normalize_seo_review_cadence("").to_string();
        }
        // Keep noindex in sync with policy when policy is set.
        if !config.site_visibility_policy.trim().is_empty() {
            let pol = config.site_visibility_policy.trim();
            if pol == "private" {
                config.site_noindex = true;
            } else if matches!(pol, "search_only" | "ai_citation" | "ai_full") {
                config.site_noindex = false;
            }
        } else if config.site_noindex {
            // Policy empty + noindex → private, for consumers that read policy.
            config.site_visibility_policy = "private".to_string();
        }

        // 音乐配置
        if let Some(v) = map.get("music_enabled") {
            // music_enabled 在数据库中是布尔值，需要转换为字符串
            config.music_enabled = if let Some(b) = v.as_bool() {
                Some(b.to_string())
            } else {
                v.as_str().map(|s| s.to_string())
            };
        }
        if let Some(v) = map.get("music_proxy_enabled") {
            if let Some(b) = v.as_bool() {
                config.music_proxy_enabled = b;
            } else if let Some(s) = v.as_str() {
                config.music_proxy_enabled = s != "false" && s != "0";
            }
        }
        if let Some(v) = map.get("music_preload_enabled") {
            if let Some(b) = v.as_bool() {
                config.music_preload_enabled = b;
            } else if let Some(s) = v.as_str() {
                config.music_preload_enabled = s != "false" && s != "0";
            }
        }

        if let Some(v) = map.get("island_show_greeting") {
            if let Some(b) = v.as_bool() {
                config.island_show_greeting = b;
            } else if let Some(s) = v.as_str() {
                config.island_show_greeting = s != "false" && s != "0";
            }
        }
        if let Some(v) = map.get("island_show_weather") {
            if let Some(b) = v.as_bool() {
                config.island_show_weather = b;
            } else if let Some(s) = v.as_str() {
                config.island_show_weather = s != "false" && s != "0";
            }
        }
        if let Some(v) = map.get("island_show_quote") {
            if let Some(b) = v.as_bool() {
                config.island_show_quote = b;
            } else if let Some(s) = v.as_str() {
                config.island_show_quote = s != "false" && s != "0";
            }
        }
        if let Some(v) = map.get("island_show_music") {
            if let Some(b) = v.as_bool() {
                config.island_show_music = b;
            } else if let Some(s) = v.as_str() {
                config.island_show_music = s != "false" && s != "0";
            }
        }
        if let Some(v) = map.get("island_show_tapp") {
            if let Some(b) = v.as_bool() {
                config.island_show_tapp = b;
            } else if let Some(s) = v.as_str() {
                config.island_show_tapp = s != "false" && s != "0";
            }
        }

        // 内存节约（高级设置）
        if let Some(v) = map.get("memory_saver_enabled") {
            if let Some(b) = v.as_bool() {
                config.memory_saver_enabled = b;
            } else if let Some(s) = v.as_str() {
                config.memory_saver_enabled = s == "true" || s == "1";
            }
        }

        // 精确位置（高级设置）：缺省 false，不向浏览器申请定位许可。
        if let Some(v) = map.get("precise_location_enabled") {
            if let Some(b) = v.as_bool() {
                config.precise_location_enabled = b;
            } else if let Some(s) = v.as_str() {
                config.precise_location_enabled = s == "true" || s == "1";
            }
        }

        config
    }

    /// 更新单个配置项
    ///
    /// 敏感 key 由 [`crate::services::data_key::is_sensitive_config_key`] 判定
    /// （密钥类 token/secret/api_key/password/npsso；排除 `*_tokens` 配额与
    /// `*_expires_at` 元数据）。本路径 `seal_config_value` 后 UPSERT；调用方传明文。
    pub async fn update_config(&self, key: &str, value: JsonValue) -> Result<()> {
        let value = crate::services::data_key::seal_config_value(key, value)?;
        upsert_configuration(&self.db, key, value).await
    }

    /// Seal every value before one atomic, parameterized multi-row UPSERT.
    pub async fn update_configs(&self, updates: HashMap<String, JsonValue>) -> Result<()> {
        Self::update_configs_on(&self.db, updates).await
    }

    /// Same batch UPSERT, optionally on a caller-owned transaction.
    pub async fn update_configs_on(
        db: &impl ConnectionTrait,
        updates: HashMap<String, JsonValue>,
    ) -> Result<()> {
        let count = updates.len();
        if count == 0 {
            return Ok(());
        }
        let sealed = seal_config_updates(updates)?;
        let mut sql = String::from("INSERT INTO configurations (key, value, updated_at) VALUES ");
        let mut values = Vec::with_capacity(count * 2);
        for (index, (key, value)) in sealed.into_iter().enumerate() {
            if index > 0 {
                sql.push_str(", ");
            }
            use std::fmt::Write;
            write!(
                sql,
                "(${}, ${}, CURRENT_TIMESTAMP)",
                index * 2 + 1,
                index * 2 + 2
            )
            .expect("writing to a String cannot fail");
            values.push(key.into());
            values.push(value.into());
        }
        sql.push_str(" ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value, updated_at = CURRENT_TIMESTAMP");
        db.execute_raw(Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            sql,
            values,
        ))
        .await
        .context("Failed to update configurations")?;
        tracing::info!("✅ Updated {count} configurations");
        Ok(())
    }
}

fn seal_config_updates(updates: HashMap<String, JsonValue>) -> Result<Vec<(String, JsonValue)>> {
    let mut sealed = Vec::with_capacity(updates.len());
    for (key, value) in updates {
        let value = crate::services::data_key::seal_config_value(&key, value)
            .with_context(|| format!("Failed to encrypt configuration {key}"))?;
        sealed.push((key, value));
    }
    Ok(sealed)
}

async fn upsert_configuration(
    db: &impl ConnectionTrait,
    key: &str,
    value: JsonValue,
) -> Result<()> {
    let sql = r#"
            INSERT INTO configurations (key, value, updated_at)
            VALUES ($1, $2, CURRENT_TIMESTAMP)
            ON CONFLICT (key) DO UPDATE
            SET value = $2, updated_at = CURRENT_TIMESTAMP
        "#;

    db.execute_raw(Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        sql,
        vec![key.into(), value.into()],
    ))
    .await
    .context("Failed to update configuration")?;

    Ok(())
}

#[cfg(test)]
mod tests {
    /// CLAUDE.md: a configuration key the settings form saves must be read
    /// back from the database, or the saved value silently does nothing.
    #[test]
    fn every_saved_setting_is_read_back() {
        fn literal_keys(source: &str, before: &str) -> std::collections::BTreeSet<String> {
            let mut keys = std::collections::BTreeSet::new();
            for piece in source.split(before).skip(1) {
                if let Some(key) = piece
                    .strip_prefix('"')
                    .and_then(|rest| rest.split('"').next())
                {
                    keys.insert(key.to_string());
                }
            }
            keys
        }
        let own = include_str!("config_service.rs");
        let parse = own
            .split("fn parse_config(map: HashMap<String, JsonValue>)")
            .nth(1)
            .and_then(|rest| rest.split("\n        config\n    }").next())
            .expect("parse_config");
        let mut read: std::collections::BTreeSet<String> = super::STANDARD_KEYS
            .iter()
            .map(|key| key.to_string())
            .collect();
        read.extend(literal_keys(&parse.replace(".get(\n", ".get("), ".get("));

        let save = include_str!("../api/config/save.rs");
        let collect = save
            .split("pub(crate) fn collect_database_updates_with_vendor(")
            .nth(1)
            .and_then(|rest| rest.split("\n}\n").next())
            .expect("collect_database_updates_with_vendor");
        let mut written = std::collections::BTreeSet::new();
        for marker in [
            "updates.insert(",
            "=> (",
            "insert_platform_field(&mut updates, ",
        ] {
            let flattened = collect.replace(&format!("{marker}\n"), marker);
            for key in literal_keys(&flattened.replace(marker, "\u{1}"), "\u{1}") {
                written.insert(key);
            }
        }
        for line in collect.lines() {
            if let Some(key) = line
                .trim()
                .split("=> \"")
                .nth(1)
                .and_then(|rest| rest.strip_suffix("\","))
            {
                written.insert(key.to_string());
            }
        }
        assert!(
            written.len() > 50,
            "found only {} saved keys",
            written.len()
        );
        let unread: Vec<_> = written.difference(&read).collect();
        assert!(unread.is_empty(), "saved but never read back: {unread:?}");
    }

    /// CLAUDE.md: a new configuration field needs its database read-back
    /// branch. For delegation flags the table is the source: every key it
    /// names must round-trip through `parse_config`.
    #[test]
    fn every_delegation_flag_reads_back_from_the_database() {
        use crate::services::permission_service::DELEGATIONS;
        for row in DELEGATIONS {
            let parsed = super::ConfigService::parse_config(std::collections::HashMap::from([(
                row.user_key.to_string(),
                serde_json::json!(true),
            )]));
            assert!((row.user)(&parsed), "{} is not read back", row.user_key);
            if let (Some(key), Some(guest)) = (row.guest_key, row.guest) {
                let parsed =
                    super::ConfigService::parse_config(std::collections::HashMap::from([(
                        key.to_string(),
                        serde_json::json!(true),
                    )]));
                assert!(guest(&parsed), "{key} is not read back");
            }
        }
    }

    #[tokio::test]
    #[ignore = "requires a disposable MYRIAD_RUNTIME_ISOLATION_TEST_DB"]
    async fn permission_policy_observes_commits_without_worker_cache_refresh() {
        use crate::services::permission_service::{TappPermissionService, UserRole};
        use sea_orm::{ConnectOptions, ConnectionTrait, Database, TransactionTrait};
        let url = std::env::var("MYRIAD_RUNTIME_ISOLATION_TEST_DB").unwrap();
        let admin = Database::connect(&url).await.unwrap();
        let schema = format!("permission_policy_{}", uuid::Uuid::new_v4().simple());
        admin
            .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
            .await
            .unwrap();
        let mut options = ConnectOptions::new(url);
        options.max_connections(2).map_sqlx_postgres_opts({
            let schema = schema.clone();
            move |options| options.options([("search_path", schema.as_str())])
        });
        let writer = Database::connect(options.clone()).await.unwrap();
        let observer = Database::connect(options).await.unwrap();
        writer
            .execute_unprepared(
                "CREATE TABLE configurations (key TEXT PRIMARY KEY, value JSONB NOT NULL)",
            )
            .await
            .unwrap();
        writer.execute_unprepared("INSERT INTO configurations VALUES ('user_perm_network_fetch', 'true'), ('openai_api_key', '\"irrelevant-test-secret\"')").await.unwrap();
        let permissions = vec!["network:fetch".to_string()];
        let stale = super::ConfigService::load_permission_config_on(&observer)
            .await
            .unwrap();
        assert_eq!(
            TappPermissionService::filter_permissions_for_role(
                &stale,
                UserRole::User,
                &permissions
            )
            .unwrap(),
            permissions
        );
        assert!(stale.openai_api_key.is_none());
        let change = writer.begin().await.unwrap();
        change
            .execute_unprepared(
                "UPDATE configurations SET value = 'false' WHERE key = 'user_perm_network_fetch'",
            )
            .await
            .unwrap();
        assert!(
            super::ConfigService::load_permission_config_on(&observer)
                .await
                .unwrap()
                .user_perm_network_fetch
        );
        change.commit().await.unwrap();
        let fresh = super::ConfigService::load_permission_config_on(&observer)
            .await
            .unwrap();
        assert!(!fresh.user_perm_network_fetch);
        assert!(stale.user_perm_network_fetch);
        assert!(
            TappPermissionService::filter_permissions_for_role(
                &fresh,
                UserRole::User,
                &permissions
            )
            .unwrap()
            .is_empty()
        );
        writer.execute_unprepared("UPDATE configurations SET value = '\"false\"' WHERE key = 'user_perm_network_fetch'").await.unwrap();
        assert!(
            super::ConfigService::load_permission_config_on(&observer)
                .await
                .is_err()
        );
        admin
            .execute_unprepared(&format!("DROP SCHEMA {schema} CASCADE"))
            .await
            .unwrap();
    }

    #[tokio::test]
    #[ignore = "requires a disposable MYRIAD_RUNTIME_ISOLATION_TEST_DB"]
    async fn update_configs_is_transactional_and_rejects_unsealed_vendor_objects() {
        use sea_orm::{ConnectOptions, ConnectionTrait, Database};
        use serde_json::json;
        use std::collections::HashMap;
        let url = std::env::var("MYRIAD_RUNTIME_ISOLATION_TEST_DB").unwrap();
        let admin = Database::connect(&url).await.unwrap();
        let schema = format!("config_txn_{}", uuid::Uuid::new_v4().simple());
        admin
            .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
            .await
            .unwrap();
        let mut options = ConnectOptions::new(url);
        options.max_connections(2).map_sqlx_postgres_opts({
            let schema = schema.clone();
            move |options| options.options([("search_path", schema.as_str())])
        });
        let db = Database::connect(options).await.unwrap();
        db.execute_unprepared(
            "CREATE TABLE configurations (
                key TEXT PRIMARY KEY,
                value JSONB NOT NULL,
                updated_at TIMESTAMPTZ NOT NULL DEFAULT CURRENT_TIMESTAMP
            )",
        )
        .await
        .unwrap();
        db.execute_unprepared(
            "INSERT INTO configurations (key, value) VALUES ('site_title', '\"old\"')",
        )
        .await
        .unwrap();
        db.execute_unprepared("CREATE TABLE write_count (n INTEGER NOT NULL); INSERT INTO write_count VALUES (0);
            CREATE FUNCTION count_config_write() RETURNS trigger LANGUAGE plpgsql AS $$
            BEGIN UPDATE write_count SET n = n + 1; RETURN NULL; END $$;
            CREATE TRIGGER count_write AFTER INSERT ON configurations FOR EACH STATEMENT EXECUTE FUNCTION count_config_write();
            ALTER TABLE configurations ADD CONSTRAINT reject_bad_value CHECK (value <> '\"reject-sql\"'::jsonb)")
            .await.unwrap();
        let service = super::ConfigService::new(db.clone());
        let mut bad = HashMap::new();
        bad.insert("site_title".into(), json!("new"));
        bad.insert("ai_vendor_sources".into(), json!({"api_key": "sk-plain"}));
        assert!(
            service.update_configs(bad).await.is_err(),
            "invalid vendor object must not persist any key in the batch"
        );
        let row = db
            .query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT value FROM configurations WHERE key = 'site_title'".to_string(),
            ))
            .await
            .unwrap()
            .unwrap();
        let stored: serde_json::Value = row.try_get("", "value").unwrap();
        assert_eq!(stored, json!("old"));
        let missing = db
            .query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT 1 FROM configurations WHERE key = 'ai_vendor_sources'".to_string(),
            ))
            .await
            .unwrap();
        assert!(missing.is_none());

        let mut ok = HashMap::new();
        ok.insert("site_title".into(), json!("saved"));
        ok.insert(
            "ai_vendor_sources".into(),
            json!([{"slug":"openai","kind":"openai","display_name":"OpenAI","enabled":true,"api_key":"sk-nested"}]),
        );
        service.update_configs(ok).await.unwrap();
        let count = db
            .query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT n FROM write_count".to_string(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i32>("", "n")
            .unwrap();
        assert_eq!(
            count, 1,
            "the production batch must execute one INSERT statement"
        );
        let mut sql_failure = HashMap::new();
        sql_failure.insert("site_title".into(), json!("partial"));
        sql_failure.insert("bad_value".into(), json!("reject-sql"));
        assert!(service.update_configs(sql_failure).await.is_err());
        service.update_configs(HashMap::new()).await.unwrap();
        let loaded = service.load_config().await.unwrap();
        assert_eq!(loaded.site_title.as_deref(), Some("saved"));
        assert_eq!(
            loaded.ai_vendor_sources[0].api_key.as_deref(),
            Some("sk-nested")
        );
        let sealed_row = db
            .query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT value FROM configurations WHERE key = 'ai_vendor_sources'".to_string(),
            ))
            .await
            .unwrap()
            .unwrap();
        let sealed: serde_json::Value = sealed_row.try_get("", "value").unwrap();
        let api_key = sealed.as_array().unwrap()[0]["api_key"].as_str().unwrap();
        assert!(
            crate::services::data_key::is_ciphertext(api_key),
            "nested api_key must be sealed at rest"
        );
        assert!(!api_key.contains("sk-nested"));
        admin
            .execute_unprepared(&format!("DROP SCHEMA {schema} CASCADE"))
            .await
            .unwrap();
    }
    use super::ConfigService;
    use crate::config::DynamicConfig;
    use serde_json::json;
    use std::collections::{HashMap, HashSet};
    use syn::visit::Visit;

    #[test]
    fn seal_config_updates_rejects_invalid_vendor_sources_before_any_upsert() {
        let mut updates = HashMap::new();
        updates.insert("site_title".into(), json!("ok"));
        updates.insert("ai_vendor_sources".into(), json!({"api_key": "sk-plain"}));
        assert!(
            super::seal_config_updates(updates).is_err(),
            "parse/shape errors must not produce a partial sealed batch"
        );
    }

    #[test]
    fn parses_ai_quota_values_from_database_config() {
        let config = ConfigService::parse_config(HashMap::from([
            ("user_ai_daily_calls".into(), json!(101)),
            ("user_ai_daily_tokens".into(), json!(202)),
            ("user_ai_cooldown_seconds".into(), json!(303)),
            ("guest_ai_daily_calls".into(), json!(404)),
            ("guest_ai_daily_tokens".into(), json!(505)),
            ("guest_ai_cooldown_seconds".into(), json!(606)),
            ("stash_hidden_capacity".into(), json!(707)),
            ("stash_hidden_idle_seconds".into(), json!(808)),
            ("resident_quota_per_app".into(), json!(9)),
            ("resident_quota_site_total".into(), json!(10)),
            ("openweather_api_key".into(), json!("weather-secret")),
            ("ui_theme".into(), json!("paper")),
            ("ui_primary_color".into(), json!("#112233")),
            ("ui_secondary_color".into(), json!("#445566")),
        ]));

        assert_eq!(config.user_ai_daily_calls, 101);
        assert_eq!(config.user_ai_daily_tokens, 202);
        assert_eq!(config.user_ai_cooldown_seconds, 303);
        assert_eq!(config.guest_ai_daily_calls, 404);
        assert_eq!(config.guest_ai_daily_tokens, 505);
        assert_eq!(config.guest_ai_cooldown_seconds, 606);
        assert_eq!(config.stash_hidden_capacity, 707);
        assert_eq!(config.stash_hidden_idle_seconds, 808);
        assert_eq!(config.resident_quota_per_app, 9);
        assert_eq!(config.resident_quota_site_total, 10);
        assert_eq!(
            config.openweather_api_key.as_deref(),
            Some("weather-secret")
        );
        assert_eq!(config.ui_theme.as_deref(), Some("paper"));
        assert_eq!(config.ui_primary_color.as_deref(), Some("#112233"));
        assert_eq!(config.ui_secondary_color.as_deref(), Some("#445566"));
    }

    #[test]
    fn parses_merope_flag_from_database_config() {
        let on =
            ConfigService::parse_config(HashMap::from([("merope_enabled".into(), json!(true))]));
        assert!(on.merope_enabled);

        let from_str =
            ConfigService::parse_config(HashMap::from([("merope_enabled".into(), json!("true"))]));
        assert!(from_str.merope_enabled);

        let off =
            ConfigService::parse_config(HashMap::from([("merope_enabled".into(), json!(false))]));
        assert!(!off.merope_enabled);
    }

    #[test]
    fn parses_merope_speech_flag_from_database_config() {
        let on = ConfigService::parse_config(HashMap::from([(
            "merope_speech_enabled".into(),
            json!(true),
        )]));
        assert!(on.merope_speech_enabled);

        let from_str = ConfigService::parse_config(HashMap::from([(
            "merope_speech_enabled".into(),
            json!("true"),
        )]));
        assert!(from_str.merope_speech_enabled);

        let off = ConfigService::parse_config(HashMap::from([(
            "merope_speech_enabled".into(),
            json!(false),
        )]));
        assert!(!off.merope_speech_enabled);

        let missing = ConfigService::parse_config(HashMap::new());
        assert!(!missing.merope_speech_enabled);
    }

    #[test]
    fn parses_precise_location_flag_from_database_config() {
        let on = ConfigService::parse_config(HashMap::from([(
            "precise_location_enabled".into(),
            json!(true),
        )]));
        assert!(on.precise_location_enabled);

        let from_str = ConfigService::parse_config(HashMap::from([(
            "precise_location_enabled".into(),
            json!("true"),
        )]));
        assert!(from_str.precise_location_enabled);

        let off = ConfigService::parse_config(HashMap::from([(
            "precise_location_enabled".into(),
            json!(false),
        )]));
        assert!(!off.precise_location_enabled);

        let missing = ConfigService::parse_config(HashMap::new());
        assert!(!missing.precise_location_enabled);
    }

    #[test]
    fn parses_island_content_flags_from_database_config() {
        let missing = ConfigService::parse_config(HashMap::new());
        assert!(missing.island_show_greeting);
        assert!(missing.island_show_weather);
        assert!(missing.island_show_quote);
        assert!(missing.island_show_music);
        assert!(missing.island_show_tapp);

        let mixed = ConfigService::parse_config(HashMap::from([
            ("island_show_greeting".into(), json!(false)),
            ("island_show_weather".into(), json!("false")),
            ("island_show_quote".into(), json!(true)),
            ("island_show_music".into(), json!("0")),
            ("island_show_tapp".into(), json!("true")),
        ]));
        assert!(!mixed.island_show_greeting);
        assert!(!mixed.island_show_weather);
        assert!(mixed.island_show_quote);
        assert!(!mixed.island_show_music);
        assert!(mixed.island_show_tapp);
    }

    #[test]
    fn parses_music_playback_flags_from_database_config() {
        let missing = ConfigService::parse_config(HashMap::new());
        assert!(missing.music_proxy_enabled);
        assert!(missing.music_preload_enabled);

        let off = ConfigService::parse_config(HashMap::from([
            ("music_proxy_enabled".into(), json!(false)),
            ("music_preload_enabled".into(), json!("false")),
        ]));
        assert!(!off.music_proxy_enabled);
        assert!(!off.music_preload_enabled);

        let on = ConfigService::parse_config(HashMap::from([
            ("music_proxy_enabled".into(), json!("true")),
            ("music_preload_enabled".into(), json!(true)),
        ]));
        assert!(on.music_proxy_enabled);
        assert!(on.music_preload_enabled);
    }

    #[test]
    fn parses_agent_rig_asset_id_from_database_config() {
        let hex = "a".repeat(64);
        let on = ConfigService::parse_config(HashMap::from([(
            "agent_rig_asset_id".into(),
            json!(hex.clone()),
        )]));
        assert_eq!(on.agent_rig_asset_id.as_deref(), Some(hex.as_str()));

        let cleared =
            ConfigService::parse_config(HashMap::from([("agent_rig_asset_id".into(), json!(""))]));
        assert_eq!(cleared.agent_rig_asset_id, None);

        let invalid = ConfigService::parse_config(HashMap::from([(
            "agent_rig_asset_id".into(),
            json!("not-a-sha256"),
        )]));
        assert_eq!(invalid.agent_rig_asset_id, None);
    }

    #[test]
    fn parses_see_through_hf_token_from_database_config() {
        assert!(crate::services::data_key::is_sensitive_config_key(
            "see_through_hf_token"
        ));

        let configured = ConfigService::parse_config(HashMap::from([(
            "see_through_hf_token".into(),
            json!("hf_test_token"),
        )]));
        assert_eq!(
            configured.see_through_hf_token.as_deref(),
            Some("hf_test_token")
        );

        let empty = ConfigService::parse_config(HashMap::from([(
            "see_through_hf_token".into(),
            json!("  "),
        )]));
        assert_eq!(empty.see_through_hf_token, None);
    }

    #[test]
    fn parses_speech_provider_fields_from_database_config() {
        let vendors = ConfigService::parse_config(HashMap::from([(
            "ai_vendor_sources".into(),
            json!([{
                "slug": "openai-work",
                "kind": "openai",
                "display_name": "Work OpenAI",
                "enabled": true,
                "api_format": "openai_responses",
                "credential_mode": "shared",
                "shared_key_ref": "openai",
                "api_key": "sk-work",
                "base_url": "https://api.openai.com/v1"
            }, {
                "slug": "legacy-openai",
                "kind": "openai_compatible",
                "display_name": "Legacy OpenAI",
                "enabled": true,
                "base_url": "https://legacy.example/v1"
            }]),
        )]));
        assert_eq!(vendors.ai_vendor_sources.len(), 2);
        assert_eq!(vendors.ai_vendor_sources[0].slug, "openai-work");
        assert_eq!(
            vendors.ai_vendor_sources[0].effective_api_format(),
            "openai_responses"
        );
        assert_eq!(
            vendors.ai_vendor_sources[0].api_key.as_deref(),
            Some("sk-work")
        );
        assert_eq!(vendors.ai_vendor_sources[0].credential_mode, "shared");
        assert_eq!(
            vendors.ai_vendor_sources[0].shared_key_ref.as_deref(),
            Some("openai")
        );
        assert_eq!(
            vendors.ai_vendor_sources[1].effective_api_format(),
            "openai"
        );

        let config = ConfigService::parse_config(HashMap::from([
            ("speech_provider".into(), json!("openai")),
            ("speech_reuse_text_credentials".into(), json!(false)),
            ("speech_stt_model".into(), json!("gpt-transcribe")),
            ("speech_tts_model".into(), json!("gpt-4o-mini-tts")),
            ("speech_tts_voice".into(), json!("marin")),
            ("speech_openai_api_key".into(), json!("sk-speech")),
            (
                "speech_openai_base_url".into(),
                json!("https://api.openai.com/v1"),
            ),
            ("speech_openrouter_api_key".into(), json!("sk-or-speech")),
        ]));
        assert_eq!(config.speech_provider, "openai");
        assert!(!config.speech_reuse_text_credentials);
        assert_eq!(config.speech_openai_api_key.as_deref(), Some("sk-speech"));
        assert_eq!(config.speech_openai_base_url, "https://api.openai.com/v1");
        assert_eq!(
            config.speech_openrouter_api_key.as_deref(),
            Some("sk-or-speech")
        );
        assert_eq!(config.speech_stt_model, "gpt-transcribe");
        assert_eq!(config.speech_tts_model, "gpt-4o-mini-tts");
        assert_eq!(config.speech_tts_voice, "marin");
    }

    #[test]
    fn parses_qq_bot_fields_from_database_config() {
        let configured = ConfigService::parse_config(HashMap::from([
            ("qq_bot_enabled".into(), json!(true)),
            ("qq_bot_app_id".into(), json!("102123456")),
            ("qq_bot_app_secret".into(), json!("qq-secret-value")),
        ]));
        assert!(configured.qq_bot_enabled);
        assert_eq!(configured.qq_bot_app_id, "102123456");
        assert_eq!(
            configured.qq_bot_app_secret.as_deref(),
            Some("qq-secret-value")
        );

        let from_str = ConfigService::parse_config(HashMap::from([
            ("qq_bot_enabled".into(), json!("true")),
            ("qq_bot_app_id".into(), json!("  ")),
            ("qq_bot_app_secret".into(), json!("  ")),
        ]));
        assert!(from_str.qq_bot_enabled);
        assert_eq!(from_str.qq_bot_app_id, "");
        assert_eq!(from_str.qq_bot_app_secret, None);

        let off =
            ConfigService::parse_config(HashMap::from([("qq_bot_enabled".into(), json!(false))]));
        assert!(!off.qq_bot_enabled);
        assert!(off.qq_bot_app_id.is_empty());
        assert_eq!(off.qq_bot_app_secret, None);
    }

    #[test]
    fn parses_telegram_bot_fields_from_database_config() {
        let configured = ConfigService::parse_config(HashMap::from([
            ("telegram_bot_enabled".into(), json!(true)),
            ("telegram_bot_token".into(), json!("123456:ABC-DEF")),
        ]));
        assert!(configured.telegram_bot_enabled);
        assert_eq!(
            configured.telegram_bot_token.as_deref(),
            Some("123456:ABC-DEF")
        );

        let from_str = ConfigService::parse_config(HashMap::from([
            ("telegram_bot_enabled".into(), json!("true")),
            ("telegram_bot_token".into(), json!("  ")),
        ]));
        assert!(from_str.telegram_bot_enabled);
        assert_eq!(from_str.telegram_bot_token, None);

        let off = ConfigService::parse_config(HashMap::from([(
            "telegram_bot_enabled".into(),
            json!(false),
        )]));
        assert!(!off.telegram_bot_enabled);
        assert_eq!(off.telegram_bot_token, None);
    }

    #[test]
    fn parses_discord_bot_fields_from_database_config() {
        let configured = ConfigService::parse_config(HashMap::from([
            ("discord_bot_enabled".into(), json!(true)),
            ("discord_bot_token".into(), json!("MTk4.Cl2FMQ.test")),
        ]));
        assert!(configured.discord_bot_enabled);
        assert_eq!(
            configured.discord_bot_token.as_deref(),
            Some("MTk4.Cl2FMQ.test")
        );

        let from_str = ConfigService::parse_config(HashMap::from([
            ("discord_bot_enabled".into(), json!("true")),
            ("discord_bot_token".into(), json!("  ")),
        ]));
        assert!(from_str.discord_bot_enabled);
        assert_eq!(from_str.discord_bot_token, None);

        let off = ConfigService::parse_config(HashMap::from([(
            "discord_bot_enabled".into(),
            json!(false),
        )]));
        assert!(!off.discord_bot_enabled);
        assert_eq!(off.discord_bot_token, None);
    }

    #[test]
    fn parses_feishu_bot_fields_from_database_config() {
        let configured = ConfigService::parse_config(HashMap::from([
            ("feishu_bot_enabled".into(), json!(true)),
            ("feishu_bot_app_id".into(), json!("cli_a")),
            ("feishu_bot_app_secret".into(), json!("fs-secret-value")),
        ]));
        assert!(configured.feishu_bot_enabled);
        assert_eq!(configured.feishu_bot_app_id, "cli_a");
        assert_eq!(
            configured.feishu_bot_app_secret.as_deref(),
            Some("fs-secret-value")
        );

        let from_str = ConfigService::parse_config(HashMap::from([
            ("feishu_bot_enabled".into(), json!("true")),
            ("feishu_bot_app_id".into(), json!("  ")),
            ("feishu_bot_app_secret".into(), json!("  ")),
        ]));
        assert!(from_str.feishu_bot_enabled);
        assert_eq!(from_str.feishu_bot_app_id, "");
        assert_eq!(from_str.feishu_bot_app_secret, None);

        let off = ConfigService::parse_config(HashMap::from([(
            "feishu_bot_enabled".into(),
            json!(false),
        )]));
        assert!(!off.feishu_bot_enabled);
        assert!(off.feishu_bot_app_id.is_empty());
        assert_eq!(off.feishu_bot_app_secret, None);
    }

    #[test]
    fn parses_tinyfish_api_key_from_database_config() {
        let configured = ConfigService::parse_config(HashMap::from([(
            "provider_tinyfish_api_key".into(),
            json!("tf-test-key"),
        )]));
        assert_eq!(
            configured.shared_tinyfish_api_key().as_deref(),
            Some("tf-test-key")
        );

        let empty = ConfigService::parse_config(HashMap::from([(
            "provider_tinyfish_api_key".into(),
            json!(""),
        )]));
        assert_eq!(empty.shared_tinyfish_api_key(), None);

        let cleared = ConfigService::parse_config(HashMap::from([(
            "provider_tinyfish_api_key".into(),
            json!(null),
        )]));
        assert_eq!(cleared.shared_tinyfish_api_key(), None);
    }

    #[test]
    fn merope_stays_off_without_required_models() {
        // Pro is required for onboarding. Lite is optional: without it,
        // Merope still runs, but Lite jobs must not fall back to Standard.
        let no_lite = DynamicConfig {
            merope_enabled: true,
            lite_enabled: false,
            pro_enabled: true,
            ..DynamicConfig::default()
        };
        assert_eq!(
            no_lite.merope_enabled_resolved(),
            no_lite.merope_switch_on()
        );
        assert!(no_lite.merope_needs_lite());
        assert!(!no_lite.merope_needs_pro());

        let no_pro = DynamicConfig {
            merope_enabled: true,
            lite_enabled: true,
            pro_enabled: false,
            ..DynamicConfig::default()
        };
        assert!(!no_pro.merope_enabled_resolved());
        assert!(!no_pro.merope_needs_lite());
        assert!(no_pro.merope_needs_pro());

        let with_tiers = DynamicConfig {
            merope_enabled: true,
            lite_enabled: true,
            pro_enabled: true,
            ..DynamicConfig::default()
        };
        assert_eq!(
            with_tiers.merope_enabled_resolved(),
            with_tiers.merope_switch_on()
        );
        assert!(!with_tiers.merope_needs_lite());
        assert!(!with_tiers.merope_needs_pro());
    }

    #[test]
    fn merope_speech_stays_off_unless_persona_is_on() {
        let speech_off = DynamicConfig {
            merope_enabled: true,
            merope_speech_enabled: false,
            pro_enabled: true,
            ..DynamicConfig::default()
        };
        assert!(!speech_off.merope_speech_enabled_resolved());

        let speech_on = DynamicConfig {
            merope_enabled: true,
            merope_speech_enabled: true,
            pro_enabled: true,
            ..DynamicConfig::default()
        };
        assert!(speech_on.merope_speech_enabled_resolved());

        let persona_off = DynamicConfig {
            merope_enabled: false,
            merope_speech_enabled: true,
            pro_enabled: true,
            ..DynamicConfig::default()
        };
        assert!(!persona_off.merope_speech_enabled_resolved());

        let no_pro = DynamicConfig {
            merope_enabled: true,
            merope_speech_enabled: true,
            pro_enabled: false,
            ..DynamicConfig::default()
        };
        assert!(!no_pro.merope_speech_enabled_resolved());
    }

    #[test]
    fn parses_control_panel_layout_from_database_config() {
        let missing = ConfigService::parse_config(HashMap::new());
        assert_eq!(missing.control_panel_layout, None);

        for (value, expected) in [
            (json!(null), None),
            (json!([]), Some("[]".to_string())),
            (json!("[]"), Some("[]".to_string())),
            (
                json!([{ "id": "cp-weather" }]),
                Some("[{\"id\":\"cp-weather\"}]".to_string()),
            ),
            (
                json!("[{\"id\":\"cp-weather\"}]"),
                Some("[{\"id\":\"cp-weather\"}]".to_string()),
            ),
        ] {
            let config = ConfigService::parse_config(HashMap::from([(
                "control_panel_layout".into(),
                value,
            )]));
            assert_eq!(config.control_panel_layout, expected);
        }
    }

    #[test]
    fn parses_dashboard_layout_mode_from_database_config() {
        let free = ConfigService::parse_config(HashMap::from([(
            "dashboard_layout_mode".into(),
            json!("free"),
        )]));
        assert_eq!(free.dashboard_layout_mode.as_deref(), Some("free"));

        let other = ConfigService::parse_config(HashMap::from([(
            "dashboard_layout_mode".into(),
            json!("  custom  "),
        )]));
        assert_eq!(other.dashboard_layout_mode.as_deref(), Some("standard"));

        let missing = ConfigService::parse_config(HashMap::new());
        assert_eq!(missing.dashboard_layout_mode, None);
    }

    #[test]
    fn empty_platform_secrets_are_absent() {
        let config = ConfigService::parse_config(HashMap::from([
            ("github_token".into(), json!("")),
            ("github_username".into(), json!("  ")),
            ("steam_api_key".into(), json!(null)),
            ("x_bearer_token".into(), json!("ghp_kept")),
        ]));
        assert_eq!(config.github_token, None);
        assert_eq!(config.github_username, None);
        assert_eq!(config.steam_api_key, None);
        assert_eq!(config.x_bearer_token.as_deref(), Some("ghp_kept"));
    }

    #[test]
    fn every_dynamic_config_field_has_a_database_parse_branch_or_documented_exemption() {
        let config_file = syn::parse_file(include_str!("../config.rs")).expect("valid config.rs");
        let dynamic_config = config_file
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Struct(item) if item.ident == "DynamicConfig" => Some(item),
                _ => None,
            })
            .expect("DynamicConfig struct exists");
        let fields: HashSet<String> = dynamic_config
            .fields
            .iter()
            .filter_map(|field| field.ident.as_ref().map(ToString::to_string))
            .collect();

        let service_file =
            syn::parse_file(include_str!("config_service.rs")).expect("valid config_service.rs");
        let parse_config = service_file
            .items
            .iter()
            .find_map(|item| match item {
                syn::Item::Impl(item) => item.items.iter().find_map(|impl_item| match impl_item {
                    syn::ImplItem::Fn(function) if function.sig.ident == "parse_config" => {
                        Some(function)
                    }
                    _ => None,
                }),
                _ => None,
            })
            .expect("ConfigService::parse_config exists");

        #[derive(Default)]
        struct MapGetVisitor {
            parsed_fields: HashSet<String>,
            active_keys: Vec<String>,
        }

        impl MapGetVisitor {
            fn map_get_key(expr: &syn::Expr) -> Option<String> {
                let syn::Expr::MethodCall(call) = expr else {
                    return None;
                };
                if call.method != "get" {
                    return Self::map_get_key(call.receiver.as_ref());
                }
                if call.args.len() != 1 {
                    return None;
                }
                let syn::Expr::Path(receiver) = call.receiver.as_ref() else {
                    return None;
                };
                if receiver.path.segments.len() != 1 || receiver.path.segments[0].ident != "map" {
                    return None;
                }
                match call.args.first()? {
                    syn::Expr::Lit(syn::ExprLit {
                        lit: syn::Lit::Str(key),
                        ..
                    }) => Some(key.value()),
                    _ => None,
                }
            }

            fn condition_key(expr: &syn::Expr) -> Option<String> {
                match expr {
                    syn::Expr::Let(expr_let) => Self::map_get_key(&expr_let.expr),
                    syn::Expr::Paren(paren) => Self::condition_key(&paren.expr),
                    _ => None,
                }
            }
        }

        impl<'ast> Visit<'ast> for MapGetVisitor {
            fn visit_expr_if(&mut self, node: &'ast syn::ExprIf) {
                if let Some(key) = Self::condition_key(&node.cond) {
                    self.active_keys.push(key);
                    syn::visit::visit_block(self, &node.then_branch);
                    self.active_keys.pop();
                    if let Some((_, branch)) = &node.else_branch {
                        self.visit_expr(branch);
                    }
                } else {
                    syn::visit::visit_expr_if(self, node);
                }
            }

            fn visit_expr_assign(&mut self, node: &'ast syn::ExprAssign) {
                if let syn::Expr::Field(field) = node.left.as_ref() {
                    if let syn::Expr::Path(base) = field.base.as_ref() {
                        if base.path.is_ident("config") {
                            if let syn::Member::Named(member) = &field.member {
                                let name = member.to_string();
                                if self.active_keys.iter().any(|key| key == &name) {
                                    self.parsed_fields.insert(name);
                                }
                            }
                        }
                    }
                }
                syn::visit::visit_expr_assign(self, node);
            }
        }

        // Fields that are intentionally derived at runtime instead of loaded from
        // the configurations table. Every entry must explain the alternate source.
        const EXEMPTIONS: &[(&str, &str)] = &[];

        let mut visitor = MapGetVisitor::default();
        visitor.visit_impl_item_fn(parse_config);
        // `standard_fields!` reads each of these keys into the field of the
        // same name.
        visitor
            .parsed_fields
            .extend(super::STANDARD_KEYS.iter().map(|key| key.to_string()));
        let exemptions: HashSet<&str> = EXEMPTIONS.iter().map(|(field, _)| *field).collect();
        assert!(
            EXEMPTIONS
                .iter()
                .all(|(_, reason)| !reason.trim().is_empty())
        );

        let mut missing: Vec<_> = fields
            .difference(&visitor.parsed_fields)
            .filter(|field| !exemptions.contains(field.as_str()))
            .cloned()
            .collect();
        missing.sort();
        assert!(
            missing.is_empty(),
            "DynamicConfig fields missing from ConfigService::parse_config: {}",
            missing.join(", ")
        );
    }
}
