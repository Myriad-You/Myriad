//! Persist admin config to the database and deploy-key `.env` writes.
use axum::{extract::State, Json};
use sea_orm::DatabaseConnection;
use serde_json::{json, Value};

use super::build::reconcile_platform_auto_refresh;
use super::secrets::{
    insert_platform_field, insert_sanitized_clearable_url, is_masked_secret_value,
    normalize_music_playlist_id, sanitize_google_site_verification, sanitize_http_base_url,
    sanitize_proxy_url, sanitize_site_favicon_url, sanitize_site_og_image_url,
    sanitize_umami_script_url, sanitize_wallpaper_url,
};
use super::types::ConfigResponse;

pub async fn update_config(
    State(db): State<DatabaseConnection>,
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
    Json(payload): Json<ConfigResponse>,
) -> Result<Json<Value>, crate::error::HttpError> {
    use crate::error::HttpError;
    use axum::http::StatusCode;

    tracing::info!("Updating configuration");

    // 1. 保存到数据库
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    if let Err(e) = save_to_database(&config_service, &payload).await {
        tracing::error!("Failed to save configuration to database: {}", e);
        return Err(HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "error": "Failed to save configuration",
                "code": "config_save_failed",
                "message": "Failed to save configuration"
            })),
        )));
    }
    tracing::info!("✅ Configuration saved to database");

    // 2. Sync deploy keys to .env (BASE_URL / OAuth client / proxy). A/B/C stay DB-only.
    let body = match save_all_configs(&payload).await {
        Ok(_) => {
            tracing::info!("✅ Deploy env synced; app config groups A/B/C stay DB-only");
            json!({
                "success": true,
                "message": "Configuration saved successfully! Changes will be applied automatically within a few seconds."
            })
        }
        Err(e) => {
            tracing::error!("Failed to save configuration to .env: {}", e);
            return Err(HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({
                    "success": false,
                    "error": "Failed to save configuration",
                    "code": "config_save_failed",
                    "message": format!("Failed to save configuration: {e}")
                })),
            )));
        }
    };

    // 3. 更新全局动态配置缓存
    match config_service.load_config().await {
        Ok(new_config) => {
            // 内存节约档：立即收紧并发/缓存/Argon2；DB 池在下次建连/重启后生效
            crate::services::memory_profile::apply_from_saver_flag(new_config.memory_saver_enabled);
            *dynamic_config.write().await = new_config;
            tracing::info!("✅ Dynamic configuration cache updated");

            // 3.1 重载全局 HTTP 客户端（以应用新的代理配置）
            crate::services::http_client::reload_global_client().await;
            tracing::info!("✅ Global HTTP client reloaded with new proxy settings");

            // OAuth redirect URLs follow site origin / provider list; reload after any config save.
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
        return Err(HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "error": "Failed to update platform auto-refresh",
                "code": "platform_refresh_reconcile_failed",
                "message": "Configuration was saved, but platform auto-refresh could not be updated."
            })),
        )));
    }

    // 5. 触发配置重载标志(虽然数据库连接可能不变,但确保其他服务知道配置已更新)
    crate::api::system::CONFIG_RELOAD_REQUESTED.store(true, std::sync::atomic::Ordering::Relaxed);

    tracing::info!(
        "🔄 Configuration reload flag set - changes will be picked up within 2-3 seconds"
    );

    Ok(Json(body))
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

fn vendor_json_is_agora(value: &serde_json::Value) -> bool {
    let kind = value
        .get("kind")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let preset = value
        .get("preset")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("");
    let slug = value
        .get("slug")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_ascii_lowercase();
    kind.eq_ignore_ascii_case("agora")
        || preset.eq_ignore_ascii_case("agora")
        || slug == "agora"
        || slug.starts_with("agora-")
}

fn merge_vendor_source_secrets(incoming: serde_json::Value) -> serde_json::Value {
    let Ok(mut sources) =
        serde_json::from_value::<Vec<crate::config::AiVendorSource>>(incoming.clone())
    else {
        return incoming;
    };
    let existing = crate::GLOBAL_DYNAMIC_CONFIG
        .try_read()
        .map(|guard| guard.effective_vendor_sources())
        .unwrap_or_default();
    for source in &mut sources {
        let previous = existing.iter().find(|item| item.slug == source.slug);
        if source
            .api_key
            .as_deref()
            .is_some_and(is_masked_secret_value)
        {
            source.api_key = previous.and_then(|item| item.api_key.clone());
        }
        if source
            .secret_id
            .as_deref()
            .is_some_and(is_masked_secret_value)
        {
            source.secret_id = previous.and_then(|item| item.secret_id.clone());
        }
        if source
            .secret_key
            .as_deref()
            .is_some_and(is_masked_secret_value)
        {
            source.secret_key = previous.and_then(|item| item.secret_key.clone());
        }
    }
    serde_json::to_value(sources).unwrap_or(incoming)
}

pub(crate) fn collect_database_updates(
    config: &ConfigResponse,
) -> std::collections::HashMap<String, Value> {
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

    // Shared with secrets / platform_test — see `is_masked_secret_value`.
    let is_masked = is_masked_secret_value;

    // 保存平台配置（空 = null 清除；掩码 = 保留；明文 = 写入）
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
                    insert_platform_field(&mut updates, key, &field.value);
                }
            }
            "Bilibili" => {
                updates.insert(
                    "bilibili_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    if field.key == "uid" {
                        insert_platform_field(&mut updates, "bilibili_uid", &field.value);
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
                    insert_platform_field(&mut updates, key, &field.value);
                }
            }
            "YouTube" => {
                updates.insert(
                    "youtube_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    let key = match field.key.as_str() {
                        "api_key" => "youtube_api_key",
                        "channel_id" => "youtube_channel_id",
                        _ => continue,
                    };
                    insert_platform_field(&mut updates, key, &field.value);
                }
            }
            "Netease Music" => {
                updates.insert(
                    "netease_enabled".to_string(),
                    JsonValue::Bool(platform.enabled),
                );
                for field in &platform.config_fields {
                    if field.key == "user_id" {
                        insert_platform_field(&mut updates, "netease_user_id", &field.value);
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
                    insert_platform_field(&mut updates, key, &field.value);
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
                    insert_platform_field(&mut updates, key, &field.value);
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
                    insert_platform_field(&mut updates, key, &field.value);
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
                    insert_platform_field(&mut updates, key, &field.value);
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
                    insert_platform_field(&mut updates, key, &field.value);
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
                    insert_platform_field(&mut updates, key, &field.value);
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
            // AI Lite 模型配置
            "lite_enabled" => ("lite_enabled", JsonValue::Bool(field.value == "true")),
            "lite_provider" => ("lite_ai_provider", JsonValue::String(field.value.clone())),
            "lite_gemini_api_key" => (
                "lite_gemini_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "lite_gemini_model" => ("lite_gemini_model", JsonValue::String(field.value.clone())),
            "lite_openai_api_key" => (
                "lite_openai_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "lite_openai_model" => ("lite_openai_model", JsonValue::String(field.value.clone())),
            "lite_openai_base_url" => (
                "lite_openai_base_url",
                JsonValue::String(field.value.clone()),
            ),
            // AI 图片生成配置
            "ai_image_provider" => ("ai_image_provider", JsonValue::String(field.value.clone())),
            "ai_image_model" => ("ai_image_model", JsonValue::String(field.value.clone())),
            "ai_image_openai_api_key" => (
                "ai_image_openai_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "ai_image_openai_base_url" => (
                "ai_image_openai_base_url",
                JsonValue::String(field.value.clone()),
            ),
            "ai_image_openrouter_api_key" => (
                "ai_image_openrouter_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "ai_image_volcengine_api_key" => (
                "ai_image_volcengine_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "ai_image_volcengine_base_url" => (
                "ai_image_volcengine_base_url",
                JsonValue::String(field.value.clone()),
            ),
            // 腾讯云语音服务配置 (TTS/ASR)
            "tencent_secret_id" => ("tencent_secret_id", JsonValue::String(field.value.clone())),
            "tencent_secret_key" => ("tencent_secret_key", JsonValue::String(field.value.clone())),
            "tencent_region" => ("tencent_region", JsonValue::String(field.value.clone())),
            "speech_provider" => ("speech_provider", JsonValue::String(field.value.clone())),
            "speech_openai_api_key" => (
                "speech_openai_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "speech_openai_base_url" => (
                "speech_openai_base_url",
                JsonValue::String(field.value.clone()),
            ),
            "speech_openrouter_api_key" => (
                "speech_openrouter_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "provider_openai_api_key" => (
                "provider_openai_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "provider_openai_base_url" => (
                "provider_openai_base_url",
                JsonValue::String(field.value.clone()),
            ),
            "provider_openrouter_api_key" => (
                "provider_openrouter_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "provider_gemini_api_key" => (
                "provider_gemini_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "provider_tinyfish_api_key" => (
                "provider_tinyfish_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "provider_volcengine_api_key" => (
                "provider_volcengine_api_key",
                JsonValue::String(field.value.clone()),
            ),
            "provider_volcengine_base_url" => (
                "provider_volcengine_base_url",
                JsonValue::String(field.value.clone()),
            ),
            "speech_stt_model" => ("speech_stt_model", JsonValue::String(field.value.clone())),
            "speech_tts_model" => ("speech_tts_model", JsonValue::String(field.value.clone())),
            "speech_tts_voice" => ("speech_tts_voice", JsonValue::String(field.value.clone())),
            "ai_source" => ("ai_source", JsonValue::String(field.value.clone())),
            "lite_ai_source" => ("lite_ai_source", JsonValue::String(field.value.clone())),
            "pro_ai_source" => ("pro_ai_source", JsonValue::String(field.value.clone())),
            "ai_image_source" => ("ai_image_source", JsonValue::String(field.value.clone())),
            "speech_source" => ("speech_source", JsonValue::String(field.value.clone())),
            "agora_convo_enabled" => (
                "agora_convo_enabled",
                JsonValue::Bool(field.value == "true"),
            ),
            "agora_app_id" => ("agora_app_id", JsonValue::String(field.value.clone())),
            "agora_app_certificate" => (
                "agora_app_certificate",
                JsonValue::String(field.value.clone()),
            ),
            "agora_customer_id" => ("agora_customer_id", JsonValue::String(field.value.clone())),
            "agora_customer_secret" => (
                "agora_customer_secret",
                JsonValue::String(field.value.clone()),
            ),
            "agora_api_base" => ("agora_api_base", JsonValue::String(field.value.clone())),
            "ai_vendor_sources" => {
                let parsed = serde_json::from_str::<JsonValue>(&field.value)
                    .unwrap_or_else(|_| JsonValue::Array(Vec::new()));
                let parsed = merge_vendor_source_secrets(parsed);
                ("ai_vendor_sources", parsed)
            }
            _ => continue,
        };
        // 掩码：保持库里原值。空密钥：写成 null 清除（与平台凭证同一套）。
        let allow_empty = matches!(
            field.key.as_str(),
            "speech_provider"
                | "speech_stt_model"
                | "speech_tts_model"
                | "speech_tts_voice"
                | "provider_openai_base_url"
                | "provider_volcengine_base_url"
                | "ai_source"
                | "lite_ai_source"
                | "pro_ai_source"
                | "ai_image_source"
                | "speech_source"
                | "ai_vendor_sources"
                | "agora_convo_enabled"
                | "agora_app_id"
                | "agora_customer_id"
                | "agora_api_base"
        );
        if is_masked(&field.value) {
            continue;
        }
        if field.value.trim().is_empty() {
            if is_secret_config_field_key(&field.key) {
                updates.insert(key.to_string(), JsonValue::Null);
            } else if allow_empty {
                updates.insert(key.to_string(), json_value);
            }
            continue;
        }
        updates.insert(key.to_string(), json_value);
    }

    if updates.contains_key("ai_vendor_sources") {
        let has_agora = updates
            .get("ai_vendor_sources")
            .and_then(JsonValue::as_array)
            .is_some_and(|sources| sources.iter().any(vendor_json_is_agora));
        if !has_agora {
            updates.insert("agora_convo_enabled".to_string(), JsonValue::Bool(false));
            updates.insert("agora_app_id".to_string(), JsonValue::String(String::new()));
            updates.insert(
                "agora_app_certificate".to_string(),
                JsonValue::String(String::new()),
            );
            updates.insert(
                "agora_customer_id".to_string(),
                JsonValue::String(String::new()),
            );
            updates.insert(
                "agora_customer_secret".to_string(),
                JsonValue::String(String::new()),
            );
        }
    }

    // 保存独立 Tripo 3D 配置。密钥掩码必须保留库中原值。
    for field in &config.tripo_config.config_fields {
        let value = field.value.trim();
        match field.key.as_str() {
            "tripo_enabled" => {
                updates.insert(
                    "tripo_enabled".to_string(),
                    JsonValue::Bool(value == "true" || value == "1"),
                );
            }
            "tripo_api_key" => {
                if is_masked(value) {
                    // keep existing
                } else if value.is_empty() {
                    updates.insert("tripo_api_key".to_string(), JsonValue::Null);
                } else {
                    updates.insert(
                        "tripo_api_key".to_string(),
                        JsonValue::String(value.to_string()),
                    );
                }
            }
            "tripo_base_url" | "tripo_model" if !value.is_empty() => {
                updates.insert(field.key.clone(), JsonValue::String(value.to_string()));
            }
            "tripo_face_limit" => {
                if let Ok(parsed) = value.parse::<i64>() {
                    updates.insert(
                        field.key.clone(),
                        JsonValue::Number(parsed.clamp(50, 20_000).into()),
                    );
                }
            }
            "tripo_poll_interval_seconds" => {
                if let Ok(parsed) = value.parse::<i64>() {
                    updates.insert(
                        field.key.clone(),
                        JsonValue::Number(parsed.clamp(2, 60).into()),
                    );
                }
            }
            "tripo_task_timeout_seconds" => {
                if let Ok(parsed) = value.parse::<i64>() {
                    updates.insert(
                        field.key.clone(),
                        JsonValue::Number(parsed.clamp(60, 3_600).into()),
                    );
                }
            }
            "tripo_max_download_mb" => {
                if let Ok(parsed) = value.parse::<i64>() {
                    updates.insert(
                        field.key.clone(),
                        JsonValue::Number(parsed.clamp(1, 150).into()),
                    );
                }
            }
            _ => {}
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
            // 可清空非敏感串：空串也写库，否则「重置本页」会被下方 is_empty 守卫吞掉
            // 仅持久化策略允许的 URL（http(s)/同站路径）；非法值跳过以免写入危险 scheme/内网
            "wallpaper_url" => {
                match sanitize_wallpaper_url(&field.value) {
                    Some(safe) => {
                        updates.insert("ui_wallpaper_url".to_string(), JsonValue::String(safe));
                    }
                    None => {
                        tracing::warn!(
                            wallpaper_url = %field.value,
                            "Rejecting wallpaper_url that failed scheme/host policy"
                        );
                    }
                }
                continue;
            }
            // Soft URL policy (scheme/format only — private hosts allowed for self-host)
            "site_favicon" => {
                insert_sanitized_clearable_url(
                    &mut updates,
                    "site_favicon",
                    &field.value,
                    sanitize_site_favicon_url,
                );
                continue;
            }
            "site_og_image" => {
                insert_sanitized_clearable_url(
                    &mut updates,
                    "site_og_image",
                    &field.value,
                    sanitize_site_og_image_url,
                );
                continue;
            }
            "google_site_verification" => {
                insert_sanitized_clearable_url(
                    &mut updates,
                    "google_site_verification",
                    &field.value,
                    sanitize_google_site_verification,
                );
                continue;
            }
            "umami_script_url" => {
                insert_sanitized_clearable_url(
                    &mut updates,
                    "umami_script_url",
                    &field.value,
                    sanitize_umami_script_url,
                );
                continue;
            }
            "proxy_url" => {
                insert_sanitized_clearable_url(
                    &mut updates,
                    "proxy_url",
                    &field.value,
                    sanitize_proxy_url,
                );
                continue;
            }
            "gemini_base_url" | "github_api_base_url" => {
                insert_sanitized_clearable_url(
                    &mut updates,
                    &field.key,
                    &field.value,
                    sanitize_http_base_url,
                );
                continue;
            }
            "site_ai_intro" => {
                // Cap to match SEO AI generate path (~500 chars) so public
                // /llms.txt cannot be bloated via the config bag.
                let capped: String = field.value.chars().take(500).collect();
                updates.insert("site_ai_intro".to_string(), JsonValue::String(capped));
                continue;
            }
            "site_title" | "site_description" | "site_keywords" | "ga_measurement_id"
            | "umami_website_id" | "music_source" | "site_icp" | "site_gongan"
            | "cloud_sponsors" | "site_footer_custom" | "proxy_bypass" => {
                updates.insert(field.key.clone(), JsonValue::String(field.value.clone()));
                continue;
            }
            "site_visibility_policy" => {
                let pol = crate::api::seo_policy::normalize_visibility_policy(&field.value, false);
                updates.insert(
                    "site_visibility_policy".to_string(),
                    JsonValue::String(pol.to_string()),
                );
                // Keep legacy noindex bit in lockstep
                updates.insert(
                    "site_noindex".to_string(),
                    JsonValue::Bool(pol == "private"),
                );
                continue;
            }
            "site_noindex" => {
                // When `site_visibility_policy` is also in this payload, policy is
                // authoritative (and already wrote `site_noindex`). Ignore raw bit.
                let has_policy = config
                    .ui_config
                    .config_fields
                    .iter()
                    .any(|f| f.key == "site_visibility_policy");
                if has_policy {
                    continue;
                }
                let enabled = field.value == "true";
                updates.insert(field.key.clone(), JsonValue::Bool(enabled));
                // Legacy-only flip: keep visibility policy in lockstep.
                updates.insert(
                    "site_visibility_policy".to_string(),
                    JsonValue::String(if enabled {
                        "private".to_string()
                    } else {
                        "ai_full".to_string()
                    }),
                );
                continue;
            }
            "music_playlist_id" => {
                // Accept full NetEase/QQ playlist URLs from the config UI and store numeric id.
                updates.insert(
                    field.key.clone(),
                    JsonValue::String(normalize_music_playlist_id(&field.value)),
                );
                continue;
            }
            "wallpaper_blur" => {
                if let Ok(n) = field.value.parse::<i64>() {
                    ("ui_wallpaper_blur", JsonValue::Number(n.into()))
                } else {
                    continue;
                }
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
            "analytics_enabled" => {
                let enabled = field.value != "false" && field.value != "0";
                ("analytics_enabled", JsonValue::Bool(enabled))
            }
            "pwa_enabled" => {
                let enabled = field.value != "false" && field.value != "0";
                ("pwa_enabled", JsonValue::Bool(enabled))
            }
            // base_url 经独立域名 API 改写；bag 若带空串勿覆盖已生效域名
            "base_url" => {
                if field.value.trim().is_empty() {
                    continue;
                }
                ("base_url", JsonValue::String(field.value.clone()))
            }
            "music_enabled" => {
                let enabled = field.value == "true";
                ("music_enabled", JsonValue::Bool(enabled))
            }
            "island_show_greeting" => {
                let enabled = field.value != "false" && field.value != "0";
                ("island_show_greeting", JsonValue::Bool(enabled))
            }
            "island_show_weather" => {
                let enabled = field.value != "false" && field.value != "0";
                ("island_show_weather", JsonValue::Bool(enabled))
            }
            "island_show_quote" => {
                let enabled = field.value != "false" && field.value != "0";
                ("island_show_quote", JsonValue::Bool(enabled))
            }
            "island_show_music" => {
                let enabled = field.value != "false" && field.value != "0";
                ("island_show_music", JsonValue::Bool(enabled))
            }
            "island_show_tapp" => {
                let enabled = field.value != "false" && field.value != "0";
                ("island_show_tapp", JsonValue::Bool(enabled))
            }
            "proxy_enabled" => {
                let enabled = field.value == "true";
                ("proxy_enabled", JsonValue::Bool(enabled))
            }
            "memory_saver_enabled" => {
                let enabled = field.value == "true";
                ("memory_saver_enabled", JsonValue::Bool(enabled))
            }
            "merope_enabled" => {
                let enabled = field.value == "true";
                ("merope_enabled", JsonValue::Bool(enabled))
            }
            "merope_speech_enabled" => {
                let enabled = field.value == "true";
                ("merope_speech_enabled", JsonValue::Bool(enabled))
            }
            _ => continue,
        };
        // 忽略屏蔽值（前端返回的掩码）与空敏感字段，避免覆盖已保存的密钥
        // 非敏感字符串若需允许清空，应在上方 match 中 early-insert（见 proxy_* / site_*）
        if !field.value.is_empty() && !is_masked(&field.value) {
            updates.insert(key.to_string(), json_value);
        }
    }

    updates
}

/// Whether a config form field key holds a secret (must not write mask/empty to .env).
fn is_secret_config_field_key(field_key: &str) -> bool {
    let k = field_key.to_ascii_lowercase();
    k.contains("token")
        || k.contains("secret")
        || k.contains("api_key")
        || k.contains("npsso")
        || k.contains("password")
        || k.ends_with("_key")
        || k == "key"
}

/// Skip empty or masked secrets so save does not clobber real .env/DB values with ••••.
pub(crate) fn should_write_env_field(field_key: &str, value: &str) -> bool {
    if is_secret_config_field_key(field_key) {
        !value.trim().is_empty() && !is_masked_secret_value(value)
    } else {
        true
    }
}

/// Save deploy keys to `.env`. App config groups A/B/C stay DB-only.
///
/// Never dual-write UI / platform credentials / AI / Tripo / music / site bag
/// to `.env` or process env. This path writes only:
/// BASE_URL, PROXY_*, GEMINI_BASE_URL, GITHUB_API_BASE_URL
/// (plus infra already outside this path).
///
/// Kept in env:
/// - infra: DATABASE_URL, SERVER_*, JWT_SECRET, CORS_ORIGINS, FRONTEND_*, RUST_LOG
/// - site origin: BASE_URL (+ site-domain FRONTEND_URL / CORS adapt)
/// - outbound runtime: PROXY_*, GEMINI_BASE_URL, GITHUB_API_BASE_URL
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

    // Keys emptied on this save (commented `# KEY=`) — must remove_var after dotenv.
    let mut env_keys_to_clear: Vec<&'static str> = Vec::new();

    // Capture previous public origin before rewriting BASE_URL so CORS replace
    // can swap the old entry instead of treating the new value as previous.
    let previous_base_url = crate::api::site_domain::read_env_key(&env_content, "BASE_URL")
        .or_else(|| std::env::var("BASE_URL").ok().filter(|s| !s.is_empty()));

    // Only deploy / outbound keys still dual-write to .env:
    // BASE_URL, proxy, API base mirrors.
    let mut saved_base_url: Option<String> = None;
    for field in &config.ui_config.config_fields {
        let key = match field.key.as_str() {
            "base_url" => "BASE_URL",
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
        if !should_write_env_field(&field.key, &field.value) {
            continue;
        }
        let env_value = match field.key.as_str() {
            "proxy_url" => match sanitize_proxy_url(&field.value) {
                Some(safe) => safe,
                None => {
                    tracing::warn!("Skipping PROXY_URL env write: failed scheme/format policy");
                    continue;
                }
            },
            "gemini_base_url" | "github_api_base_url" => {
                match sanitize_http_base_url(&field.value) {
                    Some(safe) => safe,
                    None => {
                        tracing::warn!(
                            key = %field.key,
                            "Skipping API base URL env write: failed scheme/format policy"
                        );
                        continue;
                    }
                }
            }
            _ => field.value.clone(),
        };
        env_content = update_env_var(&env_content, key, &env_value)?;
        // Commented `# KEY=` lines do not unset process env after dotenv reload.
        if env_value.trim().is_empty() {
            env_keys_to_clear.push(key);
        }
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
    tracing::info!("✅ Configuration saved to .env file (deploy keys only)");

    // 重新加载环境变量
    if let Err(e) = dotenvy::from_path_override(env_path) {
        tracing::warn!("⚠️ Failed to reload .env file after saving config: {}", e);
    } else {
        tracing::info!("♻️ Environment variables reloaded after config save");
    }

    // Drop emptied deploy keys from process env (dotenv never unsets missing keys).
    for key in env_keys_to_clear {
        std::env::remove_var(key);
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
///
/// Rejects CR/LF/NUL so a value cannot inject extra `.env` entries. Callers
/// must treat `Err` as fail-closed — do not quote or space-replace the break.
pub fn update_env_var(content: &str, key: &str, value: &str) -> Result<String, String> {
    crate::api::setup_bootstrap::validate_env_value(key, value)?;

    let mut lines: Vec<String> = content.lines().map(|s| s.to_string()).collect();
    let key_prefix = format!("{}=", key);

    // 处理值：如果包含空格、特殊字符或中文，用引号包裹
    let sanitized_value = if value.is_empty() {
        String::new()
    } else if value.contains(' ') || value.contains('#') || value.chars().any(|c| c > '\u{007F}') {
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

    Ok(lines.join("\n") + "\n")
}
