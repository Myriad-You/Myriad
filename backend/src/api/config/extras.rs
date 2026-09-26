//! Dashboard, control panel, Tapp window schemes, hitokoto, and report settings.
use axum::{Json, http::StatusCode};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

#[derive(Debug, Deserialize)]
pub struct DashboardConfigPayload {
    pub layout: Option<String>,
    pub layout_mode: Option<String>,
    pub title: Option<String>,
    pub custom_platforms: Option<String>,
    pub title_font: Option<String>,
    pub title_font_size: Option<f64>,
    pub title_color: Option<String>,
    pub widget_theme: Option<String>,
}

pub async fn update_dashboard_config(
    crate::extract::Db(db): crate::extract::Db,
    Json(payload): Json<DashboardConfigPayload>,
) -> (StatusCode, Json<Value>) {
    // Same origin set as the wallpaper, settings restore and the media upgrade,
    // so absolute sticker URLs under this site are protected as local media.
    let origins = crate::services::media::upgrade::configured_origins().await;
    save_dashboard_config(&db, payload, &origins).await
}

pub(crate) async fn save_dashboard_config(
    db: &DatabaseConnection,
    payload: DashboardConfigPayload,
    origins: &[String],
) -> (StatusCode, Json<Value>) {
    let txn = match db.begin().await {
        Ok(txn) => txn,
        Err(error) => {
            tracing::error!(%error, "failed to start dashboard config transaction");
            return dashboard_config_failed();
        }
    };
    let mut updates = std::collections::HashMap::new();
    let mut saved_layout = None;

    if let Some(layout) = payload.layout {
        match crate::services::media::bind_and_publish_dashboard_layout(&txn, &layout, origins)
            .await
        {
            Ok(rewritten) => {
                saved_layout = Some(rewritten.clone());
                updates.insert("dashboard_layout".to_string(), json!(rewritten));
            }
            Err(error) => {
                tracing::error!(%error, "failed to bind dashboard sticker references");
                let _ = txn.rollback().await;
                return media_binding_failed(&error);
            }
        }
    }

    if let Some(layout_mode) = payload.layout_mode {
        let mode = if layout_mode.trim() == "free" {
            "free"
        } else {
            "standard"
        };
        updates.insert("dashboard_layout_mode".to_string(), json!(mode));
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

    if let Err(e) =
        crate::services::config_service::ConfigService::update_configs_on(&txn, updates).await
    {
        tracing::error!("Failed to update dashboard config: {e}");
        let _ = txn.rollback().await;
        return dashboard_config_failed();
    }
    if let Err(error) = txn.commit().await {
        tracing::error!(%error, "failed to commit dashboard config");
        return dashboard_config_failed();
    }

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "ok",
            "layout": saved_layout
        })),
    )
}

/// Response for a setting whose media references could not be bound. Shared by
/// saving the dashboard and restoring a settings backup.
pub(super) fn media_binding_failed(
    error: &crate::services::media::MediaError,
) -> (StatusCode, Json<Value>) {
    use crate::services::media::MediaError;
    let status = match error {
        MediaError::Invalid { .. } | MediaError::Conflict { .. } => StatusCode::BAD_REQUEST,
        MediaError::NotReady | MediaError::InUse | MediaError::PublicInUse => StatusCode::CONFLICT,
        _ => StatusCode::INTERNAL_SERVER_ERROR,
    };
    (
        status,
        Json(json!({
            "success": false,
            "error": error.to_string(),
            "code": error.code(),
            "message": error.to_string()
        })),
    )
}

fn dashboard_config_failed() -> (StatusCode, Json<Value>) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(json!({
            "success": false,
            "error": "Failed to update dashboard config",
            "code": "config_save_failed",
            "message": "Failed to update dashboard config"
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct ControlPanelConfigPayload {
    pub control_panel_layout: Option<String>,
    pub control_panel_rows: Option<i32>,
}

pub async fn update_control_panel_config(
    crate::extract::Db(db): crate::extract::Db,
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
        tracing::error!("Failed to update control panel config: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "error": "Failed to update control panel config",
                "code": "config_save_failed",
                "message": "Failed to update control panel config"
            })),
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "ok"
        })),
    )
}

// Tapp 窗口方案 API

#[derive(Debug, Deserialize)]
pub struct TappWindowSchemesPayload {
    pub schemes: String,
}

/// Stored schemes stay a JSON array small enough to ship in `/config/ui`.
const MAX_WINDOW_SCHEMES_BYTES: usize = 64 * 1024;
const MAX_WINDOW_SCHEMES: usize = 50;

fn valid_window_schemes(raw: &str) -> bool {
    raw.len() <= MAX_WINDOW_SCHEMES_BYTES
        && serde_json::from_str::<Vec<serde_json::Map<String, Value>>>(raw)
            .is_ok_and(|schemes| schemes.len() <= MAX_WINDOW_SCHEMES)
}

pub async fn update_tapp_window_schemes(
    crate::extract::Db(db): crate::extract::Db,
    Json(payload): Json<TappWindowSchemesPayload>,
) -> (StatusCode, Json<Value>) {
    if !valid_window_schemes(&payload.schemes) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "Invalid window schemes",
                "code": "bad_request",
            })),
        );
    }
    let config_service = crate::services::config_service::ConfigService::new(db);
    let mut updates = std::collections::HashMap::new();

    updates.insert("tapp_window_schemes".to_string(), json!(payload.schemes));

    if let Err(e) = config_service.update_configs(updates).await {
        tracing::error!("Failed to update tapp window schemes: {e}");
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "success": false,
                "error": "Failed to update tapp window schemes",
                "code": "config_save_failed",
                "message": "Failed to update tapp window schemes"
            })),
        );
    }

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "message": "ok"
        })),
    )
}

// 一言（Hitokoto）配置 API

pub(crate) const HITOKOTO_CONFIG_KEY: &str = "hitokoto_config";
pub use crate::services::hitokoto::{
    HITOKOTO_BUILTIN_HOSTS, HITOKOTO_SOURCE_IDS, default_hitokoto_url,
};

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
    pub(crate) fn normalized(mut self) -> Self {
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
        .query_one_raw(Statement::from_sql_and_values(
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
    crate::extract::Db(db): crate::extract::Db,
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
    crate::extract::Db(db): crate::extract::Db,
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
                    "message": "Failed to save hitokoto config",
                    "code": "hitokoto_save_failed",
                })),
            )
        }
    }
}

// 报告过期设置

pub(crate) const REPORT_SETTINGS_KEY: &str = "report_settings";

#[derive(Debug, Serialize, Deserialize, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ReportSettings {
    /// 是否启用报告过期（关闭时报告永不过期）
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
    pub(crate) fn normalized(mut self) -> Self {
        self.expiry_days = self.expiry_days.clamp(1, 365);
        self
    }
}

pub async fn load_report_settings(db: &DatabaseConnection) -> ReportSettings {
    let sql = "SELECT value FROM configurations WHERE key = $1";
    let result = db
        .query_one_raw(Statement::from_sql_and_values(
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
    crate::extract::Db(db): crate::extract::Db,
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
    crate::extract::Db(db): crate::extract::Db,
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
                    "message": "Failed to save report settings",
                    "code": "report_settings_save_failed",
                })),
            )
        }
    }
}

#[cfg(test)]
mod hitokoto_catalog_tests {
    use super::{HITOKOTO_BUILTIN_HOSTS, HITOKOTO_SOURCE_IDS, default_hitokoto_url};

    /// Must stay aligned with frontend `BUILTIN_HITOKOTO_SOURCES` + `custom`
    /// (`frontend/src/utils/quote.ts`).
    #[test]
    fn hitokoto_source_ids_match_frontend_catalog() {
        assert_eq!(
            HITOKOTO_SOURCE_IDS,
            [
                "hitokoto-cn",
                "hitokoto-anime",
                "quotable-en",
                "meigen-ja",
                "custom",
            ]
        );
    }

    #[test]
    fn hitokoto_builtin_hosts_match_frontend_urls() {
        assert_eq!(
            HITOKOTO_BUILTIN_HOSTS,
            ["v1.hitokoto.cn", "api.quotable.io", "meigen.doodlenote.net",]
        );
        for host in HITOKOTO_BUILTIN_HOSTS {
            assert!(!host.is_empty());
            assert!(!host.contains('/'));
        }
        let default_url = default_hitokoto_url();
        assert!(
            default_url.starts_with(&format!("https://{}/", HITOKOTO_BUILTIN_HOSTS[0])),
            "{default_url}"
        );
        assert_eq!(
            default_url,
            "https://v1.hitokoto.cn/?c=d&c=i&c=k&encode=json"
        );
    }
}

#[cfg(test)]
mod window_scheme_tests {
    use super::{MAX_WINDOW_SCHEMES, valid_window_schemes};

    #[test]
    fn window_schemes_must_be_a_bounded_array_of_objects() {
        assert!(valid_window_schemes("[]"));
        assert!(valid_window_schemes(r#"[{"id":"a","windows":[]}]"#));
        assert!(!valid_window_schemes("not json"));
        assert!(!valid_window_schemes(r#"{"id":"a"}"#));
        assert!(!valid_window_schemes("[1,2]"));
        let many = format!("[{}]", vec!["{}"; MAX_WINDOW_SCHEMES + 1].join(","));
        assert!(!valid_window_schemes(&many));
    }

    #[test]
    fn window_schemes_route_is_admin_only() {
        let router = include_str!("../../router/base.rs");
        let route = router
            .split("\"/api/config/tapp-window-schemes\"")
            .nth(1)
            .and_then(|rest| rest.split(".route(").next())
            .expect("route");
        assert!(route.contains("admin_middleware"));
    }
}
