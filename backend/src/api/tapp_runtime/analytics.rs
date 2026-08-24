//! Site analytics aggregates for Tapp runtimes (read-only).
//!
//! Exposes first-party visitor statistics already shown on the admin dashboard
//! / public visitor card — never visitor hashes or identity material.
//!
//! Permission: `analytics:read` (basic, guest-safe). Requires a Runtime Grant.
//!
//! **Payload scope**
//! - **Admin** subjects: full admin summary (pages / events / referrers / countries).
//! - **Non-admin** (user / guest): visitor-card aggregates only (today / all-time /
//!   short daily trend) — never full breakdown tables.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::DatabaseConnection;
use serde_json::{json, Value};

use crate::error::HttpError;
use crate::middleware::auth::Claims;
use crate::services::permission_service::{TappPermission, UserRole};

use super::common::current_tapp_user_role;
use super::runtime_grant::RuntimeGrantContext;

/// GET /api/tapp/analytics/summary?days=7 | ?from=&to=
///
/// Admin subjects receive the same aggregate payload shape as admin
/// `GET /api/analytics/summary` (today / range / daily / pages / events /
/// referrers / countries). Non-admin subjects with `analytics:read` receive
/// reduced visitor-card aggregates only.
pub async fn get_tapp_analytics_summary(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
    Query(q): Query<crate::api::analytics::SummaryQuery>,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require(TappPermission::AnalyticsRead)?;
    tracing::debug!(
        "[TAPP] analytics.summary user={} days={:?} from={:?} to={:?}",
        claims.username,
        q.days,
        q.from,
        q.to
    );

    let enabled = {
        let cfg = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        cfg.analytics_enabled
    };
    // Align with visitor short-circuit: no aggregates when collection is off.
    if !enabled {
        return Ok(Json(json!({
            "success": true,
            "enabled": false,
            "source": "site_analytics",
        })));
    }

    let role = current_tapp_user_role(&db, &claims).await;
    if role != UserRole::Admin {
        // Guests / users: visitor-card aggregates only — never pages/referrers/etc.
        return Ok(Json(visitor_card_tapp_payload(&db).await));
    }

    let (status, Json(mut body)) = crate::api::analytics::build_analytics_summary(&db, q).await;

    if status != StatusCode::OK {
        return Err(HttpError::from((status, Json(body))));
    }

    if let Some(obj) = body.as_object_mut() {
        obj.insert("enabled".into(), json!(true));
        obj.insert("source".into(), json!("site_analytics"));
        obj.insert("scope".into(), json!("admin"));
    }

    Ok(Json(body))
}

/// GET /api/tapp/analytics/visitor
///
/// Public visitor-card aggregates (today / all-time / short trend).
/// Does not include per-visitor ordinals (those stay on the host visitor card).
pub async fn get_tapp_analytics_visitor(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime_grant: RuntimeGrantContext,
) -> Result<Json<Value>, HttpError> {
    runtime_grant.require(TappPermission::AnalyticsRead)?;
    tracing::debug!("[TAPP] analytics.visitor user={}", claims.username);

    let enabled = {
        let cfg = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
        cfg.analytics_enabled
    };
    if !enabled {
        return Ok(Json(json!({
            "success": true,
            "enabled": false,
            "source": "site_analytics",
        })));
    }

    Ok(Json(visitor_card_tapp_payload(&db).await))
}

/// Shared visitor-card envelope for Tapp (no ordinals / counted flags).
async fn visitor_card_tapp_payload(db: &DatabaseConnection) -> Value {
    let mut body = crate::api::analytics::visitor_card_aggregate(db).await;
    if let Some(obj) = body.as_object_mut() {
        obj.insert("success".into(), json!(true));
        obj.insert("enabled".into(), json!(true));
        obj.insert("source".into(), json!("site_analytics"));
        obj.insert("scope".into(), json!("visitor"));
        // Host visitor card may add per-request ordinals; Tapp API never does.
        obj.remove("your_ordinal_today");
        obj.remove("counted");
    }
    body
}
