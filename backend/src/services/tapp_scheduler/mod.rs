//! Tapp scheduled-task engine: types, frontend mailbox, loop, and tests.
//!
//! Real submodules: [`types_frontend`], [`engine`], [`accessors`], [`display_tests`].

mod accessors;
mod display_tests;
mod engine;
mod types_frontend; // Display for ExecutionTarget + unit tests

pub use accessors::{init_scheduler, scheduler_engine, try_scheduler_engine};
pub use types_frontend::{
    MAX_SCHEDULER_RETRIES, MAX_SCHEDULER_RETRY_DELAY_MS, SCHEDULER_MAILBOX_POLL_MILLIS,
    SCHEDULER_PRESENCE_REFRESH_SECONDS, TappSchedulerEngine, active_frontend_subject_count,
    backend_action_permissions_of, drain_frontend_messages, normalize_backend_actions_parsed,
    register_frontend_connection, scheduler_counters, scheduler_mailbox_depth,
    unregister_frontend_connection, validate_backend_action_declarations_of,
};
