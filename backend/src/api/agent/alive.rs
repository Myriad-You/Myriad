//! Whether she is living now (`services::agent::alive`), for the site
//! owner's settings page: times, counts, names and versions only.

use super::*;
use crate::error::HttpError;

/// GET /api/agent/alive
pub async fn get_alive(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    Ok(Json(
        crate::services::agent::alive::report(&db, owner).await,
    ))
}
