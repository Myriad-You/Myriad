//! Where her one mind is, for the site owner (the mind lease,
//! `her_log::pen`): here, on another of their devices, or being taken back
//! here.

use axum::{Extension, Json, extract::State};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::services::persona_sync::lease::{self, SITE, Where};

/// GET /api/agent/mind/where: whether her mind is here, and when not, the
/// device it is on (as the owner named it). `catchingUp` while the site is
/// taking her back and reading what that device wrote first. `leaves` while
/// what the owner says to her here reaches her there (her log synced, D6-b).
pub async fn get_mind(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let owner = super::persona::require_site_owner(&claims, &db).await?;
    Ok(Json(match lease::place() {
        Where::MindOn(holder) if holder == SITE => json!({ "here": false, "catchingUp": true }),
        Where::MindOn(holder) => {
            let name = lease::device_name(&db, &holder).await.ok().flatten();
            let leaves = crate::services::her_log::pen::in_play(
                &db,
                crate::services::her_log::settings(),
                Some(owner),
            )
            .await
            .unwrap_or(false);
            json!({ "here": false, "holder": holder, "holderName": name, "leaves": leaves })
        }
        _ => json!({ "here": true }),
    }))
}
