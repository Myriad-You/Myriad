pub mod ai_process_pure;
pub mod capability;
pub mod chat_attachments;
pub mod chat_music;
pub mod chat_prompt;
pub mod consciousness;
pub mod data_read_pure;
pub mod data_write_pure;
pub mod delegate;
pub mod error_analyzer_pure;
pub mod executor;
pub mod executor_resolve_pure;
pub mod executor_utils_pure;
pub mod external_pure;
pub mod heartbeat;
pub mod identity;
pub mod mcp;
pub mod memory;
pub mod merope;
pub mod notification_preferences;
pub mod notification_producers;
pub mod notifications;
pub mod perception_view;
pub(crate) mod playback_direction;
pub(crate) mod presence_window;
pub mod queue;
pub mod resource_create_pure;
pub mod response_agent;
pub mod run_hub;
pub mod search_output;
pub mod sessions;
pub mod skill;
pub mod skill_evolution;
pub mod system_op_pure;
pub mod task_store_pure;
pub mod tier_router;
pub(crate) mod tool_permissions;
pub mod turn;
pub mod types;
pub mod ui_analysis;
pub mod web_search;
pub(crate) mod work_loop;

#[cfg(test)]
mod behavior_contract;

#[cfg(test)]
mod self_audit;
#[cfg(test)]
mod semantic_eval;

// 重新导出核心类型
pub use types::*;

mod agent_footer;
mod agent_header;
mod confirmation_and_tasks;
mod motion_overlay;
mod process_and_recipe;
mod process_chat;
mod process_work;

pub(crate) use confirmation_and_tasks::collect_step_frontend_actions;

pub use agent_footer::{
    ensure_agent_usage_allowed, get_capabilities_summary_for_user,
    get_user_permissions, init_task_store, user_is_current_admin,
};
pub(crate) use agent_footer::{
    granted_covers_tapp_permission, max_user_agent_permissions,
    scheduler_create_actions_within_grants, scheduler_create_tapp_permissions_within_grants,
};
pub use agent_header::{Agent, LANE_QUEUE, SYSTEM_USER_ID};
