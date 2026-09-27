use once_cell::sync::Lazy;
use sea_orm::DatabaseConnection;
use std::sync::Arc;

/// 全局 Lane Queue（控制并发和会话级串行；无 session 时退回 `user:{id}`）
pub static LANE_QUEUE: Lazy<Arc<super::queue::LaneQueue>> =
    Lazy::new(|| Arc::new(super::queue::LaneQueue::new(4)));

/// 心跳的主体 ID。心跳不是一个人：它代站长运行（授予权限跟随站长，见
/// `heartbeat_delegate`），无人值守，也不是「全体用户」。需要「全体」的地方
/// 用 `Option::None` 表达，不要借用 0。
pub const SYSTEM_USER_ID: i32 = 0;

/// Agent 主入口
///
/// Chat vs Work。Work (including saved presets) uses a persistent model/tool loop.
pub struct Agent {
    pub(crate) db: DatabaseConnection,
}
