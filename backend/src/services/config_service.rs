use crate::config::DynamicConfig;
use anyhow::{Context, Result};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};
use serde_json::Value as JsonValue;
use std::collections::HashMap;

/// 配置服务 - 用于从数据库读写动态配置
pub struct ConfigService {
    db: DatabaseConnection,
}

impl ConfigService {
    pub fn new(db: DatabaseConnection) -> Self {
        Self { db }
    }

    /// 从数据库加载所有配置
    pub async fn load_config(&self) -> Result<DynamicConfig> {
        // 使用 ConnectionTrait 的方法进行查询
        let sql = "SELECT key, value FROM configurations";
        let rows = self
            .db
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
                // 未迁移的遗留明文原样通过。
                let value = crate::services::data_key::open_config_value(&key, value);
                config_map.insert(key, value);
            }
        }

        Ok(Self::parse_config(config_map))
    }

    /// 从配置映射解析为 DynamicConfig
    fn parse_config(map: HashMap<String, JsonValue>) -> DynamicConfig {
        let mut config = DynamicConfig::default();

        // AI 配置
        if let Some(v) = map.get("ai_provider") {
            if let Some(s) = v.as_str() {
                config.ai_provider = s.to_string();
            }
        }

        if let Some(v) = map.get("gemini_api_key") {
            config.gemini_api_key = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("gemini_model") {
            if let Some(s) = v.as_str() {
                config.gemini_model = s.to_string();
            }
        }

        if let Some(v) = map.get("openai_api_key") {
            config.openai_api_key = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("openai_model") {
            if let Some(s) = v.as_str() {
                config.openai_model = s.to_string();
            }
        }

        if let Some(v) = map.get("openai_base_url") {
            if let Some(s) = v.as_str() {
                config.openai_base_url = s.to_string();
            }
        }

        if let Some(v) = map.get("openai_max_tokens") {
            if let Some(n) = v.as_i64() {
                config.openai_max_tokens = n as i32;
            }
        }

        if let Some(v) = map.get("topic_style") {
            if let Some(s) = v.as_str() {
                config.topic_style = s.to_string();
            }
        }

        // AI Lite 模型配置
        if let Some(v) = map.get("lite_enabled") {
            if let Some(b) = v.as_bool() {
                config.lite_enabled = b;
            } else if let Some(s) = v.as_str() {
                config.lite_enabled = s == "true";
            }
        }
        if let Some(v) = map.get("lite_ai_provider") {
            if let Some(s) = v.as_str() {
                config.lite_ai_provider = s.to_string();
            }
        }
        if let Some(v) = map.get("lite_gemini_api_key") {
            config.lite_gemini_api_key = v.as_str().map(str::to_string);
        }
        if let Some(v) = map.get("lite_gemini_model") {
            if let Some(s) = v.as_str() {
                config.lite_gemini_model = s.to_string();
            }
        }
        if let Some(v) = map.get("lite_openai_api_key") {
            config.lite_openai_api_key = v.as_str().map(str::to_string);
        }
        if let Some(v) = map.get("lite_openai_model") {
            if let Some(s) = v.as_str() {
                config.lite_openai_model = s.to_string();
            }
        }
        if let Some(v) = map.get("lite_openai_base_url") {
            if let Some(s) = v.as_str() {
                config.lite_openai_base_url = s.to_string();
            }
        }

        // AI Pro 模型配置
        if let Some(v) = map.get("pro_enabled") {
            if let Some(b) = v.as_bool() {
                config.pro_enabled = b;
            } else if let Some(s) = v.as_str() {
                config.pro_enabled = s == "true";
            }
        }

        if let Some(v) = map.get("pro_ai_provider") {
            if let Some(s) = v.as_str() {
                config.pro_ai_provider = s.to_string();
            }
        }

        if let Some(v) = map.get("pro_gemini_api_key") {
            config.pro_gemini_api_key = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("pro_gemini_model") {
            if let Some(s) = v.as_str() {
                config.pro_gemini_model = s.to_string();
            }
        }

        if let Some(v) = map.get("pro_openai_api_key") {
            config.pro_openai_api_key = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("pro_openai_model") {
            if let Some(s) = v.as_str() {
                config.pro_openai_model = s.to_string();
            }
        }

        if let Some(v) = map.get("pro_openai_base_url") {
            if let Some(s) = v.as_str() {
                config.pro_openai_base_url = s.to_string();
            }
        }

        // 平台配置
        if let Some(v) = map.get("github_enabled") {
            if let Some(b) = v.as_bool() {
                config.github_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.github_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("github_token") {
            config.github_token = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("github_username") {
            config.github_username = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("bilibili_enabled") {
            if let Some(b) = v.as_bool() {
                config.bilibili_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.bilibili_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("bilibili_uid") {
            config.bilibili_uid = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("steam_enabled") {
            if let Some(b) = v.as_bool() {
                config.steam_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.steam_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("steam_api_key") {
            config.steam_api_key = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("steam_id") {
            config.steam_id = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("youtube_enabled") {
            if let Some(b) = v.as_bool() {
                config.youtube_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.youtube_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("youtube_api_key") {
            config.youtube_api_key = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("youtube_channel_id") {
            config.youtube_channel_id = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("netease_enabled") {
            if let Some(b) = v.as_bool() {
                config.netease_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.netease_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("netease_user_id") {
            config.netease_user_id = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("bangumi_enabled") {
            if let Some(b) = v.as_bool() {
                config.bangumi_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.bangumi_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("bangumi_username") {
            config.bangumi_username = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("bangumi_access_token") {
            config.bangumi_access_token = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("bangumi_user_agent") {
            config.bangumi_user_agent = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("x_enabled") {
            if let Some(b) = v.as_bool() {
                config.x_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.x_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("x_username") {
            config.x_username = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("x_bearer_token") {
            config.x_bearer_token = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("discord_enabled") {
            if let Some(b) = v.as_bool() {
                config.discord_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.discord_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("discord_access_token") {
            config.discord_access_token = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("discord_refresh_token") {
            config.discord_refresh_token = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("discord_token_expires_at") {
            config.discord_token_expires_at = v.as_str().map(|s| s.to_string()).or_else(|| {
                v.as_i64()
                    .map(|n| n.to_string())
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            });
        }

        if let Some(v) = map.get("mal_enabled") {
            if let Some(b) = v.as_bool() {
                config.mal_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.mal_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("mal_username") {
            config.mal_username = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("mal_client_id") {
            config.mal_client_id = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("xbox_enabled") {
            if let Some(b) = v.as_bool() {
                config.xbox_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.xbox_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("xbox_gamertag") {
            config.xbox_gamertag = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("openxbl_api_key") {
            config.openxbl_api_key = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("psn_enabled") {
            if let Some(b) = v.as_bool() {
                config.psn_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.psn_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("psn_online_id") {
            config.psn_online_id = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("psn_npsso") {
            config.psn_npsso = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("discord_user_id") {
            config.discord_user_id = v.as_str().map(|s| s.to_string()).or_else(|| {
                v.as_i64()
                    .map(|n| n.to_string())
                    .or_else(|| v.as_u64().map(|n| n.to_string()))
            });
        }

        // 平台展示顺序（JSON 数组，或历史上误存为 JSON 字符串）
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
        if let Some(v) = map.get("ui_wallpaper_url") {
            config.ui_wallpaper_url = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("ui_wallpaper_blur") {
            if let Some(n) = v.as_i64() {
                config.ui_wallpaper_blur = n as i32;
            }
        }
        if let Some(v) = map.get("ui_wallpaper_parallax") {
            if let Some(b) = v.as_bool() {
                config.ui_wallpaper_parallax = b;
            }
        }
        // Evocative 壁纸动效
        if let Some(v) = map.get("ui_evocative_parallax") {
            if let Some(b) = v.as_bool() {
                config.ui_evocative_parallax = b;
            }
        }
        if let Some(v) = map.get("ui_evocative_dynamic_blur") {
            if let Some(b) = v.as_bool() {
                config.ui_evocative_dynamic_blur = b;
            }
        }
        if let Some(v) = map.get("ui_evocative_ripple") {
            if let Some(b) = v.as_bool() {
                config.ui_evocative_ripple = b;
            }
        }
        if let Some(v) = map.get("ui_evocative_fps") {
            if let Some(n) = v.as_i64() {
                config.ui_evocative_fps = n as i32;
            }
        }
        if let Some(v) = map.get("ui_evocative_ripple_quality") {
            if let Some(n) = v.as_f64() {
                config.ui_evocative_ripple_quality = n;
            }
        }
        if let Some(v) = map.get("pet_enabled") {
            if let Some(b) = v.as_bool() {
                config.pet_enabled = b;
            } else if let Some(s) = v.as_str() {
                config.pet_enabled = s == "true";
            }
        }
        if let Some(v) = map.get("pet_image_url") {
            config.pet_image_url = v.as_str().map(|s| s.to_string());
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
        if let Some(v) = map.get("ai_image_provider") {
            if let Some(s) = v.as_str() {
                config.ai_image_provider = s.to_string();
            }
        }
        if let Some(v) = map.get("ai_image_model") {
            if let Some(s) = v.as_str() {
                config.ai_image_model = s.to_string();
            }
        }
        if let Some(v) = map.get("ai_image_openai_api_key") {
            config.ai_image_openai_api_key = v.as_str().map(str::to_string);
        }
        if let Some(v) = map.get("ai_image_openai_base_url") {
            if let Some(s) = v.as_str() {
                if !s.trim().is_empty() {
                    config.ai_image_openai_base_url = s.to_string();
                }
            }
        }
        if let Some(v) = map.get("ai_image_openrouter_api_key") {
            config.ai_image_openrouter_api_key = v.as_str().map(str::to_string);
        }
        if let Some(v) = map.get("ai_image_volcengine_api_key") {
            config.ai_image_volcengine_api_key = v.as_str().map(str::to_string);
        }
        if let Some(v) = map.get("ai_image_volcengine_base_url") {
            if let Some(s) = v.as_str() {
                config.ai_image_volcengine_base_url = s.to_string();
            }
        }

        // Tripo 3D 独立配置
        if let Some(v) = map.get("tripo_enabled") {
            config.tripo_enabled = v
                .as_bool()
                .or_else(|| v.as_str().map(|s| s == "true"))
                .unwrap_or(config.tripo_enabled);
        }
        if let Some(v) = map.get("tripo_api_key") {
            config.tripo_api_key = v.as_str().map(str::to_string);
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
        if let Some(v) = map.get("tencent_secret_id") {
            config.tencent_secret_id = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("tencent_secret_key") {
            config.tencent_secret_key = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("tencent_region") {
            config.tencent_region = v.as_str().map(|s| s.to_string());
        }

        if let Some(v) = map.get("enable_auto_fetch") {
            if let Some(b) = v.as_bool() {
                config.enable_auto_fetch = b;
            }
        }
        if let Some(v) = map.get("fetch_interval_hours") {
            if let Some(n) = v.as_i64() {
                config.fetch_interval_hours = n as i32;
            }
        }

        // OAuth 配置
        if let Some(v) = map.get("github_client_id") {
            config.github_client_id = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("github_client_secret") {
            config.github_client_secret = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("github_redirect_url") {
            if let Some(s) = v.as_str() {
                config.github_redirect_url = s.to_string();
            }
        }

        // OIDC / 自定义 OAuth providers 列表
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
            let mode = v.as_str().unwrap_or("inactivity").trim().to_ascii_lowercase();
            config.tapp_private_install_cleanup = if mode == "logout" {
                "logout".to_string()
            } else {
                "inactivity".to_string()
            };
        }
        if let Some(v) = map.get("tapp_private_install_inactivity_days") {
            let days = v.as_i64().or_else(|| v.as_u64().map(|n| n as i64)).unwrap_or(14);
            config.tapp_private_install_inactivity_days = days.clamp(1, 365) as i32;
        }

        // 站点 URL 配置
        if let Some(v) = map.get("base_url") {
            config.base_url = v.as_str().map(|s| s.to_string());
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
            if let Some(s) = v.as_str() {
                config.control_panel_layout = Some(s.to_string());
            } else {
                config.control_panel_layout = Some(v.to_string());
            }
        }
        if let Some(v) = map.get("control_panel_rows") {
            if let Some(n) = v.as_i64() {
                config.control_panel_rows = n as i32;
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
        if let Some(v) = map.get("site_title") {
            config.site_title = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("site_description") {
            config.site_description = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("site_favicon") {
            config.site_favicon = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("site_keywords") {
            config.site_keywords = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("site_og_image") {
            config.site_og_image = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("site_noindex") {
            if let Some(b) = v.as_bool() {
                config.site_noindex = b;
            } else if let Some(s) = v.as_str() {
                config.site_noindex = s == "true";
            }
        }
        if let Some(v) = map.get("site_visibility_policy") {
            if let Some(s) = v.as_str() {
                config.site_visibility_policy = s.to_string();
            }
        }
        if let Some(v) = map.get("site_ai_intro") {
            config.site_ai_intro = v.as_str().map(|s| s.to_string());
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
            // Legacy: only noindex known → treat as private for consumers that read policy.
            config.site_visibility_policy = "private".to_string();
        }
        if let Some(v) = map.get("ga_measurement_id") {
            config.ga_measurement_id = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("umami_website_id") {
            config.umami_website_id = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("umami_script_url") {
            config.umami_script_url = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("site_icp") {
            config.site_icp = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("site_gongan") {
            config.site_gongan = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("cloud_sponsors") {
            config.cloud_sponsors = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("site_footer_custom") {
            config.site_footer_custom = v.as_str().map(|s| s.to_string());
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
        if let Some(v) = map.get("music_source") {
            config.music_source = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("music_playlist_id") {
            config.music_playlist_id = v.as_str().map(|s| s.to_string());
        }

        // Tapp 权限下放配置
        // 普通用户可下放的 elevated 权限（14 项）
        if let Some(v) = map.get("user_perm_ai_generate") {
            if let Some(b) = v.as_bool() {
                config.user_perm_ai_generate = b;
            }
        }
        if let Some(v) = map.get("user_perm_ai_analyze") {
            if let Some(b) = v.as_bool() {
                config.user_perm_ai_analyze = b;
            }
        }
        if let Some(v) = map.get("user_perm_ai_chat") {
            if let Some(b) = v.as_bool() {
                config.user_perm_ai_chat = b;
            }
        }
        if let Some(v) = map.get("user_perm_ai_image") {
            if let Some(b) = v.as_bool() {
                config.user_perm_ai_image = b;
            }
        }
        if let Some(v) = map.get("user_perm_report_write") {
            if let Some(b) = v.as_bool() {
                config.user_perm_report_write = b;
            }
        }
        if let Some(v) = map.get("user_perm_network_fetch") {
            if let Some(b) = v.as_bool() {
                config.user_perm_network_fetch = b;
            }
        }
        if let Some(v) = map.get("user_perm_media_control") {
            if let Some(b) = v.as_bool() {
                config.user_perm_media_control = b;
            }
        }
        if let Some(v) = map.get("user_perm_component_theme") {
            if let Some(b) = v.as_bool() {
                config.user_perm_component_theme = b;
            }
        }
        if let Some(v) = map.get("user_perm_shortcut_register") {
            if let Some(b) = v.as_bool() {
                config.user_perm_shortcut_register = b;
            }
        }
        if let Some(v) = map.get("user_perm_event_publish") {
            if let Some(b) = v.as_bool() {
                config.user_perm_event_publish = b;
            }
        }
        if let Some(v) = map.get("user_perm_scheduler_register") {
            if let Some(b) = v.as_bool() {
                config.user_perm_scheduler_register = b;
            }
        }
        if let Some(v) = map.get("user_perm_speech_tts") {
            if let Some(b) = v.as_bool() {
                config.user_perm_speech_tts = b;
            }
        }
        if let Some(v) = map.get("user_perm_speech_asr") {
            if let Some(b) = v.as_bool() {
                config.user_perm_speech_asr = b;
            }
        }
        if let Some(v) = map.get("user_perm_widget_register") {
            if let Some(b) = v.as_bool() {
                config.user_perm_widget_register = b;
            }
        }

        // 游客可下放的 elevated 权限（13 项）
        if let Some(v) = map.get("guest_perm_ai_generate") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_ai_generate = b;
            }
        }
        if let Some(v) = map.get("guest_perm_ai_analyze") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_ai_analyze = b;
            }
        }
        if let Some(v) = map.get("guest_perm_ai_chat") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_ai_chat = b;
            }
        }
        if let Some(v) = map.get("guest_perm_ai_image") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_ai_image = b;
            }
        }
        if let Some(v) = map.get("guest_perm_report_write") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_report_write = b;
            }
        }
        if let Some(v) = map.get("guest_perm_network_fetch") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_network_fetch = b;
            }
        }
        if let Some(v) = map.get("guest_perm_media_control") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_media_control = b;
            }
        }
        if let Some(v) = map.get("guest_perm_component_theme") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_component_theme = b;
            }
        }
        if let Some(v) = map.get("guest_perm_shortcut_register") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_shortcut_register = b;
            }
        }
        if let Some(v) = map.get("guest_perm_event_publish") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_event_publish = b;
            }
        }
        if let Some(v) = map.get("guest_perm_scheduler_register") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_scheduler_register = b;
            }
        }
        if let Some(v) = map.get("guest_perm_speech_tts") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_speech_tts = b;
            }
        }
        if let Some(v) = map.get("guest_perm_speech_asr") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_speech_asr = b;
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

        // 网络代理配置
        if let Some(v) = map.get("proxy_enabled") {
            if let Some(b) = v.as_bool() {
                config.proxy_enabled = b;
            }
        }
        if let Some(v) = map.get("proxy_url") {
            config.proxy_url = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("proxy_bypass") {
            config.proxy_bypass = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("gemini_base_url") {
            config.gemini_base_url = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("github_api_base_url") {
            config.github_api_base_url = v.as_str().map(|s| s.to_string());
        }

        config
    }

    /// 更新单个配置项
    ///
    /// 敏感 key 由 [`crate::services::data_key::is_sensitive_config_key`] 判定
    /// （密钥类 token/secret/api_key/password/npsso；排除 `*_tokens` 配额与
    /// `*_expires_at` 元数据）。在这里加密后落库，调用方始终传明文。
    /// 这是配置写入的唯一漏斗，加密放在这一层就不会有绕过的写路径。
    pub async fn update_config(&self, key: &str, value: JsonValue) -> Result<()> {
        let value = crate::services::data_key::seal_config_value(key, value);

        let sql = r#"
            INSERT INTO configurations (key, value, updated_at)
            VALUES ($1, $2, CURRENT_TIMESTAMP)
            ON CONFLICT (key) DO UPDATE
            SET value = $2, updated_at = CURRENT_TIMESTAMP
        "#;

        self.db
            .execute_raw(Statement::from_sql_and_values(
                sea_orm::DatabaseBackend::Postgres,
                sql,
                vec![key.into(), value.into()],
            ))
            .await
            .context("Failed to update configuration")?;

        Ok(())
    }

    /// 批量更新配置
    pub async fn update_configs(&self, updates: HashMap<String, JsonValue>) -> Result<()> {
        let count = updates.len();
        for (key, value) in updates {
            self.update_config(&key, value).await?;
        }
        tracing::info!("✅ Updated {} configurations", count);
        Ok(())
    }
}
