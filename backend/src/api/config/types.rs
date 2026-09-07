//! Admin config response bag types.
use serde::{Deserialize, Serialize};

#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ConfigResponse {
    pub platforms: Vec<PlatformConfig>,
    pub auto_fetch: Option<PlatformAutoFetchConfig>,
    pub ai_config: AiConfig,
    pub tripo_config: TripoConfig,
    pub report_config: ReportConfig,
    pub ui_config: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct PlatformAutoFetchConfig {
    pub enabled: bool,
    pub interval_hours: i32,
}

impl Default for PlatformAutoFetchConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            interval_hours: 24,
        }
    }
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct PlatformConfig {
    pub name: String,
    pub enabled: bool,
    pub has_token: bool,
    pub config_fields: Vec<ConfigField>,
    pub description: String,
    pub icon: String,
}

#[derive(Debug, Default, Serialize, Deserialize, Clone)]
#[serde(default)]
pub struct ConfigField {
    pub key: String,
    pub label: String,
    pub field_type: String,
    pub value: String,
    pub placeholder: String,
    pub required: bool,
}

/// 管理端 `ai_config`：只暴露 bag（`config_fields`）。
/// 历史 typed 镜像（provider / model / api_key / enabled / image_provider）已废弃。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct AiConfig {
    pub config_fields: Vec<ConfigField>,
}

/// 独立的 3D 生成配置。它不属于图片生成 provider；图片只作为 3D 管线输入。
/// 管理端只暴露 bag（`config_fields`）。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct TripoConfig {
    pub config_fields: Vec<ConfigField>,
}
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ReportConfig {
    pub config_fields: Vec<ConfigField>,
}

/// 管理端 `ui_config`：**只暴露 bag**（`config_fields`）。
/// 历史 typed 镜像字段（wallpaper/pet/theme/proxy…）已废弃——保存只读 bag，
/// 公开运行时配置走 `GET /api/config/ui`。
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct UiConfig {
    pub config_fields: Vec<ConfigField>,
}
