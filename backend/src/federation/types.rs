//! MFP 类型定义
//!
//! ActivityPub 兼容 + Myriad 扩展的联邦协议类型

#![allow(dead_code)]

use serde::{Deserialize, Serialize};

// ==================== ActivityPub 标准上下文 ====================

/// ActivityStreams 2.0 标准上下文 URL
pub const AS_CONTEXT: &str = "https://www.w3.org/ns/activitystreams";
/// W3C Security 上下文（HTTP Signatures / publicKey）
pub const SECURITY_CONTEXT: &str = "https://w3id.org/security/v1";
/// Myriad Federation Protocol 扩展上下文
pub const MFP_CONTEXT: &str = "https://myriad.dev/ns/v1";

/// ActivityPub Content-Type
pub const AP_CONTENT_TYPE: &str = "application/activity+json";
/// JSON-LD Content-Type
pub const LD_CONTENT_TYPE: &str =
    "application/ld+json; profile=\"https://www.w3.org/ns/activitystreams\"";

// ==================== Actor 相关类型 ====================

/// ActivityPub Actor 对象（AP 兼容 + MFP 扩展）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Actor {
    #[serde(rename = "@context")]
    pub context: serde_json::Value,
    #[serde(rename = "type")]
    pub actor_type: String,
    pub id: String,
    #[serde(rename = "preferredUsername")]
    pub preferred_username: String,
    pub name: Option<String>,
    pub summary: Option<String>,
    pub url: Option<String>,
    pub inbox: String,
    pub outbox: String,
    pub followers: String,
    pub following: String,
    #[serde(rename = "publicKey")]
    pub public_key: ActorPublicKey,
    pub icon: Option<MediaObject>,
    pub image: Option<MediaObject>,

    // MFP 扩展字段
    #[serde(
        rename = "myriad:instanceVersion",
        skip_serializing_if = "Option::is_none"
    )]
    pub mfp_instance_version: Option<String>,
    #[serde(
        rename = "myriad:tappCapabilities",
        skip_serializing_if = "Option::is_none"
    )]
    pub mfp_tapp_capabilities: Option<Vec<TappCapability>>,
    #[serde(rename = "myriad:channels", skip_serializing_if = "Option::is_none")]
    pub mfp_channels_url: Option<String>,
}

/// Actor 公钥
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActorPublicKey {
    pub id: String,
    pub owner: String,
    #[serde(rename = "publicKeyPem")]
    pub public_key_pem: String,
}

/// 媒体对象（头像/横幅）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaObject {
    #[serde(rename = "type")]
    pub media_type: String,
    #[serde(rename = "mediaType", skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub url: String,
}

/// MFP: Tapp 能力声明
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TappCapability {
    #[serde(rename = "tappId")]
    pub tapp_id: String,
    pub name: String,
    pub version: String,
    #[serde(rename = "channelTypes", skip_serializing_if = "Option::is_none")]
    pub channel_types: Option<Vec<String>>,
}

// ==================== Activity 相关类型 ====================

/// ActivityPub Activity（通用）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Activity {
    #[serde(rename = "@context")]
    pub context: serde_json::Value,
    #[serde(rename = "type")]
    pub activity_type: String,
    pub id: String,
    pub actor: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cc: Option<Vec<String>>,
    pub published: Option<String>,
    /// 可以是内嵌对象，也可以是 URL 字符串
    pub object: serde_json::Value,
    /// 某些 Activity 需要 target（如 Add）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<serde_json::Value>,
}

/// ActivityPub Collection（Outbox, Followers 等）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderedCollection {
    #[serde(rename = "@context")]
    pub context: serde_json::Value,
    #[serde(rename = "type")]
    pub collection_type: String,
    pub id: String,
    #[serde(rename = "totalItems")]
    pub total_items: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last: Option<String>,
}

/// 分页 Collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderedCollectionPage {
    #[serde(rename = "@context")]
    pub context: serde_json::Value,
    #[serde(rename = "type")]
    pub collection_type: String,
    pub id: String,
    #[serde(rename = "partOf")]
    pub part_of: String,
    #[serde(rename = "totalItems")]
    pub total_items: u64,
    #[serde(rename = "orderedItems")]
    pub ordered_items: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub next: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prev: Option<String>,
}

// ==================== WebFinger ====================

/// WebFinger 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFingerResponse {
    pub subject: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aliases: Option<Vec<String>>,
    pub links: Vec<WebFingerLink>,
}

/// WebFinger 链接
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebFingerLink {
    pub rel: String,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub link_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub template: Option<String>,
}

// ==================== NodeInfo ====================

/// NodeInfo 2.1 响应
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub version: String,
    pub software: NodeInfoSoftware,
    pub protocols: Vec<String>,
    pub usage: NodeInfoUsage,
    #[serde(rename = "openRegistrations")]
    pub open_registrations: bool,
    /// MFP 扩展：Myriad 特有能力
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<NodeInfoMetadata>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfoSoftware {
    pub name: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub repository: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub homepage: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfoUsage {
    pub users: NodeInfoUsers,
    #[serde(rename = "localPosts")]
    pub local_posts: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfoUsers {
    pub total: u64,
    #[serde(rename = "activeMonth")]
    pub active_month: u64,
    #[serde(rename = "activeHalfyear")]
    pub active_halfyear: u64,
}

/// NodeInfo 元数据（MFP 扩展）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfoMetadata {
    /// MFP 协议版本
    #[serde(rename = "mfpVersion", skip_serializing_if = "Option::is_none")]
    pub mfp_version: Option<String>,
    /// 实例安装的公开 Tapp 列表
    #[serde(rename = "tappCapabilities", skip_serializing_if = "Option::is_none")]
    pub tapp_capabilities: Option<Vec<TappCapability>>,
    /// 支持的 Channel 类型
    #[serde(rename = "channelTypes", skip_serializing_if = "Option::is_none")]
    pub channel_types: Option<Vec<String>>,
    /// 支持的 Room 功能
    #[serde(rename = "roomSupport", skip_serializing_if = "Option::is_none")]
    pub room_support: Option<bool>,
}

/// NodeInfo 发现文档
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfoWellKnown {
    pub links: Vec<NodeInfoWellKnownLink>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfoWellKnownLink {
    pub rel: String,
    pub href: String,
}

// ==================== MFP Channel 协议类型 ====================

/// Channel 状态
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelStatus {
    Pending,
    Accepted,
    Active,
    Closed,
    Rejected,
}

/// Channel 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChannelType {
    Text,
    FileTransfer,
    Rpc,
    DataExchange,
    Stream,
}

/// Channel 传输方式
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ChannelTransport {
    Http,
    Websocket,
}

/// Channel 属性
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelProperties {
    #[serde(rename = "maxMessageSize", skip_serializing_if = "Option::is_none")]
    pub max_message_size: Option<u64>,
    #[serde(rename = "supportedFormats", skip_serializing_if = "Option::is_none")]
    pub supported_formats: Option<Vec<String>>,
    #[serde(rename = "maxFileSize", skip_serializing_if = "Option::is_none")]
    pub max_file_size: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resumable: Option<bool>,
    #[serde(rename = "chunkSize", skip_serializing_if = "Option::is_none")]
    pub chunk_size: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub methods: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub schemas: Option<Vec<String>>,
    #[serde(rename = "streamTypes", skip_serializing_if = "Option::is_none")]
    pub stream_types: Option<Vec<String>>,
}

/// MFP ChannelOpen Activity（myriad:ChannelOpen）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelOpenObject {
    #[serde(rename = "type")]
    pub object_type: String, // "myriad:Channel"
    #[serde(rename = "tappId")]
    pub tapp_id: String,
    #[serde(rename = "channelType")]
    pub channel_type: ChannelType,
    pub protocol: String, // "mfp/1.0"
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<String>>,
    #[serde(
        rename = "transportPreference",
        skip_serializing_if = "Option::is_none"
    )]
    pub transport_preference: Option<Vec<ChannelTransport>>,
}

/// MFP ChannelMessage（myriad:ChannelMessage）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMessage {
    #[serde(rename = "type")]
    pub message_type: String, // "myriad:ChannelMessage"
    pub channel: String,
    pub from: String,
    pub payload: serde_json::Value,
    pub timestamp: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

// ==================== MFP Room 协议类型 ====================

/// Room 治理类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum GovernanceType {
    Owner,
    Democratic,
    Open,
}

/// Room 成员角色
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RoomRole {
    Owner,
    Admin,
    Member,
    Observer,
}

/// Room 邀请策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum InvitePolicy {
    AdminOnly,
    MemberInvite,
    Open,
}

/// Room 消息分发策略
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum DistributionStrategy {
    FanOut,
    Mesh,
}

// ==================== MFP Ring 协议类型 ====================

/// Ring 类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RingType {
    TappStore,
    BrewRecommend,
    LibraryExchange,
    InstanceDirectory,
}

/// Gossip 配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GossipConfig {
    pub fanout: u32,
    pub ttl: u32,
    pub interval: u64, // 秒
}

// ==================== 实例信任层级 ====================

/// 实例信任层级
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(i16)]
pub enum TrustLevel {
    Unknown = 0,
    Discovered = 1,
    Followed = 2,
    Trusted = 3,
    Federated = 4,
}

impl TrustLevel {
    pub fn from_i16(v: i16) -> Self {
        match v {
            0 => Self::Unknown,
            1 => Self::Discovered,
            2 => Self::Followed,
            3 => Self::Trusted,
            4 => Self::Federated,
            _ => Self::Unknown,
        }
    }
}

// ==================== 联邦内容发布可见性 ====================

/// 内容发布可见性
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Visibility {
    Public,
    Followers,
    Mentioned,
    Direct,
}

/// 内容类型（本地内容 → 联邦发布的映射）
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FederatedContentType {
    Report,
    BrewArticle,
    Library,
    Activity,
    Tapp,
    Dashboard,
}

// ==================== 辅助函数 ====================

/// 构造标准 ActivityPub + MFP 三重上下文
pub fn build_context() -> serde_json::Value {
    serde_json::json!([
        AS_CONTEXT,
        SECURITY_CONTEXT,
        {
            "myriad": MFP_CONTEXT,
            "tappCapabilities": "myriad:tappCapabilities",
            "channels": "myriad:channels",
            "instanceVersion": "myriad:instanceVersion"
        }
    ])
}

/// 构造简单 AP 上下文（不含 MFP 扩展）
pub fn build_ap_context() -> serde_json::Value {
    serde_json::json!([AS_CONTEXT, SECURITY_CONTEXT])
}

/// 生成唯一的 Activity ID
pub fn generate_activity_id(base_url: &str) -> String {
    let id = uuid::Uuid::new_v4();
    format!("{}/activities/{}", base_url, id)
}

/// 生成唯一的 Channel ID
pub fn generate_channel_id() -> String {
    format!("ch_{}", uuid::Uuid::new_v4())
}

/// 生成唯一的 Room ID
pub fn generate_room_id() -> String {
    format!("rm_{}", uuid::Uuid::new_v4())
}

/// 生成唯一的消息 ID
pub fn generate_message_id() -> String {
    format!("msg_{}", uuid::Uuid::new_v4())
}

/// 生成唯一的文件传输 ID
pub fn generate_transfer_id() -> String {
    format!("ft_{}", uuid::Uuid::new_v4())
}

/// 生成唯一的 Ring ID
pub fn generate_ring_id() -> String {
    format!("ring_{}", uuid::Uuid::new_v4())
}

/// 从 Actor URL 提取域名（仅允许 http/https scheme，拒绝含 userinfo 的 URL）
pub fn extract_domain(actor_url: &str) -> Option<String> {
    let parsed = url::Url::parse(actor_url).ok()?;
    // 仅允许 http/https
    if !matches!(parsed.scheme(), "http" | "https") {
        return None;
    }
    // 拒绝携带 userinfo 的 URL（如 http://evil@legitimate.com）
    if !parsed.username().is_empty() || parsed.password().is_some() {
        return None;
    }
    parsed.host_str().map(|h| h.to_string())
}

/// 构造 Actor URL
pub fn actor_url(base_url: &str, username: &str) -> String {
    format!("{}/users/{}", base_url, username)
}

/// 规范化 Actor URL：trim、去掉末尾斜杠、host 转小写（保留 path 大小写与非默认端口）。
///
/// 用于 `same_actor_url` 与 `local_username_from_actor_url` 等相等性判断，
/// 避免 trailing slash / Host 大小写差异导致路由或授权误判。
pub fn normalize_actor_url(raw: &str) -> String {
    let trimmed = raw.trim().trim_end_matches('/');
    if let Ok(url) = url::Url::parse(trimmed) {
        let host = url.host_str().unwrap_or("").to_ascii_lowercase();
        let path = url.path().trim_end_matches('/');
        let port = url.port().map(|p| format!(":{}", p)).unwrap_or_default();
        return format!("{}://{}{}{}", url.scheme(), host, port, path);
    }
    trimmed.to_string()
}

/// 比较 Actor URL 时忽略末尾斜杠与 host 大小写差异，避免把自己的地址当作远程对象。
pub fn same_actor_url(left: &str, right: &str) -> bool {
    normalize_actor_url(left) == normalize_actor_url(right)
}

/// Normalize an HTTP Signature `keyId` URL: host case, trailing slash on path,
/// **preserve fragment** (`#main-key`). Actor URL normalization drops fragments.
pub fn normalize_key_id(raw: &str) -> String {
    let trimmed = raw.trim();
    if let Ok(url) = url::Url::parse(trimmed) {
        let host = url.host_str().unwrap_or("").to_ascii_lowercase();
        let path = url.path().trim_end_matches('/');
        let port = url.port().map(|p| format!(":{}", p)).unwrap_or_default();
        let fragment = url
            .fragment()
            .map(|f| format!("#{}", f))
            .unwrap_or_default();
        return format!("{}://{}{}{}{}", url.scheme(), host, port, path, fragment);
    }
    trimmed.trim_end_matches('/').to_string()
}

/// Compare Signature keyId values (host case / trailing slash / fragment).
pub fn same_key_id(left: &str, right: &str) -> bool {
    normalize_key_id(left) == normalize_key_id(right)
}

/// 若 candidate 是本实例的 Actor URL（{base_url}/users/{username}），返回 username。
///
/// 用于把入站 Activity 的 to 字段路由到本地用户。规则与 `same_actor_url` 一致：
/// 忽略末尾斜杠与 host 大小写；host/scheme/port 必须与 base_url 匹配；
/// 多段路径与空用户名不匹配。
pub fn local_username_from_actor_url(base_url: &str, candidate: &str) -> Option<String> {
    let base_norm = normalize_actor_url(base_url);
    let cand_norm = normalize_actor_url(candidate);
    let prefix = format!("{}/users/", base_norm);
    let rest = cand_norm.strip_prefix(&prefix)?;
    if rest.is_empty() || rest.contains('/') {
        return None;
    }
    Some(rest.to_string())
}

/// 构造 Key ID
pub fn key_id(base_url: &str, username: &str) -> String {
    format!("{}/users/{}#main-key", base_url, username)
}

/// 构造 Inbox URL
pub fn inbox_url(base_url: &str, username: &str) -> String {
    format!("{}/users/{}/inbox", base_url, username)
}

/// 构造 Outbox URL
pub fn outbox_url(base_url: &str, username: &str) -> String {
    format!("{}/users/{}/outbox", base_url, username)
}

/// 构造 Followers URL
pub fn followers_url(base_url: &str, username: &str) -> String {
    format!("{}/users/{}/followers", base_url, username)
}

/// 构造 Following URL
pub fn following_url(base_url: &str, username: &str) -> String {
    format!("{}/users/{}/following", base_url, username)
}

/// 获取联邦协议使用的 base_url（从全局配置读取）
pub async fn get_base_url() -> String {
    let config = crate::GLOBAL_CONFIG.read().await;
    let base_url = config
        .base_url
        .clone()
        .or_else(|| config.frontend_url.clone())
        .unwrap_or_else(|| format!("http://{}:{}", config.server_host, config.server_port));
    base_url.trim_end_matches('/').to_string()
}

/// ISO 8601 当前时间字符串
pub fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

/// 通用的数据库错误转换为 HTTP 500 响应
pub fn db_err(e: sea_orm::DbErr) -> (axum::http::StatusCode, axum::Json<serde_json::Value>) {
    tracing::error!("[Federation] DB error: {}", e);
    (
        axum::http::StatusCode::INTERNAL_SERVER_ERROR,
        axum::Json(serde_json::json!({"error": "Database error"})),
    )
}

/// SSRF 防护：检查 URL 是否指向内网/保留地址
///
/// 阻止联邦模块请求 127.x / 10.x / 172.16-31.x / 192.168.x / [::1] / 169.254.x 等
pub fn is_internal_url(url_str: &str) -> bool {
    let parsed = match url::Url::parse(url_str) {
        Ok(u) => u,
        Err(_) => return true, // 无法解析的 URL 视为不安全
    };

    // 只允许 http/https
    if parsed.scheme() != "http" && parsed.scheme() != "https" {
        return true;
    }

    let host = match parsed.host_str() {
        Some(h) => h,
        None => return true,
    };

    // 检查 IP 地址
    if let Ok(ip) = host.parse::<std::net::IpAddr>() {
        return match ip {
            std::net::IpAddr::V4(v4) => {
                v4.is_loopback()           // 127.0.0.0/8
                || v4.is_private()         // 10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16
                || v4.is_link_local()      // 169.254.0.0/16
                || v4.is_unspecified()     // 0.0.0.0
                || v4.is_broadcast()       // 255.255.255.255
                || v4.is_documentation() // 192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24
            }
            std::net::IpAddr::V6(v6) => {
                // 检查 IPv4-mapped IPv6 地址 (::ffff:x.x.x.x)
                if let Some(mapped_v4) = v6.to_ipv4_mapped() {
                    return mapped_v4.is_loopback()
                        || mapped_v4.is_private()
                        || mapped_v4.is_link_local()
                        || mapped_v4.is_unspecified()
                        || mapped_v4.is_broadcast()
                        || mapped_v4.is_documentation();
                }
                v6.is_loopback()           // ::1
                || v6.is_unspecified()     // ::
                || v6.is_unique_local()    // fc00::/7 (ULA)
                || (v6.segments()[0] & 0xffc0) == 0xfe80 // fe80::/10 (link-local)
            }
        };
    }

    // 检查主机名
    let lower = host.to_lowercase();
    lower == "localhost"
        || lower.ends_with(".local")
        || lower.ends_with(".internal")
        || lower.ends_with(".arpa")
}

/// AP 公共地址
pub const AP_PUBLIC: &str = "https://www.w3.org/ns/activitystreams#Public";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_actor_url_host_case_and_trailing_slash() {
        assert_eq!(
            normalize_actor_url("https://Myriad.Example.COM/users/alice/"),
            "https://myriad.example.com/users/alice"
        );
        assert_eq!(
            normalize_actor_url("https://myriad.example.com/users/alice"),
            "https://myriad.example.com/users/alice"
        );
        // 非默认端口保留
        assert_eq!(
            normalize_actor_url("https://myriad.example.com:8443/users/alice/"),
            "https://myriad.example.com:8443/users/alice"
        );
    }

    #[test]
    fn same_actor_url_ignores_host_case_and_slash() {
        assert!(same_actor_url(
            "https://Myriad.Example.COM/users/alice/",
            "https://myriad.example.com/users/alice"
        ));
        assert!(!same_actor_url(
            "https://myriad.example.com/users/alice",
            "https://other.example.com/users/alice"
        ));
    }

    #[test]
    fn same_key_id_preserves_fragment_and_normalizes_host() {
        assert!(same_key_id(
            "https://Myriad.Example.COM/users/alice#main-key",
            "https://myriad.example.com/users/alice#main-key"
        ));
        assert!(same_key_id(
            "https://myriad.example.com/users/alice/#main-key",
            "https://myriad.example.com/users/alice#main-key"
        ));
        assert!(!same_key_id(
            "https://myriad.example.com/users/alice#main-key",
            "https://myriad.example.com/users/alice#other-key"
        ));
        assert!(!same_key_id(
            "https://myriad.example.com/users/alice#main-key",
            "https://evil.example.com/users/alice#main-key"
        ));
        // Actor normalization must not be used for keyId: it would drop the fragment
        assert_ne!(
            normalize_actor_url("https://a.example/users/x#main-key"),
            normalize_key_id("https://a.example/users/x#main-key")
        );
    }

    #[test]
    fn local_username_from_actor_url_matches_local_actors() {
        let base = "https://myriad.example.com";
        assert_eq!(
            local_username_from_actor_url(base, "https://myriad.example.com/users/alice"),
            Some("alice".to_string())
        );
        assert_eq!(
            local_username_from_actor_url(base, "https://myriad.example.com/users/alice/"),
            Some("alice".to_string())
        );
        // base_url 带末尾斜杠也能匹配
        assert_eq!(
            local_username_from_actor_url(
                "https://myriad.example.com/",
                "https://myriad.example.com/users/alice"
            ),
            Some("alice".to_string())
        );
        // host 大小写差异（与 same_actor_url 一致）
        assert_eq!(
            local_username_from_actor_url(base, "https://Myriad.Example.COM/users/alice"),
            Some("alice".to_string())
        );
        assert_eq!(
            local_username_from_actor_url(
                "https://Myriad.Example.COM",
                "https://myriad.example.com/users/Bob/"
            ),
            Some("Bob".to_string())
        );
    }

    #[test]
    fn local_username_from_actor_url_rejects_foreign_and_malformed() {
        let base = "https://myriad.example.com";
        // 其他实例
        assert_eq!(
            local_username_from_actor_url(base, "https://other.example.com/users/alice"),
            None
        );
        // AP Public 集合地址
        assert_eq!(local_username_from_actor_url(base, AP_PUBLIC), None);
        // 多段路径 / 空用户名
        assert_eq!(
            local_username_from_actor_url(base, "https://myriad.example.com/users/alice/inbox"),
            None
        );
        assert_eq!(
            local_username_from_actor_url(base, "https://myriad.example.com/users/"),
            None
        );
        // 前缀相似的恶意域名
        assert_eq!(
            local_username_from_actor_url(base, "https://myriad.example.com.evil.com/users/alice"),
            None
        );
        // 路径前缀欺骗
        assert_eq!(
            local_username_from_actor_url(base, "https://myriad.example.com/users/alice.evil"),
            Some("alice.evil".to_string()) // 单段用户名合法；非多段
        );
        // scheme 不同不算本站
        assert_eq!(
            local_username_from_actor_url(base, "http://myriad.example.com/users/alice"),
            None
        );
    }
}
