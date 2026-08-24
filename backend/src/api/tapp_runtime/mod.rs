//! Tapp API - 第三方应用集成接口
//!
//! 模块结构：
//! - common:      共享基础（缓存、安全、权限、指标）
//! - platform:    平台数据读写 API
//! - ai_tasks:    服务端治理的 AI 任务 API
//! - reports:     报告 CRUD API
//! - data:        数据转换处理 API
//! - context:     运行上下文 API
//! - media:       媒体控制 API
//! - components:  组件注册 API
//! - shortcuts:   快捷键注册 API
//! - events:      事件声明与当前 Bridge 通知 API
//! - metrics:     性能指标 API
//! - declared_api: Tapp API 声明系统

mod agent_interactions;
mod ai_cost_ledger;
// AI quota domain: `crate::services::ai_quota` (reserve/settle/release/usage).
mod ai_tasks;
mod analytics;
pub mod common;
mod components;
mod context;
mod data;
mod data_exchange;
mod declared_api;
mod events;
mod federation;
mod host_attribution;
mod inbound_route;
mod media;
mod metrics;
mod model3d;
mod notifications;
mod platform;
mod reports;
mod runtime_grant;
pub(crate) mod shared_registry;
mod shortcuts;
mod ws_ticket;

// 公开 re-export（保持 api::tapp::* 路径兼容）

// Platform API
pub use platform::{
    add_platform_item, add_platform_items_batch, get_platform_data, get_platform_distribution,
    get_platform_stats,
};

// AI API
pub use agent_interactions::{
    accept_agent_interaction, get_agent_interaction, install_agent_interaction_executor,
    reject_agent_interaction, request_agent_intent, spawn_agent_interaction_expiry_worker,
    stream_agent_interactions, submit_agent_interaction_result,
};
pub use ai_cost_ledger::ai_cost_ledger;
pub use ai_tasks::{
    ai_usage, cancel_ai_task, create_ai_task, get_ai_task, install_governed_text_executor,
    stream_ai_task_events,
};

// Reports API
pub use reports::{
    create_report, delete_tapp_report, get_runtime_platform_report, get_runtime_report,
    get_tapp_report, list_reports, list_runtime_reports, list_tapp_reports, update_tapp_report,
};

// Server-side attribution for host-proxied legacy routes
pub use host_attribution::{
    brew_host_attribution, federation_host_attribution, speech_host_attribution,
};

// Runtime identity grants
pub use runtime_grant::{
    authorize_runtime_permission, issue_runtime_grant, revoke_all_tapp_runtime_grants,
    revoke_runtime_grant, revoke_tapp_runtime_grants, RuntimeGrantContext,
};

// Federation WebSocket one-time tickets (browser WS cannot carry grant headers).
// Domain: `services::tapp_ws_ticket`. Federation gateway consumes via services;
// mint routes stay here. Re-exports preserve `api::tapp_runtime::*` path stability.
#[allow(unused_imports)]
pub use ws_ticket::{
    consume_ws_ticket, mint_channel_ws_ticket, mint_room_ws_ticket, ConsumedWsTicket, WsTicketKind,
    TAPP_WS_TICKET_QUERY,
};

// Ensure the public query-param constant is linked (used by clients / docs).
const _: &str = TAPP_WS_TICKET_QUERY;

// Data API
pub use data::data_transform;
pub use data_exchange::{
    authorize_data_exchange, cancel_data_exchange, consume_data_exchange, prepare_data_exchange,
};

// Context API
pub use context::{
    get_context_app, get_context_geo, get_context_navigation, get_context_player,
    get_context_system, get_context_user,
};

// Media API
pub use media::{media_control, media_status};

// Tripo 3D (TAPP Runtime Grant; admin Merope routes stay separate)
pub use model3d::{
    await_task as await_model3d_task, create_task as create_model3d_task,
    get_task as get_model3d_task, status as model3d_status, upload as upload_model3d_file,
};

// Components API
pub use components::{
    list_all_components_by_type, list_components, register_component, unregister_component,
};

// Shortcuts API
pub use shortcuts::{list_shortcuts, register_shortcut, unregister_shortcut};

// Events API
pub use events::{publish_event, stream_events};

// Federation API
pub use federation::{get_federation_feed, get_federation_rooms_feed};

// Metrics API
pub use metrics::{get_rate_limit_status, get_tapp_metrics};

// Site analytics (visitor stats) for Tapp runtimes
pub use analytics::{get_tapp_analytics_summary, get_tapp_analytics_visitor};

// Notifications API
pub use notifications::create_tapp_notification;

// Declared API System
pub use declared_api::{execute_tapp_api, invalidate_tapp_apis_cache, list_tapp_apis};
pub use inbound_route::execute_inbound_route;
