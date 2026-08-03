//! 数据库 Schema 自动补全 / heal 模块
//!
//! ## Dual-path (intentional single-source *semantics*)
//!
//! | Path | Role |
//! |------|------|
//! | `migrations` (Migrator 001–006) | **Greenfield SoT** — CREATE tables for new installs |
//! | `schema_check` (`ensure_schema`) | **Runtime heals** — missing columns/indexes (generic), platform seeds, single owner, recent-feature CREATE / structural heals |
//!
//! Boot (`main`) and setup (`init_database`) both run Migrator then `ensure_schema`.
//! Do not empty-bump `SCHEMA_VERSION` without a real TableDef/heal change.
//! Drift CI: `migrations_leave_no_schema_drift` (optional `MYRIAD_SCHEMA_DRIFT_DB`).
//!
//! **Support floor: product ≥ 0.3.10.** Pre-0.3.10 field-level alignment one-shots
//! are not maintained; missing columns use `get_expected_schema` + ADD COLUMN.

mod types;
mod seeds;
mod tables_core;
mod tables_tapp;
mod tables_brew;
mod tables_agent;
mod tables_federation;
mod tables_analytics;
mod expected_schema;
mod expected_indexes;
mod ensure_heals;
mod introspect;
mod orchestrator;

pub use seeds::{
    default_platform_seeds, reconcile_retired_migration_history, DefaultPlatformSeed,
    RETIRED_MIGRATION_VERSIONS,
};
pub use orchestrator::ensure_schema;

#[cfg(test)]
mod tests;
