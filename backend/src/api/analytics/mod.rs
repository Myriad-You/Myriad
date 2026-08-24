//! Site analytics intake and admin export/import.
//!
//! Real submodules: [`intake_helpers`] (collect/pageview + shared caches),
//! [`admin_api`] (summary, visitor card, backup), and [`ai_usage`] (AI cost ledger).

mod admin_api;
mod ai_usage;
mod backup_integrity;
mod intake_helpers;

pub use admin_api::{export_analytics, get_summary, get_visitor_card, import_analytics};
pub use ai_usage::get_ai_usage_summary;
pub use intake_helpers::{collect, record_pageview};

/// Shared builders for Tapp runtime analytics API (aggregated only).
pub(crate) use admin_api::{build_analytics_summary, visitor_card_aggregate};
pub(crate) use intake_helpers::SummaryQuery;

#[cfg(test)]
mod tests_inline;
