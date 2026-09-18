//! Site owner resolution for public/dashboard surfaces.
//!
//! Prefer durable `users.is_owner`; fall back to lowest admin id.
//! Lives in services; profile re-exports it for reports/config.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

/// True when an administrator or durable owner already exists.
pub async fn installation_has_owner(db: &DatabaseConnection) -> Result<bool, String> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT EXISTS (
                SELECT 1 FROM users
                WHERE is_admin = true OR COALESCE(is_owner, false) = true
            ) AS claimed"
                .to_string(),
        ))
        .await
        .map_err(|error| {
            tracing::error!(%error, "failed to read installation claim");
            "Failed to read installation claim".to_string()
        })?
        .ok_or_else(|| "installation claim query returned no row".to_string())?;
    row.try_get::<bool>("", "claimed").map_err(|error| {
        tracing::error!(%error, "failed to decode installation claim state");
        "Failed to read installation claim".to_string()
    })
}

/// Prefer durable owner id; only fall back to lowest admin when owner is absent.
/// Query / decode errors must not be treated as "no owner".
pub(crate) fn owner_id_from_lookups(
    owner: Result<Option<i32>, String>,
    admin: Result<Option<i32>, String>,
) -> Result<i32, String> {
    match owner {
        Ok(Some(id)) => Ok(id),
        Ok(None) => admin.and_then(|id| {
            id.ok_or_else(|| "No administrator is configured as the site owner".to_string())
        }),
        Err(error) => Err(error),
    }
}

pub async fn site_owner_user_id(db: &DatabaseConnection) -> Result<i32, String> {
    let owner = match db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE is_owner = true ORDER BY id ASC LIMIT 1".to_string(),
        ))
        .await
    {
        Ok(Some(row)) => match row.try_get::<i32>("", "id") {
            Ok(id) => Ok(Some(id)),
            Err(error) => {
                tracing::error!(%error, "failed to decode site owner id");
                Err("Failed to resolve site owner".to_string())
            }
        },
        Ok(None) => Ok(None),
        Err(error) => {
            tracing::error!(%error, "failed to resolve site owner");
            Err("Failed to resolve site owner".to_string())
        }
    };
    if matches!(owner, Ok(Some(_)) | Err(_)) {
        return owner_id_from_lookups(owner, Ok(None));
    }

    let admin = match db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE is_admin = true ORDER BY id ASC LIMIT 1".to_string(),
        ))
        .await
    {
        Ok(Some(row)) => match row.try_get::<i32>("", "id") {
            Ok(id) => Ok(Some(id)),
            Err(error) => {
                tracing::error!(%error, "failed to decode site owner id");
                Err("Failed to resolve site owner".to_string())
            }
        },
        Ok(None) => Ok(None),
        Err(error) => {
            tracing::error!(%error, "failed to resolve site owner");
            Err("Failed to resolve site owner".to_string())
        }
    };
    owner_id_from_lookups(owner, admin)
}

#[cfg(test)]
mod tests {
    use super::owner_id_from_lookups;

    #[test]
    fn owner_query_error_does_not_fall_back_to_lowest_admin() {
        let error = owner_id_from_lookups(Err("db down".into()), Ok(Some(1))).unwrap_err();
        assert_eq!(error, "db down");
    }

    #[test]
    fn owner_decode_error_does_not_select_admin() {
        let error = owner_id_from_lookups(Err("Failed to resolve site owner".into()), Ok(Some(9)))
            .unwrap_err();
        assert!(error.contains("site owner"));
    }

    #[test]
    fn missing_owner_uses_lowest_admin() {
        assert_eq!(owner_id_from_lookups(Ok(None), Ok(Some(3))).unwrap(), 3);
    }

    #[test]
    fn durable_owner_wins_over_admin() {
        assert_eq!(owner_id_from_lookups(Ok(Some(7)), Ok(Some(1))).unwrap(), 7);
    }

    #[test]
    fn missing_owner_and_admin_is_unconfigured() {
        let error = owner_id_from_lookups(Ok(None), Ok(None)).unwrap_err();
        assert!(error.contains("No administrator"));
    }

    #[test]
    fn admin_query_error_is_not_unconfigured() {
        let error = owner_id_from_lookups(Ok(None), Err("db down".into())).unwrap_err();
        assert_eq!(error, "db down");
    }
}
