use myriad_error::AppError;
// Admin analytics summary, visitor card, export, and import.

use axum::{
    Json,
    extract::{ConnectInfo, Query, Request},
    http::{StatusCode, header},
};
use chrono::{Duration, NaiveDate, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement, TransactionTrait,
    Value as SeaValue,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::Instant;

fn analytics_db_error(error: impl std::fmt::Display) -> (StatusCode, Json<Value>) {
    tracing::warn!("analytics query failed: {error}");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        Json(AppError::fail_json("db_error")),
    )
}

macro_rules! analytics_rows_try {
    ($fut:expr) => {
        match $fut.await {
            Ok(rows) => rows,
            Err(error) => return Err(analytics_db_error(error)),
        }
    };
}

macro_rules! analytics_count_try {
    ($fut:expr) => {
        match $fut.await {
            Ok(value) => value,
            Err(error) => return Err(analytics_db_error(error)),
        }
    };
}

use super::backup_integrity::{
    MAX_METRIC_VALUE, content_hash, day_ok_for_import, metric_ok, prevalidate_rows, seal_integrity,
    validate_counts_object, verify_integrity,
};
use super::intake_helpers::{
    ANALYTICS_BACKUP_FORMAT, ANALYTICS_BACKUP_VERSION, DAILY_RETENTION_DAYS, ENGAGE_MARKER,
    MAX_IMPORT_COUNTRY_DAILY, MAX_IMPORT_COUNTRY_VISITOR, MAX_IMPORT_EVENT_DAILY,
    MAX_IMPORT_EVENT_VISITOR, MAX_IMPORT_PAGE_DAILY, MAX_IMPORT_REFERRER_DAILY,
    MAX_IMPORT_VISITOR_SEEN, MAX_SUMMARY_DAYS, SITE_PATH, SUMMARY_CACHE, SUMMARY_CACHE_TTL,
    SummaryQuery, VISITOR_CARD_CACHE, VISITOR_RETENTION_DAYS, analytics_collection_enabled,
    analytics_today, analytics_tz_label, compare_range_kind, count_distinct_site,
    invalidate_summary_cache, metric_delta, normalize_country_code, normalize_country_name,
    normalize_event_name, normalize_path, normalize_referrer_host, normalize_target,
    read_visitor_ordinal, resolve_visitor_hash, sum_all_time_page_views,
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

    let (from, to_day, days) = resolve_analytics_window(q.days, q.from.as_deref(), q.to.as_deref());
    let cache_key = format!("{}..{}", from.format("%Y-%m-%d"), to_day.format("%Y-%m-%d"));

    // Short TTL cache — admin UI refresh shouldn't re-scan every open.
    {
        let cache = SUMMARY_CACHE.lock().await;
        if let Some((at, body)) = cache.get(&cache_key) {
            if at.elapsed() < SUMMARY_CACHE_TTL {
                return (StatusCode::OK, Json(body.clone()));
            }
        }
    }

    let body = match summary_body(db, from, to_day, days).await {
        Ok(body) => body,
        Err(error) => return analytics_db_error(error),
    };

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

/// Page rollups for the window and the scalar comparisons, one statement:
/// `day` rows (site UV from the `SITE_PATH` rollup), the top-50 `path` rows via
/// one GROUPING SETS pass over the window, plus `all_time` / `prev_day` /
/// `prev_range` view totals from one FILTER pass over the non-site rows.
const SUMMARY_PAGE_SQL: &str = r#"
SELECT kind, key, views, unique_visitors, engagement_ms, engaged_views
FROM (
    SELECT CASE WHEN GROUPING(day) = 0 THEN 'day' ELSE 'path' END AS kind,
           COALESCE(day::text, path) AS key,
           COALESCE(SUM(views) FILTER (WHERE path <> $3), 0)::bigint AS views,
           COALESCE(MAX(unique_visitors) FILTER (WHERE path = $3), 0)::bigint AS unique_visitors,
           COALESCE(SUM(engagement_ms) FILTER (WHERE path <> $3), 0)::bigint AS engagement_ms,
           COALESCE(SUM(engaged_views) FILTER (WHERE path <> $3), 0)::bigint AS engaged_views,
           row_number() OVER (
               PARTITION BY GROUPING(day)
               ORDER BY COALESCE(SUM(views) FILTER (WHERE path <> $3), 0) DESC, path ASC
           ) AS rank
    FROM analytics_page_daily
    WHERE day >= $1 AND day <= $2
    GROUP BY GROUPING SETS ((day), (path))
    HAVING GROUPING(day) = 0 OR path <> $3
) grouped
WHERE kind = 'day' OR rank <= 50
UNION ALL
SELECT totals.kind, NULL, totals.views, 0, 0, 0
FROM (
    SELECT COALESCE(SUM(views), 0)::bigint AS all_time,
           COALESCE(SUM(views) FILTER (WHERE day = $4), 0)::bigint AS prev_day,
           COALESCE(SUM(views) FILTER (WHERE day >= $5 AND day <= $6), 0)::bigint AS prev_range
    FROM analytics_page_daily
    WHERE path <> $3
) sums
CROSS JOIN LATERAL (
    VALUES ('all_time', sums.all_time), ('prev_day', sums.prev_day), ('prev_range', sums.prev_range)
) AS totals(kind, views)
"#;

/// Distinct visitors: the site-wide counts (window, both comparisons,
/// all-time up to `$2`) in one FILTER pass over the `SITE_PATH` rows, and the
/// per-path window counts over the disjoint non-site rows.
const SUMMARY_VISITOR_SQL: &str = r#"
SELECT 'site' AS kind, NULL AS path,
       COUNT(DISTINCT visitor_hash) FILTER (WHERE day >= $1)::bigint AS unique_visitors,
       COUNT(DISTINCT visitor_hash) FILTER (WHERE day = $4)::bigint AS prev_day,
       COUNT(DISTINCT visitor_hash) FILTER (WHERE day >= $5 AND day <= $6)::bigint AS prev_range,
       COUNT(DISTINCT visitor_hash)::bigint AS all_time
FROM analytics_visitor_seen
WHERE path = $3 AND day <= $2
UNION ALL
SELECT 'path', path, COUNT(DISTINCT visitor_hash)::bigint, 0, 0, 0
FROM analytics_visitor_seen
WHERE day >= $1 AND day <= $2 AND path <> $3
GROUP BY path
"#;

/// Event counts: top-30 names and, per name, top-20 non-empty targets.
const SUMMARY_EVENT_SQL: &str = r#"
SELECT event_name, target, count
FROM (
    SELECT event_name, target, GROUPING(target) AS by_name,
           COALESCE(SUM(count), 0)::bigint AS count,
           row_number() OVER (
               PARTITION BY GROUPING(target),
                            CASE WHEN GROUPING(target) = 0 THEN event_name END
               ORDER BY COALESCE(SUM(count), 0) DESC, event_name ASC, target ASC
           ) AS rank
    FROM analytics_event_daily
    WHERE day >= $1 AND day <= $2 AND event_name <> $3
    GROUP BY GROUPING SETS ((event_name), (event_name, target))
    HAVING GROUPING(target) = 1 OR target <> ''
) grouped
WHERE (by_name = 1 AND rank <= 30) OR (by_name = 0 AND rank <= 20)
"#;

/// Event distinct visitors per name and per non-empty (name, target).
const SUMMARY_EVENT_VISITOR_SQL: &str = r#"
SELECT event_name, CASE WHEN GROUPING(target) = 0 THEN target END AS target,
       COUNT(DISTINCT visitor_hash)::bigint AS unique_visitors
FROM analytics_event_visitor
WHERE day >= $1 AND day <= $2 AND event_name <> $3
GROUP BY GROUPING SETS ((event_name), (event_name, target))
HAVING GROUPING(target) = 1 OR target <> ''
"#;

const SUMMARY_REFERRER_SQL: &str = r#"
SELECT host, COALESCE(SUM(count), 0)::bigint AS count
FROM analytics_referrer_daily
WHERE day >= $1 AND day <= $2
GROUP BY host
ORDER BY count DESC, host ASC
LIMIT 20
"#;

/// Top countries from the daily rollup plus true distinct UV per country
/// (multi-day windows cannot sum daily UV) from the visitor table.
const SUMMARY_COUNTRY_SQL: &str = r#"
(
    SELECT 'daily' AS kind, country_code,
           MAX(country_name) AS country_name,
           COALESCE(SUM(views), 0)::bigint AS views,
           COALESCE(SUM(unique_visitors), 0)::bigint AS unique_visitors
    FROM analytics_country_daily
    WHERE day >= $1 AND day <= $2
    GROUP BY country_code
    ORDER BY unique_visitors DESC, views DESC, country_code ASC
    LIMIT 12
)
UNION ALL
SELECT 'distinct', country_code, NULL, 0, COUNT(DISTINCT visitor_hash)::bigint
FROM analytics_country_visitor
WHERE day >= $1 AND day <= $2
GROUP BY country_code
"#;

/// Cache-miss body of [`build_analytics_summary`]: six independent statements
/// in one latency wave; any query or decode failure fails the whole summary.
async fn summary_body(
    db: &DatabaseConnection,
    from: NaiveDate,
    today: NaiveDate,
    days: i64,
) -> Result<Value, DbErr> {
    // 环比：今日 vs 前一日；当前区间 vs 等长上一区间。
    // kind: day / week / month / period — FE maps to 日/周/月/较上期.
    let prev_day = today - Duration::days(1);
    let prev_range_to = from - Duration::days(1);
    let prev_range_from = prev_range_to - Duration::days(days - 1);
    let window = || [SeaValue::from(from), SeaValue::from(today)];
    let with = |extra: SeaValue| -> Vec<SeaValue> {
        let mut values = window().to_vec();
        values.push(extra);
        values
    };
    let site = || SeaValue::from(SITE_PATH.to_string());
    let comparisons = || {
        let mut values = with(site());
        values.extend([
            SeaValue::from(prev_day),
            SeaValue::from(prev_range_from),
            SeaValue::from(prev_range_to),
        ]);
        values
    };
    let engage = || with(SeaValue::from(ENGAGE_MARKER.to_string()));
    let statement = |sql: &str, values: Vec<SeaValue>| {
        Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values)
    };
    let (page_rows, visitor_rows, event_rows, event_uv_rows, referrer_rows, country_rows) = tokio::try_join!(
        db.query_all_raw(statement(SUMMARY_PAGE_SQL, comparisons())),
        db.query_all_raw(statement(SUMMARY_VISITOR_SQL, comparisons())),
        db.query_all_raw(statement(SUMMARY_EVENT_SQL, engage())),
        db.query_all_raw(statement(SUMMARY_EVENT_VISITOR_SQL, engage())),
        db.query_all_raw(statement(SUMMARY_REFERRER_SQL, window().to_vec())),
        db.query_all_raw(statement(SUMMARY_COUNTRY_SQL, window().to_vec())),
    )?;

    // Page rollups: days, top paths, view totals.
    let mut by_day: HashMap<String, Value> = HashMap::new();
    let mut path_rows = Vec::new();
    let (mut range_views, mut range_engagement_ms, mut range_engaged_views) = (0i64, 0i64, 0i64);
    let (mut all_time_views, mut prev_day_views, mut prev_range_views) = (0i64, 0i64, 0i64);
    for row in &page_rows {
        let kind: String = row.try_get("", "kind")?;
        let views: i64 = row.try_get("", "views")?;
        match kind.as_str() {
            "day" => {
                let day: String = row.try_get("", "key")?;
                let eng: i64 = row.try_get("", "engagement_ms")?;
                range_views += views;
                range_engagement_ms += eng;
                range_engaged_views += row.try_get::<i64>("", "engaged_views")?;
                let value = json!({
                    "day": day,
                    "views": views,
                    "unique_visitors": row.try_get::<i64>("", "unique_visitors")?,
                    "engagement_ms": eng,
                });
                by_day.insert(day, value);
            }
            "path" => path_rows.push((
                row.try_get::<String>("", "key")?,
                views,
                row.try_get::<i64>("", "engagement_ms")?,
                row.try_get::<i64>("", "engaged_views")?,
            )),
            "all_time" => all_time_views = views,
            "prev_day" => prev_day_views = views,
            "prev_range" => prev_range_views = views,
            other => return Err(DbErr::Custom(format!("unexpected page row kind {other}"))),
        }
    }
    let mut filled = Vec::new();
    let mut cursor = from;
    while cursor <= today {
        let key = cursor.format("%Y-%m-%d").to_string();
        filled.push(by_day.remove(&key).unwrap_or_else(|| {
            json!({
                "day": key,
                "views": 0,
                "unique_visitors": 0,
                "engagement_ms": 0,
            })
        }));
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

    // Distinct visitors.
    let mut uv_by_path: HashMap<String, i64> = HashMap::new();
    let mut site_uv = None;
    for row in &visitor_rows {
        let kind: String = row.try_get("", "kind")?;
        let uv: i64 = row.try_get("", "unique_visitors")?;
        if kind == "site" {
            site_uv = Some((
                uv,
                row.try_get::<i64>("", "prev_day")?,
                row.try_get::<i64>("", "prev_range")?,
                row.try_get::<i64>("", "all_time")?,
            ));
        } else {
            uv_by_path.insert(row.try_get("", "path")?, uv);
        }
    }
    let (range_uv, prev_day_uv, prev_range_uv, all_time_uv) =
        site_uv.ok_or_else(|| DbErr::Custom("analytics site visitor row missing".into()))?;

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

    // Same order as the SQL ranking: views desc, path asc.
    path_rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    let pages: Vec<Value> = path_rows
        .into_iter()
        .map(|(path, views, eng, eng_v)| {
            let avg = if eng_v > 0 { eng / eng_v } else { 0 };
            json!({
                "unique_visitors": uv_by_path.get(&path).copied().unwrap_or(0),
                "path": path,
                "views": views,
                "avg_engagement_ms": avg,
            })
        })
        .collect();

    // Events (aggregate name across paths; optional target breakdown)
    let mut event_uv: HashMap<String, i64> = HashMap::new();
    let mut event_target_uv: HashMap<(String, String), i64> = HashMap::new();
    for row in &event_uv_rows {
        let name: String = row.try_get("", "event_name")?;
        let uv: i64 = row.try_get("", "unique_visitors")?;
        match row.try_get::<Option<String>>("", "target")? {
            Some(target) => {
                event_target_uv.insert((name, target), uv);
            }
            None => {
                event_uv.insert(name, uv);
            }
        }
    }
    let mut name_rows = Vec::new();
    let mut target_rows = Vec::new();
    for row in &event_rows {
        let name: String = row.try_get("", "event_name")?;
        let count: i64 = row.try_get("", "count")?;
        match row.try_get::<Option<String>>("", "target")? {
            Some(target) => target_rows.push((name, target, count)),
            None => name_rows.push((name, count)),
        }
    }
    // Same order as the SQL ranking: count desc, then name / target asc.
    name_rows.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    target_rows.sort_by(|a, b| {
        a.0.cmp(&b.0)
            .then_with(|| b.2.cmp(&a.2))
            .then_with(|| a.1.cmp(&b.1))
    });
    let mut targets_by_event: HashMap<String, Vec<Value>> = HashMap::new();
    for (name, target, count) in target_rows {
        if name.is_empty() {
            continue;
        }
        let uv = event_target_uv
            .get(&(name.clone(), target.clone()))
            .copied()
            .unwrap_or(0);
        targets_by_event.entry(name).or_default().push(json!({
            "target": target,
            "count": count,
            "unique_visitors": uv,
        }));
    }
    let events: Vec<Value> = name_rows
        .into_iter()
        .map(|(name, count)| {
            let targets = targets_by_event.remove(&name).unwrap_or_default();
            json!({
                "unique_visitors": event_uv.get(&name).copied().unwrap_or(0),
                "name": name,
                "count": count,
                "targets": targets,
            })
        })
        .collect();

    let referrers = referrer_rows
        .iter()
        .map(|row| {
            Ok(json!({
                "host": row.try_get::<String>("", "host")?,
                "count": row.try_get::<i64>("", "count")?,
            }))
        })
        .collect::<Result<Vec<Value>, DbErr>>()?;

    // Top countries by unique visitors in range (fallback views); prefer true
    // distinct UV over sum-of-daily when multi-day window.
    let mut country_uv: HashMap<String, i64> = HashMap::new();
    let mut daily_countries = Vec::new();
    for row in &country_rows {
        let kind: String = row.try_get("", "kind")?;
        let code: String = row.try_get("", "country_code")?;
        let uv: i64 = row.try_get("", "unique_visitors")?;
        if kind == "distinct" {
            country_uv.insert(code, uv);
        } else {
            let name: Option<String> = row.try_get("", "country_name")?;
            let views: i64 = row.try_get("", "views")?;
            daily_countries.push((code, name.unwrap_or_default(), views, uv));
        }
    }
    let mut countries: Vec<(String, String, i64, i64)> = daily_countries
        .into_iter()
        .map(|(code, name, views, daily_uv)| {
            let uv = country_uv.get(&code).copied().unwrap_or(daily_uv);
            let name = if name.is_empty() { code.clone() } else { name };
            (code, name, views, uv)
        })
        .collect();
    countries.sort_by(|a, b| b.3.cmp(&a.3).then(b.2.cmp(&a.2)).then(a.0.cmp(&b.0)));
    let countries: Vec<Value> = countries
        .into_iter()
        .map(|(code, name, views, uv)| {
            json!({
                "code": code,
                "name": name,
                "views": views,
                "unique_visitors": uv,
            })
        })
        .collect();

    Ok(json!({
        "success": true,
        "days": days,
        "from": from.format("%Y-%m-%d").to_string(),
        "to": today.format("%Y-%m-%d").to_string(),
        "timezone": analytics_tz_label(),
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
    }))
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
pub(crate) async fn visitor_card_aggregate(
    db: &DatabaseConnection,
) -> Result<Value, (StatusCode, Json<Value>)> {
    {
        let cache = VISITOR_CARD_CACHE.lock().await;
        if let Some((at, body)) = cache.as_ref() {
            if at.elapsed() < SUMMARY_CACHE_TTL {
                return Ok(body.clone());
            }
        }
    }

    let today = analytics_today();
    let from = today - Duration::days(VISITOR_CARD_DAYS - 1);
    let epoch = NaiveDate::from_ymd_opt(1970, 1, 1).unwrap_or(from);

    let daily_rows = analytics_rows_try!(db.query_all_raw(Statement::from_sql_and_values(
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
        )));

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

    let all_time_views = analytics_count_try!(sum_all_time_page_views(db));
    let all_time_uv = analytics_count_try!(count_distinct_site(db, epoch, today));

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
    Ok(body)
}

/// GET `/api/analytics/visitor?vid=…` — **public** visitor card.
///
/// Deliberately narrower than the admin summary: site-wide totals, a 5-day
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
                Json(AppError::fail_json("Invalid authentication state")),
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

    let mut body = match visitor_card_aggregate(&db).await {
        Ok(body) => body,
        Err(response) => return response,
    };

    let ordinal = if is_staff {
        None
    } else if let Some(visitor) = resolve_visitor_hash(vid.as_deref(), ip, &ua) {
        read_visitor_ordinal(&db, analytics_today(), &visitor)
            .await
            .unwrap_or(None)
    } else {
        // Production with neither ANALYTICS_SALT nor JWT_SECRET: no hash for ordinal lookup.
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
                Json(AppError::fail_json("db_error")),
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
                Json(AppError::fail_json("db_error")),
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
                Json(AppError::fail_json("db_error")),
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
                Json(AppError::fail_json("db_error")),
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
                Json(AppError::fail_json("db_error")),
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
                Json(AppError::fail_json("db_error")),
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
                Json(AppError::fail_json("db_error")),
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
                Json(AppError::fail_json("export_hash_failed")),
            );
        }
    };
    let integrity = match seal_integrity(&hash) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("analytics export integrity seal failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::fail_json("export_integrity_failed")),
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
async fn recompute_unique_metrics(conn: &impl ConnectionTrait) -> Result<(), sea_orm::DbErr> {
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
/// Bind parameters per import statement, well under PostgreSQL's 65535 limit.
const IMPORT_BIND_BUDGET: usize = 60_000;

/// Chunked multi-row `INSERT … VALUES (…), (…) ON CONFLICT …` for one import table.
///
/// Rows are bound as they are parsed and flushed every `max_rows`, so a maximal
/// backup costs O(rows / chunk) round-trips without a second full typed copy.
/// `ON CONFLICT DO UPDATE` may not touch the same key twice in one statement;
/// when a conflict key repeats inside the pending chunk, the chunk is flushed
/// first so rows still apply in input order with their per-row semantics.
pub(crate) struct ImportBatch {
    head: &'static str,
    conflict: &'static str,
    columns: usize,
    row_tail: &'static str,
    max_rows: usize,
    values: Vec<SeaValue>,
    rows: usize,
    keys: std::collections::HashSet<String>,
    affected: u64,
}

impl ImportBatch {
    pub(crate) fn new(
        head: &'static str,
        columns: usize,
        row_tail: &'static str,
        conflict: &'static str,
    ) -> Self {
        Self::with_max_rows(head, columns, row_tail, conflict, IMPORT_BIND_BUDGET / columns)
    }

    pub(crate) fn with_max_rows(
        head: &'static str,
        columns: usize,
        row_tail: &'static str,
        conflict: &'static str,
        max_rows: usize,
    ) -> Self {
        Self {
            head,
            conflict,
            columns,
            row_tail,
            max_rows: max_rows.max(1),
            values: Vec::new(),
            rows: 0,
            keys: std::collections::HashSet::new(),
            affected: 0,
        }
    }

    /// Queue one row. `key` is the conflict key for `DO UPDATE` statements.
    pub(crate) async fn push<C: ConnectionTrait>(
        &mut self,
        conn: &C,
        key: Option<String>,
        row: impl IntoIterator<Item = SeaValue>,
    ) -> Result<(), sea_orm::DbErr> {
        if let Some(key) = key {
            if self.keys.contains(&key) {
                self.flush(conn).await?;
            }
            self.keys.insert(key);
        }
        let before = self.values.len();
        self.values.extend(row);
        debug_assert_eq!(self.values.len() - before, self.columns);
        self.rows += 1;
        if self.rows >= self.max_rows {
            self.flush(conn).await?;
        }
        Ok(())
    }

    pub(crate) async fn flush<C: ConnectionTrait>(
        &mut self,
        conn: &C,
    ) -> Result<(), sea_orm::DbErr> {
        if self.rows == 0 {
            return Ok(());
        }
        let mut sql = String::with_capacity(
            self.head.len() + self.conflict.len() + self.rows * (self.columns * 8 + 8),
        );
        sql.push_str(self.head);
        for row in 0..self.rows {
            sql.push_str(if row == 0 { "(" } else { ", (" });
            for column in 0..self.columns {
                if column > 0 {
                    sql.push_str(", ");
                }
                sql.push('$');
                sql.push_str(&(row * self.columns + column + 1).to_string());
            }
            sql.push_str(self.row_tail);
            sql.push(')');
        }
        sql.push_str(self.conflict);
        let values = std::mem::take(&mut self.values);
        self.rows = 0;
        self.keys.clear();
        let result = conn
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                sql,
                values,
            ))
            .await?;
        self.affected += result.rows_affected();
        Ok(())
    }

    /// Rows inserted or updated so far (flush first).
    pub(crate) fn affected(&self) -> u64 {
        self.affected
    }
}

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
            Json(AppError::fail_json("too_many_rows")),
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
        return (StatusCode::BAD_REQUEST, Json(AppError::fail_json(e)));
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
        tracing::warn!(error = e, "analytics import integrity verification failed");
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
            _ => {
                "integrity verification failed; re-export from this instance and import without editing the file."
            }
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
            return (StatusCode::BAD_REQUEST, Json(AppError::fail_json(e)));
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
                Json(AppError::fail_json("invalid_mode")),
            );
        }
    };

    let txn = match db.begin().await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("analytics import begin failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::fail_json("db_error")),
            );
        }
    };

    macro_rules! import_db_err {
        ($txn:expr_2021, $e:expr_2021) => {{
            tracing::warn!("analytics import failed: {}", $e);
            let _ = $txn.rollback().await;
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::fail_json("db_error")),
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

    let mut skipped: u64 = 0;

    // replace: write aggregates as given (UV fixed by recompute).
    // merge: only add views + engagement_ms — never sum UV / engaged_views.
    let mut page_daily = ImportBatch::new(
        "INSERT INTO analytics_page_daily \
         (day, path, views, unique_visitors, engagement_ms, engaged_views) VALUES ",
        6,
        "",
        if replace {
            r#"
ON CONFLICT (day, path) DO UPDATE SET
  views = EXCLUDED.views,
  unique_visitors = EXCLUDED.unique_visitors,
  engagement_ms = EXCLUDED.engagement_ms,
  engaged_views = EXCLUDED.engaged_views
"#
        } else {
            r#"
ON CONFLICT (day, path) DO UPDATE SET
  views = analytics_page_daily.views + EXCLUDED.views,
  engagement_ms = analytics_page_daily.engagement_ms + EXCLUDED.engagement_ms
"#
        },
    );
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

        let key = format!("{day}\u{0}{path}");
        let pushed = page_daily
            .push(
                &txn,
                Some(key),
                [
                    SeaValue::from(day),
                    SeaValue::from(path),
                    SeaValue::from(views),
                    SeaValue::from(uv),
                    SeaValue::from(eng),
                    SeaValue::from(eng_v),
                ],
            )
            .await;
        if let Err(e) = pushed {
            import_db_err!(txn, e);
        }
    }
    if let Err(e) = page_daily.flush(&txn).await {
        import_db_err!(txn, e);
    }

    let mut visitor_seen = ImportBatch::new(
        "INSERT INTO analytics_visitor_seen (day, path, visitor_hash, ordinal) VALUES ",
        4,
        "",
        " ON CONFLICT (day, path, visitor_hash) DO NOTHING",
    );
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
        let pushed = visitor_seen
            .push(
                &txn,
                None,
                [
                    SeaValue::from(day),
                    SeaValue::from(path),
                    SeaValue::from(hash),
                    // 老备份没有 ordinal 字段 → 0（序号未知），不影响其余统计
                    SeaValue::from(ordinal),
                ],
            )
            .await;
        if let Err(e) = pushed {
            import_db_err!(txn, e);
        }
    }
    if let Err(e) = visitor_seen.flush(&txn).await {
        import_db_err!(txn, e);
    }

    let mut event_daily = ImportBatch::new(
        "INSERT INTO analytics_event_daily \
         (day, event_name, path, target, count, unique_visitors) VALUES ",
        6,
        "",
        if replace {
            r#"
ON CONFLICT (day, event_name, path, target) DO UPDATE SET
  count = EXCLUDED.count,
  unique_visitors = EXCLUDED.unique_visitors
"#
        } else {
            r#"
ON CONFLICT (day, event_name, path, target) DO UPDATE SET
  count = analytics_event_daily.count + EXCLUDED.count
"#
        },
    );
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
        let key = format!("{day}\u{0}{name}\u{0}{path}\u{0}{target}");
        let pushed = event_daily
            .push(
                &txn,
                Some(key),
                [
                    SeaValue::from(day),
                    SeaValue::from(name),
                    SeaValue::from(path),
                    SeaValue::from(target),
                    SeaValue::from(count),
                    SeaValue::from(uv),
                ],
            )
            .await;
        if let Err(e) = pushed {
            import_db_err!(txn, e);
        }
    }
    if let Err(e) = event_daily.flush(&txn).await {
        import_db_err!(txn, e);
    }

    let mut event_visitor = ImportBatch::new(
        "INSERT INTO analytics_event_visitor \
         (day, event_name, path, target, visitor_hash) VALUES ",
        5,
        "",
        " ON CONFLICT (day, event_name, path, target, visitor_hash) DO NOTHING",
    );
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
        let pushed = event_visitor
            .push(
                &txn,
                None,
                [
                    SeaValue::from(day),
                    SeaValue::from(name),
                    SeaValue::from(path),
                    SeaValue::from(target),
                    SeaValue::from(hash),
                ],
            )
            .await;
        if let Err(e) = pushed {
            import_db_err!(txn, e);
        }
    }
    if let Err(e) = event_visitor.flush(&txn).await {
        import_db_err!(txn, e);
    }

    let mut referrer_daily = ImportBatch::new(
        "INSERT INTO analytics_referrer_daily (day, host, count) VALUES ",
        3,
        "",
        if replace {
            " ON CONFLICT (day, host) DO UPDATE SET count = EXCLUDED.count"
        } else {
            r#"
ON CONFLICT (day, host) DO UPDATE SET
  count = analytics_referrer_daily.count + EXCLUDED.count
"#
        },
    );
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
        let key = format!("{day}\u{0}{host}");
        let pushed = referrer_daily
            .push(
                &txn,
                Some(key),
                [
                    SeaValue::from(day),
                    SeaValue::from(host),
                    SeaValue::from(count),
                ],
            )
            .await;
        if let Err(e) = pushed {
            import_db_err!(txn, e);
        }
    }
    if let Err(e) = referrer_daily.flush(&txn).await {
        import_db_err!(txn, e);
    }

    // unique_visitors will be recomputed from country_visitor
    let mut country_daily = ImportBatch::new(
        "INSERT INTO analytics_country_daily \
         (day, country_code, country_name, views, unique_visitors) VALUES ",
        4,
        ", 0",
        if replace {
            r#"
ON CONFLICT (day, country_code) DO UPDATE SET
  views = EXCLUDED.views,
  country_name = CASE
    WHEN EXCLUDED.country_name <> '' THEN EXCLUDED.country_name
    ELSE analytics_country_daily.country_name
  END
"#
        } else {
            r#"
ON CONFLICT (day, country_code) DO UPDATE SET
  views = analytics_country_daily.views + EXCLUDED.views,
  country_name = CASE
    WHEN EXCLUDED.country_name <> '' THEN EXCLUDED.country_name
    ELSE analytics_country_daily.country_name
  END
"#
        },
    );
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
        let key = format!("{day}\u{0}{code}");
        let pushed = country_daily
            .push(
                &txn,
                Some(key),
                [
                    SeaValue::from(day),
                    SeaValue::from(code),
                    SeaValue::from(name),
                    SeaValue::from(views),
                ],
            )
            .await;
        if let Err(e) = pushed {
            import_db_err!(txn, e);
        }
    }
    if let Err(e) = country_daily.flush(&txn).await {
        import_db_err!(txn, e);
    }

    let mut country_visitor = ImportBatch::new(
        "INSERT INTO analytics_country_visitor (day, country_code, visitor_hash) VALUES ",
        3,
        "",
        " ON CONFLICT (day, country_code, visitor_hash) DO NOTHING",
    );
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
        let pushed = country_visitor
            .push(
                &txn,
                None,
                [
                    SeaValue::from(day),
                    SeaValue::from(code),
                    SeaValue::from(hash),
                ],
            )
            .await;
        if let Err(e) = pushed {
            import_db_err!(txn, e);
        }
    }
    if let Err(e) = country_visitor.flush(&txn).await {
        import_db_err!(txn, e);
    }
    // DO UPDATE tables count every applied row; DO NOTHING tables count new rows.
    let inserted = json!({
        "page_daily": page_daily.affected(),
        "visitor_seen": visitor_seen.affected(),
        "event_daily": event_daily.affected(),
        "event_visitor": event_visitor.affected(),
        "referrer_daily": referrer_daily.affected(),
        "country_daily": country_daily.affected(),
        "country_visitor": country_visitor.affected(),
    });

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
            Json(AppError::fail_json("db_error")),
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
