//! File-transfer request/response DTOs.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// 请求/响应类型

/// 发起文件传输请求
#[derive(Debug, Deserialize)]
pub struct InitTransferRequest {
    /// 文件名
    pub filename: String,
    /// 文件大小（字节）
    pub file_size: i64,
    /// MIME 类型
    pub mime_type: Option<String>,
    /// 校验和 (SHA-256)
    pub checksum: Option<String>,
}

/// 上传文件分块请求
#[derive(Debug, Deserialize)]
pub struct UploadChunkRequest {
    /// 分块序号 (0-based)
    pub chunk_index: i32,
    /// 分块数据 (Base64 编码)
    pub chunk_data: String,
    /// 分块大小
    pub chunk_size: i64,
}

/// 文件传输摘要
#[derive(Debug, Serialize)]
pub struct TransferSummary {
    pub transfer_id: String,
    pub channel_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_id: Option<String>,
    pub filename: String,
    pub file_size: i64,
    pub mime_type: Option<String>,
    pub status: String,
    pub direction: String,
    pub progress: f64,
    pub created_at: String,
}

/// 文件传输详情
#[derive(Debug, Serialize)]
pub struct TransferDetail {
    pub transfer_id: String,
    pub channel_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub room_id: Option<String>,
    pub filename: String,
    pub file_size: i64,
    pub mime_type: Option<String>,
    pub checksum: Option<String>,
    pub status: String,
    pub direction: String,
    pub chunks_total: i32,
    pub chunks_received: i32,
    pub bytes_transferred: i64,
    pub progress: f64,
    pub created_at: String,
    pub completed_at: Option<String>,
}

/// Resolved on-disk file for a completed transfer the user is allowed to read.
#[derive(Debug)]
pub struct TransferFileContent {
    pub filename: String,
    pub mime_type: String,
    pub file_size: u64,
    pub path: PathBuf,
}
