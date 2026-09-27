//! Store availability follows the server's federation egress gate.
use super::ApiResponse;
use crate::services::federation_gate;
pub(super) use crate::services::tapp_packages::ensure_permissions_allowed;
use axum::Json;
use serde::Serialize;
use std::time::Duration;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct StorePolicy {
    federation_enabled: bool,
}

pub(super) async fn get_store_policy() -> impl axum::response::IntoResponse {
    let federation_enabled = federation_gate::wait_until_resolved(Duration::from_secs(10)).await;
    (
        [(axum::http::header::CACHE_CONTROL, "no-store")],
        Json(ApiResponse::success(StorePolicy { federation_enabled })),
    )
}
