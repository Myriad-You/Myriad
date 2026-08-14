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
    /// Installation-level write-only credentials. Values are stored by the host
    /// and may only be attached to explicitly bound declared HTTP APIs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credentials: Option<Vec<TappCredentialDef>>,
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
    /// Allowlisted external links for host-mediated `Tapp.ui.openUrl`.
    /// Runtime opens only by declared `id` (+ optional path/query under match rules).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_urls: Option<Vec<TappOpenUrlDef>>,
}

/// One install-time declared external navigation target.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappOpenUrlDef {
    /// Stable id used by `Tapp.ui.openUrl({ id })` (not a free-form URL).
    pub id: String,
    /// Base HTTPS URL (http only for localhost / 127.0.0.1).
    pub url: String,
    /// How runtime path/query may extend `url`. Defaults to exact.
    #[serde(rename = "match", default)]
    #[cfg_attr(
        feature = "tapp-contract-schema",
        schemars(extend("enum" = crate::contract_rules::OPEN_URL_MATCH_MODES))
    )]
    pub match_mode: TappOpenUrlMatch,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum TappOpenUrlMatch {
    #[default]
    Exact,
    Prefix,
    Origin,
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
    Manager,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum TappCredentialIn {
    Header,
    Query,
    Form,
    Sign,
}

impl TappCredentialIn {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Header => "header",
            Self::Query => "query",
            Self::Form => "form",
            Self::Sign => "sign",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase")]
pub enum TappCredentialEncoding {
    Base64,
}

impl TappCredentialEncoding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Base64 => "base64",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
pub enum TappCredentialSignAlg {
    #[serde(rename = "md5-sorted-kv")]
    Md5SortedKv,
    #[serde(rename = "hmac-sha256-raw")]
    HmacSha256Raw,
}

impl TappCredentialSignAlg {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Md5SortedKv => "md5-sorted-kv",
            Self::HmacSha256Raw => "hmac-sha256-raw",
        }
    }

    pub fn is_implemented(self) -> bool {
        matches!(self, Self::Md5SortedKv | Self::HmacSha256Raw)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappCredentialSign {
    pub alg: TappCredentialSignAlg,
    pub over: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub timestamp_field: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappApiCredentialBinding {
    /// Key from top-level `manifest.credentials`.
    pub key: String,
    /// Where the host applies the secret. Omitted with `header` means header (legacy).
    #[serde(default, skip_serializing_if = "Option::is_none", rename = "in")]
    pub in_placement: Option<TappCredentialIn>,
    /// Destination name: header, query parameter, form field, or signature field.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    /// Legacy header name. Equivalent to `field` when `in` is header or omitted.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub header: Option<String>,
    /// Optional literal prefix such as `Bearer `. Header placement only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    /// Optional host encoding of the secret before prefix/placement. Not for `sign`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub encoding: Option<TappCredentialEncoding>,
    /// Host-only request signature. Required when `in` is `sign`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sign: Option<TappCredentialSign>,
}

/// Normalized view of a declared credential binding after Manifest checks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedTappCredentialBinding {
    pub placement: TappCredentialIn,
    pub field: String,
    pub prefix: Option<String>,
    pub encoding: Option<TappCredentialEncoding>,
    pub sign: Option<TappCredentialSign>,
}

impl TappApiCredentialBinding {
    pub fn resolve(&self) -> Result<ResolvedTappCredentialBinding, String> {
        let placement = match self.in_placement {
            Some(placement) => placement,
            None => {
                if self.sign.is_some() {
                    return Err("credential.sign requires in: \"sign\"".into());
                }
                TappCredentialIn::Header
            }
        };

        let field = match placement {
            TappCredentialIn::Header => match (self.field.as_deref(), self.header.as_deref()) {
                (Some(field), Some(header)) if field != header => {
                    return Err("credential.field and credential.header must match".into());
                }
                (Some(field), _) => field.to_string(),
                (None, Some(header)) => header.to_string(),
                (None, None) => {
                    return Err("header credential requires field or header".into());
                }
            },
            TappCredentialIn::Query | TappCredentialIn::Form | TappCredentialIn::Sign => {
                if self.header.is_some() {
                    return Err("credential.header is only valid for header credentials".into());
                }
                self.field
                    .clone()
                    .filter(|value| !value.is_empty())
                    .ok_or_else(|| "credential.field is required".to_string())?
            }
        };

        if field.len() > crate::contract_rules::MAX_CREDENTIAL_FIELD_LEN
            || !valid_agent_name(&field)
        {
            return Err("credential.field is invalid".into());
        }

        match placement {
            TappCredentialIn::Header => {
                if self.sign.is_some() {
                    return Err("credential.sign is only valid when in is \"sign\"".into());
                }
            }
            TappCredentialIn::Query | TappCredentialIn::Form => {
                if self.sign.is_some() {
                    return Err("credential.sign is only valid when in is \"sign\"".into());
                }
                if self.prefix.is_some() {
                    return Err("credential.prefix is only valid for header credentials".into());
                }
            }
            TappCredentialIn::Sign => {
                if self.prefix.is_some() || self.encoding.is_some() {
                    return Err("sign credentials cannot declare prefix or encoding".into());
                }
                let Some(sign) = &self.sign else {
                    return Err("in: \"sign\" requires a sign block".into());
                };
                validate_credential_sign(sign, &field)?;
            }
        }

        Ok(ResolvedTappCredentialBinding {
            placement,
            field,
            prefix: self.prefix.clone(),
            encoding: self.encoding,
            sign: self.sign.clone(),
        })
    }
}

fn validate_credential_sign(sign: &TappCredentialSign, sign_field: &str) -> Result<(), String> {
    if sign.over.is_empty() {
        return Err("credential.sign.over must list at least one field".into());
    }
    if sign.over.len() > crate::contract_rules::MAX_CREDENTIAL_SIGN_OVER {
        return Err(format!(
            "credential.sign.over accepts at most {} fields",
            crate::contract_rules::MAX_CREDENTIAL_SIGN_OVER
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    for name in &sign.over {
        if !valid_agent_name(name) || !seen.insert(name.as_str()) {
            return Err("credential.sign.over contains an invalid or duplicate field".into());
        }
        if name == sign_field {
            return Err("credential.sign field cannot appear in over".into());
        }
    }
    if let Some(timestamp_field) = &sign.timestamp_field {
        if !valid_agent_name(timestamp_field) {
            return Err("credential.sign.timestampField is invalid".into());
        }
        if timestamp_field == sign_field {
            return Err("credential.sign.timestampField cannot be the sign field".into());
        }
        if !sign.over.iter().any(|name| name == timestamp_field) {
            return Err("credential.sign.timestampField must be listed in over".into());
        }
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
pub enum TappRouteVerifyOver {
    #[serde(rename = "raw-body")]
    RawBody,
    #[serde(rename = "canonical-query")]
    CanonicalQuery,
}

impl TappRouteVerifyOver {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RawBody => "raw-body",
            Self::CanonicalQuery => "canonical-query",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum TappRouteVerifyEncoding {
    #[default]
    Hex,
    Base64,
}

impl TappRouteVerifyEncoding {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Hex => "hex",
            Self::Base64 => "base64",
        }
    }

    fn is_hex(&self) -> bool {
        matches!(self, Self::Hex)
    }
}

fn default_route_max_skew_secs() -> u32 {
    crate::contract_rules::ROUTE_DEFAULT_MAX_SKEW_SECS
}

fn is_default_route_max_skew_secs(value: &u32) -> bool {
    *value == crate::contract_rules::ROUTE_DEFAULT_MAX_SKEW_SECS
}

fn default_route_methods() -> Vec<String> {
    vec!["GET".to_string()]
}

/// Host-verified inbound HMAC for `/tapi/{tappId}{path}`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappRouteVerify {
    pub key: String,
    #[cfg_attr(
        feature = "tapp-contract-schema",
        schemars(extend("enum" = crate::contract_rules::ROUTE_VERIFY_ALGS))
    )]
    pub alg: TappCredentialSignAlg,
    pub header: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prefix: Option<String>,
    #[cfg_attr(
        feature = "tapp-contract-schema",
        schemars(extend("enum" = crate::contract_rules::ROUTE_VERIFY_OVER))
    )]
    pub over: TappRouteVerifyOver,
    #[serde(default, skip_serializing_if = "TappRouteVerifyEncoding::is_hex")]
    #[cfg_attr(
        feature = "tapp-contract-schema",
        schemars(extend("enum" = crate::contract_rules::ROUTE_VERIFY_ENCODINGS))
    )]
    pub encoding: TappRouteVerifyEncoding,
    pub timestamp_header: String,
    pub nonce_header: String,
    #[serde(
        default = "default_route_max_skew_secs",
        skip_serializing_if = "is_default_route_max_skew_secs"
    )]
    pub max_skew_secs: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappApiRoute {
    pub path: String,
    #[serde(default = "default_route_methods")]
    pub methods: Vec<String>,
    pub verify: TappRouteVerify,
}

pub fn valid_inbound_route_path(path: &str) -> bool {
    path.len() >= 2
        && path.len() <= 65
        && path.starts_with('/')
        && path
            .as_bytes()
            .get(1)
            .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && path[1..]
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

pub fn valid_inbound_verify_header(name: &str) -> bool {
    if name.len() < 3 || name.len() > 65 || !name.starts_with("X-") {
        return false;
    }
    let rest = &name.as_bytes()[2..];
    rest.first()
        .is_some_and(|byte| byte.is_ascii_alphanumeric())
        && rest
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || *byte == b'-')
        && !crate::contract_rules::ROUTE_RESERVED_HEADERS
            .iter()
            .any(|reserved| name.eq_ignore_ascii_case(reserved))
}

pub fn valid_inbound_nonce(value: &str) -> bool {
    let len = value.len();
    len >= crate::contract_rules::ROUTE_MIN_NONCE_LEN
        && len <= crate::contract_rules::ROUTE_MAX_NONCE_LEN
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum TappHttpBodyMode {
    #[default]
    Json,
    Raw,
    Form,
}

impl TappHttpBodyMode {
    fn is_json(&self) -> bool {
        matches!(self, Self::Json)
    }
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
    #[cfg_attr(
        feature = "tapp-contract-schema",
        schemars(extend("enum" = crate::contract_rules::HTTP_METHODS))
    )]
    pub method: String,
    pub headers: Option<HashMap<String, String>>,
    /// Host-only credential binding. The value is never added to the template
    /// context or returned to sandbox JavaScript.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub credential: Option<TappApiCredentialBinding>,
    #[serde(default, skip_serializing_if = "TappHttpBodyMode::is_json")]
    #[cfg_attr(
        feature = "tapp-contract-schema",
        schemars(extend("default" = "json"))
    )]
    pub body_mode: TappHttpBodyMode,
    pub body: Option<serde_json::Value>,
    pub builtin: Option<String>,
    pub inject: Option<HashMap<String, String>>,
    #[serde(default)]
    pub cache_ttl: u32,
    pub spoof: Option<String>,
    pub description: Option<String>,
    /// Optional inbound HTTP mount at `/tapi/{tappId}{path}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub route: Option<TappApiRoute>,
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
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TappCredentialDef {
    pub key: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placeholder: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "tapp-contract-schema", derive(schemars::JsonSchema))]
#[serde(deny_unknown_fields)]
pub struct TappSettingOption {
    pub value: String,
    pub label: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn inbound_verify_header_rejects_proxy_and_session_names() {
        assert!(valid_inbound_verify_header("X-Signature"));
        assert!(valid_inbound_verify_header("X-Tapp-Timestamp"));
        assert!(!valid_inbound_verify_header("x-signature"));
        assert!(!valid_inbound_verify_header("Authorization"));
        assert!(!valid_inbound_verify_header("X-CSRF-Token"));
        assert!(!valid_inbound_verify_header("X-Forwarded-For"));
        assert!(!valid_inbound_verify_header("X-Real-IP"));
        assert!(!valid_inbound_verify_header("X-Request-Id"));
        assert!(!valid_inbound_verify_header("X-Tapp-Runtime-Grant"));
    }

    #[test]
    fn legacy_header_binding_still_resolves() {
        let binding: TappApiCredentialBinding = serde_json::from_value(json!({
            "key": "wegame",
            "header": "Authorization",
            "prefix": "Bearer "
        }))
        .unwrap();
        let resolved = binding.resolve().unwrap();
        assert_eq!(resolved.placement, TappCredentialIn::Header);
        assert_eq!(resolved.field, "Authorization");
        assert_eq!(resolved.prefix.as_deref(), Some("Bearer "));
    }

    #[test]
    fn sign_binding_requires_over_and_field() {
        let binding: TappApiCredentialBinding = serde_json::from_value(json!({
            "key": "afdianToken",
            "in": "sign",
            "field": "sign",
            "sign": {
                "alg": "md5-sorted-kv",
                "over": ["params", "ts", "user_id"],
                "timestampField": "ts"
            }
        }))
        .unwrap();
        let resolved = binding.resolve().unwrap();
        assert_eq!(resolved.placement, TappCredentialIn::Sign);
        assert_eq!(resolved.field, "sign");
        assert_eq!(
            resolved.sign.unwrap().timestamp_field.as_deref(),
            Some("ts")
        );
    }

    #[test]
    fn sign_without_in_is_rejected() {
        let binding: TappApiCredentialBinding = serde_json::from_value(json!({
            "key": "afdianToken",
            "field": "sign",
            "sign": { "alg": "md5-sorted-kv", "over": ["params"] }
        }))
        .unwrap();
        assert!(binding.resolve().unwrap_err().contains("in: \"sign\""));
    }
}
