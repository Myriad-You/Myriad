//! Manifest capability declarations shared by install validation and runtime APIs.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
pub enum TappCategory {
    #[serde(rename = "ai")]
    Ai,
    #[serde(
        rename = "data",
        alias = "data-extension",
        alias = "platform",
        alias = "visualization"
    )]
    Data,
    #[serde(rename = "developer", alias = "development", alias = "dev")]
    Developer,
    #[serde(rename = "game", alias = "games")]
    Game,
    #[serde(rename = "media", alias = "entertainment", alias = "music")]
    Media,
    #[serde(rename = "productivity")]
    Productivity,
    #[serde(rename = "social", alias = "communication")]
    Social,
    #[serde(
        rename = "utility",
        alias = "demo",
        alias = "page",
        alias = "test",
        alias = "tool",
        alias = "tools",
        alias = "utilities",
        alias = "widget"
    )]
    Utility,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: Option<String>,
    /// 展示文案的多语言覆盖：语言标签 → { name?, description? }
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub locales: Option<HashMap<String, TappManifestLocaleEntry>>,
    pub author: Option<TappAuthor>,
    pub main: String,
    pub styles: Option<String>,
    pub widget_styles: Option<String>,
    pub page_styles: Option<String>,
    pub page_template: Option<String>,
    pub css_mode: Option<String>,
    #[serde(default)]
    pub permissions: Vec<String>,
    pub icon: Option<String>,
    pub icon_svg: Option<String>,
    pub theme_color: Option<String>,
    pub homepage: Option<String>,
    pub repository: Option<String>,
    pub min_system_version: Option<String>,
    pub widgets: Option<Vec<TappWidgetDef>>,
    #[serde(default)]
    pub has_page: bool,
    #[serde(default)]
    pub background_requirements: Option<Vec<String>>,
    pub settings: Option<Vec<TappSettingDef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub category: Option<TappCategory>,
    #[serde(default)]
    pub page_modules: Option<Vec<String>>,
    #[serde(default)]
    pub apis: Option<HashMap<String, TappApiDef>>,
    #[serde(default)]
    pub data_exchange: Option<TappDataExchangeManifest>,
    #[serde(default)]
    pub ai: Option<TappAiManifest>,
    #[serde(default)]
    pub events: Option<TappEventsManifest>,
    #[serde(default)]
    pub agent: Option<TappAgentManifest>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub assets: Option<Vec<String>>,
}

/// 单个语言下的清单展示文案覆盖
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappManifestLocaleEntry {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum TappAiOperation {
    Generate,
    Analyze,
    Chat,
    Image,
}

impl TappAiOperation {
    pub fn permission(self) -> &'static str {
        match self {
            Self::Generate => "ai:generate",
            Self::Analyze => "ai:analyze",
            Self::Chat => "ai:chat",
            Self::Image => "ai:image",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum TappAiModelTier {
    Standard,
    Pro,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum TappAiContextSource {
    Platform,
    Report,
    Profile,
    Custom,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum TappAiOutputFormat {
    Text,
    Json,
    Image,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappAiManifest {
    pub protocol_version: u8,
    pub operations: Vec<TappAiOperation>,
    pub model_tier: TappAiModelTier,
    #[serde(default)]
    pub context_sources: Vec<TappAiContextSource>,
    pub output_formats: Vec<TappAiOutputFormat>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappEventsManifest {
    #[serde(default)]
    pub publish: Vec<String>,
    #[serde(default)]
    pub subscribe: Vec<String>,
}

pub fn valid_event_topic(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappAgentManifest {
    pub protocol_version: u8,
    pub interactions: Vec<TappAgentInteractionDef>,
    #[serde(default)]
    pub intents: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappAgentInteractionDef {
    #[serde(rename = "type")]
    pub interaction_type: String,
    #[serde(default)]
    pub input_schema: Option<String>,
    #[serde(default)]
    pub result_schema: Option<String>,
}

pub fn valid_agent_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && !value.starts_with('.')
        && !value.ends_with('.')
        && !value.contains("..")
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.'))
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappDataExchangeManifest {
    #[serde(default)]
    pub exports: Vec<TappDataExport>,
    #[serde(default)]
    pub imports: Vec<TappDataImport>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappDataExport {
    pub id: String,
    /// Supported inline JSON Schema subset; remote and file `$ref` are denied.
    pub schema: serde_json::Value,
    pub max_bytes: usize,
    #[serde(default)]
    pub max_records: Option<usize>,
    #[serde(default)]
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappDataImport {
    pub tapp_id: String,
    pub export_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum TappApiAccess {
    Public,
    #[default]
    Protected,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappApiDef {
    #[serde(default)]
    pub access: TappApiAccess,
    #[serde(rename = "type", default = "default_api_type")]
    pub api_type: String,
    pub endpoint: Option<String>,
    #[serde(default = "default_http_method")]
    pub method: String,
    pub headers: Option<HashMap<String, String>>,
    pub body: Option<serde_json::Value>,
    pub builtin: Option<String>,
    pub inject: Option<HashMap<String, String>>,
    #[serde(default)]
    pub cache_ttl: u32,
    pub spoof: Option<String>,
    pub description: Option<String>,
}

fn default_api_type() -> String {
    "http".to_string()
}

fn default_http_method() -> String {
    "GET".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TappAuthor {
    pub name: String,
    pub email: Option<String>,
    pub url: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappWidgetDef {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub default_size: String,
    pub sizes: Vec<String>,
    pub category: Option<TappWidgetCategory>,
    pub templates: Option<HashMap<String, String>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub settings: Vec<TappSettingDef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub refresh_policy: Option<TappWidgetRefreshPolicy>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
pub enum TappWidgetCategory {
    #[serde(rename = "stats")]
    Stats,
    #[serde(rename = "activity")]
    Activity,
    #[serde(rename = "visualization")]
    Visualization,
    #[serde(rename = "utility", alias = "tool")]
    Utility,
    #[serde(rename = "custom")]
    Custom,
}

impl TappWidgetCategory {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stats => "stats",
            Self::Activity => "activity",
            Self::Visualization => "visualization",
            Self::Utility => "utility",
            Self::Custom => "custom",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum TappWidgetRefreshMode {
    Event,
    Interval,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappWidgetRefreshPolicy {
    pub mode: TappWidgetRefreshMode,
    #[serde(default)]
    pub interval_seconds: Option<u32>,
    #[serde(default = "default_true")]
    pub refresh_on_visible: bool,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappSettingDef {
    pub key: String,
    pub label: String,
    #[serde(rename = "type")]
    pub setting_type: String,
    pub description: Option<String>,
    pub default_value: Option<serde_json::Value>,
    pub options: Option<Vec<TappSettingOption>>,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TappSettingOption {
    pub value: String,
    pub label: String,
}
