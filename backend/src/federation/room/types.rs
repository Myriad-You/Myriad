//! Room request/response types.
use serde::{Deserialize, Serialize};



// 请求/响应类型

/// 创建 Room 请求
#[derive(Debug, Deserialize)]
pub struct CreateRoomRequest {
    pub name: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    /// owner / democratic / open
    pub governance_type: Option<String>,
    /// admin-only / member-invite / open
    pub invite_policy: Option<String>,
    pub max_members: Option<i32>,
    pub is_public: Option<bool>,
    /// Optional structured game session bound to this room.
    pub game: Option<RoomGameConfig>,
}

/// Game metadata stored in `shared_data_config.game`.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RoomGameConfig {
    pub tapp_id: String,
    pub protocol: String,
    pub max_players: Option<i32>,
    pub max_message_bytes: Option<i32>,
}

/// 更新 Room 请求
#[derive(Debug, Deserialize)]
pub struct UpdateRoomRequest {
    pub name: Option<String>,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub invite_policy: Option<String>,
    pub max_members: Option<i32>,
    pub is_public: Option<bool>,
}

/// 邀请成员请求
#[derive(Debug, Deserialize)]
pub struct InviteMemberRequest {
    /// 远程 Actor URL 或本地用户名
    pub actor: String,
    /// member / admin / observer
    pub role: Option<String>,
}

/// Self-join open/public room (optional home for remote public rooms).
#[derive(Debug, Default, Deserialize)]
pub struct JoinRoomRequest {
    /// Home instance host[:port] when room is not local (or use `room_id@home` in path).
    pub home_server: Option<String>,
}

/// Unauthenticated public room metadata (only when `is_public = true`).
#[derive(Debug, Serialize, Deserialize)]
pub struct PublicRoomInfo {
    pub room_id: String,
    pub name: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub owner_actor: String,
    pub home_server: String,
    pub invite_policy: String,
    pub max_members: i32,
    pub is_public: bool,
    pub member_count: i64,
    /// Copied onto remote replicas so send/inbox can honor the home cap and type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub game: Option<RoomGameConfig>,
}

/// 发送 Room 消息请求
#[derive(Debug, Deserialize)]
pub struct SendRoomMessageRequest {
    pub message_type: Option<String>,
    pub payload: serde_json::Value,
    pub thread_id: Option<String>,
    pub reply_to: Option<String>,
    /// 是否使用 Room E2E 多方加密（需成员已完成密钥发布）
    #[serde(default)]
    pub encrypt: Option<bool>,
}

/// Room 概要
#[derive(Debug, Serialize)]
pub struct RoomSummary {
    pub room_id: String,
    pub name: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub owner_actor: String,
    pub governance_type: String,
    pub invite_policy: String,
    pub member_count: i64,
    pub max_members: i32,
    pub is_public: bool,
    pub my_role: Option<String>,
    /// active | pending (invite not yet accepted)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub my_membership_status: Option<String>,
    pub last_message_at: Option<String>,
    pub created_at: String,
    pub unread_count: i64,
}

/// Room 详情
#[derive(Debug, Serialize)]
pub struct RoomDetail {
    pub room_id: String,
    pub name: String,
    pub description: Option<String>,
    pub avatar_url: Option<String>,
    pub owner_actor: String,
    pub home_server: String,
    pub governance_type: String,
    pub governance_config: Option<serde_json::Value>,
    pub invite_policy: String,
    pub distribution_strategy: String,
    pub max_members: i32,
    pub is_public: bool,
    pub enabled_tapps: Option<serde_json::Value>,
    /// Includes `e2e.published_keys` (public keys only) for client E2E readiness UI.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shared_data_config: Option<serde_json::Value>,
    pub my_role: Option<String>,
    /// active | pending
    #[serde(skip_serializing_if = "Option::is_none")]
    pub my_membership_status: Option<String>,
    pub member_count: i64,
    pub created_at: String,
}

/// Room 成员
#[derive(Debug, Serialize)]
pub struct RoomMember {
    pub actor_url: String,
    pub is_local: bool,
    pub display_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    pub role: String,
    /// active | pending
    pub membership_status: String,
    pub joined_at: String,
    pub invited_by: Option<String>,
}

/// Room 消息条目
#[derive(Debug, Clone, Serialize)]
pub struct RoomMessageItem {
    pub message_id: String,
    pub sender_actor: String,
    pub message_type: String,
    pub payload: serde_json::Value,
    pub thread_id: Option<String>,
    pub reply_to: Option<String>,
    pub reactions: serde_json::Value,
    pub is_pinned: bool,
    pub is_encrypted: bool,
    pub created_at: String,
}

/// Room 附件库条目（群文件索引；不含 payload 字节）
#[derive(Debug, Clone, Serialize)]
pub struct RoomFileItem {
    /// Stable client key: message_id:transfer_id|filename or tr:transfer_id
    pub key: String,
    pub message_id: String,
    /// image | file
    pub kind: String,
    pub filename: String,
    pub size: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mime_type: Option<String>,
    pub sender_actor: String,
    pub created_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transfer_id: Option<String>,
    /// True when original message payload has inline data (bytes not returned here)
    pub has_inline: bool,
    /// ready | pending | missing
    pub status: String,
}

/// 群文件列表结果（含是否还有更早条目）
#[derive(Debug, Serialize)]
pub struct RoomFileListResult {
    pub files: Vec<RoomFileItem>,
    pub total: usize,
    pub has_more: bool,
}

/// 发送消息响应
#[derive(Debug, Serialize)]
pub struct SendRoomMessageResponse {
    pub success: bool,
    pub message_id: String,
    pub room_id: String,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_encrypted: bool,
    /// Outbound fan-out enqueue observability
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<crate::federation::delivery::DeliveryEnqueueInfo>,
}

/// 发起 Room E2E 密钥发布响应
#[derive(Debug, Serialize)]
pub struct RoomE2eKeyExchangeResponse {
    pub success: bool,
    pub room_id: String,
    pub public_key: String,
    pub algorithm: String,
    /// 当前已登记公钥的成员数（含自己）
    pub published_key_count: usize,
}

/// Pin/Unpin Room 消息请求
#[derive(Debug, Deserialize)]
pub struct PinRoomMessageRequest {
    pub pinned: bool,
}

/// Add a sticker to the room shared pack (`shared_data_config.stickers`).
/// Opt-in group share — any active member may publish their own images.
#[derive(Debug, Deserialize)]
pub struct AddRoomStickerRequest {
    /// data:image/*;base64,... (already client-compressed)
    pub data: String,
    pub name: Option<String>,
}

/// One entry in the room shared sticker pack.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomStickerItem {
    pub id: String,
    pub data: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub actor: String,
    pub created_at: String,
}

/// Response after add/remove on the room sticker pack.
#[derive(Debug, Serialize)]
pub struct RoomStickersResponse {
    pub success: bool,
    pub room_id: String,
    pub stickers: Vec<RoomStickerItem>,
}

