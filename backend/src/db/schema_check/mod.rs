//! 数据库 Schema 自动补全 / heal 模块
//!
//! ## Dual-path (intentional single-source *semantics*)
//!
//! | Path | Role |
//! |------|------|
//! | `migrations` (Migrator 001–006; extra `seaql_migrations` rows discarded before up) | **Greenfield SoT** — CREATE tables for new installs |
//! | `schema_check` (`ensure_schema`) | **Runtime heals** — missing columns/indexes (generic), platform seeds, single owner, recent-feature CREATE / structural heals |
//!
//! Boot (`main`) and setup (`init_database`) both run Migrator then `ensure_schema`.
//! Do not empty-bump `SCHEMA_VERSION` without a real TableDef/heal change.
//! Drift CI: `migrations_leave_no_schema_drift` (optional `MYRIAD_SCHEMA_DRIFT_DB`).
//!
//! **Support floor: product ≥ 0.3.10.** Pre-0.3.10 field-level alignment one-shots
//! are not maintained; missing columns use `get_expected_schema` + ADD COLUMN.

mod ensure_heals;
mod expected_indexes;
mod expected_schema;
mod introspect;
mod orchestrator;
mod phantasi_source_dedupe;
mod seeds;
mod tables_agent;
mod tables_analytics;
mod tables_core;
mod tables_federation;
mod tables_local_music;
mod tables_phantasi;
mod tables_tapp;
mod types;

#[cfg(test)]
pub(crate) use ensure_heals::AGENT_MEMORIES_DDL;
pub use orchestrator::{ensure_schema, report_schema_drift};
pub use seeds::{
    DefaultPlatformSeed, default_config_seeds, default_platform_seeds, ensure_default_config,
};

#[cfg(test)]
mod tests;
