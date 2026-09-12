//! 联邦发现（WebFinger RFC 7033 + NodeInfo 2.1）
//!
//! WebFinger (RFC 7033) + NodeInfo 2.1 端点
//! 这些端点不需要认证，是联邦互通的入口。

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Json,
};
use myriad_error::AppError;
use sea_orm::DatabaseConnection;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::Deserialize;
use serde_json::json;

use crate::federation::types::*;

/// WebFinger 查询参数
#[derive(Deserialize)]
pub struct WebFingerQuery {
    pub resource: String,
}

/// GET /.well-known/webfinger?resource=acct:user@domain
///
/// RFC 7033 WebFinger 端点 — AP 联邦发现的入口
#[allow(clippy::type_complexity)]
pub async fn webfinger(
    State(db): State<DatabaseConnection>,
    Query(query): Query<WebFingerQuery>,
) -> Result<
    (
        StatusCode,
        [(axum::http::HeaderName, &'static str); 1],
        Json<serde_json::Value>,
    ),
    (StatusCode, Json<serde_json::Value>),
> {
    let resource = &query.resource;

    // 解析 acct:username@domain 格式
    let (username, domain) = parse_acct_uri(resource).ok_or_else(|| {
        (
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json(
                "Invalid resource format. Expected acct:user@domain",
            )),
        )
    })?;

    // 仅处理本实例的用户
    let base_url = get_base_url().await;
    let our_domain = local_webfinger_domain(&domain, &base_url).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("User not found on this instance")),
        )
    })?;

    // db from AppState (no process-global fallback)

    let user_exists = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM users WHERE username = $1 LIMIT 1",
            [username.clone().into()],
        ))
        .await
        .map_err(|_| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::public_json("Database query failed")),
            )
        })?;

    if user_exists.is_none() {
        return Err((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("User not found")),
        ));
    }

    let actor_uri = actor_url(&base_url, &username);

    let response = WebFingerResponse {
        subject: format!("acct:{}@{}", username, our_domain),
        aliases: Some(vec![actor_uri.clone()]),
        links: vec![
            WebFingerLink {
                rel: "self".to_string(),
                link_type: Some(AP_CONTENT_TYPE.to_string()),
                href: Some(actor_uri.clone()),
                template: None,
            },
            WebFingerLink {
                rel: "http://webfinger.net/rel/profile-page".to_string(),
                link_type: Some("text/html".to_string()),
                href: Some(format!("{}/profile/{}", get_frontend_url().await, username)),
                template: None,
            },
        ],
    };

    // RFC 7033 §10.2 注册的类型是 application/jrd+json（无 charset）。axum Json 默认
    // application/json，这里覆盖 Content-Type。
    Ok((
        StatusCode::OK,
        [(
            axum::http::header::CONTENT_TYPE,
            "application/jrd+json; charset=utf-8",
        )],
        Json(serde_json::to_value(response).unwrap()),
    ))
}

/// GET /.well-known/nodeinfo
///
/// NodeInfo 发现文档 — 告知其他实例 NodeInfo 端点位置
pub async fn nodeinfo_wellknown() -> (StatusCode, Json<serde_json::Value>) {
    let base_url = get_base_url().await;

    let response = NodeInfoWellKnown {
        links: vec![NodeInfoWellKnownLink {
            rel: "http://nodeinfo.diaspora.software/ns/schema/2.1".to_string(),
            href: format!("{}/nodeinfo/2.1", base_url),
        }],
    };

    (
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap()),
    )
}

/// GET /nodeinfo/2.1
///
/// NodeInfo 2.1 实例信息 — 公开实例的基本统计和能力
pub async fn nodeinfo(
    State(db): State<DatabaseConnection>,
) -> Result<(StatusCode, Json<serde_json::Value>), (StatusCode, Json<serde_json::Value>)> {
    // 查询用户统计
    let (total_users, active_month) = get_user_stats(&db).await.unwrap_or((0, 0));
    // 查询本地发布的 Activity 数量
    let local_posts = get_local_post_count(&db).await.unwrap_or(0);

    let response = NodeInfo {
        version: "2.1".to_string(),
        software: NodeInfoSoftware {
            name: "myriad".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            repository: Some("https://github.com/myriad-project/myriad".to_string()),
            homepage: Some(get_base_url().await),
        },
        protocols: vec!["activitypub".to_string()],
        usage: NodeInfoUsage {
            users: NodeInfoUsers {
                total: total_users,
                active_month,
                active_halfyear: active_month, // 简化处理
            },
            local_posts,
        },
        open_registrations: false, // hardcoded; unread allow_local_registration
        metadata: Some(NodeInfoMetadata {
            mfp_version: Some("1.0".to_string()),
            tapp_capabilities: None, // NodeInfo 未填此字段
            channel_types: Some(vec![
                "text".to_string(),
                "file-transfer".to_string(),
                "rpc".to_string(),
            ]),
            room_support: Some(true),
        }),
    };

    Ok((
        StatusCode::OK,
        Json(serde_json::to_value(response).unwrap()),
    ))
}

// 辅助函数

/// Canonical `acct:` domain for a WebFinger resource addressed at this instance.
///
/// Returns `None` when the resource belongs to some other host.
///
/// An instance on a non-default port must accept both `host` and `host:port`
/// because [`extract_domain`] drops the port. The returned value is the
/// addressable form so `subject` stays consistent with the Actor URL's host.
/// http(s) actor/profile URLs skip WebFinger (`resolve_actor_reference` pass-through).
fn local_webfinger_domain(resource_domain: &str, base_url: &str) -> Option<String> {
    let host = extract_domain(base_url)?;
    let host_port = extract_host_port(base_url).unwrap_or_else(|| host.clone());
    if resource_domain.eq_ignore_ascii_case(&host)
        || resource_domain.eq_ignore_ascii_case(&host_port)
    {
        Some(host_port)
    } else {
        None
    }
}

fn parse_acct_uri(resource: &str) -> Option<(String, String)> {
    let stripped = resource.trim().strip_prefix("acct:")?;
    let parts: Vec<&str> = stripped.splitn(2, '@').collect();
    let username = parts.first()?.trim();
    let domain = parts.get(1)?.trim();
    if !username.is_empty() && !domain.is_empty() {
        Some((username.to_string(), domain.to_ascii_lowercase()))
    } else {
        None
    }
}

/// 获取前端 URL
async fn get_frontend_url() -> String {
    let config = crate::GLOBAL_CONFIG.read().await;
    let frontend_url = config.frontend_url.clone().unwrap_or_else(|| {
        config
            .base_url
            .clone()
            .unwrap_or_else(|| format!("http://{}:{}", config.server_host, config.server_port))
    });
    frontend_url.trim_end_matches('/').to_string()
}

/// 查询用户统计
async fn get_user_stats(db: &sea_orm::DatabaseConnection) -> Result<(u64, u64), sea_orm::DbErr> {
    let total = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) as count FROM users",
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "count").unwrap_or(0) as u64)
        .unwrap_or(0);

    let active = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) as count FROM users WHERE last_login_at > NOW() - INTERVAL '30 days'",
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "count").unwrap_or(0) as u64)
        .unwrap_or(0);

    Ok((total, active))
}

/// 查询本地发布数
async fn get_local_post_count(db: &sea_orm::DatabaseConnection) -> Result<u64, sea_orm::DbErr> {
    let count = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) as count FROM federation_activities WHERE is_local = true",
        ))
        .await?
        .map(|r| r.try_get::<i64>("", "count").unwrap_or(0) as u64)
        .unwrap_or(0);

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_acct_uri() {
        assert_eq!(
            parse_acct_uri("acct:alice@example.com"),
            Some(("alice".to_string(), "example.com".to_string()))
        );
        assert_eq!(
            parse_acct_uri(" acct:alice@EXAMPLE.com "),
            Some(("alice".to_string(), "example.com".to_string()))
        );
        assert_eq!(parse_acct_uri("alice@example.com"), None);
        assert_eq!(parse_acct_uri("acct:@example.com"), None);
        assert_eq!(parse_acct_uri("acct:alice@"), None);
        assert_eq!(parse_acct_uri("acct:alice"), None);
    }

    /// An instance on a non-default port must resolve its own users by handle.
    #[test]
    fn webfinger_accepts_handle_with_non_default_port() {
        let base = "http://127.0.0.1:1103";
        assert_eq!(
            local_webfinger_domain("127.0.0.1:1103", base).as_deref(),
            Some("127.0.0.1:1103")
        );
        // Bare host still resolves, and subject reports the addressable form.
        assert_eq!(
            local_webfinger_domain("127.0.0.1", base).as_deref(),
            Some("127.0.0.1:1103")
        );
        // A different port is a different instance.
        assert_eq!(local_webfinger_domain("127.0.0.1:1102", base), None);
        assert_eq!(local_webfinger_domain("example.com", base), None);
    }

    #[test]
    fn webfinger_domain_unchanged_on_default_ports() {
        for base in ["https://example.com", "https://example.com:443"] {
            assert_eq!(
                local_webfinger_domain("example.com", base).as_deref(),
                Some("example.com"),
                "base {base}"
            );
            assert_eq!(
                local_webfinger_domain("EXAMPLE.COM", base).as_deref(),
                Some("example.com"),
                "base {base} (case-insensitive)"
            );
            assert_eq!(local_webfinger_domain("other.example", base), None);
        }
    }

    #[test]
    fn parse_acct_uri_lowercases_domain() {
        assert_eq!(
            parse_acct_uri("acct:Bob@Example.COM"),
            Some(("Bob".into(), "example.com".into()))
        );
    }

    #[test]
    fn parse_acct_uri_rejects_no_acct_prefix() {
        assert_eq!(parse_acct_uri("bob@example.com"), None);
        assert_eq!(parse_acct_uri("acct:"), None);
        assert_eq!(parse_acct_uri("acct:@onlydomain"), None);
    }

    #[test]
    fn parse_acct_uri_trims_whitespace() {
        assert_eq!(
            parse_acct_uri("  acct:alice@example.com  "),
            Some(("alice".into(), "example.com".into()))
        );
    }

    #[test]
    fn w175_parse_acct_lowercases_domain() {
        assert_eq!(
            parse_acct_uri("acct:Alice@Example.COM"),
            Some(("Alice".into(), "example.com".into()))
        );
    }

    #[test]
    fn w175_parse_acct_rejects_bare() {
        assert_eq!(parse_acct_uri("alice@example.com"), None);
        assert_eq!(parse_acct_uri("acct:"), None);
        assert_eq!(parse_acct_uri("acct:@only"), None);
    }

    #[test]
    fn w175_parse_acct_trims() {
        assert_eq!(
            parse_acct_uri("  acct:bob@example.com  "),
            Some(("bob".into(), "example.com".into()))
        );
    }
}
