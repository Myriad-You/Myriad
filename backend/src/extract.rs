//! 请求提取器（axum `FromRequestParts`）。
//!
//! 从 `AppState` / `DatabaseConnection` 取 DB；取不到 503。
//! `FromRequestParts<()>` hard-503s. Config-mode handlers that need DB still
//! take `extract::Db` and get 503 until a connection exists.

use axum::Json;
use axum::extract::FromRequestParts;
use axum::http::StatusCode;
use axum::http::request::Parts;
use myriad_error::AppError;
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

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

/// 认证边界已解析的持久用户 ID（`sub > 0`）。
///
/// 直接读取认证中间件注入的 `Claims` 上已解析的 typed subject
/// （[`Claims::subject`](crate::middleware::auth::Claims::subject)），不再解析
/// `claims.sub`，也不 clone Claims。未挂认证中间件时 401；游客（负数）与 `0`
/// 主体 403，在 handler 与任何业务写之前拒绝。
#[derive(Debug, Clone, Copy)]
pub struct DurableUserId(pub i32);

impl<S: Send + Sync> FromRequestParts<S> for DurableUserId {
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let subject = parts
            .extensions
            .get::<Claims>()
            .and_then(Claims::subject)
            .ok_or_else(|| {
                (
                    StatusCode::UNAUTHORIZED,
                    Json(AppError::public_json("Not authenticated")),
                )
            })?;
        subject.durable_user_id().map(DurableUserId).ok_or_else(|| {
            (
                StatusCode::FORBIDDEN,
                Json(json!({
                    "error": "A durable user account is required",
                    "code": "invalid_subject",
                })),
            )
        })
    }
}

/// 可选认证路由上的访问者：`claims` 为 `None` 即游客；`is_admin` 只来自本请求的
/// 当前管理员核验标记，不看 JWT 里的 `is_admin`。
///
/// 仅在已挂 `optional_current_admin_auth_middleware` /
/// `lenient_current_admin_auth_middleware` 的路由上使用。漏挂时 500，
/// 不会把带凭据的请求静默当成游客。
#[derive(Debug)]
pub struct OptionalViewer {
    pub claims: Option<Claims>,
    pub is_admin: bool,
}

impl<S: Send + Sync> FromRequestParts<S> for OptionalViewer {
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let crate::middleware::auth::OptionalClaims(claims) = parts
            .extensions
            .get::<crate::middleware::auth::OptionalClaims>()
            .cloned()
            .ok_or_else(|| {
                tracing::error!("OptionalViewer used on a route without optional auth middleware");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(AppError::public_json("Authentication is not configured")),
                )
            })?;
        let is_admin = claims.is_some()
            && parts
                .extensions
                .get::<crate::middleware::auth::CurrentAdminVerified>()
                .is_some();
        Ok(OptionalViewer { claims, is_admin })
    }
}

/// 当前仍然是管理员的调用者。
///
/// 5 个 ring 写端点 + `federation_update_trust_policy` 的路由只有 router 级 `auth_middleware`，`AdminClaims` 是它们唯一的管理员防线。
/// 与 `admin_middleware` / 可选认证中间件叠加时复用其本请求核验结果（通过或 403），不再二次查库；
/// 否则由 `ensure_current_admin_on` 回查数据库，不只看 JWT `is_admin`。
///
/// 可选认证路由上未带凭据时，401 与 `auth_middleware` 缺凭据的响应同形。
#[derive(Debug)]
pub struct AdminClaims(pub Claims);

/// 取已认证 claims；可选认证中间件已判定「未带凭据」时给 `auth_middleware` 同形 401。
fn admin_subject(parts: &Parts) -> Result<Claims, (StatusCode, Json<Value>)> {
    if let Some(claims) = parts.extensions.get::<Claims>() {
        return Ok(claims.clone());
    }
    if matches!(
        parts
            .extensions
            .get::<crate::middleware::auth::OptionalClaims>(),
        Some(crate::middleware::auth::OptionalClaims(None))
    ) {
        return Err(crate::middleware::auth::missing_credential());
    }
    Err((
        StatusCode::UNAUTHORIZED,
        Json(AppError::public_json("Not authenticated")),
    ))
}

/// Full-mode: admin check uses AppState DB (no process global).
impl FromRequestParts<crate::state::AppState> for AdminClaims {
    type Rejection = (StatusCode, Json<Value>);

    async fn from_request_parts(
        parts: &mut Parts,
        state: &crate::state::AppState,
    ) -> Result<Self, Self::Rejection> {
        let claims = admin_subject(parts)?;
        if parts
            .extensions
            .get::<crate::middleware::auth::CurrentAdminVerified>()
            .is_some()
        {
            return Ok(AdminClaims(claims));
        }
        if parts
            .extensions
            .get::<crate::middleware::auth::CurrentAdminDenied>()
            .is_some()
        {
            return Err(crate::middleware::auth::admin_forbidden());
        }
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
        let claims = admin_subject(parts)?;
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
            subject: crate::middleware::auth::AuthSubject::from_test_sub(sub),
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

    #[tokio::test]
    async fn admin_claims_answers_a_credential_less_optional_request_like_auth_middleware() {
        // 可选认证中间件判定未带凭据 → 与 auth_middleware 缺凭据同形 401
        let mut parts = parts_with(Some(crate::middleware::auth::OptionalClaims(None)));
        let err = AdminClaims::from_request_parts(&mut parts, &())
            .await
            .unwrap_err();
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
        assert_eq!(err.1.0, crate::middleware::auth::missing_credential().1.0);
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
