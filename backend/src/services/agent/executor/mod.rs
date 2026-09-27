//! Capability execution infrastructure for the Work loop.
//!
//! The DAG recipe executor is gone; what remains is shared by every Work
//! call.
//!
//! ## 模块结构
//!
//! - `handlers` - 各类能力的具体执行实现
//! - `params` - `*From` 引用解析、请求上下文注入、输出契约
//! - `task_store` - 任务状态存储和持久化
//! - `events` / `frontend_ack` - 步骤事件与前端动作回执
//! - `utils` - 工具函数（输出摘要等）
//!
//! ## 整合项目现有服务
//!
//! - `services/analyzer` - AI 分析服务
//! - `api/reports` - 报告生成系统
//! - `services/phantasi_parser` - RSS/Atom 解析

pub mod events;
pub mod frontend_ack;
pub mod handlers;
pub mod task_store;
pub mod utils;

// 重新导出常用类型
pub use frontend_ack::submit as submit_frontend_ack;
pub use task_store::{
    TASK_STORE, cancel_task_for_user, enqueue_steering, get_task_for_user, get_user_tasks,
    init_task_store_db, is_cancelled, refresh_task_for_user, request_cancel, take_steering,
};
pub use utils::{extract_image_url, summarize_output};

pub(crate) mod executor_footer;
pub(crate) mod params;
