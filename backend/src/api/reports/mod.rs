//! Platform report generation, listing, and library extract helpers.
//!
//! Real submodules: [`extract`] (library/stats helpers), [`generate`] (AI + write path),
//! [`latest_and_list`] (public latest + auto-regen), [`prompts`] (report prompt assembly).

mod extract;
mod generate;
mod latest_and_list;
pub(crate) mod locale;
mod mock;
mod prompt_data;
mod prompts;

pub use generate::{generate_all_reports, generate_platform_reports};
pub(crate) use generate::{public_report_owner_user_id, resolve_report_user_id_for_public_read};
pub use latest_and_list::get_latest_report;
