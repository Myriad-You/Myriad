//! Platform data refresh and disk cache (shared by profile HTTP and Tapp scheduler).
//!
//! Extracted from `api::profile` so `services::tapp_scheduler` does not depend on the HTTP layer.

mod arms_core;
mod arms_extended;
mod cache;
mod clean;
mod errors;
mod fetch;

pub use cache::*;
pub use errors::{
    humanize_platform_fetch_error_for, platform_data_warning, platform_data_warning_for,
    resolve_platform_fetch_message, resolve_platform_fetch_message_for,
};
pub use fetch::{fetch_fresh_platform_data, refresh_platform_for_scheduler, FreshPlatformData};
