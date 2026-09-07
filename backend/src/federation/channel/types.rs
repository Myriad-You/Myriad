//! Channel request/response types.
use serde::{Deserialize, Serialize};

// 请求/响应类型

/// 创建 Channel 请求
#[derive(Debug, Deserialize)]
pub struct CreateChannelRequest {
    /// 远程 Actor URL 或 acct:user@domain / @user@domain / user@domain
    pub remote_actor: String,
    /// 通道类型: text, file-transfer, rpc, data-exchange, stream
    pub channel_type: Option<String>,
    /// 关联 Tapp ID
    pub tapp_id: Option<String>,
    /// 传输方式: http, websocket
    pub transport: Option<String>,
}

/// 发送消息请求
#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    /// 消息类型: text, file-meta, rpc-request, rpc-response, system
    pub message_type: Option<String>,
    /// 消息载荷
    pub payload: serde_json::Value,
    /// 回复的消息 ID
    pub reply_to: Option<String>,
    /// 是否使用 Channel E2E 加密载荷（需先完成密钥交换）
    #[serde(default)]
    pub encrypt: Option<bool>,
}

/// Channel 概要
#[derive(Debug, Serialize)]
pub struct ChannelSummary {
    pub channel_id: String,
    pub remote_actor_url: String,
    pub remote_actor_name: Option<String>,
    pub remote_actor_avatar: Option<String>,
    pub channel_type: String,
    pub status: String,
    pub transport: String,
    pub initiated_by: String,
    pub last_activity_at: Option<String>,
    pub created_at: String,
    pub unread_count: i64,
}

/// Channel 详情
#[derive(Debug, Serialize)]
pub struct ChannelDetail {
    pub channel_id: String,
    pub remote_actor_url: String,
    pub remote_actor_name: Option<String>,
    pub remote_actor_avatar: Option<String>,
    pub channel_type: String,
    pub status: String,
    pub transport: String,
    pub tapp_id: Option<String>,
    pub properties: Option<serde_json::Value>,
    pub initiated_by: String,
    pub last_activity_at: Option<String>,
    pub created_at: String,
}

/// 消息条目
#[derive(Debug, Clone, Serialize)]
pub struct MessageItem {
    pub message_id: String,
    pub sender_actor: String,
    pub message_type: String,
    pub payload: serde_json::Value,
    pub reply_to: Option<String>,
    pub is_encrypted: bool,
    pub created_at: String,
}

/// 发送消息响应
#[derive(Debug, Serialize)]
pub struct SendMessageResponse {
    pub success: bool,
    pub message_id: String,
    pub channel_id: String,
    /// 是否已对 payload 做 E2E 加密
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_encrypted: bool,
    /// Outbound delivery enqueue observability (remote peer)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delivery: Option<crate::federation::delivery::DeliveryEnqueueInfo>,
}

/// 发起 E2E 密钥交换响应
#[derive(Debug, Serialize)]
pub struct E2eKeyExchangeResponse {
    pub success: bool,
    pub channel_id: String,
    pub public_key: String,
    pub algorithm: String,
    pub established: bool,
}
