//! 请求提取器（axum `FromRequestParts`）。
//!
//! 从 `AppState` / `DatabaseConnection` 取 DB；取不到 503。
//! `FromRequestParts<()>` hard-503s. Config-mode handlers that need DB still
//! take `extract::Db` and get 503 until a connection exists.

use axum::extract::FromRequestParts;
use axum::http::request::Parts;
use axum::http::StatusCode;
use axum::Json;
use myriad_error::AppError;
use sea_orm::DatabaseConnection;
use serde_json::{json, Value};

use crate::middleware::auth::Claims;

/// 已连接的数据库。
///
/// 数据库不可用时 503。
#[derive(Debug)]
pub struct Db(pub DatabaseConnection);

fn db_unavailable() -> (StatusCode, Json<Value>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({
            "error": "Database not connected",
            "code": "database_error",
        })),
    )
}

impl FromRequestParts<crate::state::AppState> for Db {
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request_parts(
        _parts: &mut Parts,
        state: &crate::state::AppState,
    ) -> Result<Self, Self::Rejection> {
        // Shared slot with process registry — reconnect updates AppState in place.
        state.db().map(Db).ok_or_else(db_unavailable)
    }
}

impl FromRequestParts<DatabaseConnection> for Db {
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request_parts(
        _parts: &mut Parts,
        state: &DatabaseConnection,
    ) -> Result<Self, Self::Rejection> {
        Ok(Db(state.clone()))
    }
}

/// Stateless routers must not pull a process DB.
/// Config-mode handlers that need DB take `extract::Db` (503 until wired).
/// Full-mode routes are always registered under `Router<AppState>`.
///
/// This impl remains so unit tests can assert a clean 503 when state is `()`,
/// without reading process globals.
impl FromRequestParts<()> for Db {
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request_parts(_parts: &mut Parts, _state: &()) -> Result<Self, Self::Rejection> {
        Err(db_unavailable())
    }
}

/// 认证中间件放进扩展的 JWT claims。
///
/// 仅在已挂 `auth_middleware` / `admin_middleware` 的路由上使用 —— 中间件负责
/// 验签，这里只是把结果取出来。没有中间件时缺失即 401，不会误放行。
#[derive(Debug)]
pub struct AuthedClaims(pub Claims);

impl<S: Send + Sync> FromRequestParts<S> for AuthedClaims {
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        parts
            .extensions
            .get::<Claims>()
            .cloned()
            .map(AuthedClaims)
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(AppError::public_json("Not authenticated")),
                )
            })
    }
}

/// 当前仍然是管理员的调用者。
///
/// 5 个 ring 写端点 + `federation_update_trust_policy` 的路由只有 router 级 `auth_middleware`，`AdminClaims` 是它们唯一的管理员防线。
/// 与 `admin_middleware` 叠加时幂等。`ensure_current_admin_on` 回查数据库，不只看 JWT `is_admin`。
#[derive(Debug)]
pub struct AdminClaims(pub Claims);

/// Full-mode: admin check uses AppState DB (no process global).
impl FromRequestParts<crate::state::AppState> for AdminClaims {
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &crate::state::AppState,
    ) -> Result<Self, Self::Rejection> {
        let AuthedClaims(claims) = AuthedClaims::from_request_parts(parts, state).await?;
        let db = state.db().ok_or_else(db_unavailable)?;
        crate::middleware::auth::ensure_current_admin_on(&claims, &db).await?;
        Ok(AdminClaims(claims))
    }
}

/// Stateless / unit tests: no process-DB fallback.
///
/// `is_admin=false` still fails closed before any DB (same as production).
/// `is_admin=true` cannot re-verify without AppState → 503.
impl FromRequestParts<()> for AdminClaims {
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request_parts(parts: &mut Parts, state: &()) -> Result<Self, Self::Rejection> {
        let AuthedClaims(claims) = AuthedClaims::from_request_parts(parts, state).await?;
        if !claims.is_admin {
            return Err((
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "Forbidden",
                    "message": "Administrator access required. Only current admin users can perform this action."
                })),
            ));
        }
        let _ = state;
        Err((
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({
                "error": "Database not connected",
                "message": "Administrator status cannot be verified without AppState."
            })),
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::Request;

    fn parts_with<T: Clone + Send + Sync + 'static>(ext: Option<T>) -> Parts {
        let mut req = Request::builder().body(()).unwrap();
        if let Some(v) = ext {
            req.extensions_mut().insert(v);
        }
        req.into_parts().0
    }

    fn claims(sub: &str) -> Claims {
        Claims {
            sub: sub.to_string(),
            username: "tester".into(),
            is_admin: false,
            is_owner: false,
            exp: 0,
            iat: 0,
            tv: 0,
        }
    }

    #[tokio::test]
    async fn authed_claims_reads_middleware_extension() {
        let mut parts = parts_with(Some(claims("42")));
        let got = AuthedClaims::from_request_parts(&mut parts, &())
            .await
            .unwrap();
        assert_eq!(got.0.sub, "42");
    }

    #[tokio::test]
    async fn authed_claims_rejects_when_middleware_absent() {
        // 路由漏挂认证中间件时必须 401，绝不能放行
        let mut parts = parts_with::<Claims>(None);
        let err = AuthedClaims::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn db_extractor_503s_without_app_state() {
        // No AppState / no DatabaseConnection state → hard 503.
        // Must not fall back to process-global DB.
        let mut parts = parts_with::<Claims>(None);
        let err = Db::from_request_parts(&mut parts, &()).await.unwrap_err();
        assert_eq!(err.0, StatusCode::SERVICE_UNAVAILABLE);
    }

    #[tokio::test]
    async fn admin_claims_rejects_when_middleware_absent() {
        // 路由漏挂认证中间件 → 401，绝不放行
        let mut parts = parts_with::<Claims>(None);
        let err = AdminClaims::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn admin_claims_rejects_non_admin_before_touching_the_db() {
        // is_admin=false 在回查数据库之前就短路 —— 这条不依赖数据库可用
        let mut parts = parts_with(Some(claims("42")));
        let err = AdminClaims::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::FORBIDDEN);
    }

    /// 每个管理端点的签名里都必须带 `AdminClaims`。
    ///
    /// 对 5 个 ring 写端点 + `federation_update_trust_policy` 这是**唯一**的管理员防线
    /// （路由只有 router 级 `auth_middleware`）；对站点管理端点它与 `admin_middleware` 叠加，
    /// 保证路由被重挂时防护不会随之消失。
    ///
    /// 降级成 `AuthedClaims` 会把这些能力开放给任何登录用户。
    #[test]
    fn admin_endpoints_keep_the_admin_extractor() {
        // Federation HTTP handlers live under api/federation; site admin wrappers remain in main.rs.
        let src = [
            include_str!("main.rs"),
            concat!(
                include_str!("api/federation/mod.rs"),
                include_str!("api/federation/social.rs"),
                include_str!("api/federation/rooms_and_router.rs")
            ),
        ]
        .concat();
        for handler in [
            // 路由只有 auth_middleware —— AdminClaims 是唯一防线
            "federation_create_ring",
            "federation_add_ring_peer",
            "federation_remove_ring_peer",
            "federation_leave_ring",
            "federation_trigger_ring_sync",
            "federation_update_trust_policy",
            // 路由有 admin_middleware —— AdminClaims 是纵深防御
            "export_settings",
            "update_config",
            "restore_settings",
            "preview_settings_restore",
            "change_site_domain",
            "admin_federation_domain_move",
        ] {
            let sig = src
                .split(&format!("async fn {handler}("))
                .nth(1)
                .unwrap_or_else(|| panic!("{handler} not found"));
            let params = sig.split(") -> Response").next().unwrap();
            assert!(
                params.contains("AdminClaims"),
                "{handler} must take AdminClaims; downgrading to AuthedClaims would \
                 expose an administrative capability to any logged-in user"
            );
        }
    }
}
