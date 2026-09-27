use crate::services::permission_service::{TappPermissionService, UserRole};
use myriad_error::AppError;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
/// Refuse new Tapp installs when an admin has locked the account.
pub(crate) async fn ensure_tapp_install_allowed(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<(), AppError> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT tapp_install_disabled FROM users WHERE id = $1",
            [user_id.into()],
        ))
        .await
        .map_err(|_| AppError::internal("Failed to check Tapp install permission"))?;
    let disabled = row
        .and_then(|row| row.try_get::<bool>("", "tapp_install_disabled").ok())
        .unwrap_or(false);
    if disabled {
        return Err(AppError::forbidden(
            "Tapp installation is disabled for this account",
        ));
    }
    Ok(())
}

pub(crate) async fn filter_install_permissions(
    dynamic_config: &tokio::sync::RwLock<crate::config::DynamicConfig>,
    role: UserRole,
    permissions: Vec<String>,
) -> Result<Vec<String>, AppError> {
    let config = dynamic_config.read().await;
    TappPermissionService::filter_permissions_for_role(&config, role, &permissions)
        .map_err(|error| AppError::conflict(error.message()).with_code(error.code()))
}

fn policy_error(_message: &str) -> AppError {
    crate::services::federation_gate::disabled_region_app_error(403)
}

/// Eligibility uses declarations, never the selected approval subset. Runtime
/// grants remain independently filtered by role and platform configuration.
pub(crate) async fn ensure_permissions_allowed(permissions: &[String]) -> Result<(), AppError> {
    crate::services::federation_gate::ensure_tapp_install_allowed(permissions)
        .await
        .map_err(policy_error)
}

pub(crate) async fn get_admin_user_id(db: &DatabaseConnection) -> Result<i32, AppError> {
    use crate::services::tapp_ownership::TappAccessError;
    crate::services::tapp_ownership::get_admin_user_id(db)
        .await
        .map_err(|err| match err {
            TappAccessError::NoAdmin => AppError::internal(err.error_code()).with_code("no_admin"),
            _ => AppError::internal(err.message()).with_code("tapp_access_check_failed"),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::federation_gate;

    fn check_permissions(permissions: &[String], enabled: bool) -> Result<(), AppError> {
        federation_gate::check_tapp_install_permissions(permissions, enabled).map_err(policy_error)
    }

    #[test]
    fn closed_gate_rejects_declared_federation_even_without_approval() {
        for permission in ["federation:read", "federation:room", "federation:future"] {
            let error = check_permissions(&[permission.into()], false).unwrap_err();
            let app = error;
            assert_eq!(app.status_u16(), 403);
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
