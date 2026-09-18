//! 公开申请友联；工作台订阅审核通过后才写入 phantasi_sources。

use axum::{
    Json,
    extract::{ConnectInfo, Path, Query, State},
    http::{HeaderMap, StatusCode},
};
use chrono::{Duration, Utc};
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, ConnectionTrait, DatabaseBackend,
    DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect,
    Statement, TransactionTrait,
};
use serde::Deserialize;
use serde_json::json;
use std::net::SocketAddr;

use crate::error::HttpError;
use crate::middleware::client_ip::{client_ip_from_parts, trusted_proxy_headers_enabled};
use crate::models::entities::{
    phantasi_source_applications::{self, ApplicationResponse, CreateApplicationRequest},
    phantasi_sources,
};
use crate::services::phantasi_scheduler::get_phantasi_scheduler;
use myriad_error::AppError;

use super::helpers::{
    get_admin_user_id_from_headers, get_phantasi_viewer, normalize_http_url, phantasi_http_err,
    phantasi_store_http, url_match_key,
};

const FRIEND_CATEGORY: &str = "友情链接";
const MAX_NAME: usize = 120;
const MAX_URL: usize = 2048;
const MAX_DESC: usize = 500;
const MAX_MESSAGE: usize = 1000;
const MAX_APPLICANT: usize = 80;
const MAX_EMAIL: usize = 200;
const MAX_NOTE: usize = 500;
const RATE_WINDOW: Duration = Duration::hours(1);
const RATE_MAX: u64 = 8;
const LIST_LIMIT: u64 = 300;

fn trim_opt(value: Option<String>, max: usize) -> Result<Option<String>, HttpError> {
    let Some(raw) = value else {
        return Ok(None);
    };
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }
    if trimmed.chars().count() > max {
        return Err(phantasi_http_err(
            StatusCode::BAD_REQUEST,
            "Field is too long",
        ));
    }
    Ok(Some(trimmed.to_string()))
}

fn require_text(value: &str, max: usize, empty: &str) -> Result<String, HttpError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(phantasi_http_err(StatusCode::BAD_REQUEST, empty));
    }
    if trimmed.chars().count() > max {
        return Err(phantasi_http_err(
            StatusCode::BAD_REQUEST,
            "Field is too long",
        ));
    }
    Ok(trimmed.to_string())
}

fn parse_public_url(raw: &str) -> Result<String, HttpError> {
    if raw.trim().chars().count() > MAX_URL {
        return Err(phantasi_http_err(
            StatusCode::BAD_REQUEST,
            "URL is too long",
        ));
    }
    normalize_http_url(raw)
        .map(|url| url.to_string())
        .map_err(|error| phantasi_http_err(StatusCode::BAD_REQUEST, error))
}

fn looks_like_email(raw: &str) -> bool {
    let Some((user, host)) = raw.split_once('@') else {
        return false;
    };
    !user.is_empty()
        && host.contains('.')
        && !host.starts_with('.')
        && !host.ends_with('.')
        && !host.contains(' ')
}

fn source_matches_key(source: &phantasi_sources::Model, key: &str) -> bool {
    url_match_key(&source.url) == key
        || source
            .site_url
            .as_deref()
            .is_some_and(|site| url_match_key(site) == key)
}

async fn find_existing_source<C: ConnectionTrait>(
    db: &C,
    keys: &[String],
) -> Result<Option<phantasi_sources::Model>, HttpError> {
    let sources = phantasi_sources::Entity::find()
        .all(db)
        .await
        .map_err(|error| phantasi_store_http("find existing source", error))?;
    Ok(sources
        .into_iter()
        .find(|source| keys.iter().any(|key| source_matches_key(source, key))))
}

fn unique_violation(err: &impl std::fmt::Display) -> bool {
    let lower = err.to_string().to_ascii_lowercase();
    lower.contains("23505") || lower.contains("duplicate key") || lower.contains("unique")
}

pub(crate) fn application_url_lock_key(match_key: &str) -> String {
    format!("myriad:phantasi:source_url:{match_key}")
}

async fn lock_url_match_keys<C: ConnectionTrait>(db: &C, keys: &[String]) -> Result<(), HttpError> {
    let mut sorted = keys.to_vec();
    sorted.sort();
    sorted.dedup();
    for key in sorted {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
            [application_url_lock_key(&key).into()],
        ))
        .await
        .map_err(|error| phantasi_store_http("lock application URL", error))?;
    }
    Ok(())
}

async fn find_pending_for_keys<C: ConnectionTrait>(
    db: &C,
    keys: &[String],
) -> Result<Option<phantasi_source_applications::Model>, HttpError> {
    let rows = phantasi_source_applications::Entity::find()
        .filter(phantasi_source_applications::Column::Status.eq("pending"))
        .all(db)
        .await
        .map_err(|error| phantasi_store_http("find pending application", error))?;
    Ok(rows.into_iter().find(|row| {
        keys.iter().any(|key| {
            url_match_key(&row.site_url) == *key
                || row
                    .feed_url
                    .as_deref()
                    .is_some_and(|feed| url_match_key(feed) == *key)
        })
    }))
}

async fn create_friend_source<C: ConnectionTrait>(
    db: &C,
    admin_id: i32,
    app: &phantasi_source_applications::Model,
) -> Result<phantasi_sources::Model, HttpError> {
    let now = Utc::now();
    let has_feed = app
        .feed_url
        .as_deref()
        .is_some_and(|url| !url.trim().is_empty());
    let source_type = if has_feed {
        phantasi_sources::SourceType::Rss
    } else {
        phantasi_sources::SourceType::Link
    };
    let url = if has_feed {
        app.feed_url.clone().unwrap_or_else(|| app.site_url.clone())
    } else {
        app.site_url.clone()
    };
    let new_source = phantasi_sources::ActiveModel {
        user_id: Set(admin_id),
        name: Set(app.site_name.clone()),
        url: Set(url),
        feed_type: Set(phantasi_sources::FeedType::Rss),
        source_type: Set(source_type.clone()),
        category: Set(Some(FRIEND_CATEGORY.to_string())),
        description: Set(app.description.clone()),
        site_url: Set(Some(app.site_url.clone())),
        update_interval: Set(if has_feed { 30 } else { 0 }),
        enabled: Set(true),
        error_count: Set(0),
        item_count: Set(0),
        unread_count: Set(0),
        admin_only: Set(false),
        created_at: Set(now.into()),
        updated_at: Set(now.into()),
        ..Default::default()
    };
    match new_source.insert(db).await {
        Ok(source) => Ok(source),
        Err(error) if unique_violation(&error) => {
            let mut keys = vec![url_match_key(&app.site_url)];
            if let Some(feed) = app.feed_url.as_deref() {
                let key = url_match_key(feed);
                if !keys.contains(&key) {
                    keys.push(key);
                }
            }
            find_existing_source(db, &keys)
                .await?
                .ok_or_else(|| phantasi_store_http("save source", error))
        }
        Err(error) => Err(phantasi_store_http("save source", error)),
    }
}

/// 公开申请友联。可匿名；登录则记下申请人。
pub(crate) async fn create_application(
    State(db): State<DatabaseConnection>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    headers: HeaderMap,
    Json(req): Json<CreateApplicationRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let (user_id, _) = get_phantasi_viewer(&headers, &db).await?;
    let site_name = require_text(&req.site_name, MAX_NAME, "Site name is required")?;
    let site_url = parse_public_url(&req.site_url)?;
    let feed_url = match trim_opt(req.feed_url, MAX_URL)? {
        Some(raw) => Some(parse_public_url(&raw)?),
        None => None,
    };
    let description = trim_opt(req.description, MAX_DESC)?;
    let message = trim_opt(req.message, MAX_MESSAGE)?;
    let applicant_name = trim_opt(req.applicant_name, MAX_APPLICANT)?;
    let applicant_email = match trim_opt(req.applicant_email, MAX_EMAIL)? {
        Some(email) if looks_like_email(&email) => Some(email),
        Some(_) => {
            return Err(phantasi_http_err(
                StatusCode::BAD_REQUEST,
                "Email looks invalid",
            ));
        }
        None => None,
    };

    let mut keys = vec![url_match_key(&site_url)];
    if let Some(feed) = feed_url.as_deref() {
        let key = url_match_key(feed);
        if !keys.contains(&key) {
            keys.push(key);
        }
    }

    // 与 rate_limit 的 extract_client_ip 同一套：TCP peer + 可信代理头。
    // peer 不能是 None，否则 should_trust_proxy_headers 直接失败，限流和审计都空。
    let applicant_ip =
        client_ip_from_parts(&headers, Some(peer.ip()), trusted_proxy_headers_enabled())
            .map(|ip| ip.to_string());

    let txn = db
        .begin()
        .await
        .map_err(|error| phantasi_store_http("begin application create", error))?;
    let row = async {
        lock_url_match_keys(&txn, &keys).await?;
        if find_existing_source(&txn, &keys).await?.is_some() {
            return Err(HttpError::from((
                StatusCode::CONFLICT,
                Json(AppError::fail_json("This site is already listed")),
            )));
        }
        if find_pending_for_keys(&txn, &keys).await?.is_some() {
            return Err(HttpError::from((
                StatusCode::CONFLICT,
                Json(AppError::fail_json("An application is already pending")),
            )));
        }
        if let Some(ip) = applicant_ip.as_deref() {
            let since = Utc::now() - RATE_WINDOW;
            let recent = phantasi_source_applications::Entity::find()
                .filter(phantasi_source_applications::Column::ApplicantIp.eq(ip))
                .filter(phantasi_source_applications::Column::CreatedAt.gt(since))
                .count(&txn)
                .await
                .map_err(|error| phantasi_store_http("count applications", error))?;
            if recent >= RATE_MAX {
                return Err(phantasi_http_err(
                    StatusCode::TOO_MANY_REQUESTS,
                    "Too many applications. Try again later.",
                ));
            }
        }

        let now = Utc::now();
        let row = phantasi_source_applications::ActiveModel {
            kind: Set("friend".into()),
            status: Set("pending".into()),
            site_name: Set(site_name),
            site_url: Set(site_url),
            feed_url: Set(feed_url),
            description: Set(description),
            message: Set(message),
            applicant_name: Set(applicant_name),
            applicant_email: Set(applicant_email),
            applicant_user_id: Set(user_id),
            applicant_ip: Set(applicant_ip),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            ..Default::default()
        }
        .insert(&txn)
        .await;
        match row {
            Ok(row) => Ok(row),
            Err(error) if unique_violation(&error) => Err(HttpError::from((
                StatusCode::CONFLICT,
                Json(AppError::fail_json("An application is already pending")),
            ))),
            Err(error) => Err(phantasi_store_http("save application", error)),
        }
    }
    .await;
    let row = match row {
        Ok(row) => {
            txn.commit()
                .await
                .map_err(|error| phantasi_store_http("commit application create", error))?;
            row
        }
        Err(error) => {
            if let Err(rollback) = txn.rollback().await {
                tracing::warn!(error = %rollback, "application create rollback failed");
            }
            return Err(error);
        }
    };

    let public = ApplicationResponse::from_model(row, false);
    Ok(Json(json!({
        "success": true,
        "application": {
            "id": public.id,
            "status": public.status,
        }
    })))
}

#[derive(Deserialize, Default)]
pub(crate) struct AdminApplicationsQuery {
    q: Option<String>,
    status: Option<String>,
}

/// 工作台订阅审核列表。仅站长。
pub(crate) async fn list_applications(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Query(query): Query<AdminApplicationsQuery>,
) -> Result<Json<serde_json::Value>, HttpError> {
    get_admin_user_id_from_headers(&headers, &db).await?;
    let mut rows = phantasi_source_applications::Entity::find()
        .order_by_desc(phantasi_source_applications::Column::CreatedAt)
        .limit(LIST_LIMIT)
        .all(&db)
        .await
        .map_err(|error| phantasi_store_http("list applications", error))?;

    if let Some(status) = query
        .status
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "all")
    {
        rows.retain(|row| row.status == status);
    }
    if let Some(needle) = query
        .q
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.to_ascii_lowercase())
    {
        rows.retain(|row| {
            [
                row.site_name.as_str(),
                row.site_url.as_str(),
                row.feed_url.as_deref().unwrap_or(""),
                row.description.as_deref().unwrap_or(""),
                row.message.as_deref().unwrap_or(""),
                row.applicant_name.as_deref().unwrap_or(""),
                row.applicant_email.as_deref().unwrap_or(""),
            ]
            .iter()
            .any(|field| field.to_ascii_lowercase().contains(&needle))
        });
    }

    rows.sort_by(|a, b| {
        status_rank(&a.status)
            .cmp(&status_rank(&b.status))
            .then(b.created_at.cmp(&a.created_at))
    });

    let applications: Vec<ApplicationResponse> = rows
        .into_iter()
        .map(|row| ApplicationResponse::from_model(row, true))
        .collect();
    Ok(Json(
        json!({ "success": true, "applications": applications }),
    ))
}

fn status_rank(status: &str) -> u8 {
    match status {
        "pending" => 0,
        "rejected" => 1,
        _ => 2,
    }
}

async fn load_application(
    db: &DatabaseConnection,
    id: i32,
) -> Result<phantasi_source_applications::Model, HttpError> {
    phantasi_source_applications::Entity::find_by_id(id)
        .one(db)
        .await
        .map_err(|error| phantasi_store_http("find application", error))?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::NOT_FOUND,
                Json(AppError::fail_json("Application not found")),
            ))
        })
}

/// 通过申请：有 RSS 建订阅，没有建入口型，分类都是友情链接。
pub(crate) async fn approve_application(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(id): Path<i32>,
    Json(req): Json<phantasi_source_applications::ReviewApplicationRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let admin_id = get_admin_user_id_from_headers(&headers, &db).await?;
    let review_note = trim_opt(req.review_note, MAX_NOTE)?;
    let (updated, source, created) =
        approve_pending_application(&db, admin_id, id, review_note).await?;
    // Fetching is an external side effect and must happen only after the review commits.
    if created && source.source_type.is_fetchable() {
        if let Some(scheduler) = get_phantasi_scheduler() {
            let _ = scheduler.refresh_source(source.id).await;
        }
    }
    let response: phantasi_sources::SourceResponse = source.into();
    Ok(Json(json!({
        "success": true,
        "application": ApplicationResponse::from_model(updated, true),
        "source": response,
    })))
}

async fn approve_pending_application(
    db: &DatabaseConnection,
    admin_id: i32,
    id: i32,
    review_note: Option<String>,
) -> Result<
    (
        phantasi_source_applications::Model,
        phantasi_sources::Model,
        bool,
    ),
    HttpError,
> {
    let txn = db
        .begin()
        .await
        .map_err(|error| phantasi_store_http("begin application approval", error))?;
    let outcome = async {
        let app = phantasi_source_applications::Entity::find_by_id(id)
            .lock_exclusive()
            .one(&txn)
            .await
            .map_err(|error| phantasi_store_http("find application", error))?
            .ok_or_else(|| phantasi_http_err(StatusCode::NOT_FOUND, "Application not found"))?;
        if app.status != "pending" {
            return Err(phantasi_http_err(
                StatusCode::CONFLICT,
                "Application is not pending",
            ));
        }
        let mut keys = vec![url_match_key(&app.site_url)];
        if let Some(feed) = app.feed_url.as_deref() {
            let key = url_match_key(feed);
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        lock_url_match_keys(&txn, &keys).await?;
        let (source, created) = match find_existing_source(&txn, &keys).await? {
            Some(existing) => (existing, false),
            None => (create_friend_source(&txn, admin_id, &app).await?, true),
        };
        let now = Utc::now();
        let mut active: phantasi_source_applications::ActiveModel = app.into();
        active.status = Set("approved".into());
        active.result_source_id = Set(Some(source.id));
        active.review_note = Set(review_note);
        active.reviewed_by = Set(Some(admin_id));
        active.reviewed_at = Set(Some(now.into()));
        active.updated_at = Set(now.into());
        let updated = active
            .update(&txn)
            .await
            .map_err(|error| phantasi_store_http("approve application", error))?;
        Ok((updated, source, created))
    }
    .await;
    match outcome {
        Ok(reviewed) => {
            txn.commit()
                .await
                .map_err(|error| phantasi_store_http("commit application approval", error))?;
            Ok(reviewed)
        }
        Err(error) => {
            if let Err(rollback) = txn.rollback().await {
                tracing::warn!(error = %rollback, "application approval rollback failed");
            }
            Err(error)
        }
    }
}

pub(crate) async fn reject_application(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(id): Path<i32>,
    Json(req): Json<phantasi_source_applications::ReviewApplicationRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let admin_id = get_admin_user_id_from_headers(&headers, &db).await?;
    let review_note = trim_opt(req.review_note, MAX_NOTE)?;
    let updated = reject_pending_application(&db, admin_id, id, review_note).await?;
    Ok(Json(json!({
        "success": true,
        "application": ApplicationResponse::from_model(updated, true),
    })))
}

async fn reject_pending_application(
    db: &DatabaseConnection,
    admin_id: i32,
    id: i32,
    review_note: Option<String>,
) -> Result<phantasi_source_applications::Model, HttpError> {
    let now = Utc::now();
    let mut active = <phantasi_source_applications::ActiveModel as Default>::default();
    active.status = Set("rejected".into());
    active.review_note = Set(review_note);
    active.reviewed_by = Set(Some(admin_id));
    active.reviewed_at = Set(Some(now.into()));
    active.updated_at = Set(now.into());
    let result = phantasi_source_applications::Entity::update_many()
        .set(active)
        .filter(phantasi_source_applications::Column::Id.eq(id))
        .filter(phantasi_source_applications::Column::Status.eq("pending"))
        .exec(db)
        .await
        .map_err(|error| phantasi_store_http("reject application", error))?;
    let saved = load_application(db, id).await?;
    if result.rows_affected == 0 {
        return Err(phantasi_http_err(
            StatusCode::CONFLICT,
            "Application is not pending",
        ));
    }
    Ok(saved)
}

pub(crate) async fn delete_application(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    get_admin_user_id_from_headers(&headers, &db).await?;
    let result = phantasi_source_applications::Entity::delete_by_id(id)
        .exec(&db)
        .await
        .map_err(|error| phantasi_store_http("delete application", error))?;
    if result.rows_affected == 0 {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::fail_json("Application not found")),
        )));
    }
    Ok(Json(json!({ "success": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn isolated_application_db() -> Option<DatabaseConnection> {
        use sea_orm::{ConnectOptions, ConnectionTrait, Database, DatabaseBackend, Schema};
        let url = std::env::var("PHANTASI_TEST_DATABASE_URL").ok()?;
        let mut options = ConnectOptions::new(url);
        options
            .max_connections(1)
            .min_connections(1)
            .sqlx_logging(false);
        let db = Database::connect(options).await.unwrap();
        let schema = Schema::new(DatabaseBackend::Postgres);
        for statement in [
            schema.create_table_from_entity(phantasi_sources::Entity),
            schema.create_table_from_entity(phantasi_source_applications::Entity),
        ] {
            let sql = statement
                .to_string(sea_orm::sea_query::PostgresQueryBuilder)
                .replacen("CREATE TABLE", "CREATE TEMP TABLE", 1);
            db.execute_unprepared(&sql).await.unwrap();
        }
        db.execute_unprepared(
            r#"
CREATE UNIQUE INDEX IF NOT EXISTS idx_phantasi_source_applications_pending_site
    ON phantasi_source_applications (regexp_replace(site_url, '/+$', ''))
    WHERE status = 'pending';
CREATE UNIQUE INDEX IF NOT EXISTS idx_phantasi_source_applications_pending_feed
    ON phantasi_source_applications (regexp_replace(feed_url, '/+$', ''))
    WHERE status = 'pending' AND feed_url IS NOT NULL AND btrim(feed_url) <> '';
"#,
        )
        .await
        .unwrap();
        Some(db)
    }

    async fn pending_application(db: &DatabaseConnection) -> phantasi_source_applications::Model {
        let now = Utc::now();
        phantasi_source_applications::ActiveModel {
            kind: Set("friend".into()),
            status: Set("pending".into()),
            site_name: Set("Test friend".into()),
            site_url: Set("https://friend.example".into()),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            ..Default::default()
        }
        .insert(db)
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn application_failure_does_not_leave_an_approved_source() {
        use sea_orm::ConnectionTrait;
        let Some(db) = isolated_application_db().await else {
            return;
        };
        let app = pending_application(&db).await;
        db.execute_unprepared("ALTER TABLE phantasi_source_applications ADD CONSTRAINT reject_test_approval CHECK (status <> 'approved')").await.unwrap();
        assert!(
            approve_pending_application(&db, 1, app.id, None)
                .await
                .is_err()
        );
        assert_eq!(
            phantasi_sources::Entity::find().count(&db).await.unwrap(),
            0,
            "an application update failure must roll back its new source"
        );
        assert_eq!(load_application(&db, app.id).await.unwrap(), app);
    }

    #[tokio::test]
    async fn application_approval_is_linked_and_cannot_be_repeated() {
        let Some(db) = isolated_application_db().await else {
            return;
        };
        let app = pending_application(&db).await;
        let (reviewed, source, created) = approve_pending_application(&db, 1, app.id, None)
            .await
            .unwrap();
        assert!(created);
        assert_eq!(reviewed.status, "approved");
        assert_eq!(reviewed.result_source_id, Some(source.id));
        assert_eq!(source.category.as_deref(), Some("友情链接"));
        assert_eq!(source.source_type, phantasi_sources::SourceType::Link);
        let error = approve_pending_application(&db, 2, app.id, None)
            .await
            .unwrap_err();
        assert_eq!(error.0.status(), StatusCode::CONFLICT);
        assert_eq!(
            phantasi_sources::Entity::find().count(&db).await.unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn application_rejection_cannot_overwrite_a_completed_review() {
        let Some(db) = isolated_application_db().await else {
            return;
        };
        let app = pending_application(&db).await;
        let (approved, _, _) = approve_pending_application(&db, 1, app.id, None)
            .await
            .unwrap();
        let error = reject_pending_application(&db, 2, app.id, Some("stale review".into()))
            .await
            .unwrap_err();
        assert_eq!(error.0.status(), StatusCode::CONFLICT);
        assert_eq!(load_application(&db, app.id).await.unwrap(), approved);

        let another = pending_application(&db).await;
        let rejected = reject_pending_application(&db, 2, another.id, Some("declined".into()))
            .await
            .unwrap();
        assert_eq!(rejected.status, "rejected");
        assert_eq!(rejected.reviewed_by, Some(2));
        assert_eq!(rejected.review_note.as_deref(), Some("declined"));
        let error = approve_pending_application(&db, 1, another.id, None)
            .await
            .unwrap_err();
        assert_eq!(error.0.status(), StatusCode::CONFLICT);
        assert_eq!(
            phantasi_sources::Entity::find().count(&db).await.unwrap(),
            1
        );
    }

    #[tokio::test]
    async fn application_approval_reuses_an_existing_source() {
        let Some(db) = isolated_application_db().await else {
            return;
        };
        let first = pending_application(&db).await;
        let (_, source, _) = approve_pending_application(&db, 1, first.id, None)
            .await
            .unwrap();
        let second = pending_application(&db).await;
        let (reviewed, reused, created) = approve_pending_application(&db, 2, second.id, None)
            .await
            .unwrap();
        assert!(
            !created,
            "reused sources must not trigger first-fetch side effects"
        );
        assert_eq!(source.id, reused.id);
        assert_eq!(reviewed.result_source_id, Some(source.id));
        assert_eq!(
            phantasi_sources::Entity::find().count(&db).await.unwrap(),
            1
        );
    }

    #[test]
    fn email_needs_user_and_host() {
        assert!(looks_like_email("hi@example.com"));
        assert!(!looks_like_email("nope"));
        assert!(!looks_like_email("@example.com"));
        assert!(!looks_like_email("hi@localhost"));
    }

    #[test]
    fn public_url_rejects_schemes_and_secrets() {
        assert!(parse_public_url("https://ok.example/blog").is_ok());
        assert!(parse_public_url("ok.example").is_ok());
        assert!(parse_public_url("javascript:alert(1)").is_err());
        assert!(parse_public_url("https://user:pass@ok.example").is_err());
    }

    #[test]
    fn review_routes_are_registered() {
        let src = include_str!("routes.rs");
        assert!(src.contains("/applications"));
        assert!(src.contains("/applications/{id}/approve"));
        assert!(src.contains("/applications/{id}/reject"));
        assert!(src.contains("applications::create_application"));
        assert!(src.contains("applications::list_applications"));
    }

    #[test]
    fn create_application_binds_tcp_peer() {
        let src = include_str!("applications.rs");
        let handler = src.split("#[cfg(test)]").next().expect("handler");
        assert!(handler.contains("ConnectInfo(peer)"));
        assert!(handler.contains("Some(peer.ip())"));
        assert!(!handler.contains(", None, trusted_proxy_headers_enabled()"));
    }

    #[test]
    fn create_and_approve_lock_normalized_url_in_the_write_transaction() {
        let src = include_str!("applications.rs");
        let create = src
            .split("pub(crate) async fn create_application")
            .nth(1)
            .and_then(|rest| rest.split("pub(crate) async fn list_applications").next())
            .expect("create_application");
        assert!(create.contains("lock_url_match_keys"));
        assert!(create.contains("begin application create"));
        assert!(create.contains("unique_violation"));
        let approve = src
            .split("async fn approve_pending_application")
            .nth(1)
            .and_then(|rest| rest.split("pub(crate) async fn reject_application").next())
            .expect("approve_pending_application");
        assert!(approve.contains("lock_url_match_keys"));
        let lock_at = approve.find("lock_url_match_keys").expect("lock");
        let create_at = approve.find("create_friend_source").expect("create source");
        assert!(lock_at < create_at);
    }

    #[test]
    fn application_url_lock_key_is_stable() {
        assert_eq!(
            application_url_lock_key("https://example.com/blog"),
            "myriad:phantasi:source_url:https://example.com/blog"
        );
    }

    #[tokio::test]
    async fn pending_application_url_is_unique() {
        let Some(db) = isolated_application_db().await else {
            return;
        };
        let _ = pending_application(&db).await;
        let now = Utc::now();
        let duplicate = phantasi_source_applications::ActiveModel {
            kind: Set("friend".into()),
            status: Set("pending".into()),
            site_name: Set("Copy".into()),
            site_url: Set("https://friend.example/".into()),
            created_at: Set(now.into()),
            updated_at: Set(now.into()),
            ..Default::default()
        }
        .insert(&db)
        .await;
        assert!(
            duplicate.is_err(),
            "trailing-slash URL must collide with the pending unique index"
        );
    }

    #[test]
    fn rate_limit_needs_tcp_peer() {
        use crate::middleware::client_ip::client_ip_from_parts_with_allowlist;
        let headers = HeaderMap::new();
        assert_eq!(
            client_ip_from_parts_with_allowlist(&headers, None, true, &[]),
            None
        );
        let peer = "203.0.113.9".parse().unwrap();
        assert_eq!(
            client_ip_from_parts_with_allowlist(&headers, Some(peer), false, &[]),
            Some(peer)
        );
    }
}
