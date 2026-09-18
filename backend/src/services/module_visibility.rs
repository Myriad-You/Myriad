//! Module visibility for Agent — re-export of workspace crate.
//!
//! Prefer this over `api::config` so services never import the HTTP layer.

pub use myriad_module_visibility::{
    agent_visibility_for_authorization, try_load_module_visibility_preferences,
};

/// Convenience: agent visibility level only (`all` / `authenticated` / `admin`).
///
/// Authorization boundary: DB/JSON errors stay errors instead of defaulting to `all`.
pub async fn agent_module_visibility(db: &sea_orm::DatabaseConnection) -> Result<String, String> {
    agent_visibility_for_authorization(try_load_module_visibility_preferences(db).await)
}
