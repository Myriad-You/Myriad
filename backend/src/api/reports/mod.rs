//! Platform report generation, listing, and library extract helpers.
//!
//! Real submodules: [`extract`] (library/stats helpers), [`facts`] (numbers and
//! card fields, no model), [`script`] (headline + chapters the model writes),
//! [`style_api`] (owner's writing styles; the styles themselves live in
//! `services::report_styles`), [`generate`] (write path),
//! [`latest_and_list`] (public latest + auto-regen), [`prompts`] (report prompt assembly).

mod extract;
mod facts;
mod generate;
mod latest_and_list;
pub(crate) mod locale;
mod mock;
mod prompt_data;
mod prompts;
mod script;
mod style_api;

#[cfg(test)]
pub(crate) use generate::enabled_report_platforms;
pub use generate::{generate_all_reports, generate_platform_reports};
pub use latest_and_list::get_latest_report;
pub use style_api::{get_report_styles, preview_report_style, update_report_styles};
