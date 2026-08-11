// Admin analytics summary, visitor card, export, and import.

use axum::{
    extract::{ConnectInfo, Query, Request},
    http::{header, StatusCode},
    Json,
};
use chrono::{Duration, NaiveDate, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait,
    Value as SeaValue,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Instant;

use super::backup_integrity::{
    content_hash, day_ok_for_import, metric_ok, prevalidate_rows, seal_integrity,
    validate_counts_object, verify_integrity, MAX_METRIC_VALUE,
};
use super::intake_helpers::{
    analytics_collection_enabled, analytics_today, analytics_tz_label, compare_range_kind,
    count_distinct_site, invalidate_summary_cache, metric_delta, normalize_country_code,
    normalize_country_name, normalize_event_name, normalize_path, normalize_referrer_host,
    normalize_target, read_visitor_ordinal, resolve_visitor_hash, sum_page_views,
    ANALYTICS_BACKUP_FORMAT, ANALYTICS_BACKUP_VERSION, DAILY_RETENTION_DAYS, ENGAGE_MARKER,
    MAX_IMPORT_COUNTRY_DAILY, MAX_IMPORT_COUNTRY_VISITOR, MAX_IMPORT_EVENT_DAILY,
    MAX_IMPORT_EVENT_VISITOR, MAX_IMPORT_PAGE_DAILY, MAX_IMPORT_REFERRER_DAILY,
    MAX_IMPORT_VISITOR_SEEN, MAX_SUMMARY_DAYS, SITE_PATH, SUMMARY_CACHE, SUMMARY_CACHE_TTL,
    SummaryQuery, VISITOR_CARD_CACHE, VISITOR_RETENTION_DAYS,
};

/// GET /api/analytics/summary?days=7  or  ?from=YYYY-MM-DD&to=YYYY-MM-DD
pub async fn get_summary(
    crate::extract::Db(db): crate::extract::Db,
    Query(q): Query<SummaryQuery>,
) -> (StatusCode, Json<Value>) {
    build_analytics_summary(&db, q).await
}

/// Shared analytics summary builder (admin UI + Tapp runtime API).
///
/// Aggregates only — never includes visitor hashes or raw identity material.
pub(crate) async fn build_analytics_summary(
    db: &DatabaseConnection,
    q: SummaryQuery,
) -> (StatusCode, Json<Value>) {
    use super::intake_helpers::resolve_analytics_window;

    let (from, to_day, days) = resolve_analytics_window(
        q.days,
        q.from.as_deref(),
        q.to.as_deref(),
    );
    let cache_key = format!(
        "{}..{}",
        from.format("%Y-%m-%d"),
        to_day.format("%Y-%m-%d")
    );

    // Short TTL cache — admin UI refresh shouldn't re-scan every open.
    {
        let cache = SUMMARY_CACHE.lock().await;
        if let Some((at, body)) = cache.get(&cache_key) {
            if at.elapsed() < SUMMARY_CACHE_TTL {
                return (StatusCode::OK, Json(body.clone()));
            }
        }
    }

    let today = to_day; // end of selected window (for fill loop / “today” tiles)
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or(from);
    let tz_label = analytics_tz_label();

    let daily_rows = match db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT day::text AS day,
       COALESCE(SUM(CASE WHEN path <> $3 THEN views ELSE 0 END), 0)::bigint AS views,
       COALESCE(MAX(CASE WHEN path = $3 THEN unique_visitors ELSE 0 END), 0)::bigint AS unique_visitors,
       COALESCE(SUM(CASE WHEN path <> $3 THEN engagement_ms ELSE 0 END), 0)::bigint AS engagement_ms,
       COALESCE(SUM(CASE WHEN path <> $3 THEN engaged_views ELSE 0 END), 0)::bigint AS engaged_views
FROM analytics_page_daily
WHERE day >= $1 AND day <= $2
GROUP BY day
ORDER BY day ASC
"#,
            [
                SeaValue::from(from),
                SeaValue::from(today),
                SeaValue::from(SITE_PATH.to_string()),
            ],
        ))
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("analytics summary daily failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": "db_error" })),
            );
        }
    };

    let mut daily = Vec::new();
    let mut range_views: i64 = 0;
    let mut range_engagement_ms: i64 = 0;
    let mut range_engaged_views: i64 = 0;
    for row in &daily_rows {
        let day: String = row.try_get("", "day").unwrap_or_default();
        let views: i64 = row.try_get("", "views").unwrap_or(0);
        let uv: i64 = row.try_get("", "unique_visitors").unwrap_or(0);
        let eng: i64 = row.try_get("", "engagement_ms").unwrap_or(0);
        let eng_v: i64 = row.try_get("", "engaged_views").unwrap_or(0);
        range_views += views;
        range_engagement_ms += eng;
        range_engaged_views += eng_v;
        daily.push(json!({
            "day": day,
            "views": views,
            "unique_visitors": uv,
            "engagement_ms": eng,
        }));
    }

    let mut filled = Vec::new();
    let mut cursor = from;
    let by_day: HashMap<String, &Value> = daily
        .iter()
        .filter_map(|v| v.get("day").and_then(|d| d.as_str()).map(|d| (d.to_string(), v)))
        .collect();
    while cursor <= today {
        let key = cursor.format("%Y-%m-%d").to_string();
        if let Some(v) = by_day.get(&key) {
            filled.push((*v).clone());
        } else {
            filled.push(json!({
                "day": key,
                "views": 0,
                "unique_visitors": 0,
                "engagement_ms": 0,
            }));
        }
        cursor += Duration::days(1);
    }

    let today_views = filled
        .last()
        .and_then(|v| v.get("views").and_then(|x| x.as_i64()))
        .unwrap_or(0);
    let today_uv = filled
        .last()
        .and_then(|v| v.get("unique_visitors").and_then(|x| x.as_i64()))
        .unwrap_or(0);

    let range_uv = count_distinct_site(db, from, today).await;

    // 环比：今日 vs 前一日；当前区间 vs 等长上一区间。
    // kind: day / week / month / period — FE maps to 日/周/月/较上期.
    let prev_day = today - Duration::days(1);
    let prev_range_to = from - Duration::days(1);
    let prev_range_from = prev_range_to - Duration::days(days - 1);
    let prev_day_views = sum_page_views(db, prev_day, prev_day).await;
    let prev_day_uv = count_distinct_site(db, prev_day, prev_day).await;
    let prev_range_views = sum_page_views(db, prev_range_from, prev_range_to).await;
    let prev_range_uv = count_distinct_site(db, prev_range_from, prev_range_to).await;
    let compare = json!({
        "day": {
            "kind": "day",
            "views": metric_delta(today_views, prev_day_views),
            "unique_visitors": metric_delta(today_uv, prev_day_uv),
        },
        "range": {
            "kind": compare_range_kind(days),
            "views": metric_delta(range_views, prev_range_views),
            "unique_visitors": metric_delta(range_uv, prev_range_uv),
        },
    });
    // Avg dwell among visits that reported engagement (engaged_views = unique
    // visitor×path×day with ≥1 engagement flush, not per soft-flush).
    let avg_engagement_ms = if range_engaged_views > 0 {
        range_engagement_ms / range_engaged_views
    } else {
        0
    };
    // Shallow-visit approx: pageviews with no engagement credit yet.
    // engaged_views can exceed views slightly under race; clamp.
    let short_engage = if range_views > 0 {
        let unengaged = (range_views - range_engaged_views.min(range_views)).max(0);
        (unengaged as f64 / range_views as f64 * 1000.0).round() as i64
    } else {
        0
    };

    let page_view_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT path,
       COALESCE(SUM(views), 0)::bigint AS views,
       COALESCE(SUM(engagement_ms), 0)::bigint AS engagement_ms,
       COALESCE(SUM(engaged_views), 0)::bigint AS engaged_views
FROM analytics_page_daily
WHERE day >= $1 AND day <= $2 AND path <> $3
GROUP BY path
ORDER BY views DESC, path ASC
LIMIT 50
"#,
            [
                SeaValue::from(from),
                SeaValue::from(today),
                SeaValue::from(SITE_PATH.to_string()),
            ],
        ))
        .await
        .unwrap_or_default();

    let page_uv_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT path, COUNT(DISTINCT visitor_hash)::bigint AS unique_visitors
FROM analytics_visitor_seen
WHERE day >= $1 AND day <= $2 AND path <> $3
GROUP BY path
"#,
            [
                SeaValue::from(from),
                SeaValue::from(today),
                SeaValue::from(SITE_PATH.to_string()),
            ],
        ))
        .await
        .unwrap_or_default();

    let mut uv_by_path: HashMap<String, i64> = HashMap::new();
    for row in &page_uv_rows {
        let p: String = row.try_get("", "path").unwrap_or_default();
        let n: i64 = row.try_get("", "unique_visitors").unwrap_or(0);
        uv_by_path.insert(p, n);
    }

    let pages: Vec<Value> = page_view_rows
        .iter()
        .map(|row| {
            let path: String = row.try_get("", "path").unwrap_or_default();
            let views: i64 = row.try_get("", "views").unwrap_or(0);
            let eng: i64 = row.try_get("", "engagement_ms").unwrap_or(0);
            let eng_v: i64 = row.try_get("", "engaged_views").unwrap_or(0);
            let avg = if eng_v > 0 { eng / eng_v } else { 0 };
            json!({
                "path": path,
                "views": views,
                "unique_visitors": uv_by_path.get(&path).copied().unwrap_or(0),
                "avg_engagement_ms": avg,
            })
        })
        .collect();

    // Events (aggregate name across paths; optional target breakdown)
    let event_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT event_name,
       COALESCE(SUM(count), 0)::bigint AS count
FROM analytics_event_daily
WHERE day >= $1 AND day <= $2 AND event_name <> $3
GROUP BY event_name
ORDER BY count DESC, event_name ASC
LIMIT 30
"#,
            [
                SeaValue::from(from),
                SeaValue::from(today),
                SeaValue::from(ENGAGE_MARKER.to_string()),
            ],
        ))
        .await
        .unwrap_or_default();

    let event_uv_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT event_name, COUNT(DISTINCT visitor_hash)::bigint AS unique_visitors
FROM analytics_event_visitor
WHERE day >= $1 AND day <= $2 AND event_name <> $3
GROUP BY event_name
"#,
            [
                SeaValue::from(from),
                SeaValue::from(today),
                SeaValue::from(ENGAGE_MARKER.to_string()),
            ],
        ))
        .await
        .unwrap_or_default();
    let mut event_uv: HashMap<String, i64> = HashMap::new();
    for row in &event_uv_rows {
        let n: String = row.try_get("", "event_name").unwrap_or_default();
        let c: i64 = row.try_get("", "unique_visitors").unwrap_or(0);
        event_uv.insert(n, c);
    }

    // Per (event_name, target) counts — only non-empty targets for UI drill-down.
    let event_target_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT event_name,
       target,
       COALESCE(SUM(count), 0)::bigint AS count
FROM analytics_event_daily
WHERE day >= $1 AND day <= $2
  AND event_name <> $3
  AND target <> ''
GROUP BY event_name, target
ORDER BY event_name ASC, count DESC, target ASC
"#,
            [
                SeaValue::from(from),
                SeaValue::from(today),
                SeaValue::from(ENGAGE_MARKER.to_string()),
            ],
        ))
        .await
        .unwrap_or_default();
    let event_target_uv_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT event_name,
       target,
       COUNT(DISTINCT visitor_hash)::bigint AS unique_visitors
FROM analytics_event_visitor
WHERE day >= $1 AND day <= $2
  AND event_name <> $3
  AND target <> ''
GROUP BY event_name, target
"#,
            [
                SeaValue::from(from),
                SeaValue::from(today),
                SeaValue::from(ENGAGE_MARKER.to_string()),
            ],
        ))
        .await
        .unwrap_or_default();
    let mut event_target_uv: HashMap<(String, String), i64> = HashMap::new();
    for row in &event_target_uv_rows {
        let n: String = row.try_get("", "event_name").unwrap_or_default();
        let t: String = row.try_get("", "target").unwrap_or_default();
        let c: i64 = row.try_get("", "unique_visitors").unwrap_or(0);
        event_target_uv.insert((n, t), c);
    }
    let mut targets_by_event: HashMap<String, Vec<Value>> = HashMap::new();
    for row in &event_target_rows {
        let name: String = row.try_get("", "event_name").unwrap_or_default();
        let target: String = row.try_get("", "target").unwrap_or_default();
        if name.is_empty() || target.is_empty() {
            continue;
        }
        let count: i64 = row.try_get("", "count").unwrap_or(0);
        let uv = event_target_uv
            .get(&(name.clone(), target.clone()))
            .copied()
            .unwrap_or(0);
        let list = targets_by_event.entry(name).or_default();
        // Cap breakdowns per event so the admin payload stays small.
        if list.len() < 20 {
            list.push(json!({
                "target": target,
                "count": count,
                "unique_visitors": uv,
            }));
        }
    }

    let events: Vec<Value> = event_rows
        .iter()
        .map(|row| {
            let name: String = row.try_get("", "event_name").unwrap_or_default();
            let targets = targets_by_event.remove(&name).unwrap_or_default();
            json!({
                "name": name,
                "count": row.try_get::<i64>("", "count").unwrap_or(0),
                "unique_visitors": event_uv.get(&name).copied().unwrap_or(0),
                "targets": targets,
            })
        })
        .collect();

    let referrer_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT host, COALESCE(SUM(count), 0)::bigint AS count
FROM analytics_referrer_daily
WHERE day >= $1 AND day <= $2
GROUP BY host
ORDER BY count DESC, host ASC
LIMIT 20
"#,
            [SeaValue::from(from), SeaValue::from(today)],
        ))
        .await
        .unwrap_or_default();
    let referrers: Vec<Value> = referrer_rows
        .iter()
        .map(|row| {
            json!({
                "host": row.try_get::<String>("", "host").unwrap_or_default(),
                "count": row.try_get::<i64>("", "count").unwrap_or(0),
            })
        })
        .collect();

    // Top countries by unique visitors in range (fallback views)
    let country_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT country_code,
       MAX(country_name) AS country_name,
       COALESCE(SUM(views), 0)::bigint AS views,
       COALESCE(SUM(unique_visitors), 0)::bigint AS unique_visitors
FROM analytics_country_daily
WHERE day >= $1 AND day <= $2
GROUP BY country_code
ORDER BY unique_visitors DESC, views DESC, country_code ASC
LIMIT 12
"#,
            [SeaValue::from(from), SeaValue::from(today)],
        ))
        .await
        .unwrap_or_default();
    // Prefer true distinct UV over sum-of-daily when multi-day window.
    let country_uv_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT country_code, COUNT(DISTINCT visitor_hash)::bigint AS unique_visitors
FROM analytics_country_visitor
WHERE day >= $1 AND day <= $2
GROUP BY country_code
"#,
            [SeaValue::from(from), SeaValue::from(today)],
        ))
        .await
        .unwrap_or_default();
    let mut country_uv: HashMap<String, i64> = HashMap::new();
    for row in &country_uv_rows {
        let code: String = row.try_get("", "country_code").unwrap_or_default();
        let n: i64 = row.try_get("", "unique_visitors").unwrap_or(0);
        country_uv.insert(code, n);
    }
    let mut countries: Vec<Value> = country_rows
        .iter()
        .map(|row| {
            let code: String = row.try_get("", "country_code").unwrap_or_default();
            let name: String = row.try_get("", "country_name").unwrap_or_default();
            let views: i64 = row.try_get("", "views").unwrap_or(0);
            let uv = country_uv
                .get(&code)
                .copied()
                .unwrap_or_else(|| row.try_get::<i64>("", "unique_visitors").unwrap_or(0));
            json!({
                "code": code,
                "name": if name.is_empty() { code.clone() } else { name },
                "views": views,
                "unique_visitors": uv,
            })
        })
        .collect();
    countries.sort_by(|a, b| {
        let ua = a.get("unique_visitors").and_then(|v| v.as_i64()).unwrap_or(0);
        let ub = b.get("unique_visitors").and_then(|v| v.as_i64()).unwrap_or(0);
        let va = a.get("views").and_then(|v| v.as_i64()).unwrap_or(0);
        let vb = b.get("views").and_then(|v| v.as_i64()).unwrap_or(0);
        ub.cmp(&ua)
            .then(vb.cmp(&va))
            .then(
                a.get("code")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .cmp(b.get("code").and_then(|v| v.as_str()).unwrap_or("")),
            )
    });
    if countries.len() > 12 {
        countries.truncate(12);
    }

    let all_time_views = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT COALESCE(SUM(views), 0)::bigint AS views
FROM analytics_page_daily WHERE path <> $1
"#,
            [SeaValue::from(SITE_PATH.to_string())],
        ))
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<i64>("", "views").ok())
        .unwrap_or(0);
    let all_time_uv = count_distinct_site(db, epoch, today).await;

    let body = json!({
        "success": true,
        "days": days,
        "from": from.format("%Y-%m-%d").to_string(),
        "to": today.format("%Y-%m-%d").to_string(),
        "timezone": tz_label,
        "retention": {
            "visitor_days": VISITOR_RETENTION_DAYS,
            "daily_days": DAILY_RETENTION_DAYS,
        },
        "definitions": {
            "unique_visitors": "distinct_visitor_hash",
            "range_unique": "true_distinct_over_range",
            "daily_unique": "per_server_local_calendar_day",
            "avg_engagement_ms": "sum(engagement_ms) / engaged_visitor_path_days",
            "approx_bounce_permille": "pageviews_without_engagement_credit / pageviews * 1000 (approx, not session bounce)",
            "staff_excluded": "is_admin || is_owner JWT sessions are not recorded",
        },
        "today": {
            "views": today_views,
            "unique_visitors": today_uv,
        },
        "range": {
            "views": range_views,
            "unique_visitors": range_uv,
            "engagement_ms": range_engagement_ms,
            "engaged_views": range_engaged_views,
            "avg_engagement_ms": avg_engagement_ms,
            "approx_bounce_permille": short_engage,
        },
        "compare": compare,
        "all_time": {
            "views": all_time_views,
            "unique_visitors": all_time_uv,
            "unique_visitors_note": "bounded_by_visitor_seen_retention",
        },
        "daily": filled,
        "pages": pages,
        "events": events,
        "referrers": referrers,
        "countries": countries,
    });

    {
        let mut cache = SUMMARY_CACHE.lock().await;
        cache.insert(cache_key, (Instant::now(), body.clone()));
        // Keep map small (only a few day windows are ever queried)
        if cache.len() > 12 {
            cache.retain(|_, (at, _)| at.elapsed() < SUMMARY_CACHE_TTL * 2);
        }
    }

    (StatusCode::OK, Json(body))
}

// ── Public visitor card ────────────────────────────────────────────────────

/// Trend window on the public card (kept small — it is a widget, not a report).
const VISITOR_CARD_DAYS: i64 = 5;

/// `vid` out of the raw query string.
///
/// A valid vid is `[A-Za-z0-9_-]{16,64}` (see [`is_valid_vid`]), so there is
/// nothing to percent-decode; anything that needed decoding would fail
/// validation anyway and fall through to the ip+ua fingerprint.
pub(crate) fn vid_from_query(uri: &axum::http::Uri) -> Option<String> {
    uri.query()?.split('&').find_map(|kv| {
        let (k, v) = kv.split_once('=')?;
        (k == "vid").then(|| v.to_string())
    })
}

/// Today / all-time / trend, shared by every visitor and cached briefly.
/// Also used by the Tapp analytics visitor-card endpoint.
pub(crate) async fn visitor_card_aggregate(db: &DatabaseConnection) -> Value {
    {
        let cache = VISITOR_CARD_CACHE.lock().await;
        if let Some((at, body)) = cache.as_ref() {
            if at.elapsed() < SUMMARY_CACHE_TTL {
                return body.clone();
            }
        }
    }

    let today = analytics_today();
    let from = today - Duration::days(VISITOR_CARD_DAYS - 1);
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or(from);

    let daily_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT day::text AS day,
       COALESCE(SUM(CASE WHEN path <> $3 THEN views ELSE 0 END), 0)::bigint AS views,
       COALESCE(MAX(CASE WHEN path = $3 THEN unique_visitors ELSE 0 END), 0)::bigint AS unique_visitors
FROM analytics_page_daily
WHERE day >= $1 AND day <= $2
GROUP BY day
ORDER BY day ASC
"#,
            [
                SeaValue::from(from),
                SeaValue::from(today),
                SeaValue::from(SITE_PATH.to_string()),
            ],
        ))
        .await
        .unwrap_or_default();

    let mut by_day: HashMap<String, (i64, i64)> = HashMap::new();
    for row in &daily_rows {
        let day: String = row.try_get("", "day").unwrap_or_default();
        let views: i64 = row.try_get("", "views").unwrap_or(0);
        let uv: i64 = row.try_get("", "unique_visitors").unwrap_or(0);
        by_day.insert(day, (views, uv));
    }

    // Gap-fill so the sparkline always has one column per day in the window.
    let mut daily = Vec::new();
    let mut cursor = from;
    while cursor <= today {
        let key = cursor.format("%Y-%m-%d").to_string();
        let (views, uv) = by_day.get(&key).copied().unwrap_or((0, 0));
        daily.push(json!({ "day": key, "views": views, "unique_visitors": uv }));
        cursor += Duration::days(1);
    }

    let (today_views, today_uv) = by_day
        .get(&today.format("%Y-%m-%d").to_string())
        .copied()
        .unwrap_or((0, 0));

    let all_time_views = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
SELECT COALESCE(SUM(views), 0)::bigint AS views
FROM analytics_page_daily WHERE path <> $1
"#,
            [SeaValue::from(SITE_PATH.to_string())],
        ))
        .await
        .ok()
        .flatten()
        .and_then(|r| r.try_get::<i64>("", "views").ok())
        .unwrap_or(0);
    let all_time_uv = count_distinct_site(db, epoch, today).await;

    let body = json!({
        "days": VISITOR_CARD_DAYS,
        "from": from.format("%Y-%m-%d").to_string(),
        "to": today.format("%Y-%m-%d").to_string(),
        "timezone": analytics_tz_label(),
        "today": { "views": today_views, "unique_visitors": today_uv },
        "all_time": {
            "views": all_time_views,
            "unique_visitors": all_time_uv,
            "unique_visitors_note": "bounded_by_visitor_seen_retention",
        },
        "daily": daily,
    });

    *VISITOR_CARD_CACHE.lock().await = Some((Instant::now(), body.clone()));
    body
}

/// GET `/api/analytics/visitor?vid=…` — **public** visitor card.
///
/// Deliberately narrower than the admin summary: site-wide totals, a 7-day
/// trend, and the caller's own arrival ordinal. Per-page, per-referrer,
/// per-country and engagement breakdowns stay admin-only.
///
/// Read-only — it never creates a visitor row, so polling this endpoint cannot
/// inflate the counters. `your_ordinal_today` is therefore null until the
/// visitor's own pageview beacon has landed.
pub async fn get_visitor_card(
    axum::extract::State(dynamic_config): axum::extract::State<
        std::sync::Arc<tokio::sync::RwLock<crate::config::DynamicConfig>>,
    >,
    crate::extract::Db(db): crate::extract::Db,
    ConnectInfo(_peer): ConnectInfo<SocketAddr>,
    request: Request,
) -> (StatusCode, Json<Value>) {
    if !analytics_collection_enabled(&*dynamic_config.read().await) {
        return (
            StatusCode::OK,
            Json(json!({ "success": true, "enabled": false })),
        );
    }

    // Staff sessions (admin or site owner) are never recorded (see `collect`),
    // so they have no ordinal of their own. Report that rather than a blank slot.
    let is_staff = match crate::middleware::auth::authenticate_optional_request(
        request.headers(),
        &db,
    )
    .await
    {
        Ok(claims) => claims
            .map(|claims| claims.is_admin || claims.is_owner)
            .unwrap_or(false),
        Err(response) => {
            return (
                response.status(),
                Json(json!({"success": false, "error": "Invalid authentication state"})),
            );
        }
    };
    let ip = crate::middleware::client_ip::extract_client_ip(&request);
    let ua = request
        .headers()
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let vid = vid_from_query(request.uri());

    let mut body = visitor_card_aggregate(&db).await;

    let ordinal = if is_staff {
        None
    } else if let Some(visitor) = resolve_visitor_hash(vid.as_deref(), ip, &ua) {
        read_visitor_ordinal(&db, analytics_today(), &visitor)
            .await
            .unwrap_or(None)
    } else {
        // Production without ANALYTICS_SALT: no shared default hash for ordinal lookup.
        None
    };

    if let Some(obj) = body.as_object_mut() {
        obj.insert("success".into(), json!(true));
        obj.insert("enabled".into(), json!(true));
        obj.insert("your_ordinal_today".into(), json!(ordinal));
        obj.insert("counted".into(), json!(!is_staff));
    }
    (StatusCode::OK, Json(body))
}

// ── Export / import (admin backup) ─────────────────────────────────────────

// parse_day_str / valid_visitor_hash / i64_nonneg live in intake_helpers
// (shared with backup_integrity prevalidation).
pub(crate) use super::intake_helpers::{i64_nonneg, parse_day_str, valid_visitor_hash};

/// GET /api/analytics/export — full first-party analytics backup (admin).
pub async fn export_analytics(
    crate::extract::Db(db): crate::extract::Db,
) -> (StatusCode, Json<Value>) {
    let page_daily = match db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT day::text AS day, path, views, unique_visitors, engagement_ms, engaged_views
FROM analytics_page_daily
ORDER BY day ASC, path ASC
"#
            .to_string(),
        ))
        .await
    {
        Ok(rows) => rows
            .iter()
            .map(|row| {
                json!({
                    "day": row.try_get::<String>("", "day").unwrap_or_default(),
                    "path": row.try_get::<String>("", "path").unwrap_or_default(),
                    "views": row.try_get::<i64>("", "views").unwrap_or(0),
                    "unique_visitors": row.try_get::<i64>("", "unique_visitors").unwrap_or(0),
                    "engagement_ms": row.try_get::<i64>("", "engagement_ms").unwrap_or(0),
                    "engaged_views": row.try_get::<i64>("", "engaged_views").unwrap_or(0),
                })
            })
            .collect::<Vec<_>>(),
        Err(e) => {
            tracing::warn!("analytics export page_daily failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": "db_error" })),
            );
        }
    };

    let visitor_seen = match db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT day::text AS day, path, visitor_hash, ordinal
FROM analytics_visitor_seen
ORDER BY day ASC, path ASC, visitor_hash ASC
"#
            .to_string(),
        ))
        .await
    {
        Ok(rows) => rows
            .iter()
            .map(|row| {
                json!({
                    "day": row.try_get::<String>("", "day").unwrap_or_default(),
                    "path": row.try_get::<String>("", "path").unwrap_or_default(),
                    "visitor_hash": row.try_get::<String>("", "visitor_hash").unwrap_or_default(),
                    "ordinal": row.try_get::<i64>("", "ordinal").unwrap_or(0),
                })
            })
            .collect::<Vec<_>>(),
        Err(e) => {
            tracing::warn!("analytics export visitor_seen failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": "db_error" })),
            );
        }
    };

    let event_daily = match db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT day::text AS day, event_name, path, target, count, unique_visitors
FROM analytics_event_daily
ORDER BY day ASC, event_name ASC, path ASC, target ASC
"#
            .to_string(),
        ))
        .await
    {
        Ok(rows) => rows
            .iter()
            .map(|row| {
                json!({
                    "day": row.try_get::<String>("", "day").unwrap_or_default(),
                    "event_name": row.try_get::<String>("", "event_name").unwrap_or_default(),
                    "path": row.try_get::<String>("", "path").unwrap_or_default(),
                    "target": row.try_get::<String>("", "target").unwrap_or_default(),
                    "count": row.try_get::<i64>("", "count").unwrap_or(0),
                    "unique_visitors": row.try_get::<i64>("", "unique_visitors").unwrap_or(0),
                })
            })
            .collect::<Vec<_>>(),
        Err(e) => {
            tracing::warn!("analytics export event_daily failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": "db_error" })),
            );
        }
    };

    let event_visitor = match db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT day::text AS day, event_name, path, target, visitor_hash
FROM analytics_event_visitor
ORDER BY day ASC, event_name ASC, path ASC, target ASC, visitor_hash ASC
"#
            .to_string(),
        ))
        .await
    {
        Ok(rows) => rows
            .iter()
            .map(|row| {
                json!({
                    "day": row.try_get::<String>("", "day").unwrap_or_default(),
                    "event_name": row.try_get::<String>("", "event_name").unwrap_or_default(),
                    "path": row.try_get::<String>("", "path").unwrap_or_default(),
                    "target": row.try_get::<String>("", "target").unwrap_or_default(),
                    "visitor_hash": row.try_get::<String>("", "visitor_hash").unwrap_or_default(),
                })
            })
            .collect::<Vec<_>>(),
        Err(e) => {
            tracing::warn!("analytics export event_visitor failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": "db_error" })),
            );
        }
    };

    let referrer_daily = match db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT day::text AS day, host, count
FROM analytics_referrer_daily
ORDER BY day ASC, host ASC
"#
            .to_string(),
        ))
        .await
    {
        Ok(rows) => rows
            .iter()
            .map(|row| {
                json!({
                    "day": row.try_get::<String>("", "day").unwrap_or_default(),
                    "host": row.try_get::<String>("", "host").unwrap_or_default(),
                    "count": row.try_get::<i64>("", "count").unwrap_or(0),
                })
            })
            .collect::<Vec<_>>(),
        Err(e) => {
            tracing::warn!("analytics export referrer_daily failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": "db_error" })),
            );
        }
    };

    let country_daily = match db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT day::text AS day, country_code, country_name, views, unique_visitors
FROM analytics_country_daily
ORDER BY day ASC, country_code ASC
"#
            .to_string(),
        ))
        .await
    {
        Ok(rows) => rows
            .iter()
            .map(|row| {
                json!({
                    "day": row.try_get::<String>("", "day").unwrap_or_default(),
                    "country_code": row.try_get::<String>("", "country_code").unwrap_or_default(),
                    "country_name": row.try_get::<String>("", "country_name").unwrap_or_default(),
                    "views": row.try_get::<i64>("", "views").unwrap_or(0),
                    "unique_visitors": row.try_get::<i64>("", "unique_visitors").unwrap_or(0),
                })
            })
            .collect::<Vec<_>>(),
        Err(e) => {
            tracing::warn!("analytics export country_daily failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": "db_error" })),
            );
        }
    };

    let country_visitor = match db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            r#"
SELECT day::text AS day, country_code, visitor_hash
FROM analytics_country_visitor
ORDER BY day ASC, country_code ASC, visitor_hash ASC
"#
            .to_string(),
        ))
        .await
    {
        Ok(rows) => rows
            .iter()
            .map(|row| {
                json!({
                    "day": row.try_get::<String>("", "day").unwrap_or_default(),
                    "country_code": row.try_get::<String>("", "country_code").unwrap_or_default(),
                    "visitor_hash": row.try_get::<String>("", "visitor_hash").unwrap_or_default(),
                })
            })
            .collect::<Vec<_>>(),
        Err(e) => {
            tracing::warn!("analytics export country_visitor failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": "db_error" })),
            );
        }
    };

    let bucket_today = analytics_today().format("%Y-%m-%d").to_string();

    let counts = json!({
        "page_daily": page_daily.len(),
        "visitor_seen": visitor_seen.len(),
        "event_daily": event_daily.len(),
        "event_visitor": event_visitor.len(),
        "referrer_daily": referrer_daily.len(),
        "country_daily": country_daily.len(),
        "country_visitor": country_visitor.len(),
    });

    // Instance-bound integrity: field-canonical SHA-256 sealed with data key.
    // Import rejects hand-edited tables unless the same key can open the token.
    let hash = match content_hash(
        ANALYTICS_BACKUP_FORMAT,
        ANALYTICS_BACKUP_VERSION,
        &page_daily,
        &visitor_seen,
        &event_daily,
        &event_visitor,
        &referrer_daily,
        &country_daily,
        &country_visitor,
    ) {
        Ok(h) => h,
        Err(e) => {
            tracing::warn!("analytics export content hash failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": "export_hash_failed" })),
            );
        }
    };
    let integrity = match seal_integrity(&hash) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("analytics export integrity seal failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": "export_integrity_failed" })),
            );
        }
    };

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "format": ANALYTICS_BACKUP_FORMAT,
            "version": ANALYTICS_BACKUP_VERSION,
            "exported_at": Utc::now().to_rfc3339(),
            "timezone": analytics_tz_label(),
            // Process-local calendar day used by analytics_today buckets.
            // FE backup filenames prefer this over max(day) in payload tables
            // (max day can lag when today's rows are still empty).
            "bucket_today": bucket_today,
            "counts": counts,
            "page_daily": page_daily,
            "visitor_seen": visitor_seen,
            "event_daily": event_daily,
            "event_visitor": event_visitor,
            "referrer_daily": referrer_daily,
            "country_daily": country_daily,
            "country_visitor": country_visitor,
            "integrity": integrity,
        })),
    )
}

#[derive(Debug, Deserialize)]
pub struct AnalyticsImportBody {
    pub format: Option<String>,
    pub version: Option<u32>,
    /// `replace` (default): truncate then insert. `merge`: upsert / add.
    #[serde(default)]
    pub mode: Option<String>,
    /// Declared row counts from export (optional; validated when present).
    #[serde(default)]
    pub counts: Option<Value>,
    /// Instance-bound integrity block from export (always required).
    #[serde(default)]
    pub integrity: Option<Value>,
    #[serde(default)]
    pub page_daily: Vec<Value>,
    #[serde(default)]
    pub visitor_seen: Vec<Value>,
    #[serde(default)]
    pub event_daily: Vec<Value>,
    #[serde(default)]
    pub event_visitor: Vec<Value>,
    #[serde(default)]
    pub referrer_daily: Vec<Value>,
    #[serde(default)]
    pub country_daily: Vec<Value>,
    #[serde(default)]
    pub country_visitor: Vec<Value>,
}

/// After import, unique_visitors / engaged_views come from detail tables so
/// merge never double-counts UV by summing aggregate fields.
async fn recompute_unique_metrics(
    conn: &impl ConnectionTrait,
) -> Result<(), sea_orm::DbErr> {
    conn.execute_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        r#"
UPDATE analytics_page_daily p
SET unique_visitors = COALESCE((
  SELECT COUNT(*)::bigint
  FROM analytics_visitor_seen v
  WHERE v.day = p.day AND v.path = p.path
), 0)
"#
        .to_string(),
    ))
    .await?;

    conn.execute_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        r#"
UPDATE analytics_page_daily p
SET engaged_views = COALESCE((
  SELECT COUNT(*)::bigint
  FROM analytics_event_visitor e
  WHERE e.day = p.day
    AND e.path = p.path
    AND e.event_name = '__engage__'
), 0)
"#
        .to_string(),
    ))
    .await?;

    conn.execute_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        r#"
UPDATE analytics_event_daily e
SET unique_visitors = COALESCE((
  SELECT COUNT(*)::bigint
  FROM analytics_event_visitor v
  WHERE v.day = e.day
    AND v.event_name = e.event_name
    AND v.path = e.path
    AND v.target = e.target
), 0)
"#
        .to_string(),
    ))
    .await?;

    conn.execute_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        r#"
UPDATE analytics_country_daily c
SET unique_visitors = COALESCE((
  SELECT COUNT(*)::bigint
  FROM analytics_country_visitor v
  WHERE v.day = c.day AND v.country_code = c.country_code
), 0)
"#
        .to_string(),
    ))
    .await?;

    Ok(())
}

pub(crate) fn normalize_import_path(path_raw: &str) -> Option<String> {
    if path_raw == SITE_PATH {
        Some(SITE_PATH.to_string())
    } else {
        normalize_path(path_raw)
    }
}

pub(crate) fn normalize_import_event_name(name_raw: &str) -> Option<String> {
    if name_raw == ENGAGE_MARKER {
        Some(ENGAGE_MARKER.to_string())
    } else {
        normalize_event_name(name_raw)
    }
}

pub(crate) fn normalize_import_event_path(path_raw: &str) -> Option<String> {
    if path_raw.is_empty() {
        Some(String::new())
    } else {
        normalize_path(path_raw)
    }
}

/// POST /api/analytics/import — restore or merge a backup (admin).
///
/// Entire import runs in one DB transaction (truncate + inserts + UV recompute).
/// On any database error the transaction is rolled back so replace never leaves
/// a half-wiped table set.
///
/// Integrity is always required: export seals a field-canonical content hash
/// with this instance's data key; hand-edited metrics fail verification.
pub async fn import_analytics(
    crate::extract::Db(db): crate::extract::Db,
    Json(body): Json<AnalyticsImportBody>,
) -> (StatusCode, Json<Value>) {
    let format = body.format.as_deref().unwrap_or("").trim();
    if format != ANALYTICS_BACKUP_FORMAT {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "invalid_format",
                "hint": format!(
                    "expected format \"{ANALYTICS_BACKUP_FORMAT}\"; re-export from this instance"
                ),
            })),
        );
    }
    let version = body.version.unwrap_or(0);
    if version != ANALYTICS_BACKUP_VERSION {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "unsupported_version",
                "hint": format!(
                    "backup version {version} is not supported; this instance expects version {ANALYTICS_BACKUP_VERSION}. Re-export from a compatible instance or upgrade Myriad."
                ),
                "expected_version": ANALYTICS_BACKUP_VERSION,
                "got_version": version,
            })),
        );
    }

    if body.page_daily.len() > MAX_IMPORT_PAGE_DAILY
        || body.visitor_seen.len() > MAX_IMPORT_VISITOR_SEEN
        || body.event_daily.len() > MAX_IMPORT_EVENT_DAILY
        || body.event_visitor.len() > MAX_IMPORT_EVENT_VISITOR
        || body.referrer_daily.len() > MAX_IMPORT_REFERRER_DAILY
        || body.country_daily.len() > MAX_IMPORT_COUNTRY_DAILY
        || body.country_visitor.len() > MAX_IMPORT_COUNTRY_VISITOR
    {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": "too_many_rows" })),
        );
    }

    if let Err(e) = validate_counts_object(
        body.counts.as_ref(),
        body.page_daily.len(),
        body.visitor_seen.len(),
        body.event_daily.len(),
        body.event_visitor.len(),
        body.referrer_daily.len(),
        body.country_daily.len(),
        body.country_visitor.len(),
    ) {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "success": false, "error": e })),
        );
    }

    let Some(integrity) = body.integrity.as_ref() else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "missing_integrity",
                "hint": "integrity block is required; re-export analytics backup from this Myriad instance (hand-built JSON cannot be imported)",
            })),
        );
    };

    let expected_hash = match content_hash(
        format,
        version,
        &body.page_daily,
        &body.visitor_seen,
        &body.event_daily,
        &body.event_visitor,
        &body.referrer_daily,
        &body.country_daily,
        &body.country_visitor,
    ) {
        Ok(h) => h,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "success": false,
                    "error": e,
                    "hint": "backup payload could not be hashed; ensure arrays/fields match the export schema",
                })),
            );
        }
    };

    if let Err(e) = verify_integrity(integrity, &expected_hash) {
        tracing::warn!(
            error = e,
            "analytics import integrity verification failed"
        );
        let hint = match e {
            "content_hash_mismatch" => {
                "payload was modified after export (content hash does not match). Re-export without editing metrics."
            }
            "integrity_key_mismatch" | "integrity_token_mismatch" | "invalid_integrity_token" => {
                "integrity seal does not match this instance's data key (export is bound to the originating instance). Import only on the same Myriad instance, or re-export here."
            }
            "unsupported_integrity_alg" => {
                "integrity algorithm is not supported by this build; upgrade Myriad or re-export from a compatible version."
            }
            "missing_integrity_token" => {
                "integrity.token is missing or not ciphertext; re-export from this instance."
            }
            _ => "integrity verification failed; re-export from this instance and import without editing the file.",
        };
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": e,
                "hint": hint,
            })),
        );
    }

    // Structural pre-check (day range, metric ceiling, path/hash shapes).
    // Signed restores must be fully clean — no silent row skips.
    let pre_skipped = match prevalidate_rows(
        &body.page_daily,
        &body.visitor_seen,
        &body.event_daily,
        &body.event_visitor,
        &body.referrer_daily,
        &body.country_daily,
        &body.country_visitor,
    ) {
        Ok(n) => n,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "success": false, "error": e })),
            );
        }
    };
    if pre_skipped > 0 {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "row_validation_failed",
                "skipped": pre_skipped,
            })),
        );
    }

    let mode = body
        .mode
        .as_deref()
        .unwrap_or("replace")
        .trim()
        .to_ascii_lowercase();
    let replace = match mode.as_str() {
        "replace" => true,
        "merge" => false,
        _ => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({ "success": false, "error": "invalid_mode" })),
            );
        }
    };

    let txn = match db.begin().await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("analytics import begin failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": "db_error" })),
            );
        }
    };

    macro_rules! import_db_err {
        ($txn:expr, $e:expr) => {{
            tracing::warn!("analytics import failed: {}", $e);
            let _ = $txn.rollback().await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "success": false, "error": "db_error" })),
            );
        }};
    }

    if replace {
        if let Err(e) = txn
            .execute_unprepared(
                r#"
TRUNCATE analytics_page_daily,
         analytics_visitor_seen,
         analytics_event_daily,
         analytics_event_visitor,
         analytics_referrer_daily,
         analytics_country_daily,
         analytics_country_visitor
"#,
            )
            .await
        {
            import_db_err!(txn, e);
        }
    }

    let mut inserted = json!({
        "page_daily": 0u64,
        "visitor_seen": 0u64,
        "event_daily": 0u64,
        "event_visitor": 0u64,
        "referrer_daily": 0u64,
        "country_daily": 0u64,
        "country_visitor": 0u64,
    });
    let mut skipped: u64 = 0;

    for row in &body.page_daily {
        let Some(day) = row
            .get("day")
            .and_then(|v| v.as_str())
            .and_then(parse_day_str)
            .filter(|d| day_ok_for_import(*d))
        else {
            skipped += 1;
            continue;
        };
        let path_raw = row.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let Some(path) = normalize_import_path(path_raw) else {
            skipped += 1;
            continue;
        };
        let Some(views) = i64_nonneg(row.get("views")).filter(|&n| metric_ok(n)) else {
            skipped += 1;
            continue;
        };
        // UV / engaged_views are recomputed after detail rows land; accept any
        // non-negative placeholder (including 0) from the backup file.
        let uv = i64_nonneg(row.get("unique_visitors"))
            .filter(|&n| metric_ok(n))
            .unwrap_or(0);
        let eng = i64_nonneg(row.get("engagement_ms"))
            .filter(|&n| metric_ok(n))
            .unwrap_or(0);
        let eng_v = i64_nonneg(row.get("engaged_views"))
            .filter(|&n| metric_ok(n))
            .unwrap_or(0);

        // replace: write aggregates as given (UV fixed by recompute).
        // merge: only add views + engagement_ms — never sum UV / engaged_views.
        let sql = if replace {
            r#"
INSERT INTO analytics_page_daily (day, path, views, unique_visitors, engagement_ms, engaged_views)
VALUES ($1, $2, $3, $4, $5, $6)
ON CONFLICT (day, path) DO UPDATE SET
  views = EXCLUDED.views,
  unique_visitors = EXCLUDED.unique_visitors,
  engagement_ms = EXCLUDED.engagement_ms,
  engaged_views = EXCLUDED.engaged_views
"#
        } else {
            r#"
INSERT INTO analytics_page_daily (day, path, views, unique_visitors, engagement_ms, engaged_views)
VALUES ($1, $2, $3, $4, $5, $6)
ON CONFLICT (day, path) DO UPDATE SET
  views = analytics_page_daily.views + EXCLUDED.views,
  engagement_ms = analytics_page_daily.engagement_ms + EXCLUDED.engagement_ms
"#
        };
        match txn
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                sql,
                [
                    SeaValue::from(day),
                    SeaValue::from(path),
                    SeaValue::from(views),
                    SeaValue::from(uv),
                    SeaValue::from(eng),
                    SeaValue::from(eng_v),
                ],
            ))
            .await
        {
            Ok(_) => {
                if let Some(n) = inserted.get_mut("page_daily") {
                    *n = json!(n.as_u64().unwrap_or(0) + 1);
                }
            }
            Err(e) => import_db_err!(txn, e),
        }
    }

    for row in &body.visitor_seen {
        let Some(day) = row
            .get("day")
            .and_then(|v| v.as_str())
            .and_then(parse_day_str)
            .filter(|d| day_ok_for_import(*d))
        else {
            skipped += 1;
            continue;
        };
        let path_raw = row.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let Some(path) = normalize_import_path(path_raw) else {
            skipped += 1;
            continue;
        };
        let hash = row
            .get("visitor_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !valid_visitor_hash(hash) {
            skipped += 1;
            continue;
        }
        let ordinal = i64_nonneg(row.get("ordinal"))
            .filter(|&n| metric_ok(n))
            .unwrap_or(0);
        match txn
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"
INSERT INTO analytics_visitor_seen (day, path, visitor_hash, ordinal)
VALUES ($1, $2, $3, $4)
ON CONFLICT (day, path, visitor_hash) DO NOTHING
"#,
                [
                    SeaValue::from(day),
                    SeaValue::from(path),
                    SeaValue::from(hash.to_string()),
                    // 老备份没有 ordinal 字段 → 0（序号未知），不影响其余统计
                    SeaValue::from(ordinal),
                ],
            ))
            .await
        {
            Ok(res) => {
                if res.rows_affected() > 0 {
                    if let Some(n) = inserted.get_mut("visitor_seen") {
                        *n = json!(n.as_u64().unwrap_or(0) + 1);
                    }
                }
            }
            Err(e) => import_db_err!(txn, e),
        }
    }

    for row in &body.event_daily {
        let Some(day) = row
            .get("day")
            .and_then(|v| v.as_str())
            .and_then(parse_day_str)
            .filter(|d| day_ok_for_import(*d))
        else {
            skipped += 1;
            continue;
        };
        let name_raw = row.get("event_name").and_then(|v| v.as_str()).unwrap_or("");
        let Some(name) = normalize_import_event_name(name_raw) else {
            skipped += 1;
            continue;
        };
        let path_raw = row.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let Some(path) = normalize_import_event_path(path_raw) else {
            skipped += 1;
            continue;
        };
        let target = row
            .get("target")
            .and_then(|v| v.as_str())
            .map(normalize_target)
            .unwrap_or_default();
        let Some(count) = i64_nonneg(row.get("count")).filter(|&n| metric_ok(n)) else {
            skipped += 1;
            continue;
        };
        let uv = i64_nonneg(row.get("unique_visitors"))
            .filter(|&n| metric_ok(n))
            .unwrap_or(0);
        let sql = if replace {
            r#"
INSERT INTO analytics_event_daily (day, event_name, path, target, count, unique_visitors)
VALUES ($1, $2, $3, $4, $5, $6)
ON CONFLICT (day, event_name, path, target) DO UPDATE SET
  count = EXCLUDED.count,
  unique_visitors = EXCLUDED.unique_visitors
"#
        } else {
            r#"
INSERT INTO analytics_event_daily (day, event_name, path, target, count, unique_visitors)
VALUES ($1, $2, $3, $4, $5, $6)
ON CONFLICT (day, event_name, path, target) DO UPDATE SET
  count = analytics_event_daily.count + EXCLUDED.count
"#
        };
        match txn
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                sql,
                [
                    SeaValue::from(day),
                    SeaValue::from(name),
                    SeaValue::from(path),
                    SeaValue::from(target),
                    SeaValue::from(count),
                    SeaValue::from(uv),
                ],
            ))
            .await
        {
            Ok(_) => {
                if let Some(n) = inserted.get_mut("event_daily") {
                    *n = json!(n.as_u64().unwrap_or(0) + 1);
                }
            }
            Err(e) => import_db_err!(txn, e),
        }
    }

    for row in &body.event_visitor {
        let Some(day) = row
            .get("day")
            .and_then(|v| v.as_str())
            .and_then(parse_day_str)
            .filter(|d| day_ok_for_import(*d))
        else {
            skipped += 1;
            continue;
        };
        let name_raw = row.get("event_name").and_then(|v| v.as_str()).unwrap_or("");
        let Some(name) = normalize_import_event_name(name_raw) else {
            skipped += 1;
            continue;
        };
        let path_raw = row.get("path").and_then(|v| v.as_str()).unwrap_or("");
        let Some(path) = normalize_import_event_path(path_raw) else {
            skipped += 1;
            continue;
        };
        let target = row
            .get("target")
            .and_then(|v| v.as_str())
            .map(normalize_target)
            .unwrap_or_default();
        let hash = row
            .get("visitor_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !valid_visitor_hash(hash) {
            skipped += 1;
            continue;
        }
        match txn
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"
INSERT INTO analytics_event_visitor (day, event_name, path, target, visitor_hash)
VALUES ($1, $2, $3, $4, $5)
ON CONFLICT (day, event_name, path, target, visitor_hash) DO NOTHING
"#,
                [
                    SeaValue::from(day),
                    SeaValue::from(name),
                    SeaValue::from(path),
                    SeaValue::from(target),
                    SeaValue::from(hash.to_string()),
                ],
            ))
            .await
        {
            Ok(res) => {
                if res.rows_affected() > 0 {
                    if let Some(n) = inserted.get_mut("event_visitor") {
                        *n = json!(n.as_u64().unwrap_or(0) + 1);
                    }
                }
            }
            Err(e) => import_db_err!(txn, e),
        }
    }

    for row in &body.referrer_daily {
        let Some(day) = row
            .get("day")
            .and_then(|v| v.as_str())
            .and_then(parse_day_str)
            .filter(|d| day_ok_for_import(*d))
        else {
            skipped += 1;
            continue;
        };
        let host_raw = row.get("host").and_then(|v| v.as_str()).unwrap_or("");
        let Some(host) = normalize_referrer_host(host_raw) else {
            skipped += 1;
            continue;
        };
        let Some(count) = i64_nonneg(row.get("count")).filter(|&n| metric_ok(n)) else {
            skipped += 1;
            continue;
        };
        let sql = if replace {
            r#"
INSERT INTO analytics_referrer_daily (day, host, count)
VALUES ($1, $2, $3)
ON CONFLICT (day, host) DO UPDATE SET count = EXCLUDED.count
"#
        } else {
            r#"
INSERT INTO analytics_referrer_daily (day, host, count)
VALUES ($1, $2, $3)
ON CONFLICT (day, host) DO UPDATE SET
  count = analytics_referrer_daily.count + EXCLUDED.count
"#
        };
        match txn
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                sql,
                [
                    SeaValue::from(day),
                    SeaValue::from(host),
                    SeaValue::from(count),
                ],
            ))
            .await
        {
            Ok(_) => {
                if let Some(n) = inserted.get_mut("referrer_daily") {
                    *n = json!(n.as_u64().unwrap_or(0) + 1);
                }
            }
            Err(e) => import_db_err!(txn, e),
        }
    }

    for row in &body.country_daily {
        let Some(day) = row
            .get("day")
            .and_then(|v| v.as_str())
            .and_then(parse_day_str)
            .filter(|d| day_ok_for_import(*d))
        else {
            skipped += 1;
            continue;
        };
        let code_raw = row
            .get("country_code")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let Some(code) = normalize_country_code(code_raw) else {
            skipped += 1;
            continue;
        };
        let name = row
            .get("country_name")
            .and_then(|v| v.as_str())
            .map(normalize_country_name)
            .unwrap_or_default();
        let Some(views) = i64_nonneg(row.get("views")).filter(|&n| metric_ok(n)) else {
            skipped += 1;
            continue;
        };
        // unique_visitors will be recomputed from country_visitor
        let sql = if replace {
            r#"
INSERT INTO analytics_country_daily (day, country_code, country_name, views, unique_visitors)
VALUES ($1, $2, $3, $4, 0)
ON CONFLICT (day, country_code) DO UPDATE SET
  views = EXCLUDED.views,
  country_name = CASE
    WHEN EXCLUDED.country_name <> '' THEN EXCLUDED.country_name
    ELSE analytics_country_daily.country_name
  END
"#
        } else {
            r#"
INSERT INTO analytics_country_daily (day, country_code, country_name, views, unique_visitors)
VALUES ($1, $2, $3, $4, 0)
ON CONFLICT (day, country_code) DO UPDATE SET
  views = analytics_country_daily.views + EXCLUDED.views,
  country_name = CASE
    WHEN EXCLUDED.country_name <> '' THEN EXCLUDED.country_name
    ELSE analytics_country_daily.country_name
  END
"#
        };
        match txn
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                sql,
                [
                    SeaValue::from(day),
                    SeaValue::from(code),
                    SeaValue::from(name),
                    SeaValue::from(views),
                ],
            ))
            .await
        {
            Ok(_) => {
                if let Some(n) = inserted.get_mut("country_daily") {
                    *n = json!(n.as_u64().unwrap_or(0) + 1);
                }
            }
            Err(e) => import_db_err!(txn, e),
        }
    }

    for row in &body.country_visitor {
        let Some(day) = row
            .get("day")
            .and_then(|v| v.as_str())
            .and_then(parse_day_str)
            .filter(|d| day_ok_for_import(*d))
        else {
            skipped += 1;
            continue;
        };
        let code_raw = row
            .get("country_code")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let Some(code) = normalize_country_code(code_raw) else {
            skipped += 1;
            continue;
        };
        let hash = row
            .get("visitor_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !valid_visitor_hash(hash) {
            skipped += 1;
            continue;
        }
        match txn
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"
INSERT INTO analytics_country_visitor (day, country_code, visitor_hash)
VALUES ($1, $2, $3)
ON CONFLICT (day, country_code, visitor_hash) DO NOTHING
"#,
                [
                    SeaValue::from(day),
                    SeaValue::from(code),
                    SeaValue::from(hash.to_string()),
                ],
            ))
            .await
        {
            Ok(res) => {
                if res.rows_affected() > 0 {
                    if let Some(n) = inserted.get_mut("country_visitor") {
                        *n = json!(n.as_u64().unwrap_or(0) + 1);
                    }
                }
            }
            Err(e) => import_db_err!(txn, e),
        }
    }

    // Integrity-sealed restore is all-or-nothing. Prevalidate should have
    // rejected bad rows already; if any row was still soft-skipped during
    // insert (validation drift), roll back so replace never leaves a partial
    // dataset after TRUNCATE.
    if skipped > 0 {
        let _ = txn.rollback().await;
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({
                "success": false,
                "error": "row_validation_failed",
                "skipped": skipped,
                "hint": "sealed analytics restore is all-or-nothing; re-export a clean backup or fix invalid rows",
            })),
        );
    }

    if let Err(e) = recompute_unique_metrics(&txn).await {
        import_db_err!(txn, e);
    }

    if let Err(e) = txn.commit().await {
        tracing::warn!("analytics import commit failed: {}", e);
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "success": false, "error": "db_error" })),
        );
    }

    invalidate_summary_cache().await;

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "mode": if replace { "replace" } else { "merge" },
            "inserted": inserted,
            "skipped": 0,
            "integrity_verified": true,
            "unique_recomputed": true,
            // Surface ceiling so operators know hard bounds (not a secret).
            "metric_ceiling": MAX_METRIC_VALUE,
        })),
    )
}
