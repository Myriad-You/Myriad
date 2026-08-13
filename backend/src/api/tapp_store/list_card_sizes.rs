//! List card sizes (1x1 / 2x1).
//!
//! - GET is public (optional auth): guests **read** the site-owner layout.
//! - PUT requires a durable logged-in user (`sub >= 0`). Guests are rejected
//!   with 403 even if a guest claim somehow reaches the handler.

use axum::{extract::State, http::StatusCode, Extension, Json};
use serde_json::{json, Value};

use crate::middleware::auth::{Claims, OptionalClaims};
use crate::services::tapp_list_card_sizes::{self, TappListCardSizes};
use crate::services::tapp_ownership::parse_authenticated_subject_id;
use crate::state::AppState;

fn require_db(state: &AppState) -> Result<sea_orm::DatabaseConnection, (StatusCode, Json<Value>)> {
    state.db().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"success": false, "message": "Database is not connected"})),
        )
    })
}

/// Durable account only — guests (`sub < 0` / guest session claims) cannot write.
fn require_durable_user(claims: &Claims) -> Result<i32, (StatusCode, Json<Value>)> {
    match parse_authenticated_subject_id(&claims.sub) {
        Some(user_id) => Ok(user_id),
        None => {
            // Distinguish guest cookie claims from garbage `sub`
            let is_guest = claims
                .sub
                .parse::<i32>()
                .map(|id| id < 0)
                .unwrap_or(false)
                || claims.username.starts_with("guest:");
            if is_guest {
                Err((
                    StatusCode::FORBIDDEN,
                    Json(json!({
                        "success": false,
                        "code": "GUEST_LAYOUT_READONLY",
                        "message": "Guests cannot modify list card layout",
                    })),
                ))
            } else {
                Err((
                    StatusCode::UNAUTHORIZED,
                    Json(json!({
                        "success": false,
                        "message": "Authentication required to modify list card layout",
                    })),
                ))
            }
        }
    }
}

/// GET /api/tapps/list-card-sizes
///
/// Public read.
/// - Guests: `sizes`/`order` = site-owner layout; `site_*` mirrors the same.
/// - Authenticated: `sizes`/`order` = **pure personal** prefs (no owner fill);
///   `site_sizes`/`site_order` = site-owner layout for the site scope view.
pub async fn get_list_card_sizes(
    State(state): State<AppState>,
    Extension(OptionalClaims(claims)): Extension<OptionalClaims>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let db = require_db(&state)?;
    let viewer = claims
        .as_ref()
        .and_then(|c| parse_authenticated_subject_id(&c.sub));
    let layout = tapp_list_card_sizes::load_for_viewer(&db, viewer).await;
    // Primary payload: guests see site layout; authed users see personal only.
    let primary = if layout.source == "site_owner" {
        &layout.site
    } else {
        &layout.personal
    };
    Ok((
        StatusCode::OK,
        Json(json!({
            "success": true,
            "sizes": primary.sizes,
            "order": primary.order,
            "site_sizes": layout.site.sizes,
            "site_order": layout.site.order,
            "source": layout.source,
            "writable": layout.writable,
        })),
    ))
}

/// PUT /api/tapps/list-card-sizes
///
/// Auth required (route layer). Handler additionally rejects guest subjects
/// so layout writes never land without a durable user row.
pub async fn put_list_card_sizes(
    State(state): State<AppState>,
    Extension(claims): Extension<Claims>,
    Json(payload): Json<TappListCardSizes>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let user_id = require_durable_user(&claims)?;
    let db = require_db(&state)?;
    match tapp_list_card_sizes::save(&db, user_id, payload).await {
        Ok(prefs) => Ok((
            StatusCode::OK,
            Json(json!({
                "success": true,
                "sizes": prefs.sizes,
                "order": prefs.order,
            })),
        )),
        Err(message) => {
            let status = if message == "User not found" {
                StatusCode::NOT_FOUND
            } else {
                StatusCode::INTERNAL_SERVER_ERROR
            };
            Err((
                status,
                Json(json!({
                    "success": false,
                    "message": message,
                })),
            ))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claims(sub: &str, username: &str) -> Claims {
        Claims {
            sub: sub.to_string(),
            username: username.to_string(),
            is_admin: false,
            is_owner: false,
            exp: i64::MAX,
            iat: 0,
            tv: 0,
        }
    }

    #[test]
    fn durable_user_accepted() {
        assert_eq!(require_durable_user(&claims("42", "alice")).unwrap(), 42);
    }

    #[test]
    fn guest_sub_forbidden() {
        let err = require_durable_user(&claims("-12345", "guest:abcdef01")).unwrap_err();
        assert_eq!(err.0, StatusCode::FORBIDDEN);
        assert_eq!(err.1["code"], "GUEST_LAYOUT_READONLY");
    }

    #[test]
    fn garbage_sub_unauthorized() {
        let err = require_durable_user(&claims("not-a-number", "x")).unwrap_err();
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    }
}
