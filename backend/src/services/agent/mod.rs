pub mod capability;
pub mod escalation;
pub mod executor;
pub mod heartbeat;
pub mod identity;
pub mod intent;
pub mod mcp;
pub mod memory;
pub mod notification_preferences;
pub mod notification_producers;
pub mod notifications;
pub mod orchestrator;
pub mod planner;
pub mod queue;
pub mod recipe;
pub mod response_agent;
pub mod routing;
pub mod run_hub;
pub mod skill;
pub mod skill_evolution;
pub mod tier_router;
pub mod types;
pub mod ui_analysis;
pub mod data_read_pure;
pub mod ai_process_pure;
pub mod system_op_pure;
pub mod resource_create_pure;
pub mod external_pure;
pub mod data_write_pure;
pub mod retry_pure;
pub mod task_store_pure;
pub mod executor_resolve_pure;
pub mod resume_answer_pure;
pub mod step_generator_pure;
pub mod executor_loop_pure;
pub mod error_analyzer_pure;
pub mod executor_utils_pure;
pub mod dag_pure;

// 重新导出核心类型
pub use types::*;

mod agent_header;
mod agent_footer;
mod process_and_recipe;
mod confirmation_and_tasks;

pub use agent_header::{Agent, LANE_QUEUE, SYSTEM_USER_ID};
pub use agent_footer::{
    apply_pre_param_answer_to_recipe,
    cleanup_expired_confirmations,
    ensure_agent_usage_allowed,
    get_capabilities_summary,
    get_capabilities_summary_for_user,
    get_user_permissions,
    init_task_store,
    parse_pre_param_question_id,
    user_is_current_admin,
};
