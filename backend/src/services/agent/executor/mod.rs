//! 执行引擎模块
//!
//! 负责执行 Recipe 中的步骤，管理任务状态
//!
//! ## 模块结构
//!
//! - `task_store` - 任务状态存储和持久化
//! - `utils` - 工具函数（相似度计算、输出摘要等）
//! - `handlers` - 各类能力的具体执行实现
//!
//! ## 整合项目现有服务
//!
//! - `services/analyzer` - AI 分析服务
//! - `api/reports` - 报告生成系统
//! - `api/tapp` - 平台数据 API
//! - `services/brew_parser` - RSS/Atom 解析
//! - `services/tapp_api_service` - Tapp API 执行

pub mod dag;
pub mod error_analyzer;
pub mod events;
pub mod frontend_ack;
pub mod handlers;
pub mod retry;
pub mod task_store;
pub mod utils;

// 重新导出常用类型
pub use frontend_ack::submit as submit_frontend_ack;
pub use task_store::{
    cancel_task_for_user, claim_task_for_resume, clear_cancellation, enqueue_steering,
    get_task_for_user, get_user_tasks, init_task_store_db, is_cancelled, maybe_cleanup_tasks,
    persist_task_async, refresh_task_for_user, request_cancel, take_steering, TASK_STORE,
};
pub use utils::{extract_image_url, summarize_output, truncate_str};

use crate::services::analyzer::AiAnalyzer;
use sea_orm::DatabaseConnection;

/// 执行引擎
pub struct Executor {
    /// 数据库连接
    pub(crate) db: DatabaseConnection,
    /// Pro 层级 AI 分析器（复杂推理、规划、创造性任务）
    pub(crate) pro_analyzer: Option<AiAnalyzer>,
    /// Standard 层级 AI 分析器（常规任务、数据处理、定式化操作）
    pub(crate) standard_analyzer: Option<AiAnalyzer>,
}

mod execute_core;
mod execute_step;
mod executor_footer;
mod path_ai_helpers;
mod resume_and_dynamic;
