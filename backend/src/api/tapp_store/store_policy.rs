//! Store availability follows the server's federation egress gate.
use super::ApiResponse;
use crate::error::HttpError;
use crate::services::federation_gate;
use axum::Json;
use axum::http::StatusCode;
use serde::Serialize;
use std::time::Duration;

fn policy_error(_message: &str) -> HttpError {
    HttpError(federation_gate::disabled_region_app_error(
        StatusCode::FORBIDDEN.as_u16(),
    ))
}

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

/// Uses declarations, never the caller's selected approval subset. This is
/// installation eligibility; runtime grants remain independently filtered.
pub(super) async fn ensure_permissions_allowed(permissions: &[String]) -> Result<(), HttpError> {
    federation_gate::ensure_tapp_install_allowed(permissions)
        .await
        .map_err(policy_error)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn check_permissions(permissions: &[String], enabled: bool) -> Result<(), HttpError> {
        federation_gate::check_tapp_install_permissions(permissions, enabled).map_err(policy_error)
    }

    #[test]
    fn closed_gate_rejects_declared_federation_even_without_approval() {
        for permission in ["federation:read", "federation:room", "federation:future"] {
            let error = check_permissions(&[permission.into()], false).unwrap_err();
            let app = myriad_error::AppError::from(error);
            assert_eq!(app.status(), StatusCode::FORBIDDEN);
            assert_eq!(app.code(), Some("federation_disabled_region"));
            assert_eq!(
                app.body().message.as_deref(),
                Some("Federation is not supported in this region")
            );
        }
    }

    #[test]
    fn server_location_decision_controls_install_eligibility() {
        use crate::services::server_location::unavailable_assessment;
        for (codes, allowed) in [
            (vec!["CN"], false),
            (vec!["JP", "CN"], false),
            (vec!["JP"], true),
            (vec!["HK"], true),
            (vec!["MO"], true),
            (vec!["TW"], true),
            (vec![], true),
        ] {
            let mut assessment = unavailable_assessment(false);
            assessment.country_codes = codes.into_iter().map(str::to_owned).collect();
            let (enabled, _) = federation_gate::decide(&assessment);
            assert_eq!(
                check_permissions(&["federation:read".into()], enabled).is_ok(),
                allowed
            );
        }
    }

    #[test]
    fn ordinary_apps_and_open_gate_are_unaffected() {
        assert!(check_permissions(&[], false).is_ok());
        assert!(check_permissions(&["network:fetch".into()], false).is_ok());
        assert!(check_permissions(&["federation:read".into()], true).is_ok());
    }
}
