//! Settings backup export / preview / restore and the configuration-key registry.
use axum::{extract::State, http::StatusCode, Json};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::build::{build_config, reconcile_platform_auto_refresh};
use super::extras::{HitokotoConfig, ReportSettings, HITOKOTO_CONFIG_KEY, REPORT_SETTINGS_KEY};
use super::save::collect_database_updates;
use super::types::ConfigResponse;
use myriad_module_visibility::{ModuleVisibilityPreferences, MODULE_VISIBILITY_PREFERENCES_KEY};

pub(crate) const SETTINGS_BACKUP_FORMAT: &str = "myriad-settings-backup";
pub(crate) const SETTINGS_BACKUP_VERSION: u32 = 2;
pub(crate) const MIN_SETTINGS_BACKUP_VERSION: u32 = 1;
pub(crate) const MAX_SETTINGS_BACKUP_ENTRIES: usize = 10_000;

pub(crate) fn default_setting_schema_version() -> u32 {
    1
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct SettingDescriptor {
    schema_version: u32,
    introduced_in_backup_version: u32,
}

// 这是配置备份唯一的后端注册表。新增或删除非 ConfigResponse 设置时只需要改这里；
// 恢复、预检和导出过滤全部从该注册表派生。
/// 备份/恢复 registry：含仍在用的键。已下线的 `pet_*` / `ui_wallpaper_parallax` /
/// `github_client_*` / `github_redirect_url` 不在表里——旧备份里这些键会进 ignored。
pub(crate) const REGISTERED_CONFIGURATION_KEYS_V1: &[&str] = &[
    "merope_enabled",
    "merope_speech_enabled",
    "agent_rig_asset_id",
    "ai_image_model",
    "ai_image_openai_api_key",
    "ai_image_openai_base_url",
    "ai_image_openrouter_api_key",
    "ai_image_provider",
    "ai_image_volcengine_api_key",
    "ai_image_source",
    "ai_image_volcengine_base_url",
    "ai_provider",
    "ai_source",
    "ai_vendor_sources",
    "lite_ai_source",
    "pro_ai_source",
    "speech_source",
    "agora_api_base",
    "agora_app_certificate",
    "agora_app_id",
    "agora_convo_enabled",
    "agora_customer_id",
    "agora_customer_secret",
    "allow_local_registration",
    "analytics_enabled",
    "tapp_private_install_cleanup",
    "tapp_private_install_inactivity_days",
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
    "dashboard_layout_mode",
    "dashboard_title",
    "discord_access_token",
    "discord_enabled",
    "discord_refresh_token",
    "discord_token_expires_at",
    "discord_user_id",
    "enable_auto_fetch",
    "fetch_interval_hours",
    "ga_measurement_id",
    "gemini_api_key",
    "gemini_base_url",
    "gemini_model",
    "github_api_base_url",
    "github_enabled",
    "github_token",
    "github_username",
    "guest_ai_cooldown_seconds",
    "guest_ai_daily_calls",
    "guest_ai_daily_tokens",
    "guest_perm_ai_analyze",
    "guest_perm_ai_chat",
    "guest_perm_ai_generate",
    "guest_perm_ai_image",
    "guest_perm_ai_search",
    "guest_perm_3d_generate",
    "guest_perm_component_theme",
    "guest_perm_event_publish",
    "guest_perm_media_control",
    "guest_perm_network_fetch",
    "lite_ai_provider",
    "lite_enabled",
    "lite_gemini_api_key",
    "lite_gemini_model",
    "lite_openai_api_key",
    "lite_openai_base_url",
    "lite_openai_model",
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
    "platform_order",
    "provider_gemini_api_key",
    "provider_openai_api_key",
    "provider_openai_base_url",
    "provider_openrouter_api_key",
    "provider_tinyfish_api_key",
    "provider_volcengine_api_key",
    "provider_volcengine_base_url",
    "pro_ai_provider",
    "pro_enabled",
    "pro_gemini_api_key",
    "pro_gemini_model",
    "pro_openai_api_key",
    "pro_openai_base_url",
    "pro_openai_model",
    "memory_saver_enabled",
    "proxy_bypass",
    "proxy_enabled",
    "proxy_url",
    "psn_enabled",
    "psn_npsso",
    "psn_online_id",
    "pwa_enabled",
    "qq_bot_app_id",
    "qq_bot_app_secret",
    "qq_bot_enabled",
    "report_settings",
    "site_description",
    "site_favicon",
    "site_footer_custom",
    "site_gongan",
    "site_icp",
    "site_ai_intro",
    "google_site_verification",
    "site_keywords",
    "site_noindex",
    "site_og_image",
    "site_title",
    "site_visibility_policy",
    "speech_openai_api_key",
    "speech_openai_base_url",
    "speech_openrouter_api_key",
    "speech_provider",
    "speech_stt_model",
    "speech_tts_model",
    "speech_tts_voice",
    "steam_api_key",
    "steam_enabled",
    "steam_id",
    "youtube_api_key",
    "youtube_channel_id",
    "youtube_enabled",
    "tapp_window_schemes",
    "tencent_region",
    "tencent_secret_id",
    "tencent_secret_key",
    "title_color",
    "title_font",
    "title_font_size",
    "topic_style",
    "tripo_api_key",
    "tripo_base_url",
    "tripo_enabled",
    "tripo_face_limit",
    "tripo_max_download_mb",
    "tripo_model",
    "tripo_poll_interval_seconds",
    "tripo_task_timeout_seconds",
    "ui_evocative_dynamic_blur",
    "ui_evocative_fps",
    "ui_evocative_parallax",
    "ui_evocative_ripple",
    "ui_evocative_ripple_quality",
    "ui_primary_color",
    "ui_secondary_color",
    "ui_theme",
    "ui_wallpaper_blur",
    "ui_wallpaper_url",
    "umami_script_url",
    "umami_website_id",
    "user_ai_cooldown_seconds",
    "user_ai_daily_calls",
    "user_ai_daily_tokens",
    "user_perm_ai_analyze",
    "user_perm_ai_chat",
    "user_perm_ai_generate",
    "user_perm_ai_image",
    "user_perm_ai_search",
    "user_perm_3d_generate",
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

pub(crate) fn settings_registry() -> std::collections::HashMap<&'static str, SettingDescriptor> {
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

pub(crate) struct SettingsRestorePlan {
    entries: Vec<SettingsBackupEntry>,
    preview: SettingsRestorePreview,
}

pub(crate) fn validate_settings_backup(backup: &SettingsBackup) -> Result<(), String> {
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

/// 敏感 key 判定。
///
/// 单一定义放在 `data_key`，这样"标记为已加密"和"实际加密"永远同源 ——
/// 修复前这个列只是个标签，值仍然明文落库。
fn is_sensitive_configuration_key(key: &str) -> bool {
    crate::services::data_key::is_sensitive_config_key(key)
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

pub(crate) fn build_settings_restore_plan(backup: &SettingsBackup) -> SettingsRestorePlan {
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

// merged from settings.rs

pub async fn export_settings(
    State(db): State<DatabaseConnection>,
    user_id: i32,
) -> (StatusCode, Json<Value>) {
    let registry = settings_registry();
    let effective_config = build_config(&db, true).await;
    let rows = match db
        .query_all_raw(Statement::from_string(
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
                Json(json!({"error": "Failed to read settings", "code": "settings_backup_failed"})),
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
                    Json(
                        json!({"error": "Failed to decode settings", "code": "settings_backup_failed"}),
                    ),
                );
            }
        };
        let Some(descriptor) = registry.get(key.as_str()) else {
            // Stale rows from removed settings are intentionally not re-exported.
            continue;
        };
        let entry = SettingsBackupEntry {
            value: match row.try_get("", "value") {
                // 备份**明文**导出：这样导出的文件可以恢复到任意新实例，不必
                // 同时带上密钥文件。这是有意的取舍 —— 导出是管理员主动执行的
                // 认证操作（双重 admin 校验），同一个管理员在设置页本来就能看到
                // 这些值；而加密要挡的是数据库副本泄露，那条路径上库里仍是密文。
                Ok(value) => crate::services::data_key::open_config_value(&key, value),
                Err(error) => {
                    tracing::error!("Failed to decode configuration value: {}", error);
                    return (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(
                            json!({"error": "Failed to decode settings", "code": "settings_backup_failed"}),
                        ),
                    );
                }
            },
            key,
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
    State(dynamic_config): State<std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>>,
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
                Json(
                    json!({"error": "Failed to start settings restore", "code": "settings_backup_failed"}),
                ),
            );
        }
    };

    let restore_result: Result<(), sea_orm::DbErr> = async {
        for entry in entries {
            transaction
                .execute_raw(Statement::from_sql_and_values(
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
                        entry.key.clone().into(),
                        // 备份里是明文（见 export_settings），落库前重新加密。
                        crate::services::data_key::seal_config_value(&entry.key, entry.value)
                            .into(),
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
            .execute_raw(Statement::from_sql_and_values(
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
            Json(json!({"error": "Failed to restore settings", "code": "settings_backup_failed"})),
        );
    }

    crate::services::agent::notification_preferences::cache_restored(
        user_id,
        notification_preferences,
    )
    .await;

    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    match config_service.load_config().await {
        Ok(new_config) => {
            // Same Arc as AppState.dynamic_config after from_shared — write via State.
            *dynamic_config.write().await = new_config;
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
    use crate::api::config::{
        collect_database_updates, db_or_env_clearable, is_masked_secret_value,
        normalize_music_playlist_id, platform_configured_flags, public_ui_config_value,
        sanitize_google_site_verification, sanitize_http_base_url, sanitize_proxy_url,
        sanitize_site_favicon_url, sanitize_site_og_image_url, sanitize_umami_script_url,
        sanitize_wallpaper_url, should_write_env_field, update_env_var, AiConfig, ConfigField,
        ConfigResponse, PlatformAutoFetchConfig, PlatformConfig, MODULE_VISIBILITY_PREFERENCES_KEY,
    };

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
    fn ai_config_bag_drops_legacy_typed_mirrors() {
        let raw = json!({
            "provider": "gemini",
            "model": "gemini-x",
            "api_key": "secret",
            "enabled": true,
            "image_provider": "openrouter",
            "config_fields": [{
                "key": "provider",
                "label": "AI Provider",
                "field_type": "select",
                "value": "openai",
                "placeholder": "",
                "required": true
            }]
        });
        let parsed: AiConfig = serde_json::from_value(raw).expect("legacy payload");
        assert_eq!(parsed.config_fields.len(), 1);
        let out = serde_json::to_value(&parsed).expect("serialize");
        assert!(out.get("provider").is_none());
        assert!(out.get("model").is_none());
        assert!(out.get("api_key").is_none());
        assert!(out.get("enabled").is_none());
        assert!(out.get("image_provider").is_none());
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
    fn restore_plan_ignores_retired_pet_parallax_and_flat_github_oauth_keys() {
        let plan = build_settings_restore_plan(&backup_with_entries(vec![
            entry("pet_enabled"),
            entry("pet_image_url"),
            entry("ui_wallpaper_parallax"),
            entry("github_client_id"),
            entry("github_client_secret"),
            entry("github_redirect_url"),
            entry("site_title"),
        ]));
        for key in [
            "pet_enabled",
            "pet_image_url",
            "ui_wallpaper_parallax",
            "github_client_id",
            "github_client_secret",
            "github_redirect_url",
        ] {
            assert!(
                plan.preview.ignored_keys.iter().any(|k| k == key),
                "{key} should be ignored"
            );
        }
        assert!(plan.entries.iter().any(|e| e.key == "site_title"));
        assert!(!plan.entries.iter().any(|e| e.key.starts_with("pet_")));
        assert!(!plan
            .entries
            .iter()
            .any(|e| e.key.starts_with("github_client")));
    }

    #[test]
    fn platform_configured_flags_follow_explicit_enabled_and_credentials() {
        let mut config = crate::config::DynamicConfig::default();
        assert!(platform_configured_flags(&config)
            .iter()
            .all(|(_, on)| !*on));

        config.steam_api_key = Some("k".into());
        let map: std::collections::HashMap<_, _> =
            platform_configured_flags(&config).into_iter().collect();
        assert_eq!(map.get("steam"), Some(&true));

        config.steam_enabled = Some(false);
        let map: std::collections::HashMap<_, _> =
            platform_configured_flags(&config).into_iter().collect();
        assert_eq!(map.get("steam"), Some(&false));
    }

    #[test]
    fn public_ui_config_value_exposes_display_fields_without_secrets() {
        let mut config = crate::config::DynamicConfig::default();
        config.analytics_enabled = true;
        config.pwa_enabled = false;
        config.ui_wallpaper_url = Some("https://example.test/w.jpg".into());
        config.site_title = Some("Myriad".into());
        config.music_enabled = Some("true".into());

        let ui = public_ui_config_value(&config);
        assert_eq!(ui["analytics_enabled"], json!(true));
        assert_eq!(ui["pwa_enabled"], json!(false));
        assert_eq!(ui["wallpaper_url"], json!("https://example.test/w.jpg"));
        assert_eq!(ui["site_title"], json!("Myriad"));
        assert_eq!(ui["music_enabled"], json!("true"));
        assert!(ui.get("github_client_secret").is_none());
        assert!(ui.get("openai_api_key").is_none());
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

    fn ui_field(key: &str, value: &str) -> ConfigField {
        ConfigField {
            key: key.to_string(),
            label: String::new(),
            field_type: "text".to_string(),
            value: value.to_string(),
            placeholder: String::new(),
            required: false,
        }
    }

    #[test]
    fn saves_qq_bot_write_only_credentials() {
        let mut config = empty_config();
        config.ai_config.config_fields = vec![
            ui_field("qq_bot_enabled", "true"),
            ui_field("qq_bot_app_id", "102123456"),
            ui_field("qq_bot_app_secret", "qq-secret-value"),
        ];
        let set = collect_database_updates(&config);
        assert_eq!(set.get("qq_bot_enabled"), Some(&json!(true)));
        assert_eq!(set.get("qq_bot_app_id"), Some(&json!("102123456")));
        assert_eq!(set.get("qq_bot_app_secret"), Some(&json!("qq-secret-value")));

        config.ai_config.config_fields = vec![
            ui_field("qq_bot_enabled", "true"),
            ui_field("qq_bot_app_id", "102123456"),
            ui_field("qq_bot_app_secret", "••••••••"),
        ];
        let masked = collect_database_updates(&config);
        assert_eq!(masked.get("qq_bot_enabled"), Some(&json!(true)));
        assert_eq!(masked.get("qq_bot_app_id"), Some(&json!("102123456")));
        assert!(
            !masked.contains_key("qq_bot_app_secret"),
            "mask must keep the stored secret"
        );

        config.ai_config.config_fields = vec![
            ui_field("qq_bot_enabled", "false"),
            ui_field("qq_bot_app_id", ""),
            ui_field("qq_bot_app_secret", ""),
        ];
        let cleared = collect_database_updates(&config);
        assert_eq!(cleared.get("qq_bot_enabled"), Some(&json!(false)));
        assert_eq!(cleared.get("qq_bot_app_id"), Some(&json!("")));
        assert_eq!(cleared.get("qq_bot_app_secret"), Some(&Value::Null));
    }

    #[test]
    fn saves_agora_realtime_talk_fields() {
        let mut config = empty_config();
        config.ai_config.config_fields = vec![
            ui_field("agora_convo_enabled", "true"),
            ui_field("agora_app_id", "970ca35de60c44645bbae8a215061b33"),
            ui_field("agora_api_base", "https://api.agora.io/cn"),
        ];
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("agora_convo_enabled"), Some(&json!(true)));
        assert_eq!(
            updates.get("agora_app_id"),
            Some(&json!("970ca35de60c44645bbae8a215061b33"))
        );
        assert_eq!(
            updates.get("agora_api_base"),
            Some(&json!("https://api.agora.io/cn"))
        );
    }

    #[test]
    fn saving_vendors_without_agora_clears_legacy_realtime_talk() {
        let mut config = empty_config();
        config.ai_config.config_fields = vec![ui_field(
            "ai_vendor_sources",
            r#"[{"slug":"openai","kind":"openai","display_name":"OpenAI","enabled":true}]"#,
        )];
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("agora_convo_enabled"), Some(&json!(false)));
        assert_eq!(updates.get("agora_app_id"), Some(&json!("")));
        assert_eq!(updates.get("agora_app_certificate"), Some(&json!("")));
        assert_eq!(updates.get("agora_customer_id"), Some(&json!("")));
        assert_eq!(updates.get("agora_customer_secret"), Some(&json!("")));
    }

    #[test]
    fn tripo_config_is_independent_clamped_and_keeps_masked_key() {
        let mut config = empty_config();
        config.tripo_config.config_fields = vec![
            ui_field("tripo_enabled", "true"),
            ui_field("tripo_api_key", "••••••••"),
            ui_field("tripo_model", "P1-20260311"),
            ui_field("tripo_face_limit", "99999"),
            ui_field("tripo_poll_interval_seconds", "1"),
            ui_field("tripo_task_timeout_seconds", "99999"),
            ui_field("tripo_max_download_mb", "999"),
        ];

        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("tripo_enabled"), Some(&json!(true)));
        assert!(!updates.contains_key("tripo_api_key"));
        assert_eq!(updates.get("tripo_model"), Some(&json!("P1-20260311")));
        assert_eq!(updates.get("tripo_face_limit"), Some(&json!(20_000)));
        assert_eq!(updates.get("tripo_poll_interval_seconds"), Some(&json!(2)));
        assert_eq!(
            updates.get("tripo_task_timeout_seconds"),
            Some(&json!(3_600))
        );
        assert_eq!(updates.get("tripo_max_download_mb"), Some(&json!(150)));
    }

    #[test]
    fn ui_network_proxy_and_mirror_fields_can_be_cleared() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![
            ui_field("proxy_url", ""),
            ui_field("proxy_bypass", ""),
            ui_field("gemini_base_url", ""),
            ui_field("github_api_base_url", ""),
            ui_field("proxy_enabled", "false"),
        ];

        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("proxy_url"), Some(&json!("")));
        assert_eq!(updates.get("proxy_bypass"), Some(&json!("")));
        assert_eq!(updates.get("gemini_base_url"), Some(&json!("")));
        assert_eq!(updates.get("github_api_base_url"), Some(&json!("")));
        assert_eq!(updates.get("proxy_enabled"), Some(&json!(false)));
    }

    #[test]
    fn ui_memory_saver_flag_persists_bool() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![ui_field("memory_saver_enabled", "true")];
        let on = collect_database_updates(&config);
        assert_eq!(on.get("memory_saver_enabled"), Some(&json!(true)));

        config.ui_config.config_fields = vec![ui_field("memory_saver_enabled", "false")];
        let off = collect_database_updates(&config);
        assert_eq!(off.get("memory_saver_enabled"), Some(&json!(false)));
    }

    #[test]
    fn ui_merope_flag_persists_bool() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![ui_field("merope_enabled", "true")];
        let on = collect_database_updates(&config);
        assert_eq!(on.get("merope_enabled"), Some(&json!(true)));

        config.ui_config.config_fields = vec![ui_field("merope_enabled", "false")];
        let off = collect_database_updates(&config);
        assert_eq!(off.get("merope_enabled"), Some(&json!(false)));
    }

    #[test]
    fn ui_merope_speech_flag_persists_bool() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![ui_field("merope_speech_enabled", "true")];
        let on = collect_database_updates(&config);
        assert_eq!(on.get("merope_speech_enabled"), Some(&json!(true)));

        config.ui_config.config_fields = vec![ui_field("merope_speech_enabled", "false")];
        let off = collect_database_updates(&config);
        assert_eq!(off.get("merope_speech_enabled"), Some(&json!(false)));
    }

    #[test]
    fn ui_network_proxy_and_mirror_fields_persist_non_empty() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![
            ui_field("proxy_url", "http://127.0.0.1:7890"),
            ui_field("proxy_bypass", "localhost,127.0.0.1"),
            ui_field("gemini_base_url", "https://gemini.example.com"),
            ui_field("github_api_base_url", "https://gh.example.com"),
        ];

        let updates = collect_database_updates(&config);
        assert_eq!(
            updates.get("proxy_url"),
            Some(&json!("http://127.0.0.1:7890"))
        );
        assert_eq!(
            updates.get("proxy_bypass"),
            Some(&json!("localhost,127.0.0.1"))
        );
        // Bare origins: strip Url's forced trailing `/` (joiners use `{base}/v1/...`)
        assert_eq!(
            updates.get("gemini_base_url"),
            Some(&json!("https://gemini.example.com"))
        );
        assert_eq!(
            updates.get("github_api_base_url"),
            Some(&json!("https://gh.example.com"))
        );
    }

    #[test]
    fn ui_bag_ignores_retired_typed_mirrors() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![
            ui_field("github_client_secret", "real-secret"),
            ui_field("github_client_id", "client"),
            ui_field("pet_enabled", "true"),
            ui_field("pet_image_url", "https://example.com/pet.png"),
            ui_field("wallpaper_parallax", "true"),
        ];
        let updates = collect_database_updates(&config);
        assert!(!updates.contains_key("github_client_secret"));
        assert!(!updates.contains_key("github_client_id"));
        assert!(!updates.contains_key("pet_enabled"));
        assert!(!updates.contains_key("pet_image_url"));
        assert!(!updates.contains_key("ui_wallpaper_parallax"));
    }

    #[test]
    fn ui_clearable_fields_persist_empty_strings() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![
            ui_field("site_title", ""),
            ui_field("site_description", ""),
            ui_field("site_favicon", ""),
            ui_field("site_keywords", ""),
            ui_field("site_og_image", ""),
            ui_field("google_site_verification", ""),
            ui_field("site_noindex", "false"),
            ui_field("site_visibility_policy", "ai_full"),
            ui_field("site_ai_intro", ""),
            ui_field("ga_measurement_id", ""),
            ui_field("umami_website_id", ""),
            ui_field("umami_script_url", ""),
            ui_field("wallpaper_url", ""),
            ui_field("music_playlist_id", ""),
            ui_field("site_icp", ""),
            ui_field("site_footer_custom", ""),
        ];
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("site_title"), Some(&json!("")));
        assert_eq!(updates.get("site_description"), Some(&json!("")));
        assert_eq!(updates.get("site_favicon"), Some(&json!("")));
        assert_eq!(updates.get("site_keywords"), Some(&json!("")));
        assert_eq!(updates.get("site_og_image"), Some(&json!("")));
        assert_eq!(updates.get("google_site_verification"), Some(&json!("")));
        assert_eq!(updates.get("site_noindex"), Some(&json!(false)));
        // Both fields present: policy is authoritative; raw noindex alone must not
        // rewrite policy away from the explicit site_visibility_policy value.
        assert_eq!(
            updates.get("site_visibility_policy"),
            Some(&json!("ai_full"))
        );
        assert_eq!(updates.get("ga_measurement_id"), Some(&json!("")));
        assert_eq!(updates.get("umami_website_id"), Some(&json!("")));
        assert_eq!(updates.get("umami_script_url"), Some(&json!("")));
        assert_eq!(updates.get("ui_wallpaper_url"), Some(&json!("")));
        assert_eq!(updates.get("music_playlist_id"), Some(&json!("")));
        assert_eq!(updates.get("site_icp"), Some(&json!("")));
        assert_eq!(updates.get("site_footer_custom"), Some(&json!("")));
    }

    #[test]
    fn site_noindex_alone_syncs_visibility_policy() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![ui_field("site_noindex", "true")];
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("site_noindex"), Some(&json!(true)));
        assert_eq!(
            updates.get("site_visibility_policy"),
            Some(&json!("private"))
        );

        config.ui_config.config_fields = vec![ui_field("site_noindex", "false")];
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("site_noindex"), Some(&json!(false)));
        assert_eq!(
            updates.get("site_visibility_policy"),
            Some(&json!("ai_full"))
        );
    }

    #[test]
    fn site_visibility_policy_overrides_raw_noindex_in_same_payload() {
        let mut config = empty_config();
        // Conflicting legacy bit + explicit policy: policy wins, noindex follows policy.
        config.ui_config.config_fields = vec![
            ui_field("site_noindex", "true"),
            ui_field("site_visibility_policy", "ai_full"),
        ];
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("site_noindex"), Some(&json!(false)));
        assert_eq!(
            updates.get("site_visibility_policy"),
            Some(&json!("ai_full"))
        );
    }

    #[test]
    fn sanitize_wallpaper_url_allows_http_https_and_paths() {
        assert_eq!(sanitize_wallpaper_url(""), Some(String::new()));
        assert_eq!(
            sanitize_wallpaper_url("https://images.unsplash.com/photo-1"),
            Some("https://images.unsplash.com/photo-1".to_string())
        );
        assert_eq!(
            sanitize_wallpaper_url("/uploads/wall.jpg"),
            Some("/uploads/wall.jpg".to_string())
        );
        assert_eq!(
            sanitize_wallpaper_url("//cdn.example.com/a.jpg"),
            Some("https://cdn.example.com/a.jpg".to_string())
        );
    }

    #[test]
    fn sanitize_wallpaper_url_rejects_schemes_and_private_hosts() {
        assert_eq!(sanitize_wallpaper_url("javascript:alert(1)"), None);
        assert_eq!(sanitize_wallpaper_url("data:image/png;base64,aaa"), None);
        assert_eq!(sanitize_wallpaper_url("http://127.0.0.1/a.jpg"), None);
        assert_eq!(sanitize_wallpaper_url("http://192.168.1.1/a.jpg"), None);
        assert_eq!(sanitize_wallpaper_url("http://localhost/a.jpg"), None);
        assert_eq!(
            sanitize_wallpaper_url("https://user:pass@cdn.example.com/a.jpg"),
            None
        );
    }

    #[test]
    fn collect_rejects_unsafe_wallpaper_url() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![ui_field("wallpaper_url", "javascript:alert(1)")];
        let updates = collect_database_updates(&config);
        assert!(!updates.contains_key("ui_wallpaper_url"));

        config.ui_config.config_fields =
            vec![ui_field("wallpaper_url", "https://cdn.example.com/w.jpg")];
        let updates = collect_database_updates(&config);
        assert_eq!(
            updates.get("ui_wallpaper_url"),
            Some(&json!("https://cdn.example.com/w.jpg"))
        );
    }

    #[test]
    fn soft_url_fields_allow_normal_self_host_usage() {
        // Favicon: path, public, LAN, data:image
        assert_eq!(
            sanitize_site_favicon_url("/favicon.webp"),
            Some("/favicon.webp".to_string())
        );
        assert_eq!(
            sanitize_site_favicon_url("https://cdn.example.com/icon.png"),
            Some("https://cdn.example.com/icon.png".to_string())
        );
        assert_eq!(
            sanitize_site_favicon_url("http://192.168.1.5/logo.png"),
            Some("http://192.168.1.5/logo.png".to_string())
        );
        assert!(sanitize_site_favicon_url("data:image/png;base64,aaa")
            .unwrap()
            .starts_with("data:image/png"));
        assert_eq!(sanitize_site_favicon_url("javascript:alert(1)"), None);
        assert_eq!(sanitize_site_favicon_url("data:text/html,x"), None);

        // OG: no data:
        assert_eq!(
            sanitize_site_og_image_url("/og.png"),
            Some("/og.png".to_string())
        );
        assert_eq!(sanitize_site_og_image_url("data:image/png;base64,x"), None);

        assert_eq!(
            sanitize_google_site_verification("AbC-_123"),
            Some("AbC-_123".to_string())
        );
        assert_eq!(
            sanitize_google_site_verification(
                r#"<meta name="google-site-verification" content="Tok_en-1" />"#
            ),
            Some("Tok_en-1".to_string())
        );
        assert_eq!(sanitize_google_site_verification(""), Some(String::new()));
        assert_eq!(sanitize_google_site_verification("<script>"), None);

        // Umami + API base: http(s), private OK
        assert_eq!(
            sanitize_umami_script_url("http://10.0.0.2:3000/script.js"),
            Some("http://10.0.0.2:3000/script.js".to_string())
        );
        assert_eq!(sanitize_umami_script_url("javascript:x"), None);
        assert_eq!(
            sanitize_http_base_url("http://127.0.0.1:11434/v1"),
            Some("http://127.0.0.1:11434/v1".to_string())
        );
        // Bare origin must not keep the slash that Url::to_string() adds.
        assert_eq!(
            sanitize_http_base_url("https://gemini.example.com"),
            Some("https://gemini.example.com".to_string())
        );
        assert_eq!(
            sanitize_http_base_url("https://gemini.example.com/"),
            Some("https://gemini.example.com".to_string())
        );

        // Proxy: socks + localhost OK
        assert_eq!(
            sanitize_proxy_url("http://127.0.0.1:7890"),
            Some("http://127.0.0.1:7890".to_string())
        );
        assert_eq!(
            sanitize_proxy_url("socks5://127.0.0.1:1080"),
            Some("socks5://127.0.0.1:1080".to_string())
        );
        assert_eq!(sanitize_proxy_url("javascript:x"), None);
    }

    #[test]
    fn collect_soft_url_fields_reject_only_dangerous_schemes() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![
            ui_field("site_favicon", "javascript:x"),
            ui_field("proxy_url", "http://127.0.0.1:7890"),
            ui_field("umami_script_url", "https://cloud.umami.is/script.js"),
        ];
        let updates = collect_database_updates(&config);
        assert!(!updates.contains_key("site_favicon"));
        assert_eq!(
            updates.get("proxy_url"),
            Some(&json!("http://127.0.0.1:7890"))
        );
        assert_eq!(
            updates.get("umami_script_url"),
            Some(&json!("https://cloud.umami.is/script.js"))
        );
    }

    #[test]
    fn google_site_verification_persists_extracted_token() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![ui_field(
            "google_site_verification",
            r#"<meta name="google-site-verification" content="Tok_en-1" />"#,
        )];
        let updates = collect_database_updates(&config);
        assert_eq!(
            updates.get("google_site_verification"),
            Some(&json!("Tok_en-1"))
        );

        config.ui_config.config_fields = vec![ui_field(
            "google_site_verification",
            "<script>alert(1)</script>",
        )];
        let updates = collect_database_updates(&config);
        assert!(!updates.contains_key("google_site_verification"));
    }

    #[test]
    fn clearable_db_empty_wins_over_env_fallback() {
        // UI clear writes Some(""); that must not be treated as "missing → env".
        assert_eq!(
            db_or_env_clearable(Some(String::new()), "GA_MEASUREMENT_ID", ""),
            ""
        );
        assert_eq!(
            db_or_env_clearable(Some("G-ABC".into()), "GA_MEASUREMENT_ID", ""),
            "G-ABC"
        );
        // None = never set; may use env (unset here → default).
        assert_eq!(
            db_or_env_clearable(None, "MYRIAD_TEST_UNSET_ENV_KEY_XYZ", "fallback"),
            "fallback"
        );
    }

    #[test]
    fn ui_empty_base_url_does_not_overwrite() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![ui_field("base_url", "")];
        let updates = collect_database_updates(&config);
        assert!(!updates.contains_key("base_url"));

        config.ui_config.config_fields = vec![ui_field("base_url", "https://example.com")];
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("base_url"), Some(&json!("https://example.com")));
    }

    #[test]
    fn normalize_music_playlist_id_accepts_urls() {
        assert_eq!(normalize_music_playlist_id("2884035"), "2884035");
        assert_eq!(
            normalize_music_playlist_id("https://music.163.com/#/playlist?id=2884035"),
            "2884035"
        );
        assert_eq!(
            normalize_music_playlist_id("https://y.qq.com/n/ryqq/playlist/8039305244"),
            "8039305244"
        );
        assert_eq!(normalize_music_playlist_id("  42  "), "42");
    }

    #[test]
    fn secret_env_fields_skip_masks() {
        assert!(!should_write_env_field("token", "••••••••"));
        assert!(!should_write_env_field("api_key", "********"));
        // AI/OAuth secrets still treat empty as "keep" (omit write).
        assert!(!should_write_env_field("gemini_api_key", ""));
        assert!(should_write_env_field("token", "ghp_real_token"));
        assert!(should_write_env_field("username", "octocat"));
        assert!(should_write_env_field("username", ""));
    }

    #[test]
    fn platform_env_fields_write_empty_but_skip_masks() {
        assert!(is_masked_secret_value("••••••••"));
        assert!(is_masked_secret_value("********"));
        // Empty platform secrets/usernames must clear .env (not keep).
        assert!(!is_masked_secret_value(""));
        assert!(!is_masked_secret_value("ghp_real_token"));
        assert!(!is_masked_secret_value("octocat"));
    }

    #[test]
    fn platform_credentials_clear_empty_username_and_token() {
        let mut config = empty_config();
        config.platforms.push(PlatformConfig {
            name: "GitHub".to_string(),
            enabled: true,
            has_token: true,
            config_fields: vec![
                ui_field("username", "octocat"),
                ConfigField {
                    key: "token".to_string(),
                    label: String::new(),
                    field_type: "password".to_string(),
                    value: "ghp_set".to_string(),
                    placeholder: String::new(),
                    required: false,
                },
            ],
            description: String::new(),
            icon: String::new(),
        });
        let set = collect_database_updates(&config);
        assert_eq!(set.get("github_username"), Some(&json!("octocat")));
        assert_eq!(set.get("github_token"), Some(&json!("ghp_set")));

        // Clear both with empty strings (not masks).
        config.platforms[0].config_fields = vec![
            ui_field("username", ""),
            ConfigField {
                key: "token".to_string(),
                label: String::new(),
                field_type: "password".to_string(),
                value: String::new(),
                placeholder: String::new(),
                required: false,
            },
        ];
        let cleared = collect_database_updates(&config);
        assert_eq!(cleared.get("github_username"), Some(&json!(null)));
        assert_eq!(cleared.get("github_token"), Some(&json!(null)));
    }

    #[test]
    fn ai_secret_empty_clears_and_mask_keeps() {
        let mut config = empty_config();
        config.ai_config.config_fields = vec![
            ui_field("provider_tinyfish_api_key", "tf-new"),
            ui_field("provider_gemini_api_key", "••••••••"),
        ];
        let set = collect_database_updates(&config);
        assert_eq!(set.get("provider_tinyfish_api_key"), Some(&json!("tf-new")));
        assert!(!set.contains_key("provider_gemini_api_key"));

        config.ai_config.config_fields = vec![ui_field("provider_tinyfish_api_key", "")];
        let cleared = collect_database_updates(&config);
        assert_eq!(cleared.get("provider_tinyfish_api_key"), Some(&json!(null)));
    }

    #[test]
    fn platform_credentials_mask_keeps_secret() {
        let mut config = empty_config();
        config.platforms.push(PlatformConfig {
            name: "GitHub".to_string(),
            enabled: true,
            has_token: true,
            config_fields: vec![
                ui_field("username", "octocat"),
                ConfigField {
                    key: "token".to_string(),
                    label: String::new(),
                    field_type: "password".to_string(),
                    value: "••••••••".to_string(),
                    placeholder: String::new(),
                    required: false,
                },
            ],
            description: String::new(),
            icon: String::new(),
        });
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("github_username"), Some(&json!("octocat")));
        assert!(!updates.contains_key("github_token"));
    }

    #[test]
    fn platform_clear_covers_bangumi_x_steam_and_psn() {
        let mut config = empty_config();
        for (name, fields) in [
            (
                "Bangumi",
                vec![("username", ""), ("access_token", ""), ("user_agent", "")],
            ),
            ("X", vec![("username", ""), ("bearer_token", "")]),
            ("Steam", vec![("api_key", ""), ("steam_id", "")]),
            ("PlayStation", vec![("online_id", ""), ("npsso", "")]),
        ] {
            config.platforms.push(PlatformConfig {
                name: name.to_string(),
                enabled: false,
                has_token: false,
                config_fields: fields.into_iter().map(|(k, v)| ui_field(k, v)).collect(),
                description: String::new(),
                icon: String::new(),
            });
        }
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("bangumi_username"), Some(&json!(null)));
        assert_eq!(updates.get("bangumi_access_token"), Some(&json!(null)));
        assert_eq!(updates.get("x_username"), Some(&json!(null)));
        assert_eq!(updates.get("x_bearer_token"), Some(&json!(null)));
        assert_eq!(updates.get("steam_api_key"), Some(&json!(null)));
        assert_eq!(updates.get("steam_id"), Some(&json!(null)));
        assert_eq!(updates.get("psn_online_id"), Some(&json!(null)));
        assert_eq!(updates.get("psn_npsso"), Some(&json!(null)));
    }

    #[test]
    fn platform_resolve_prefers_explicit_empty_db_over_env() {
        let env_key = "MYRIAD_TEST_PLATFORM_RESOLVE_EMPTY";
        std::env::set_var(env_key, "stale-from-env");
        assert_eq!(db_or_env_clearable(Some(String::new()), env_key, ""), "");
        assert_eq!(db_or_env_clearable(None, env_key, ""), "stale-from-env");
        assert_eq!(
            db_or_env_clearable(Some("from-db".to_string()), env_key, ""),
            "from-db"
        );
        std::env::remove_var(env_key);
    }

    #[test]
    fn update_env_var_clears_platform_secret_line() {
        let content = "GITHUB_TOKEN=ghp_old\nGITHUB_USERNAME=octocat\n";
        let next = update_env_var(content, "GITHUB_TOKEN", "").expect("empty secret is valid");
        assert!(
            next.lines().any(|l| l == "# GITHUB_TOKEN="),
            "empty secret should comment out env key, got:\n{next}"
        );
        assert!(next.contains("GITHUB_USERNAME=octocat"));
        let next = update_env_var(&next, "GITHUB_USERNAME", "").expect("empty username is valid");
        assert!(next.lines().any(|l| l == "# GITHUB_USERNAME="));
        assert!(
            update_env_var(content, "BASE_URL", "https://x.example\nJWT_SECRET=pwned").is_err(),
            "CR/LF in a value must not become extra .env entries"
        );
        assert!(update_env_var(content, "BASE_URL", "https://x.example\0").is_err());
    }

    #[test]
    fn music_playlist_id_normalized_in_db_updates() {
        let mut config = empty_config();
        config.ui_config.config_fields = vec![ui_field(
            "music_playlist_id",
            "https://music.163.com/#/playlist?id=2884035",
        )];
        let updates = collect_database_updates(&config);
        assert_eq!(updates.get("music_playlist_id"), Some(&json!("2884035")));
    }
}
