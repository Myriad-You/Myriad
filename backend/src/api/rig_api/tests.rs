//! The rig API and its management over HTTP, against a database: a key
//! issued by the admin endpoints calls `GET /rig-api/v1/me` through the
//! site's router; refused keys, a disabled tenant and a tenant over its
//! rate get their codes; keys and sessions do not cross; the job quota's
//! refusals answer 429.

use axum::Router;
use axum::body::Body;
use axum::http::{Method, Request, StatusCode, header};
use axum::middleware::Next;
use chrono::TimeDelta;
use sea_orm::ConnectionTrait;
use serde_json::{Value, json};
use tower::ServiceExt;

use crate::middleware::auth::mint_session_claims;
use crate::services::rig_api::admission::Refusal;
use crate::state::AppState;

async fn schema(prefix: &str) -> Option<crate::db::IsolatedSchema> {
    let url = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"])?;
    let schema = crate::db::IsolatedSchema::migrated(&url, prefix).await;
    schema
        .db
        .execute_unprepared(
            "INSERT INTO users (id, username, is_admin) VALUES \
             (1, 'admin', TRUE), (7, 'developer', FALSE)",
        )
        .await
        .unwrap();
    Some(schema)
}

fn state(db: &sea_orm::DatabaseConnection) -> AppState {
    AppState::new(
        db.clone(),
        crate::config::AppConfig::default(),
        crate::config::DynamicConfig::default(),
    )
}

/// The site's whole router, as requests reach it.
fn site(db: &sea_orm::DatabaseConnection) -> Router {
    crate::router::test_api_router(state(db))
}

/// The admin handlers with the session of user `user_id` already
/// authenticated, as `admin_middleware` leaves it; each handler's
/// `AdminClaims` still checks the account is an admin now.
fn admin_as(db: &sea_orm::DatabaseConnection, user_id: i32) -> Router {
    use super::admin;
    use axum::routing::{delete, get, post, put};
    let claims = mint_session_claims(user_id, "someone", true, false, 0);
    Router::new()
        .route(
            "/api/admin/rig-api/tenants",
            get(admin::list_tenants).post(admin::create_tenant),
        )
        .route(
            "/api/admin/rig-api/tenants/{id}",
            get(admin::get_tenant).patch(admin::update_tenant),
        )
        .route(
            "/api/admin/rig-api/tenants/{id}/limits",
            put(admin::set_limits),
        )
        .route(
            "/api/admin/rig-api/tenants/{id}/keys",
            post(admin::issue_key),
        )
        .route(
            "/api/admin/rig-api/tenants/{id}/keys/{key_id}",
            delete(admin::revoke_key),
        )
        .route(
            "/api/admin/rig-api/defaults",
            get(admin::get_defaults).put(admin::set_defaults),
        )
        .layer(axum::middleware::from_fn(
            move |mut req: Request<Body>, next: Next| {
                let claims = claims.clone();
                async move {
                    req.extensions_mut().insert(claims);
                    next.run(req).await
                }
            },
        ))
        .with_state(state(db))
}

struct Answer {
    status: StatusCode,
    headers: axum::http::HeaderMap,
    body: Value,
}

async fn call(
    app: &Router,
    method: Method,
    uri: &str,
    bearer: Option<&str>,
    body: Value,
) -> Answer {
    let mut request = Request::builder().method(method).uri(uri);
    if let Some(token) = bearer {
        request = request.header(header::AUTHORIZATION, format!("Bearer {token}"));
    }
    let request = if body.is_null() {
        request.body(Body::empty())
    } else {
        request
            .header(header::CONTENT_TYPE, "application/json")
            .body(Body::from(body.to_string()))
    }
    .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    let status = response.status();
    let headers = response.headers().clone();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    Answer {
        status,
        headers,
        body: serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    }
}

async fn get_me(app: &Router, key: Option<&str>) -> Answer {
    call(app, Method::GET, "/rig-api/v1/me", key, Value::Null).await
}

/// A tenant of user 7's, made through the admin endpoint, and a key of it.
async fn tenant_with_key(admin: &Router, limits: Value) -> (String, String, String) {
    let created = call(
        admin,
        Method::POST,
        "/api/admin/rig-api/tenants",
        None,
        json!({ "ownerUserId": 7, "name": "Studio", "limits": limits }),
    )
    .await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    let tenant = created.body["tenant"]["id"].as_str().unwrap().to_owned();
    let issued = call(
        admin,
        Method::POST,
        &format!("/api/admin/rig-api/tenants/{tenant}/keys"),
        None,
        json!({ "name": "ci" }),
    )
    .await;
    assert_eq!(issued.status, StatusCode::CREATED, "{}", issued.body);
    assert_eq!(issued.headers[header::CACHE_CONTROL], "no-store");
    let key_id = issued.body["key"]["id"].as_str().unwrap().to_owned();
    let secret = issued.body["secret"].as_str().unwrap().to_owned();
    (tenant, key_id, secret)
}

#[tokio::test]
async fn a_key_issued_by_the_admin_calls_me_and_stops_when_revoked() {
    let Some(schema) = schema("rig_api_http_me").await else {
        return;
    };
    let admin = admin_as(&schema.db, 1);
    let site = site(&schema.db);
    let (tenant, key_id, secret) = tenant_with_key(&admin, json!({ "concurrentJobs": 4 })).await;

    let me = get_me(&site, Some(&secret)).await;
    assert_eq!(me.status, StatusCode::OK, "{}", me.body);
    assert_eq!(me.body["tenant"]["id"], json!(tenant));
    assert_eq!(me.body["key"]["id"], json!(key_id));
    assert_eq!(me.body["limits"]["concurrentJobs"], json!(4));
    let defaults = crate::config::DynamicConfig::default();
    assert_eq!(
        me.body["limits"]["requestsPerMinute"],
        json!(defaults.rig_api_requests_per_minute)
    );
    assert_eq!(me.body["usage"]["requestsThisMinute"], json!(1));
    assert_eq!(me.body["usage"]["concurrentJobs"], json!(0));
    assert_eq!(
        me.headers["x-ratelimit-remaining"],
        (defaults.rig_api_requests_per_minute - 1).to_string()
    );

    // The tenant as the admin reads it: its key by prefix, never the secret.
    let detail = call(
        &admin,
        Method::GET,
        &format!("/api/admin/rig-api/tenants/{tenant}"),
        None,
        Value::Null,
    )
    .await;
    assert_eq!(detail.status, StatusCode::OK);
    assert!(!detail.body.to_string().contains(&secret[20..]));
    assert_eq!(detail.body["keys"][0]["id"], json!(key_id));
    assert!(detail.body["keys"][0]["lastUsedAt"].is_string());
    assert_eq!(detail.body["usage"]["requestsThisMinute"], json!(1));

    let revoked = call(
        &admin,
        Method::DELETE,
        &format!("/api/admin/rig-api/tenants/{tenant}/keys/{key_id}"),
        None,
        Value::Null,
    )
    .await;
    assert_eq!(revoked.status, StatusCode::OK);
    assert_eq!(revoked.body["key"]["live"], json!(false));
    let refused = get_me(&site, Some(&secret)).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert_eq!(refused.body["code"], "rig_api_key_revoked");
    schema.drop().await;
}

#[tokio::test]
async fn refused_keys_and_a_disabled_tenant_get_their_codes() {
    let Some(schema) = schema("rig_api_http_refused").await else {
        return;
    };
    let admin = admin_as(&schema.db, 1);
    let site = site(&schema.db);
    let (tenant, _, secret) = tenant_with_key(&admin, json!({})).await;

    let missing = get_me(&site, None).await;
    assert_eq!(missing.status, StatusCode::UNAUTHORIZED);
    assert_eq!(missing.body["code"], "rig_api_key_required");
    assert!(
        missing.headers[header::WWW_AUTHENTICATE]
            .to_str()
            .unwrap()
            .starts_with("Bearer")
    );

    for malformed in ["mrk_nope", "not a key at all"] {
        let answer = get_me(&site, Some(malformed)).await;
        assert_eq!(answer.status, StatusCode::UNAUTHORIZED, "{malformed}");
        assert_eq!(answer.body["code"], "rig_api_key_malformed", "{malformed}");
    }
    let basic = site
        .clone()
        .oneshot(
            Request::builder()
                .uri("/rig-api/v1/me")
                .header(header::AUTHORIZATION, format!("Basic {secret}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(basic.status(), StatusCode::UNAUTHORIZED);

    let mut forged = secret.clone();
    let last = forged.pop().unwrap();
    forged.push(if last == 'z' { 'y' } else { 'z' });
    let unknown = get_me(&site, Some(&forged)).await;
    assert_eq!(unknown.status, StatusCode::UNAUTHORIZED);
    assert_eq!(unknown.body["code"], "rig_api_key_invalid");

    // An expiry in the past cannot be issued; a key's expiry passing is refused.
    let past = call(
        &admin,
        Method::POST,
        &format!("/api/admin/rig-api/tenants/{tenant}/keys"),
        None,
        json!({ "expiresAt": crate::clock::now() - TimeDelta::minutes(1) }),
    )
    .await;
    assert_eq!(past.status, StatusCode::BAD_REQUEST);
    assert_eq!(past.body["code"], "rig_api_invalid_key_expiry");
    let expiring = call(
        &admin,
        Method::POST,
        &format!("/api/admin/rig-api/tenants/{tenant}/keys"),
        None,
        json!({ "expiresAt": crate::clock::now() + TimeDelta::hours(1) }),
    )
    .await;
    let expiring_secret = expiring.body["secret"].as_str().unwrap().to_owned();
    assert_eq!(
        get_me(&site, Some(&expiring_secret)).await.status,
        StatusCode::OK
    );
    schema
        .db
        .execute_unprepared(&format!(
            "UPDATE rig_api_keys SET expires_at = NOW() - INTERVAL '1 second' WHERE id = '{}'",
            expiring.body["key"]["id"].as_str().unwrap()
        ))
        .await
        .unwrap();
    let expired = get_me(&site, Some(&expiring_secret)).await;
    assert_eq!(expired.status, StatusCode::UNAUTHORIZED);
    assert_eq!(expired.body["code"], "rig_api_key_expired");

    let disabled = call(
        &admin,
        Method::PATCH,
        &format!("/api/admin/rig-api/tenants/{tenant}"),
        None,
        json!({ "disabled": true }),
    )
    .await;
    assert_eq!(disabled.body["tenant"]["disabled"], json!(true));
    let refused = get_me(&site, Some(&secret)).await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);
    assert_eq!(refused.body["code"], "rig_api_tenant_disabled");
    schema.drop().await;
}

#[tokio::test]
async fn a_tenant_over_its_requests_per_minute_gets_429_with_retry_after() {
    let Some(schema) = schema("rig_api_http_rate").await else {
        return;
    };
    let admin = admin_as(&schema.db, 1);
    let site = site(&schema.db);
    let (_, _, secret) = tenant_with_key(&admin, json!({ "requestsPerMinute": 2 })).await;
    let mut statuses = Vec::new();
    let mut limited = None;
    // Three calls in a row may straddle a minute; six cannot all fit in two.
    for _ in 0..6 {
        let answer = get_me(&site, Some(&secret)).await;
        statuses.push(answer.status);
        if answer.status == StatusCode::TOO_MANY_REQUESTS && limited.is_none() {
            limited = Some(answer);
        }
    }
    let limited = limited.unwrap_or_else(|| panic!("never limited: {statuses:?}"));
    assert_eq!(limited.body["code"], "rig_api_rate_limited");
    assert_eq!(limited.body["details"]["limit"], json!(2));
    let retry_after: u64 = limited.headers[header::RETRY_AFTER]
        .to_str()
        .unwrap()
        .parse()
        .unwrap();
    assert!((1..=60).contains(&retry_after), "{retry_after}");
    assert_eq!(limited.headers["x-ratelimit-remaining"], "0");
    schema.drop().await;
}

#[tokio::test]
async fn keys_and_site_sessions_do_not_cross() {
    let Some(schema) = schema("rig_api_http_cross").await else {
        return;
    };
    if crate::middleware::auth::session_secret().is_none() {
        // SAFETY: tests that need a signing secret set the same kind of value.
        unsafe { std::env::set_var("JWT_SECRET", "rig-api-test-jwt-secret-at-least-32b") };
    }
    let admin = admin_as(&schema.db, 1);
    let site = site(&schema.db);
    let (_, _, secret) = tenant_with_key(&admin, json!({})).await;

    // A key is no session: the site's admin routes refuse it.
    let as_session = call(
        &site,
        Method::GET,
        "/api/admin/rig-api/tenants",
        Some(&secret),
        Value::Null,
    )
    .await;
    assert_eq!(as_session.status, StatusCode::UNAUTHORIZED);
    // Without credentials the admin routes are closed too.
    let anonymous = call(
        &site,
        Method::GET,
        "/api/admin/rig-api/tenants",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(anonymous.status, StatusCode::UNAUTHORIZED);

    // A session is no key: the rig API refuses an admin's session token.
    let session = crate::middleware::auth::encode_session_token(&mint_session_claims(
        1, "admin", true, false, 0,
    ))
    .unwrap();
    let refused = get_me(&site, Some(&session)).await;
    assert_eq!(refused.status, StatusCode::UNAUTHORIZED);
    assert_eq!(refused.body["code"], "rig_api_key_malformed");
    schema.drop().await;
}

#[tokio::test]
async fn only_a_current_admin_manages_and_limits_are_checked() {
    let Some(schema) = schema("rig_api_http_admin").await else {
        return;
    };
    let admin = admin_as(&schema.db, 1);
    // A session claiming admin for an account that is not one now.
    let demoted = admin_as(&schema.db, 7);
    let refused = call(
        &demoted,
        Method::GET,
        "/api/admin/rig-api/tenants",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN);

    let no_owner = call(
        &admin,
        Method::POST,
        "/api/admin/rig-api/tenants",
        None,
        json!({ "ownerUserId": 404, "name": "Ghost" }),
    )
    .await;
    assert_eq!(no_owner.status, StatusCode::BAD_REQUEST);
    assert_eq!(no_owner.body["code"], "rig_api_owner_missing");
    let no_name = call(
        &admin,
        Method::POST,
        "/api/admin/rig-api/tenants",
        None,
        json!({ "ownerUserId": 7, "name": "" }),
    )
    .await;
    assert_eq!(no_name.body["code"], "rig_api_invalid_tenant_name");

    let (tenant, _, _) = tenant_with_key(&admin, json!({})).await;
    let too_many = call(
        &admin,
        Method::PUT,
        &format!("/api/admin/rig-api/tenants/{tenant}/limits"),
        None,
        json!({ "requestsPerMinute": -5 }),
    )
    .await;
    assert_eq!(too_many.status, StatusCode::BAD_REQUEST);
    assert_eq!(too_many.body["code"], "rig_api_invalid_limit");

    // Defaults set by the admin are what a tenant without its own gets.
    let set = call(
        &admin,
        Method::PUT,
        "/api/admin/rig-api/defaults",
        None,
        json!({ "concurrentJobs": 6, "jobPeriod": "day" }),
    )
    .await;
    assert_eq!(set.status, StatusCode::OK, "{}", set.body);
    assert_eq!(set.body["defaults"]["concurrentJobs"], json!(6));
    let own = call(
        &admin,
        Method::PUT,
        &format!("/api/admin/rig-api/tenants/{tenant}/limits"),
        None,
        json!({ "jobsPerPeriod": 3 }),
    )
    .await;
    assert_eq!(own.status, StatusCode::OK, "{}", own.body);
    assert_eq!(own.body["tenant"]["limits"]["concurrentJobs"], json!(6));
    assert_eq!(own.body["tenant"]["limits"]["jobPeriod"], json!("day"));
    assert_eq!(own.body["tenant"]["limits"]["jobsPerPeriod"], json!(3));
    assert_eq!(
        own.body["tenant"]["ownLimits"]["concurrentJobs"],
        Value::Null
    );

    let listed = call(
        &admin,
        Method::GET,
        "/api/admin/rig-api/tenants",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(listed.body["tenants"].as_array().unwrap().len(), 1);
    let unknown = call(
        &admin,
        Method::GET,
        "/api/admin/rig-api/tenants/ten_missing",
        None,
        Value::Null,
    )
    .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND);
    schema.drop().await;
}

#[tokio::test]
async fn a_job_over_a_quota_is_answered_429_with_its_code() {
    async fn read(
        response: axum::response::Response,
    ) -> (StatusCode, axum::http::HeaderMap, Value) {
        let status = response.status();
        let headers = response.headers().clone();
        let bytes = axum::body::to_bytes(response.into_body(), 1 << 16)
            .await
            .unwrap();
        (status, headers, serde_json::from_slice(&bytes).unwrap())
    }
    let (status, headers, body) = read(super::job_refused(&Refusal::Concurrency {
        limit: 2,
        running: 2,
    }))
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], "rig_api_concurrency_limited");
    assert!(headers.get(header::RETRY_AFTER).is_none());

    let resets_at = crate::clock::now() + TimeDelta::hours(2);
    let (status, headers, body) = read(super::job_refused(&Refusal::Period {
        limit: 10,
        used: 10,
        resets_at: Some(resets_at),
        retry_after_secs: Some(7200),
    }))
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], "rig_api_job_quota_exhausted");
    assert_eq!(headers[header::RETRY_AFTER], "7200");

    // A limit of none: nothing to wait for, so no Retry-After.
    let (status, headers, body) = read(super::job_refused(&Refusal::Period {
        limit: 0,
        used: 0,
        resets_at: None,
        retry_after_secs: None,
    }))
    .await;
    assert_eq!(status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(body["code"], "rig_api_job_quota_exhausted");
    assert!(headers.get(header::RETRY_AFTER).is_none());
}

#[tokio::test]
async fn a_tenant_with_no_requests_allowed_gets_429_without_retry_after() {
    let Some(schema) = schema("rig_api_http_zero").await else {
        return;
    };
    let admin = admin_as(&schema.db, 1);
    let site = site(&schema.db);
    let (_, _, secret) = tenant_with_key(&admin, json!({ "requestsPerMinute": 0 })).await;
    let refused = get_me(&site, Some(&secret)).await;
    assert_eq!(refused.status, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(refused.body["code"], "rig_api_rate_limited");
    assert!(refused.headers.get(header::RETRY_AFTER).is_none());
    schema.drop().await;
}

/// The site's router with the site's per-address limiter in front, as
/// `start_unified_server` layers it, and requests from `ip`.
fn limited_site(db: &sea_orm::DatabaseConnection) -> Router {
    site(db).layer(axum::middleware::from_fn(
        crate::middleware::rate_limit::rate_limit_middleware,
    ))
}

async fn get_me_from(app: &Router, ip: [u8; 4], key: Option<&str>) -> StatusCode {
    let mut request = Request::builder().uri("/rig-api/v1/me");
    if let Some(key) = key {
        request = request.header(header::AUTHORIZATION, format!("Bearer {key}"));
    }
    let mut request = request.body(Body::empty()).unwrap();
    request
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            ip, 40000,
        ))));
    app.clone().oneshot(request).await.unwrap().status()
}

#[tokio::test]
async fn a_good_key_is_held_to_its_tenant_not_to_the_per_address_buckets() {
    let Some(schema) = schema("rig_api_http_ip_ok").await else {
        return;
    };
    let admin = admin_as(&schema.db, 1);
    let app = limited_site(&schema.db);
    let (_, _, secret) = tenant_with_key(&admin, json!({ "requestsPerMinute": 100000 })).await;
    // More than the site's 200 a minute per address and path.
    for n in 0..260 {
        let status = get_me_from(&app, [198, 51, 100, 7], Some(&secret)).await;
        assert_eq!(status, StatusCode::OK, "request {n}");
    }
    schema.drop().await;
}

#[tokio::test]
async fn an_address_refused_too_often_is_held_off_the_rig_api() {
    let Some(schema) = schema("rig_api_http_ip_refused").await else {
        return;
    };
    let admin = admin_as(&schema.db, 1);
    let app = limited_site(&schema.db);
    let (_, _, secret) = tenant_with_key(&admin, json!({ "requestsPerMinute": 100000 })).await;
    let guesser = [203, 0, 113, 9];
    let forged = format!("mrk_{}_{}", "G".repeat(12), "h".repeat(43));
    for n in 0..60 {
        let key = if n % 2 == 0 {
            Some(forged.as_str())
        } else {
            None
        };
        assert_eq!(
            get_me_from(&app, guesser, key).await,
            StatusCode::UNAUTHORIZED,
            "request {n}"
        );
    }
    // The 61st is held off before its key is looked at, with a code.
    let mut request = Request::builder()
        .uri("/rig-api/v1/me")
        .header(header::AUTHORIZATION, format!("Bearer {forged}"))
        .body(Body::empty())
        .unwrap();
    request
        .extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            guesser, 40000,
        ))));
    let held = app.clone().oneshot(request).await.unwrap();
    assert_eq!(held.status(), StatusCode::TOO_MANY_REQUESTS);
    assert!(held.headers().get(header::RETRY_AFTER).is_some());
    let bytes = axum::body::to_bytes(held.into_body(), 1 << 16)
        .await
        .unwrap();
    let body: Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(body["code"], "rig_api_ip_rate_limited");
    // Every request from that address is held off for the window, a good key's too…
    assert_eq!(
        get_me_from(&app, guesser, Some(&secret)).await,
        StatusCode::TOO_MANY_REQUESTS
    );
    // …and another address's good key is not.
    assert_eq!(
        get_me_from(&app, [203, 0, 113, 10], Some(&secret)).await,
        StatusCode::OK
    );
    schema.drop().await;
}
