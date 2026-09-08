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

        if let Some(v) = map.get("openweather_api_key") {
            config.openweather_api_key = v.as_str().map(str::to_string);
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
            config.github_token = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("github_username") {
            config.github_username = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("bilibili_enabled") {
            if let Some(b) = v.as_bool() {
                config.bilibili_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.bilibili_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("bilibili_uid") {
            config.bilibili_uid = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("steam_enabled") {
            if let Some(b) = v.as_bool() {
                config.steam_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.steam_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("steam_api_key") {
            config.steam_api_key = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("steam_id") {
            config.steam_id = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("youtube_enabled") {
            if let Some(b) = v.as_bool() {
                config.youtube_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.youtube_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("youtube_api_key") {
            config.youtube_api_key = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("youtube_channel_id") {
            config.youtube_channel_id = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("netease_enabled") {
            if let Some(b) = v.as_bool() {
                config.netease_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.netease_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("netease_user_id") {
            config.netease_user_id = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("bangumi_enabled") {
            if let Some(b) = v.as_bool() {
                config.bangumi_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.bangumi_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("bangumi_username") {
            config.bangumi_username = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("bangumi_access_token") {
            config.bangumi_access_token = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("bangumi_user_agent") {
            config.bangumi_user_agent = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("x_enabled") {
            if let Some(b) = v.as_bool() {
                config.x_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.x_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("x_username") {
            config.x_username = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("x_bearer_token") {
            config.x_bearer_token = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("discord_enabled") {
            if let Some(b) = v.as_bool() {
                config.discord_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.discord_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("discord_access_token") {
            config.discord_access_token = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("discord_refresh_token") {
            config.discord_refresh_token = opt_nonempty_string(v);
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
            config.mal_username = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("mal_client_id") {
            config.mal_client_id = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("xbox_enabled") {
            if let Some(b) = v.as_bool() {
                config.xbox_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.xbox_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("xbox_gamertag") {
            config.xbox_gamertag = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("openxbl_api_key") {
            config.openxbl_api_key = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("psn_enabled") {
            if let Some(b) = v.as_bool() {
                config.psn_enabled = Some(b);
            } else if let Some(s) = v.as_str() {
                config.psn_enabled = Some(s == "true");
            }
        }

        if let Some(v) = map.get("psn_online_id") {
            config.psn_online_id = opt_nonempty_string(v);
        }

        if let Some(v) = map.get("psn_npsso") {
            config.psn_npsso = opt_nonempty_string(v);
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
        if let Some(v) = map.get("ui_theme") {
            config.ui_theme = v.as_str().map(str::to_string);
        }
        if let Some(v) = map.get("ui_primary_color") {
            config.ui_primary_color = v.as_str().map(str::to_string);
        }
        if let Some(v) = map.get("ui_secondary_color") {
            config.ui_secondary_color = v.as_str().map(str::to_string);
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
        if let Some(v) = map.get("see_through_hf_token") {
            config.see_through_hf_token = opt_nonempty_string(v);
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
        if let Some(v) = map.get("speech_provider") {
            if let Some(s) = v.as_str() {
                config.speech_provider = s.to_string();
            }
        }
        if let Some(v) = map.get("speech_reuse_text_credentials") {
            if let Some(b) = v.as_bool() {
                config.speech_reuse_text_credentials = b;
            } else if let Some(s) = v.as_str() {
                config.speech_reuse_text_credentials = s == "true" || s == "1";
            }
        }
        if let Some(v) = map.get("speech_openai_api_key") {
            config.speech_openai_api_key = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("speech_openai_base_url") {
            if let Some(s) = v.as_str() {
                config.speech_openai_base_url = s.to_string();
            }
        }
        if let Some(v) = map.get("speech_openrouter_api_key") {
            config.speech_openrouter_api_key = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("provider_openai_api_key") {
            config.provider_openai_api_key = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("provider_openai_base_url") {
            if let Some(s) = v.as_str() {
                config.provider_openai_base_url = s.to_string();
            }
        }
        if let Some(v) = map.get("provider_openrouter_api_key") {
            config.provider_openrouter_api_key = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("provider_gemini_api_key") {
            config.provider_gemini_api_key = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("provider_tinyfish_api_key") {
            config.provider_tinyfish_api_key = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("provider_volcengine_api_key") {
            config.provider_volcengine_api_key = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("provider_volcengine_base_url") {
            if let Some(s) = v.as_str() {
                config.provider_volcengine_base_url = s.to_string();
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
        if let Some(v) = map.get("ai_source") {
            if let Some(s) = v.as_str() {
                config.ai_source = s.to_string();
            }
        }
        if let Some(v) = map.get("lite_ai_source") {
            if let Some(s) = v.as_str() {
                config.lite_ai_source = s.to_string();
            }
        }
        if let Some(v) = map.get("pro_ai_source") {
            if let Some(s) = v.as_str() {
                config.pro_ai_source = s.to_string();
            }
        }
        if let Some(v) = map.get("ai_image_source") {
            if let Some(s) = v.as_str() {
                config.ai_image_source = s.to_string();
            }
        }
        if let Some(v) = map.get("speech_source") {
            if let Some(s) = v.as_str() {
                config.speech_source = s.to_string();
            }
        }
        if let Some(v) = map.get("speech_stt_model") {
            if let Some(s) = v.as_str() {
                config.speech_stt_model = s.to_string();
            }
        }
        if let Some(v) = map.get("speech_tts_model") {
            if let Some(s) = v.as_str() {
                config.speech_tts_model = s.to_string();
            }
        }
        if let Some(v) = map.get("speech_tts_voice") {
            if let Some(s) = v.as_str() {
                config.speech_tts_voice = s.to_string();
            }
        }
        if let Some(v) = map.get("agora_convo_enabled") {
            config.agora_convo_enabled = v
                .as_bool()
                .or_else(|| v.as_str().map(|s| s == "true" || s == "1"))
                .unwrap_or(config.agora_convo_enabled);
        }
        if let Some(v) = map.get("agora_app_id") {
            if let Some(s) = v.as_str() {
                config.agora_app_id = s.to_string();
            }
        }
        if let Some(v) = map.get("agora_app_certificate") {
            if let Some(s) = v.as_str() {
                config.agora_app_certificate = s.to_string();
            }
        }
        if let Some(v) = map.get("agora_customer_id") {
            if let Some(s) = v.as_str() {
                config.agora_customer_id = s.to_string();
            }
        }
        if let Some(v) = map.get("agora_customer_secret") {
            config.agora_customer_secret = v.as_str().map(|s| s.to_string());
        }
        if let Some(v) = map.get("agora_api_base") {
            if let Some(s) = v.as_str() {
                config.agora_api_base = s.to_string();
            }
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
        if let Some(v) = map.get("qq_bot_app_secret") {
            config.qq_bot_app_secret = opt_nonempty_string(v);
        }
        if let Some(v) = map.get("telegram_bot_enabled") {
            config.telegram_bot_enabled = v
                .as_bool()
                .or_else(|| v.as_str().map(|s| s == "true" || s == "1"))
                .unwrap_or(config.telegram_bot_enabled);
        }
        if let Some(v) = map.get("telegram_bot_token") {
            config.telegram_bot_token = opt_nonempty_string(v);
        }
        if let Some(v) = map.get("discord_bot_enabled") {
            config.discord_bot_enabled = v
                .as_bool()
                .or_else(|| v.as_str().map(|s| s == "true" || s == "1"))
                .unwrap_or(config.discord_bot_enabled);
        }
        if let Some(v) = map.get("discord_bot_token") {
            config.discord_bot_token = opt_nonempty_string(v);
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
        if let Some(v) = map.get("google_site_verification") {
            config.google_site_verification = v.as_str().map(|s| s.to_string());
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

        // Tapp 权限下放配置
        // 普通用户可下放的 elevated 权限
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
        if let Some(v) = map.get("user_perm_ai_search") {
            if let Some(b) = v.as_bool() {
                config.user_perm_ai_search = b;
            }
        }
        if let Some(v) = map.get("user_perm_ai_image") {
            if let Some(b) = v.as_bool() {
                config.user_perm_ai_image = b;
            }
        }
        if let Some(v) = map.get("user_perm_3d_generate") {
            if let Some(b) = v.as_bool() {
                config.user_perm_3d_generate = b;
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
        if let Some(v) = map.get("user_perm_storage_write") {
            if let Some(b) = v.as_bool() {
                config.user_perm_storage_write = b;
            }
        }
        if let Some(v) = map.get("user_perm_federation_post") {
            if let Some(b) = v.as_bool() {
                config.user_perm_federation_post = b;
            }
        }
        if let Some(v) = map.get("user_perm_federation_channel") {
            if let Some(b) = v.as_bool() {
                config.user_perm_federation_channel = b;
            }
        }
        if let Some(v) = map.get("user_perm_federation_room") {
            if let Some(b) = v.as_bool() {
                config.user_perm_federation_room = b;
            }
        }
        if let Some(v) = map.get("user_perm_brew_comment_write") {
            if let Some(b) = v.as_bool() {
                config.user_perm_brew_comment_write = b;
            }
        }

        // 游客可下放的 elevated 权限
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
        if let Some(v) = map.get("guest_perm_ai_search") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_ai_search = b;
            }
        }
        if let Some(v) = map.get("guest_perm_ai_image") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_ai_image = b;
            }
        }
        if let Some(v) = map.get("guest_perm_3d_generate") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_3d_generate = b;
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
        if let Some(v) = map.get("guest_perm_storage_write") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_storage_write = b;
            }
        }
        if let Some(v) = map.get("guest_perm_federation_post") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_federation_post = b;
            }
        }
        if let Some(v) = map.get("guest_perm_federation_channel") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_federation_channel = b;
            }
        }
        if let Some(v) = map.get("guest_perm_federation_room") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_federation_room = b;
            }
        }
        if let Some(v) = map.get("guest_perm_brew_comment_write") {
            if let Some(b) = v.as_bool() {
                config.guest_perm_brew_comment_write = b;
            }
        }

        // AI 使用限额配置
        if let Some(v) = map.get("user_ai_daily_calls") {
            if let Some(n) = v.as_i64() {
                config.user_ai_daily_calls = n as i32;
            }
        }
        if let Some(v) = map.get("user_ai_daily_tokens") {
            if let Some(n) = v.as_i64() {
                config.user_ai_daily_tokens = n as i32;
            }
        }
        if let Some(v) = map.get("user_ai_cooldown_seconds") {
            if let Some(n) = v.as_i64() {
                config.user_ai_cooldown_seconds = n as i32;
            }
        }
        if let Some(v) = map.get("guest_ai_daily_calls") {
            if let Some(n) = v.as_i64() {
                config.guest_ai_daily_calls = n as i32;
            }
        }
        if let Some(v) = map.get("guest_ai_daily_tokens") {
            if let Some(n) = v.as_i64() {
                config.guest_ai_daily_tokens = n as i32;
            }
        }
        if let Some(v) = map.get("guest_ai_cooldown_seconds") {
            if let Some(n) = v.as_i64() {
                config.guest_ai_cooldown_seconds = n as i32;
            }
        }

        // 沙箱生命周期配额
        if let Some(v) = map.get("stash_hidden_capacity") {
            if let Some(n) = v.as_i64() {
                config.stash_hidden_capacity = n as i32;
            }
        }
        if let Some(v) = map.get("stash_hidden_idle_seconds") {
            if let Some(n) = v.as_i64() {
                config.stash_hidden_idle_seconds = n as i32;
            }
        }
        if let Some(v) = map.get("resident_quota_per_app") {
            if let Some(n) = v.as_i64() {
                config.resident_quota_per_app = n as i32;
            }
        }
        if let Some(v) = map.get("resident_quota_site_total") {
            if let Some(n) = v.as_i64() {
                config.resident_quota_site_total = n as i32;
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

#[cfg(test)]
mod tests {
    use super::ConfigService;
    use crate::config::DynamicConfig;
    use serde_json::json;
    use std::collections::{HashMap, HashSet};
    use syn::visit::Visit;

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
                "api_key": "sk-work",
                "base_url": "https://api.openai.com/v1"
            }]),
        )]));
        assert_eq!(vendors.ai_vendor_sources.len(), 1);
        assert_eq!(vendors.ai_vendor_sources[0].slug, "openai-work");
        assert_eq!(
            vendors.ai_vendor_sources[0].api_key.as_deref(),
            Some("sk-work")
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
        let exemptions: HashSet<&str> = EXEMPTIONS.iter().map(|(field, _)| *field).collect();
        assert!(EXEMPTIONS
            .iter()
            .all(|(_, reason)| !reason.trim().is_empty()));

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
