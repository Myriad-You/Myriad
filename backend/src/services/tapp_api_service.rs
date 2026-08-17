//! Tapp API 执行服务
//!
//! 负责：
//! 1. 解析 Tapp manifest 中的 API 声明
//! 2. 执行 API 调用（HTTP 或内置）
//! 3. 自动注入非敏感上下文（geo、user）
//! 4. 权限检查（public / protected / manager）
//! 5. 响应缓存
//! 6. 区域伪装（绕过地区限制）

use once_cell::sync::Lazy;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::str::FromStr;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

use crate::services::http_client::TAPP_HTTP_CLIENT;
use crate::services::permission_service::UserRole;
use crate::services::spoof_utils::{generate_spoof_headers, SpoofConfig};
use myriad_tapp_contract::contract_rules::{
    HTTP_BODY_METHODS, MAX_TAPP_NON_JSON_HTTP_REQUEST_BYTES,
};
use myriad_tapp_contract::manifest::{
    TappAiOperation, TappApiAccess, TappApiDef, TappCredentialEncoding, TappCredentialIn,
    TappCredentialSignAlg, TappHttpBodyMode,
};

// 预编译模板变量正则，避免每次调用都重新编译
static TEMPLATE_RE: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r"\{\{([^}]+)\}\}").expect("Invalid template regex"));

// Geo 信息缓存（按 IP，10分钟 TTL）
static GEO_CACHE: Lazy<RwLock<HashMap<String, (GeoInfo, Instant)>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));

const GEO_CACHE_TTL: Duration = Duration::from_secs(600);
const MAX_GEO_CACHE_ENTRIES: usize = 2048; // default profile; runtime: memory_profile

// API 响应缓存

struct CacheEntry {
    data: Value,
    /// Approximate JSON size for byte-budget eviction.
    size_bytes: usize,
    expires_at: Instant,
    cached_at: Instant,
}

fn approx_json_bytes(value: &Value) -> usize {
    serde_json::to_vec(value).map(|v| v.len()).unwrap_or(0)
}

static API_CACHE: Lazy<RwLock<HashMap<String, CacheEntry>>> =
    Lazy::new(|| RwLock::new(HashMap::new()));
const MAX_API_CACHE_ENTRIES: usize = 2048; // default profile; runtime: memory_profile
const MAX_TAPP_HTTP_RESPONSE_BYTES: usize = 2 * 1024 * 1024;

// 上下文类型

/// API 执行上下文
#[derive(Debug, Clone)]
pub struct ApiExecutionContext {
    /// 用户 ID（负数表示游客）
    pub user_id: i32,
    /// Manifest 所属安装 owner；共享管理员 Tapp 与用户 Tapp 不能共用响应缓存。
    pub owner_id: i32,
    /// 用户名
    pub username: String,
    /// 是否是管理员
    pub is_admin: bool,
    /// 客户端 IP
    pub client_ip: Option<String>,
    /// Tapp 已授权的权限
    pub granted_permissions: Vec<String>,
    /// Manifest AI model tier used by governed builtin adapters.
    pub ai_model_tier: Option<crate::config::ModelTier>,
    /// Host-only material resolved after install/runtime binding checks.
    pub credential: Option<crate::services::tapp_credentials::ResolvedApiCredential>,
    /// Installation settings the declared API may interpolate as `settings.*`.
    pub settings: std::collections::BTreeMap<String, Value>,
}

/// 地理位置信息
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GeoInfo {
    pub lat: f64,
    pub lon: f64,
    pub city: String,
    pub region: String,
    pub country: String,
}

/// API 执行结果
#[derive(Debug, Serialize)]
pub struct ApiExecutionResult {
    pub success: bool,
    pub data: Option<Value>,
    pub error: Option<String>,
    pub cached: bool,
}

// Tapp API 服务

pub struct TappApiService;

#[derive(Debug)]
struct EncodedHttpBody {
    bytes: Vec<u8>,
    default_content_type: Option<&'static str>,
}

struct PreparedHttpRequest {
    url: String,
    body: Option<EncodedHttpBody>,
    secret_header: Option<(HeaderName, HeaderValue)>,
    redaction_needles: Vec<String>,
}

fn present_credential_value(
    raw: &str,
    encoding: Option<TappCredentialEncoding>,
) -> Result<String, String> {
    match encoding {
        None => Ok(raw.to_string()),
        Some(TappCredentialEncoding::Base64) => {
            use base64::Engine;
            Ok(base64::engine::general_purpose::STANDARD.encode(raw.as_bytes()))
        }
    }
}

fn credential_redaction_needles(raw: &str, presented: &str, prefix: Option<&str>) -> Vec<String> {
    let mut needles = Vec::new();
    if !raw.is_empty() {
        needles.push(raw.to_string());
    }
    if !presented.is_empty() && presented != raw {
        needles.push(presented.to_string());
    }
    if let Some(prefix) = prefix {
        if !prefix.is_empty() {
            needles.push(format!("{prefix}{presented}"));
        }
    }
    needles
}

fn append_credential_query(url: &str, field: &str, value: &str) -> Result<String, String> {
    let mut parsed = url::Url::parse(url).map_err(|_| "Invalid credential URL".to_string())?;
    if parsed.query_pairs().any(|(name, _)| name.as_ref() == field) {
        return Err("credential query field is already present".into());
    }
    parsed.query_pairs_mut().append_pair(field, value);
    Ok(parsed.into())
}

fn scalar_to_sign_string(field: &str, value: &Value) -> Result<String, String> {
    match value {
        Value::String(value) => Ok(value.clone()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Null => Ok(String::new()),
        Value::Array(_) | Value::Object(_) => Err(format!(
            "signed or form field {field} must resolve to a scalar value"
        )),
    }
}

fn signed_sorted_kv_material(
    fields: &serde_json::Map<String, Value>,
    sign: &myriad_tapp_contract::manifest::TappCredentialSign,
) -> Result<String, String> {
    let mut pieces = Vec::new();
    let mut names = sign.over.clone();
    names.sort();
    for name in names {
        let value = fields
            .get(&name)
            .ok_or_else(|| format!("signed field {name} is missing from the body"))?;
        pieces.push(name.clone());
        pieces.push(scalar_to_sign_string(&name, value)?);
    }
    Ok(pieces.concat())
}

fn apply_body_signature(
    fields: &mut serde_json::Map<String, Value>,
    sign_field: &str,
    sign: &myriad_tapp_contract::manifest::TappCredentialSign,
    token: &str,
) -> Result<(), String> {
    if let Some(timestamp_field) = &sign.timestamp_field {
        let unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        fields.insert(timestamp_field.clone(), json!(unix));
    }
    if fields.contains_key(sign_field) {
        return Err("signature field is already present in the body".into());
    }
    let sorted = signed_sorted_kv_material(fields, sign)?;
    let digest = match sign.alg {
        TappCredentialSignAlg::Md5SortedKv => {
            let mut material = String::from(token);
            material.push_str(&sorted);
            format!("{:x}", md5::compute(material.as_bytes()))
        }
        TappCredentialSignAlg::HmacSha256Raw => hex::encode(
            crate::services::tapp_hmac::hmac_sha256(token.as_bytes(), sorted.as_bytes()),
        ),
    };
    fields.insert(sign_field.to_string(), Value::String(digest));
    Ok(())
}

impl TappApiService {
    /// 执行 Tapp API 调用
    ///
    /// # 参数
    /// - `tapp_id`: Tapp ID
    /// - `api_name`: API 名称（在 manifest.apis 中定义的 key）
    /// - `api_def`: API 定义
    /// - `params`: 前端传入的参数
    /// - `context`: 执行上下文
    ///
    /// # 返回
    /// API 执行结果
    pub async fn execute(
        tapp_id: &str,
        api_name: &str,
        api_def: &TappApiDef,
        params: Option<Value>,
        context: &ApiExecutionContext,
    ) -> ApiExecutionResult {
        // 1. 权限检查
        if let Err(e) = Self::check_permission(api_def, context).await {
            return ApiExecutionResult {
                success: false,
                data: None,
                error: Some(e),
                cached: false,
            };
        }

        // 2. 检查缓存
        let cache_key = Self::generate_cache_key(tapp_id, api_name, api_def, &params, context);
        if api_def.cache_ttl > 0 {
            if let Some(cached) = Self::get_cached(&cache_key).await {
                return ApiExecutionResult {
                    success: true,
                    data: Some(cached),
                    error: None,
                    cached: true,
                };
            }
        }

        // 3. 构建注入上下文
        let inject_context = match Self::build_inject_context(api_def, context).await {
            Ok(ctx) => ctx,
            Err(e) => {
                return ApiExecutionResult {
                    success: false,
                    data: None,
                    error: Some(e),
                    cached: false,
                };
            }
        };

        // 4. 合并前端参数
        let mut full_context = inject_context;
        if let Some(params) = params {
            if let Some(obj) = params.as_object() {
                for (k, v) in obj {
                    full_context.insert(format!("params.{}", k), v.clone());
                }
            }
        }

        // 5. 执行 API
        let result = match api_def.api_type.as_str() {
            "http" => {
                Self::execute_http_api_with_credential(
                    api_def,
                    &full_context,
                    context.credential.as_ref(),
                )
                .await
            }
            "builtin" => Self::execute_builtin_api(tapp_id, api_def, &full_context, context).await,
            _ => Err(format!("Unknown API type: {}", api_def.api_type)),
        };

        match result {
            Ok(data) => {
                // 缓存结果
                if api_def.cache_ttl > 0 {
                    Self::set_cached(&cache_key, &data, api_def.cache_ttl).await;
                }

                ApiExecutionResult {
                    success: true,
                    data: Some(data),
                    error: None,
                    cached: false,
                }
            }
            Err(e) => ApiExecutionResult {
                success: false,
                data: None,
                error: Some(e),
                cached: false,
            },
        }
    }

    /// 权限检查
    async fn check_permission(
        api_def: &TappApiDef,
        context: &ApiExecutionContext,
    ) -> Result<(), String> {
        if api_def.api_type == "http"
            && !context
                .granted_permissions
                .iter()
                .any(|permission| permission == "network:fetch")
        {
            return Err("Permission 'network:fetch' required".to_string());
        }

        if api_def.access == TappApiAccess::Protected && context.user_id < 0 {
            return Err("Protected API requires login".to_string());
        }
        if api_def.access == TappApiAccess::Manager
            && context.user_id != context.owner_id
            && !context.is_admin
        {
            return Err(
                "Manager API requires the installation owner or an administrator".to_string(),
            );
        }

        Ok(())
    }

    /// 构建注入上下文
    async fn build_inject_context(
        api_def: &TappApiDef,
        context: &ApiExecutionContext,
    ) -> Result<HashMap<String, Value>, String> {
        let mut inject_context: HashMap<String, Value> = HashMap::new();

        // 添加用户上下文
        inject_context.insert("user.id".to_string(), json!(context.user_id));
        inject_context.insert("user.username".to_string(), json!(context.username));
        inject_context.insert("user.isAdmin".to_string(), json!(context.is_admin));

        for (key, value) in &context.settings {
            inject_context.insert(format!("settings.{key}"), value.clone());
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default();
        inject_context.insert("time.unix".to_string(), json!(now.as_secs()));
        inject_context.insert("time.unixMs".to_string(), json!(now.as_millis() as u64));
        inject_context.insert(
            "time.iso8601".to_string(),
            json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        );
        let nonce = rand::random::<[u8; 16]>();
        inject_context.insert("time.nonce".to_string(), json!(hex::encode(nonce)));

        // Every templated HTTP surface can consume host context. Inspect them
        // all so direct references in headers/body do not resolve to empty
        // strings merely because endpoint itself has no placeholder.
        let needs_geo = Self::api_uses_template_prefix(api_def, "{{geo.");

        if needs_geo {
            let geo = Self::get_geo_info(context.client_ip.as_deref()).await;
            inject_context.insert("geo.lat".to_string(), json!(geo.lat));
            inject_context.insert("geo.lon".to_string(), json!(geo.lon));
            inject_context.insert("geo.city".to_string(), json!(geo.city));
            inject_context.insert("geo.region".to_string(), json!(geo.region));
            inject_context.insert("geo.country".to_string(), json!(geo.country));
        }

        if Self::api_uses_template_prefix(api_def, "{{secrets.") {
            return Err("Host secret templates are not available to Tapps".to_string());
        }

        Self::apply_inject_aliases(api_def.inject.as_ref(), &mut inject_context);

        Ok(inject_context)
    }

    fn api_uses_template_prefix(api_def: &TappApiDef, prefix: &str) -> bool {
        api_def.endpoint.iter().any(|value| value.contains(prefix))
            || api_def
                .headers
                .iter()
                .flat_map(|values| values.values())
                .any(|value| value.contains(prefix))
            || api_def
                .inject
                .iter()
                .flat_map(|values| values.values())
                .any(|value| value.contains(prefix))
            || api_def
                .body
                .as_ref()
                .is_some_and(|body| Self::json_contains_template_prefix(body, prefix))
    }

    fn json_contains_template_prefix(value: &Value, prefix: &str) -> bool {
        match value {
            Value::String(value) => value.contains(prefix),
            Value::Array(values) => values
                .iter()
                .any(|value| Self::json_contains_template_prefix(value, prefix)),
            Value::Object(values) => values
                .values()
                .any(|value| Self::json_contains_template_prefix(value, prefix)),
            _ => false,
        }
    }

    fn apply_inject_aliases(
        aliases: Option<&HashMap<String, String>>,
        context: &mut HashMap<String, Value>,
    ) {
        let Some(aliases) = aliases else {
            return;
        };
        // Resolve every alias from the same host context snapshot. This keeps
        // behavior deterministic and prevents HashMap iteration order from
        // turning alias-to-alias chains into an accidental API contract.
        let source = context.clone();
        for (alias, template) in aliases {
            let value = Self::resolve_json_templates(&Value::String(template.clone()), &source);
            context.insert(alias.clone(), value);
        }
    }

    /// 获取地理位置信息（带缓存）
    async fn get_geo_info(client_ip: Option<&str>) -> GeoInfo {
        let ip = client_ip.unwrap_or("auto");

        // 检查缓存
        {
            let cache = GEO_CACHE.read().await;
            if let Some((geo, cached_at)) = cache.get(ip) {
                if cached_at.elapsed() < GEO_CACHE_TTL {
                    return geo.clone();
                }
            }
        }

        // Private / loopback / missing → server egress (local dev or broken proxy trust).
        let is_local = crate::middleware::client_ip::is_private_or_local_str(ip);

        let target_ip = if is_local {
            tracing::warn!(
                client_ip = %ip,
                "tapp geo: private/unresolved client IP; falling back to server egress \
                 (check TRUST_PROXY_HEADERS / TRUST_PROXY_PEERS and proxy X-Real-IP)"
            );
            // 获取服务器公网 IP
            if let Ok(resp) = TAPP_HTTP_CLIENT
                .get("https://api.ipify.org?format=json")
                .send()
                .await
            {
                if let Ok(data) = resp.json::<Value>().await {
                    data.get("ip")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                } else {
                    None
                }
            } else {
                None
            }
        } else {
            Some(ip.to_string())
        };

        let target_ip = target_ip.unwrap_or_else(|| "auto".to_string());

        // 使用 ip-api.com 获取地理位置
        let url = format!(
            "http://ip-api.com/json/{}?fields=status,lat,lon,city,regionName,country",
            if target_ip == "auto" { "" } else { &target_ip }
        );

        if let Ok(resp) = TAPP_HTTP_CLIENT.get(&url).send().await {
            if let Ok(data) = resp.json::<Value>().await {
                if data.get("status").and_then(|s| s.as_str()) == Some("success") {
                    let geo = GeoInfo {
                        lat: data.get("lat").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        lon: data.get("lon").and_then(|v| v.as_f64()).unwrap_or(0.0),
                        city: data
                            .get("city")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        region: data
                            .get("regionName")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                        country: data
                            .get("country")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string(),
                    };
                    let mut cache = GEO_CACHE.write().await;
                    cache.retain(|_, (_, cached_at)| cached_at.elapsed() < GEO_CACHE_TTL);
                    let geo_cap = crate::services::memory_profile::max_geo_cache_entries();
                    let geo_bytes_cap = crate::services::memory_profile::max_geo_cache_bytes();
                    // Geo entries are small (~200B); enforce entry + coarse byte budget.
                    let entry_bytes = 256usize;
                    while cache.len() >= geo_cap
                        || cache.len().saturating_mul(entry_bytes) >= geo_bytes_cap
                    {
                        let Some(oldest) = cache
                            .iter()
                            .min_by_key(|(_, (_, cached_at))| *cached_at)
                            .map(|(key, _)| key.clone())
                        else {
                            break;
                        };
                        cache.remove(&oldest);
                    }
                    cache.insert(ip.to_string(), (geo.clone(), Instant::now()));
                    return geo;
                }
            }
        }

        GeoInfo::default()
    }

    /// Declared-API egress: honor the admin/env proxy that `http_client`
    /// documents for China, instead of always pinning local DNS.
    ///
    /// `build_public_http_client` replaced the shared Tapp client and started
    /// failing closed when any resolved address was non-public. That dropped
    /// the working proxy path and broke `hub.docker.com` on polluted DNS.
    async fn declared_api_http_client(
        url: &str,
    ) -> Result<(reqwest::Url, reqwest::Client), String> {
        let timeout = Duration::from_secs(30);
        let user_agent = Some("Myriad-Tapp/1.0 (declared-api)");
        let proxy_config = crate::services::http_client::ProxyConfig::from_dynamic_config().await;
        if proxy_config.should_use_proxy() && !proxy_config.should_bypass(url) {
            if let Some(proxy_url) = &proxy_config.proxy_url {
                let mut proxy = reqwest::Proxy::all(proxy_url)
                    .map_err(|error| format!("Invalid outbound proxy: {error}"))?;
                let bypass_str = proxy_config
                    .bypass_list
                    .iter()
                    .map(|value| value.trim())
                    .filter(|value| !value.is_empty())
                    .collect::<Vec<_>>()
                    .join(",");
                if !bypass_str.is_empty() {
                    if let Some(no_proxy) = reqwest::NoProxy::from_string(&bypass_str) {
                        proxy = proxy.no_proxy(Some(no_proxy));
                    }
                }
                return crate::services::outbound_security::build_public_http_client_via_proxy(
                    url, timeout, user_agent, proxy,
                )
                .await;
            }
        }
        crate::services::outbound_security::build_public_http_client(url, timeout, user_agent).await
    }

    /// 执行 HTTP API
    async fn execute_http_api(
        api_def: &TappApiDef,
        context: &HashMap<String, Value>,
    ) -> Result<Value, String> {
        Self::execute_http_api_with_credential(api_def, context, None).await
    }

    async fn execute_http_api_with_credential(
        api_def: &TappApiDef,
        context: &HashMap<String, Value>,
        credential: Option<&crate::services::tapp_credentials::ResolvedApiCredential>,
    ) -> Result<Value, String> {
        let prepared = Self::prepare_http_request(api_def, context, credential)?;

        // 构建请求
        let method = reqwest::Method::from_str(&api_def.method.to_uppercase())
            .map_err(|_| format!("Invalid method: {}", api_def.method))?;
        let (target_url, client) = Self::declared_api_http_client(&prepared.url)
            .await
            .map_err(|error| Self::redact_needles(&error, &prepared.redaction_needles))?;
        let mut request = client.request(method, target_url);

        // 应用区域伪装（如果配置了 spoof 参数）
        if let Some(spoof_region) = &api_def.spoof {
            let spoof_config = SpoofConfig::new(spoof_region);
            let spoof_headers = generate_spoof_headers(&spoof_config);

            // 应用伪装请求头
            let mut header_map = HeaderMap::new();
            spoof_headers.apply_to(&mut header_map);

            for (name, value) in header_map.iter() {
                request = request.header(name.clone(), value.clone());
            }

            tracing::debug!("Applied spoof headers for region '{spoof_region}'");
        }

        // 添加用户定义的请求头（会覆盖伪装头）
        if let Some(headers) = &api_def.headers {
            for (key, value) in headers {
                let resolved_value = Self::resolve_template(value, context);
                let name = HeaderName::from_str(key)
                    .map_err(|_| format!("Invalid HTTP header name: {key}"))?;
                crate::services::outbound_security::validate_outbound_header(&name)?;
                let value = HeaderValue::from_str(&resolved_value)
                    .map_err(|_| format!("Invalid HTTP header value: {key}"))?;
                request = request.header(name, value);
            }
        }

        if let Some((name, value)) = prepared.secret_header {
            request = request.header(name, value);
        }

        // Serialize once, enforce the cap on the final bytes, and send those exact
        // bytes. This keeps payload hashing/signing aligned with the wire body.
        if let Some(encoded) = prepared.body {
            let has_declared_content_type = api_def.headers.as_ref().is_some_and(|headers| {
                headers
                    .keys()
                    .any(|name| name.eq_ignore_ascii_case(CONTENT_TYPE.as_str()))
            });
            if !has_declared_content_type {
                if let Some(content_type) = encoded.default_content_type {
                    request = request.header(CONTENT_TYPE, content_type);
                }
            }
            request = request.body(encoded.bytes);
        }

        // 发送请求
        let response = request.send().await.map_err(|error| {
            Self::redact_needles(
                &format!("HTTP request failed: {error}"),
                &prepared.redaction_needles,
            )
        })?;

        let status = response.status();
        let body = crate::services::outbound_security::read_limited_body(
            response,
            MAX_TAPP_HTTP_RESPONSE_BYTES,
        )
        .await?;
        let mut body = String::from_utf8_lossy(&body).into_owned();
        for secret in &prepared.redaction_needles {
            if !secret.is_empty() {
                body = body.replace(secret, "[REDACTED]");
            }
        }

        // Parse after the raw-text pass, then redact the parsed tree as well.
        // JSON escaping can hide the literal byte sequence (for example a
        // quote in the secret becomes `\"` on the wire), so text replacement
        // alone is not a complete exact-secret reflection guard.
        let parsed = serde_json::from_str::<Value>(&body);
        let is_json = parsed.is_ok();
        let mut data = parsed.unwrap_or_else(|_| json!({ "text": body }));
        for secret in &prepared.redaction_needles {
            Self::redact_secret_from_json(&mut data, secret);
        }

        if status.is_success() {
            Ok(data)
        } else {
            let safe_body = if is_json {
                serde_json::to_string(&data).unwrap_or_else(|_| "[REDACTED]".to_string())
            } else {
                body
            };
            Err(format!("HTTP {} - {}", status.as_u16(), safe_body))
        }
    }

    fn redact_needles(text: &str, needles: &[String]) -> String {
        let mut redacted = text.to_string();
        for secret in needles {
            if !secret.is_empty() {
                redacted = redacted.replace(secret, "[REDACTED]");
            }
        }
        redacted
    }

    fn redact_secret_from_json(value: &mut Value, secret: &str) {
        if secret.is_empty() {
            return;
        }
        match value {
            Value::String(text) => {
                *text = text.replace(secret, "[REDACTED]");
            }
            Value::Array(items) => {
                for item in items {
                    Self::redact_secret_from_json(item, secret);
                }
            }
            Value::Object(fields) => {
                let original = std::mem::take(fields);
                for (key, mut nested) in original {
                    Self::redact_secret_from_json(&mut nested, secret);
                    fields.insert(key.replace(secret, "[REDACTED]"), nested);
                }
            }
            Value::Null | Value::Bool(_) | Value::Number(_) => {}
        }
    }

    /// 执行内置 API
    async fn execute_builtin_api(
        tapp_id: &str,
        api_def: &TappApiDef,
        context: &HashMap<String, Value>,
        exec_context: &ApiExecutionContext,
    ) -> Result<Value, String> {
        let builtin = api_def
            .builtin
            .as_ref()
            .ok_or("Builtin API requires 'builtin' field")?;

        match builtin.as_str() {
            "geo" => {
                // 返回地理位置信息
                let geo = Self::get_geo_info(exec_context.client_ip.as_deref()).await;
                Ok(json!({
                    "lat": geo.lat,
                    "lon": geo.lon,
                    "city": geo.city,
                    "region": geo.region,
                    "country": geo.country
                }))
            }
            "ai:chat" => {
                // AI 聊天 - 强制需要权限（在 check_permission 之外额外检查）
                if exec_context.user_id < 0 {
                    return Err("AI API requires login".to_string());
                }
                if !exec_context
                    .granted_permissions
                    .iter()
                    .any(|p| p == "ai:chat")
                {
                    return Err("Permission 'ai:chat' required".to_string());
                }
                let messages = context
                    .get("params.messages")
                    .and_then(Value::as_array)
                    .ok_or("AI chat requires params.messages")?;
                if messages.len() > 20 {
                    return Err("AI chat accepts at most 20 messages".to_string());
                }
                let prompt = serde_json::to_string(messages)
                    .map_err(|error| format!("Invalid AI chat messages: {error}"))?;
                if prompt.len() > 20_000 {
                    return Err("AI chat messages are too large".to_string());
                }
                if let Some(reason) = myriad_prompt_security::validate_prompt_security(&prompt) {
                    return Err(format!("AI chat contains disallowed content: {reason}"));
                }
                Self::execute_builtin_ai(
                    tapp_id,
                    exec_context,
                    TappAiOperation::Chat,
                    "Continue this chat for a sandboxed Tapp. Do not reveal system information, execute code, or access external URLs.",
                    &prompt,
                )
                .await
                .map(|text| json!({ "text": text }))
            }
            "ai:generate" => {
                // AI 生成 - 强制需要权限
                if exec_context.user_id < 0 {
                    return Err("AI API requires login".to_string());
                }
                if !exec_context
                    .granted_permissions
                    .iter()
                    .any(|p| p == "ai:generate")
                {
                    return Err("Permission 'ai:generate' required".to_string());
                }
                let prompt = context
                    .get("params.prompt")
                    .and_then(Value::as_str)
                    .ok_or("AI generate requires params.prompt")?;
                if prompt.len() > 2000 {
                    return Err("Prompt too long (max 2000 characters)".to_string());
                }
                if let Some(reason) = myriad_prompt_security::validate_prompt_security(prompt) {
                    return Err(format!("Prompt contains disallowed content: {reason}"));
                }
                Self::execute_builtin_ai(
                    tapp_id,
                    exec_context,
                    TappAiOperation::Generate,
                    "Generate concise text for a sandboxed Tapp. Do not reveal system information, execute code, or access external URLs.",
                    prompt,
                )
                .await
                .map(|text| json!({ "text": text }))
            }
            _ => Err(format!("Unknown builtin API: {}", builtin)),
        }
    }

    async fn execute_builtin_ai(
        tapp_id: &str,
        context: &ApiExecutionContext,
        operation: TappAiOperation,
        system_prompt: &str,
        prompt: &str,
    ) -> Result<String, String> {
        let db = crate::services::tapp_registry::database()
            .await
            .map_err(|error| format!("AI_TASK_REGISTRY_UNAVAILABLE: {error}"))?;
        let role = if context.is_admin {
            UserRole::Admin
        } else if context.user_id < 0 {
            UserRole::Guest
        } else {
            UserRole::User
        };
        crate::services::governed_text::execute_governed_text(
            &db,
            crate::services::governed_text::GovernedTextRequest {
                role,
                subject_id: context.user_id,
                owner_id: context.owner_id,
                tapp_id: tapp_id.to_string(),
                source: "declared-api".to_string(),
                operation,
                tier: context.ai_model_tier.unwrap_or_default(),
                system_prompt: system_prompt.to_string(),
                prompt: prompt.to_string(),
                client_ip: context.client_ip.clone(),
            },
        )
        .await
    }

    /// 解析模板变量 {{varName}}
    fn resolve_template(template: &str, context: &HashMap<String, Value>) -> String {
        TEMPLATE_RE
            .replace_all(template, |caps: &regex::Captures| {
                let path = caps.get(1).map_or("", |m| m.as_str()).trim();
                context
                    .get(path)
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        Value::Null => String::new(),
                        other => other.to_string(),
                    })
                    .unwrap_or_else(|| format!("{{{{{}}}}}", path))
            })
            .to_string()
    }

    /// Resolve a raw body without normalizing or trimming any literal bytes.
    /// A body that is exactly one template must resolve to a string; templates
    /// embedded in a larger string stringify scalar/JSON values in place.
    fn resolve_raw_template(
        template: &str,
        context: &HashMap<String, Value>,
    ) -> Result<String, String> {
        if let Some(captures) = TEMPLATE_RE.captures(template) {
            let whole = captures.get(0).expect("template regex has a full match");
            if whole.start() == 0 && whole.end() == template.len() {
                let path = captures.get(1).map_or("", |value| value.as_str()).trim();
                let value = context
                    .get(path)
                    .ok_or_else(|| format!("raw body template is unresolved: {path}"))?;
                return value
                    .as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| "raw body must resolve to a string".to_string());
            }
        }

        let mut resolved = String::with_capacity(template.len());
        let mut previous_end = 0;
        for captures in TEMPLATE_RE.captures_iter(template) {
            let whole = captures.get(0).expect("template regex has a full match");
            resolved.push_str(&template[previous_end..whole.start()]);
            let path = captures.get(1).map_or("", |value| value.as_str()).trim();
            let value = context
                .get(path)
                .ok_or_else(|| format!("raw body template is unresolved: {path}"))?;
            match value {
                Value::String(value) => resolved.push_str(value),
                Value::Null => {}
                other => resolved.push_str(&other.to_string()),
            }
            previous_end = whole.end();
        }
        resolved.push_str(&template[previous_end..]);
        Ok(resolved)
    }

    /// 解析 JSON 中的模板变量
    fn resolve_json_templates(value: &Value, context: &HashMap<String, Value>) -> Value {
        match value {
            Value::String(s) => {
                let trimmed = s.trim();
                if trimmed.starts_with("{{")
                    && trimmed.ends_with("}}")
                    && trimmed.matches("{{").count() == 1
                {
                    let path = &trimmed[2..trimmed.len() - 2].trim();
                    if let Some(val) = context.get(*path) {
                        return val.clone();
                    }
                }
                Value::String(Self::resolve_template(s, context))
            }
            Value::Object(map) => {
                let new_map: serde_json::Map<String, Value> = map
                    .iter()
                    .map(|(k, v)| (k.clone(), Self::resolve_json_templates(v, context)))
                    .collect();
                Value::Object(new_map)
            }
            Value::Array(arr) => Value::Array(
                arr.iter()
                    .map(|v| Self::resolve_json_templates(v, context))
                    .collect(),
            ),
            other => other.clone(),
        }
    }

    fn prepare_http_request(
        api_def: &TappApiDef,
        context: &HashMap<String, Value>,
        credential: Option<&crate::services::tapp_credentials::ResolvedApiCredential>,
    ) -> Result<PreparedHttpRequest, String> {
        let base_url = api_def
            .endpoint
            .as_ref()
            .ok_or("HTTP API requires endpoint")?;
        let mut url = Self::resolve_template(base_url, context);
        let mut object_body = Self::resolved_object_body(api_def, context)?;
        let mut secret_header = None;
        let mut redaction_needles = Vec::new();

        if let Some(binding) = &api_def.credential {
            let credential = credential.ok_or_else(|| {
                "Required Tapp credential was not resolved by the host".to_string()
            })?;
            let resolved = binding.resolve()?;
            let presented = present_credential_value(credential.value(), resolved.encoding)?;
            redaction_needles.extend(credential_redaction_needles(
                credential.value(),
                &presented,
                resolved.prefix.as_deref(),
            ));

            match resolved.placement {
                TappCredentialIn::Header => {
                    let name = HeaderName::from_str(&resolved.field)
                        .map_err(|_| "Invalid credential header".to_string())?;
                    crate::services::outbound_security::validate_outbound_header(&name)?;
                    let mut header_value = resolved.prefix.clone().unwrap_or_default();
                    header_value.push_str(&presented);
                    let value = HeaderValue::from_str(&header_value)
                        .map_err(|_| "Invalid credential header value".to_string())?;
                    secret_header = Some((name, value));
                }
                TappCredentialIn::Query => {
                    url = append_credential_query(&url, &resolved.field, &presented)?;
                }
                TappCredentialIn::Form => {
                    let fields = object_body
                        .as_mut()
                        .ok_or_else(|| "form credentials require a form object body".to_string())?;
                    if fields.contains_key(&resolved.field) {
                        return Err("credential form field is already present".into());
                    }
                    fields.insert(resolved.field, Value::String(presented));
                }
                TappCredentialIn::Sign => {
                    let sign = resolved
                        .sign
                        .as_ref()
                        .ok_or_else(|| "signed credential is missing a sign block".to_string())?;
                    let fields = object_body.as_mut().ok_or_else(|| {
                        "signed credentials require a JSON or form object body".to_string()
                    })?;
                    apply_body_signature(fields, &resolved.field, sign, credential.value())?;
                }
            }
        }

        let encoded_body = match object_body {
            Some(fields) => Some(Self::encode_resolved_object_body(
                api_def,
                Value::Object(fields),
            )?),
            None => Self::encode_http_body(api_def, context)?,
        };

        Ok(PreparedHttpRequest {
            url,
            body: encoded_body,
            secret_header,
            redaction_needles,
        })
    }

    fn resolved_object_body(
        api_def: &TappApiDef,
        context: &HashMap<String, Value>,
    ) -> Result<Option<serde_json::Map<String, Value>>, String> {
        let needs_object = api_def.credential.as_ref().is_some_and(|binding| {
            binding.resolve().ok().is_some_and(|resolved| {
                matches!(
                    resolved.placement,
                    TappCredentialIn::Form | TappCredentialIn::Sign
                )
            })
        });
        if !needs_object {
            return Ok(None);
        }
        let Some(body) = &api_def.body else {
            return Err("signed or form credentials require a declared object body".into());
        };
        let resolved = Self::resolve_json_templates(body, context);
        resolved
            .as_object()
            .cloned()
            .ok_or_else(|| "signed or form credentials require an object body".into())
            .map(Some)
    }

    fn encode_resolved_object_body(
        api_def: &TappApiDef,
        body: Value,
    ) -> Result<EncodedHttpBody, String> {
        if !HTTP_BODY_METHODS.contains(&api_def.method.as_str()) {
            return Err(format!(
                "signed or form credentials require one of: {}",
                HTTP_BODY_METHODS.join(", ")
            ));
        }
        let (bytes, default_content_type) = match api_def.body_mode {
            TappHttpBodyMode::Json => (
                serde_json::to_vec(&body)
                    .map_err(|error| format!("Failed to serialize JSON body: {error}"))?,
                Some("application/json"),
            ),
            TappHttpBodyMode::Form => {
                let fields = body
                    .as_object()
                    .ok_or_else(|| "form body must resolve to an object".to_string())?;
                let mut serializer = url::form_urlencoded::Serializer::new(String::new());
                for (name, value) in fields {
                    serializer.append_pair(name, &scalar_to_sign_string(name, value)?);
                }
                (
                    serializer.finish().into_bytes(),
                    Some("application/x-www-form-urlencoded"),
                )
            }
            TappHttpBodyMode::Raw => {
                return Err("signed or form credentials cannot use bodyMode raw".into());
            }
        };
        if api_def.body_mode != TappHttpBodyMode::Json
            && bytes.len() > MAX_TAPP_NON_JSON_HTTP_REQUEST_BYTES
        {
            return Err(format!(
                "non-JSON HTTP request body exceeds {MAX_TAPP_NON_JSON_HTTP_REQUEST_BYTES} bytes"
            ));
        }
        Ok(EncodedHttpBody {
            bytes,
            default_content_type,
        })
    }

    fn encode_http_body(
        api_def: &TappApiDef,
        context: &HashMap<String, Value>,
    ) -> Result<Option<EncodedHttpBody>, String> {
        if api_def.body_mode != TappHttpBodyMode::Json
            && !HTTP_BODY_METHODS.contains(&api_def.method.as_str())
        {
            return Err(format!(
                "HTTP bodyMode {} requires one of: {}",
                match api_def.body_mode {
                    TappHttpBodyMode::Json => unreachable!("checked above"),
                    TappHttpBodyMode::Raw => "raw",
                    TappHttpBodyMode::Form => "form",
                },
                HTTP_BODY_METHODS.join(", ")
            ));
        }

        let Some(body) = &api_def.body else {
            return match api_def.body_mode {
                TappHttpBodyMode::Json => Ok(None),
                TappHttpBodyMode::Raw => Err("raw body must be declared".to_string()),
                TappHttpBodyMode::Form => Err("form body must be declared".to_string()),
            };
        };
        let (bytes, default_content_type) = match api_def.body_mode {
            TappHttpBodyMode::Json => {
                let resolved_body = Self::resolve_json_templates(body, context);
                (
                    serde_json::to_vec(&resolved_body)
                        .map_err(|error| format!("Failed to serialize JSON body: {error}"))?,
                    Some("application/json"),
                )
            }
            TappHttpBodyMode::Raw => {
                let template = body
                    .as_str()
                    .ok_or_else(|| "raw body must be declared as a string".to_string())?;
                let value = Self::resolve_raw_template(template, context)?;
                (value.as_bytes().to_vec(), None)
            }
            TappHttpBodyMode::Form => {
                let resolved_body = Self::resolve_json_templates(body, context);
                let fields = resolved_body
                    .as_object()
                    .ok_or_else(|| "form body must resolve to an object".to_string())?;
                let mut serializer = url::form_urlencoded::Serializer::new(String::new());
                for (name, value) in fields {
                    let value = match value {
                        Value::String(value) => value.clone(),
                        Value::Number(value) => value.to_string(),
                        Value::Bool(value) => value.to_string(),
                        Value::Null => String::new(),
                        Value::Array(_) | Value::Object(_) => {
                            return Err(format!(
                                "form body field {name} must resolve to a scalar value"
                            ));
                        }
                    };
                    serializer.append_pair(name, &value);
                }
                (
                    serializer.finish().into_bytes(),
                    Some("application/x-www-form-urlencoded"),
                )
            }
        };

        if api_def.body_mode != TappHttpBodyMode::Json
            && bytes.len() > MAX_TAPP_NON_JSON_HTTP_REQUEST_BYTES
        {
            return Err(format!(
                "non-JSON HTTP request body exceeds {MAX_TAPP_NON_JSON_HTTP_REQUEST_BYTES} bytes"
            ));
        }

        Ok(Some(EncodedHttpBody {
            bytes,
            default_content_type,
        }))
    }

    /// 生成缓存 key
    fn generate_cache_key(
        tapp_id: &str,
        api_name: &str,
        api_def: &TappApiDef,
        params: &Option<Value>,
        context: &ApiExecutionContext,
    ) -> String {
        let params_hash = params
            .as_ref()
            .map(|params| hex::encode(Sha256::digest(params.to_string().as_bytes())))
            .unwrap_or_else(|| "none".to_string());
        let ip_hash = context
            .client_ip
            .as_ref()
            .map(|ip| hex::encode(Sha256::digest(ip.as_bytes())))
            .unwrap_or_else(|| "none".to_string());
        let username_hash = hex::encode(Sha256::digest(context.username.as_bytes()));
        let definition = serde_json::to_vec(api_def).unwrap_or_default();
        let definition_hash = hex::encode(Sha256::digest(definition));
        let credential_revision = context
            .credential
            .as_ref()
            .map(|credential| credential.revision())
            .unwrap_or("none");
        let settings_hash = if context.settings.is_empty() {
            "none".to_string()
        } else {
            let encoded = serde_json::to_vec(&context.settings).unwrap_or_default();
            hex::encode(Sha256::digest(encoded))
        };
        format!(
            "tapp_api:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
            tapp_id,
            context.owner_id,
            context.user_id,
            username_hash,
            context.is_admin,
            ip_hash,
            api_name,
            definition_hash,
            credential_revision,
            settings_hash,
            params_hash
        )
    }

    /// 获取缓存
    async fn get_cached(key: &str) -> Option<Value> {
        let cache = API_CACHE.read().await;
        cache.get(key).and_then(|entry| {
            if entry.expires_at > Instant::now() {
                Some(entry.data.clone())
            } else {
                None
            }
        })
    }

    /// 设置缓存
    async fn set_cached(key: &str, data: &Value, ttl: u32) {
        let mut cache = API_CACHE.write().await;
        let now = Instant::now();
        cache.retain(|_, entry| entry.expires_at > now);
        let api_cap = crate::services::memory_profile::max_api_cache_entries();
        let api_bytes_cap = crate::services::memory_profile::max_api_cache_bytes();
        let size_bytes = approx_json_bytes(data);
        // Skip caching absurd single values that alone exceed the byte budget.
        if size_bytes > api_bytes_cap {
            return;
        }
        // Replace: drop old entry first so size accounting is accurate.
        cache.remove(key);
        while cache.len() >= api_cap
            || cache.values().map(|e| e.size_bytes).sum::<usize>() + size_bytes > api_bytes_cap
        {
            let Some(oldest) = cache
                .iter()
                .min_by_key(|(_, entry)| entry.cached_at)
                .map(|(k, _)| k.clone())
            else {
                break;
            };
            cache.remove(&oldest);
        }
        cache.insert(
            key.to_string(),
            CacheEntry {
                data: data.clone(),
                size_bytes,
                expires_at: now + Duration::from_secs(ttl as u64),
                cached_at: now,
            },
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn api_def() -> TappApiDef {
        TappApiDef {
            access: TappApiAccess::Protected,
            api_type: "http".to_string(),
            endpoint: Some("https://example.com".to_string()),
            method: "GET".to_string(),
            headers: None,
            credential: None,
            body_mode: TappHttpBodyMode::Json,
            body: None,
            builtin: None,
            inject: None,
            cache_ttl: 0,
            spoof: None,
            description: None,
            route: None,
        }
    }

    fn context(user_id: i32, ip: &str) -> ApiExecutionContext {
        ApiExecutionContext {
            user_id,
            owner_id: user_id,
            username: format!("user-{user_id}"),
            is_admin: false,
            client_ip: Some(ip.to_string()),
            granted_permissions: Vec::new(),
            ai_model_tier: None,
            credential: None,
            settings: std::collections::BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn public_http_api_still_requires_network_permission() {
        let mut api = api_def();
        api.access = TappApiAccess::Public;
        let mut execution_context = context(1, "203.0.113.1");

        assert!(TappApiService::check_permission(&api, &execution_context)
            .await
            .is_err());

        execution_context
            .granted_permissions
            .push("network:fetch".to_string());
        assert!(TappApiService::check_permission(&api, &execution_context)
            .await
            .is_ok());
    }

    #[test]
    fn declared_api_cache_isolated_by_user_and_client_context() {
        let params = Some(json!({ "query": "same" }));
        let first = TappApiService::generate_cache_key(
            "com.example.app",
            "profile",
            &api_def(),
            &params,
            &context(1, "203.0.113.1"),
        );
        let other_user = TappApiService::generate_cache_key(
            "com.example.app",
            "profile",
            &api_def(),
            &params,
            &context(2, "203.0.113.1"),
        );
        let other_ip = TappApiService::generate_cache_key(
            "com.example.app",
            "profile",
            &api_def(),
            &params,
            &context(1, "203.0.113.2"),
        );

        assert_ne!(first, other_user);
        assert_ne!(first, other_ip);
    }

    #[test]
    fn declared_api_cache_changes_with_owner_and_definition() {
        let params = Some(json!({ "query": "same" }));
        let base = api_def();
        let first_context = context(1, "203.0.113.1");
        let first = TappApiService::generate_cache_key(
            "com.example.app",
            "profile",
            &base,
            &params,
            &first_context,
        );

        let mut other_owner = first_context.clone();
        other_owner.owner_id = 99;
        let owner_key = TappApiService::generate_cache_key(
            "com.example.app",
            "profile",
            &base,
            &params,
            &other_owner,
        );

        let mut elevated = first_context.clone();
        elevated.is_admin = true;
        let role_key = TappApiService::generate_cache_key(
            "com.example.app",
            "profile",
            &base,
            &params,
            &elevated,
        );

        let mut changed = base.clone();
        changed.endpoint = Some("https://changed.example".to_string());
        let definition_key = TappApiService::generate_cache_key(
            "com.example.app",
            "profile",
            &changed,
            &params,
            &first_context,
        );

        assert_ne!(first, owner_key);
        assert_ne!(first, role_key);
        assert_ne!(first, definition_key);
    }

    #[test]
    fn credential_redaction_handles_json_escaped_values_keys_and_nesting() {
        let secret = "top\"secret\\tail";
        let mut fields = serde_json::Map::new();
        fields.insert(
            format!("header-{secret}"),
            json!({ "nested": [format!("before-{secret}-after")] }),
        );
        let wire_body = Value::Object(fields).to_string();
        assert!(
            !wire_body.contains(secret),
            "the regression requires JSON escaping to hide the raw sequence"
        );

        let mut parsed: Value = serde_json::from_str(&wire_body).unwrap();
        TappApiService::redact_secret_from_json(&mut parsed, secret);

        assert_eq!(
            parsed["header-[REDACTED]"]["nested"][0],
            "before-[REDACTED]-after"
        );
        assert!(!parsed.to_string().contains("top\\\"secret"));
    }

    #[test]
    fn declared_api_inject_aliases_preserve_host_value_types() {
        let mut values = HashMap::from([
            ("geo.city".to_string(), json!("Tokyo")),
            ("user.id".to_string(), json!(42)),
        ]);
        let aliases = HashMap::from([
            ("city".to_string(), "{{geo.city}}".to_string()),
            ("viewerId".to_string(), "{{user.id}}".to_string()),
            (
                "label".to_string(),
                "city={{geo.city}} user={{user.id}}".to_string(),
            ),
        ]);

        TappApiService::apply_inject_aliases(Some(&aliases), &mut values);

        assert_eq!(values.get("city"), Some(&json!("Tokyo")));
        assert_eq!(values.get("viewerId"), Some(&json!(42)));
        assert_eq!(values.get("label"), Some(&json!("city=Tokyo user=42")));
    }

    #[test]
    fn declared_api_scans_every_templated_http_surface() {
        let mut api = api_def();
        api.endpoint = Some("https://example.com".to_string());
        api.headers = Some(HashMap::from([(
            "X-City".to_string(),
            "{{geo.city}}".to_string(),
        )]));
        api.body = Some(json!({ "token": "{{secrets.OPENWEATHER_KEY}}" }));

        assert!(TappApiService::api_uses_template_prefix(&api, "{{geo."));
        assert!(TappApiService::api_uses_template_prefix(&api, "{{secrets."));
        assert!(!TappApiService::api_uses_template_prefix(&api, "{{user."));
    }

    #[test]
    fn json_body_mode_preserves_existing_serialization() {
        let mut api = api_def();
        api.method = "POST".to_string();
        api.body = Some(json!({ "message": "hello\n世界" }));

        let encoded = TappApiService::encode_http_body(&api, &HashMap::new())
            .unwrap()
            .unwrap();

        assert_eq!(
            encoded.bytes,
            serde_json::to_vec(api.body.as_ref().unwrap()).unwrap()
        );
        assert_eq!(encoded.default_content_type, Some("application/json"));
    }

    #[test]
    fn raw_body_mode_preserves_exact_utf8_bytes() {
        let mut api = api_def();
        api.method = "POST".to_string();
        api.body_mode = TappHttpBodyMode::Raw;
        api.body = Some(json!("{{params.body}}"));
        let raw = "中文\n<InvalidationBatch>&\"\\</InvalidationBatch>";
        let context = HashMap::from([("params.body".to_string(), json!(raw))]);

        let encoded = TappApiService::encode_http_body(&api, &context)
            .unwrap()
            .unwrap();

        assert_eq!(encoded.bytes, raw.as_bytes());
        assert!(!encoded.bytes.ends_with(b"\n"));
        assert_eq!(encoded.default_content_type, None);
        assert_eq!(
            hex::encode(Sha256::digest(&encoded.bytes)),
            hex::encode(Sha256::digest(raw.as_bytes()))
        );
    }

    #[test]
    fn raw_body_mode_preserves_literal_whitespace_around_templates() {
        let mut api = api_def();
        api.method = "POST".to_string();
        api.body_mode = TappHttpBodyMode::Raw;
        api.body = Some(json!(" \n{{params.body}}\n "));
        let context = HashMap::from([("params.body".to_string(), json!("payload"))]);

        let encoded = TappApiService::encode_http_body(&api, &context)
            .unwrap()
            .unwrap();

        assert_eq!(encoded.bytes, b" \npayload\n ");
    }

    #[test]
    fn raw_body_mode_rejects_unresolved_templates() {
        let mut api = api_def();
        api.method = "POST".to_string();
        api.body_mode = TappHttpBodyMode::Raw;
        api.body = Some(json!("prefix={{params.missing}}"));

        let error = TappApiService::encode_http_body(&api, &HashMap::new()).unwrap_err();

        assert_eq!(error, "raw body template is unresolved: params.missing");
    }

    #[tokio::test]
    async fn runtime_rejects_non_json_body_modes_on_unsupported_methods_before_outbound() {
        for body_mode in [TappHttpBodyMode::Raw, TappHttpBodyMode::Form] {
            let mut api = api_def();
            api.endpoint = Some("https://invalid.invalid/should-not-resolve".to_string());
            api.method = "GET".to_string();
            api.body_mode = body_mode;
            api.body = Some(json!("payload"));

            let error = TappApiService::execute_http_api(&api, &HashMap::new())
                .await
                .unwrap_err();

            assert!(error.starts_with("HTTP bodyMode "));
            assert!(error.contains("requires one of: POST, PUT, PATCH, DELETE"));
        }
    }

    #[tokio::test]
    async fn credential_is_attached_only_as_host_header_and_redacted_from_response() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let _guard = crate::services::outbound_security::tests_lab_env_lock().await;
        let previous_environment = std::env::var("ENVIRONMENT").ok();
        let previous_lab_flag = std::env::var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND").ok();
        std::env::remove_var("ENVIRONMENT");
        std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "1");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 4096];
            loop {
                let read = stream.read(&mut chunk).await.unwrap();
                assert!(read > 0);
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|part| part == b"\r\n\r\n") {
                    break;
                }
            }
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 21\r\nConnection: close\r\n\r\n{\"echo\":\"top-secret\"}",
                )
                .await
                .unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });

        let mut api = api_def();
        api.endpoint = Some(format!("http://{address}/credential"));
        api.credential = Some(myriad_tapp_contract::manifest::TappApiCredentialBinding {
            key: "wegame".into(),
            in_placement: None,
            field: None,
            header: Some("Authorization".into()),
            prefix: Some("Bearer ".into()),
            encoding: None,
            sign: None,
        });
        let credential =
            crate::services::tapp_credentials::ResolvedApiCredential::for_test("top-secret");

        let response = TappApiService::execute_http_api_with_credential(
            &api,
            &HashMap::new(),
            Some(&credential),
        )
        .await
        .unwrap();
        let request = server.await.unwrap();

        assert!(request
            .to_ascii_lowercase()
            .contains("authorization: bearer top-secret"));
        assert_eq!(response, json!({ "echo": "[REDACTED]" }));

        match previous_environment {
            Some(value) => std::env::set_var("ENVIRONMENT", value),
            None => std::env::remove_var("ENVIRONMENT"),
        }
        match previous_lab_flag {
            Some(value) => std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", value),
            None => std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND"),
        }
    }

    #[tokio::test]
    async fn injects_settings_and_time_into_template_context() {
        let mut api = api_def();
        api.body = Some(json!({
            "user_id": "{{settings.userId}}",
            "ts": "{{time.unix}}"
        }));
        let mut execution_context = context(1, "203.0.113.1");
        execution_context
            .settings
            .insert("userId".into(), json!("abc"));
        let inject = TappApiService::build_inject_context(&api, &execution_context)
            .await
            .unwrap();
        assert_eq!(inject.get("settings.userId"), Some(&json!("abc")));
        assert!(inject.get("time.unix").and_then(Value::as_u64).is_some());
        assert!(inject.get("time.nonce").and_then(Value::as_str).is_some());

        let spaced =
            TappApiService::resolve_json_templates(&json!({ "ts": "{{ time.unix }}" }), &inject);
        assert!(spaced["ts"].as_u64().is_some());
    }

    #[test]
    fn md5_sorted_kv_matches_afdian_open_vector() {
        let mut fields = serde_json::Map::new();
        fields.insert("user_id".into(), json!("abc"));
        fields.insert("params".into(), Value::String(r#"{"a":333}"#.into()));
        fields.insert("ts".into(), json!(1_624_339_905_u64));
        apply_body_signature(
            &mut fields,
            "sign",
            &myriad_tapp_contract::manifest::TappCredentialSign {
                alg: TappCredentialSignAlg::Md5SortedKv,
                over: vec!["params".into(), "ts".into(), "user_id".into()],
                timestamp_field: None,
            },
            "123",
        )
        .unwrap();
        assert_eq!(
            fields.get("sign").and_then(Value::as_str),
            Some("a4acc28b81598b7e5d84ebdc3e91710c")
        );
    }

    #[test]
    fn hmac_sha256_raw_signs_sorted_kv_material() {
        let mut fields = serde_json::Map::new();
        fields.insert("user_id".into(), json!("abc"));
        fields.insert("params".into(), Value::String(r#"{"a":333}"#.into()));
        fields.insert("ts".into(), json!(1_624_339_905_u64));
        apply_body_signature(
            &mut fields,
            "sign",
            &myriad_tapp_contract::manifest::TappCredentialSign {
                alg: TappCredentialSignAlg::HmacSha256Raw,
                over: vec!["params".into(), "ts".into(), "user_id".into()],
                timestamp_field: None,
            },
            "123",
        )
        .unwrap();
        let expected = hex::encode(crate::services::tapp_hmac::hmac_sha256(
            b"123",
            b"params{\"a\":333}ts1624339905user_idabc",
        ));
        assert_eq!(
            fields.get("sign").and_then(Value::as_str),
            Some(expected.as_str())
        );
    }

    #[tokio::test]
    async fn query_credential_is_appended_and_redacted() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let _guard = crate::services::outbound_security::tests_lab_env_lock().await;
        let previous_environment = std::env::var("ENVIRONMENT").ok();
        let previous_lab_flag = std::env::var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND").ok();
        std::env::remove_var("ENVIRONMENT");
        std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "1");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 4096];
            loop {
                let read = stream.read(&mut chunk).await.unwrap();
                assert!(read > 0);
                request.extend_from_slice(&chunk[..read]);
                if request.windows(4).any(|part| part == b"\r\n\r\n") {
                    break;
                }
            }
            stream
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 21\r\nConnection: close\r\n\r\n{\"echo\":\"top-secret\"}",
                )
                .await
                .unwrap();
            String::from_utf8_lossy(&request).into_owned()
        });

        let mut api = api_def();
        api.endpoint = Some(format!("http://{address}/weather?q=tokyo"));
        api.credential = Some(myriad_tapp_contract::manifest::TappApiCredentialBinding {
            key: "owm".into(),
            in_placement: Some(TappCredentialIn::Query),
            field: Some("appid".into()),
            header: None,
            prefix: None,
            encoding: None,
            sign: None,
        });
        let credential =
            crate::services::tapp_credentials::ResolvedApiCredential::for_test("top-secret");

        let response = TappApiService::execute_http_api_with_credential(
            &api,
            &HashMap::new(),
            Some(&credential),
        )
        .await
        .unwrap();
        let request = server.await.unwrap();

        assert!(request.contains("appid=top-secret") || request.contains("appid=top-secret"));
        assert!(request.contains("/weather?q=tokyo"));
        assert_eq!(response, json!({ "echo": "[REDACTED]" }));

        match previous_environment {
            Some(value) => std::env::set_var("ENVIRONMENT", value),
            None => std::env::remove_var("ENVIRONMENT"),
        }
        match previous_lab_flag {
            Some(value) => std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", value),
            None => std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND"),
        }
    }

    #[tokio::test]
    async fn query_credential_connect_failure_redacts_secret_from_error() {
        let _guard = crate::services::outbound_security::tests_lab_env_lock().await;
        let previous_environment = std::env::var("ENVIRONMENT").ok();
        let previous_lab_flag = std::env::var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND").ok();
        std::env::remove_var("ENVIRONMENT");
        std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "1");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        drop(listener);

        let mut api = api_def();
        api.endpoint = Some(format!("http://{address}/weather?q=tokyo"));
        api.credential = Some(myriad_tapp_contract::manifest::TappApiCredentialBinding {
            key: "owm".into(),
            in_placement: Some(TappCredentialIn::Query),
            field: Some("appid".into()),
            header: None,
            prefix: None,
            encoding: None,
            sign: None,
        });
        let credential =
            crate::services::tapp_credentials::ResolvedApiCredential::for_test("top-secret");

        let error = TappApiService::execute_http_api_with_credential(
            &api,
            &HashMap::new(),
            Some(&credential),
        )
        .await
        .unwrap_err();

        assert!(error.contains("HTTP request failed"), "{error}");
        assert!(
            !error.contains("top-secret"),
            "sandbox error leaked query credential: {error}"
        );

        match previous_environment {
            Some(value) => std::env::set_var("ENVIRONMENT", value),
            None => std::env::remove_var("ENVIRONMENT"),
        }
        match previous_lab_flag {
            Some(value) => std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", value),
            None => std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND"),
        }
    }

    #[tokio::test]
    async fn raw_body_mode_sends_exact_bytes_on_the_wire() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        let _guard = crate::services::outbound_security::tests_lab_env_lock().await;
        let previous_environment = std::env::var("ENVIRONMENT").ok();
        let previous_lab_flag = std::env::var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND").ok();
        std::env::remove_var("ENVIRONMENT");
        std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "1");

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut chunk = [0_u8; 4096];
            loop {
                let read = stream.read(&mut chunk).await.unwrap();
                assert!(read > 0, "connection closed before request body completed");
                request.extend_from_slice(&chunk[..read]);
                let Some(header_end) = request.windows(4).position(|part| part == b"\r\n\r\n")
                else {
                    continue;
                };
                let header_end = header_end + 4;
                let headers = String::from_utf8_lossy(&request[..header_end]);
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap();
                if request.len() >= header_end + content_length {
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 2\r\nConnection: close\r\n\r\n{}",
                        )
                        .await
                        .unwrap();
                    return (
                        headers.into_owned(),
                        request[header_end..header_end + content_length].to_vec(),
                    );
                }
            }
        });

        let raw = "第一行\n中文 & <xml attr=\"value\">\\结束</xml>";
        let mut api = api_def();
        api.endpoint = Some(format!("http://{address}/submit"));
        api.method = "POST".to_string();
        api.body_mode = TappHttpBodyMode::Raw;
        api.headers = Some(HashMap::from([(
            "Content-Type".to_string(),
            "text/plain; charset=utf-8".to_string(),
        )]));
        api.body = Some(json!("{{params.body}}"));
        let context = HashMap::from([("params.body".to_string(), json!(raw))]);

        let result = TappApiService::execute_http_api(&api, &context).await;

        match previous_environment {
            Some(value) => std::env::set_var("ENVIRONMENT", value),
            None => std::env::remove_var("ENVIRONMENT"),
        }
        match previous_lab_flag {
            Some(value) => std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", value),
            None => std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND"),
        }

        if let Err(error) = result {
            server.abort();
            panic!("raw request failed: {error}");
        }
        let (headers, body) = server.await.unwrap();
        assert!(headers
            .lines()
            .any(|line| line.eq_ignore_ascii_case("content-type: text/plain; charset=utf-8")));
        assert_eq!(body, raw.as_bytes());
        assert!(!body.ends_with(b"\n"));
    }

    #[test]
    fn raw_body_mode_rejects_non_string_resolution() {
        let mut api = api_def();
        api.method = "POST".to_string();
        api.body_mode = TappHttpBodyMode::Raw;
        api.body = Some(json!("{{params.body}}"));
        let context = HashMap::from([("params.body".to_string(), json!({ "not": "raw" }))]);

        let error = TappApiService::encode_http_body(&api, &context).unwrap_err();

        assert_eq!(error, "raw body must resolve to a string");
    }

    #[test]
    fn form_body_mode_url_encodes_scalar_fields() {
        let mut api = api_def();
        api.method = "POST".to_string();
        api.body_mode = TappHttpBodyMode::Form;
        api.body = Some(json!({
            "grant_type": "client_credentials",
            "scope": "read write/中文",
            "enabled": true,
            "empty": null
        }));

        let encoded = TappApiService::encode_http_body(&api, &HashMap::new())
            .unwrap()
            .unwrap();
        let body = String::from_utf8(encoded.bytes).unwrap();
        let decoded: HashMap<String, String> = url::form_urlencoded::parse(body.as_bytes())
            .into_owned()
            .collect();

        assert_eq!(
            decoded.get("grant_type").map(String::as_str),
            Some("client_credentials")
        );
        assert_eq!(
            decoded.get("scope").map(String::as_str),
            Some("read write/中文")
        );
        assert_eq!(decoded.get("enabled").map(String::as_str), Some("true"));
        assert_eq!(decoded.get("empty").map(String::as_str), Some(""));
        assert!(body.contains("scope=read+write%2F"));
        assert_eq!(
            encoded.default_content_type,
            Some("application/x-www-form-urlencoded")
        );
    }

    #[test]
    fn form_body_mode_rejects_nested_values_after_resolution() {
        let mut api = api_def();
        api.method = "POST".to_string();
        api.body_mode = TappHttpBodyMode::Form;
        api.body = Some(json!({ "scope": "{{params.scope}}" }));
        let context = HashMap::from([("params.scope".to_string(), json!(["read", "write"]))]);

        let error = TappApiService::encode_http_body(&api, &context).unwrap_err();

        assert_eq!(
            error,
            "form body field scope must resolve to a scalar value"
        );
    }

    #[test]
    fn serialized_http_body_enforces_byte_limit() {
        let mut api = api_def();
        api.method = "POST".to_string();
        api.body_mode = TappHttpBodyMode::Raw;
        api.body = Some(json!("{{params.body}}"));
        let oversized = "界".repeat(MAX_TAPP_NON_JSON_HTTP_REQUEST_BYTES / 3 + 1);
        let context = HashMap::from([("params.body".to_string(), json!(oversized))]);

        let error = TappApiService::encode_http_body(&api, &context).unwrap_err();

        assert_eq!(
            error,
            format!(
                "non-JSON HTTP request body exceeds {MAX_TAPP_NON_JSON_HTTP_REQUEST_BYTES} bytes"
            )
        );
    }

    #[test]
    fn json_body_mode_remains_compatible_above_non_json_limit() {
        let mut api = api_def();
        api.method = "POST".to_string();
        api.body = Some(json!({
            "payload": "a".repeat(MAX_TAPP_NON_JSON_HTTP_REQUEST_BYTES + 1)
        }));

        let encoded = TappApiService::encode_http_body(&api, &HashMap::new())
            .unwrap()
            .unwrap();

        assert!(encoded.bytes.len() > MAX_TAPP_NON_JSON_HTTP_REQUEST_BYTES);
    }
}
