//! Admin AI usage summary from `tapp_ai_cost_ledger`.
//!
//! Breaks down site-wide AI calls (text, image, speech) by calendar day, user
//! (`subject_id`), model, and source. Complements the per-user journal at
//! `GET /api/tapp/ai/v2/ledger`.

use axum::{extract::Query, http::StatusCode, Json};
use chrono::Duration;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement, Value as SeaValue};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use super::intake_helpers::{
    analytics_tz_label, compare_range_kind, metric_delta, resolve_analytics_window,
    DEFAULT_SUMMARY_DAYS, MAX_SUMMARY_DAYS,
};

#[derive(Debug, Deserialize)]
pub struct AiUsageSummaryQuery {
    pub days: Option<i64>,
    /// Inclusive start day `YYYY-MM-DD` (custom range with `to`).
    pub from: Option<String>,
    /// Inclusive end day `YYYY-MM-DD`.
    pub to: Option<String>,
    /// Optional filter: ledger subject (user id). `0` is unattributed `internal` fallback; guests are negative ids.
    pub subject_id: Option<i32>,
    /// Optional exact model id filter.
    pub model: Option<String>,
    /// Optional ledger source (`agent`, `merope`, `runtime`, …).
    pub source: Option<String>,
}

fn row_i64(row: &sea_orm::QueryResult, col: &str) -> i64 {
    row.try_get::<i64>("", col).unwrap_or(0)
}

fn row_i32(row: &sea_orm::QueryResult, col: &str) -> i32 {
    row.try_get::<i32>("", col).unwrap_or(0)
}

fn row_string(row: &sea_orm::QueryResult, col: &str) -> String {
    row.try_get::<String>("", col).unwrap_or_default()
}

const SOURCE_EXPR: &str = "COALESCE(NULLIF(TRIM(source), ''), 'unknown')";
const SOURCE_EXPR_L: &str = "COALESCE(NULLIF(TRIM(l.source), ''), 'unknown')";

/// Build day-range WHERE on bare table (no alias) plus optional subject/model/source.
fn bare_where(subject: bool, model: bool, source: bool) -> String {
    let mut s = String::from("WHERE occurred_at::date >= $1 AND occurred_at::date <= $2");
    let mut idx = 3u32;
    if subject {
        s.push_str(&format!(" AND subject_id = ${idx}"));
        idx += 1;
    }
    if model {
        s.push_str(&format!(" AND model = ${idx}"));
        idx += 1;
    }
    if source {
        s.push_str(&format!(" AND {SOURCE_EXPR} = ${idx}"));
    }
    s
}

/// Same filters with table alias `l`.
fn aliased_where(subject: bool, model: bool, source: bool) -> String {
    let mut s = String::from("WHERE l.occurred_at::date >= $1 AND l.occurred_at::date <= $2");
    let mut idx = 3u32;
    if subject {
        s.push_str(&format!(" AND l.subject_id = ${idx}"));
        idx += 1;
    }
    if model {
        s.push_str(&format!(" AND l.model = ${idx}"));
        idx += 1;
    }
    if source {
        s.push_str(&format!(" AND {SOURCE_EXPR_L} = ${idx}"));
    }
    s
}

fn base_params(
    from: chrono::NaiveDate,
    today: chrono::NaiveDate,
    subject_id: Option<i32>,
    model: Option<&str>,
    source: Option<&str>,
) -> Vec<SeaValue> {
    let mut values: Vec<SeaValue> = vec![SeaValue::from(from), SeaValue::from(today)];
    if let Some(sid) = subject_id {
        values.push(SeaValue::Int(Some(sid)));
    }
    if let Some(m) = model {
        values.push(SeaValue::String(Some(m.to_string())));
    }
    if let Some(src) = source {
        values.push(SeaValue::String(Some(src.to_string())));
    }
    values
}

/// GET /api/analytics/ai-usage?days=7&subject_id=&model=&source=
/// or ?from=YYYY-MM-DD&to=YYYY-MM-DD
pub async fn get_ai_usage_summary(
    crate::extract::Db(db): crate::extract::Db,
    Query(q): Query<AiUsageSummaryQuery>,
) -> (StatusCode, Json<Value>) {
    let (from, to_day, days) = resolve_analytics_window(q.days, q.from.as_deref(), q.to.as_deref());
    let tz_label = analytics_tz_label();

    let subject_filter = q.subject_id;
    let model_filter = q
        .model
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let source_filter = q
        .source
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());

    let has_subject = subject_filter.is_some();
    let has_model = model_filter.is_some();
    let has_source = source_filter.is_some();
    let params = base_params(
        from,
        to_day,
        subject_filter,
        model_filter.as_deref(),
        source_filter.as_deref(),
    );
    let where_bare = bare_where(has_subject, has_model, has_source);
    let where_l = aliased_where(has_subject, has_model, has_source);

    let daily_sql = format!(
        r#"
SELECT occurred_at::date::text AS day,
       COUNT(*)::bigint AS calls,
       COALESCE(SUM(input_tokens), 0)::bigint AS input_tokens,
       COALESCE(SUM(output_tokens), 0)::bigint AS output_tokens,
       COALESCE(SUM(input_tokens + output_tokens), 0)::bigint AS tokens
FROM tapp_ai_cost_ledger
{where_bare}
GROUP BY occurred_at::date
ORDER BY day ASC
"#
    );

    let daily_rows = match db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &daily_sql,
            params.clone(),
        ))
        .await
    {
        Ok(rows) => rows,
        Err(e) => {
            tracing::warn!("ai usage daily failed: {}", e);
            return (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::fail_json("db_error")),
            );
        }
    };

    let mut daily_raw = Vec::new();
    for row in &daily_rows {
        daily_raw.push(json!({
            "day": row_string(row, "day"),
            "calls": row_i64(row, "calls"),
            "input_tokens": row_i64(row, "input_tokens"),
            "output_tokens": row_i64(row, "output_tokens"),
            "tokens": row_i64(row, "tokens"),
        }));
    }

    let by_day: HashMap<String, &Value> = daily_raw
        .iter()
        .filter_map(|v| {
            v.get("day")
                .and_then(|d| d.as_str())
                .map(|d| (d.to_string(), v))
        })
        .collect();

    let mut filled = Vec::new();
    let mut cursor = from;
    let mut range_calls: i64 = 0;
    let mut range_tokens: i64 = 0;
    let mut range_input: i64 = 0;
    let mut range_output: i64 = 0;
    while cursor <= to_day {
        let key = cursor.format("%Y-%m-%d").to_string();
        if let Some(v) = by_day.get(&key) {
            range_calls += v.get("calls").and_then(|x| x.as_i64()).unwrap_or(0);
            range_tokens += v.get("tokens").and_then(|x| x.as_i64()).unwrap_or(0);
            range_input += v.get("input_tokens").and_then(|x| x.as_i64()).unwrap_or(0);
            range_output += v.get("output_tokens").and_then(|x| x.as_i64()).unwrap_or(0);
            filled.push((*v).clone());
        } else {
            filled.push(json!({
                "day": key,
                "calls": 0,
                "input_tokens": 0,
                "output_tokens": 0,
                "tokens": 0,
            }));
        }
        cursor += Duration::days(1);
    }

    let today_calls = filled
        .last()
        .and_then(|v| v.get("calls").and_then(|x| x.as_i64()))
        .unwrap_or(0);
    let today_tokens = filled
        .last()
        .and_then(|v| v.get("tokens").and_then(|x| x.as_i64()))
        .unwrap_or(0);

    // 环比：窗口末日 vs 前一日；当前区间 vs 等长上一区间。
    let prev_day = to_day - Duration::days(1);
    let prev_range_to = from - Duration::days(1);
    let prev_range_from = prev_range_to - Duration::days(days - 1);
    let sum_sql = format!(
        r#"
SELECT COUNT(*)::bigint AS calls,
       COALESCE(SUM(input_tokens + output_tokens), 0)::bigint AS tokens
FROM tapp_ai_cost_ledger
{where_bare}
"#
    );
    let (prev_day_calls, prev_day_tokens) = {
        let p = base_params(
            prev_day,
            prev_day,
            subject_filter,
            model_filter.as_deref(),
            source_filter.as_deref(),
        );
        db.query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sum_sql,
            p,
        ))
        .await
        .ok()
        .flatten()
        .map(|row| (row_i64(&row, "calls"), row_i64(&row, "tokens")))
        .unwrap_or((0, 0))
    };
    let (prev_range_calls, prev_range_tokens) = {
        let p = base_params(
            prev_range_from,
            prev_range_to,
            subject_filter,
            model_filter.as_deref(),
            source_filter.as_deref(),
        );
        db.query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &sum_sql,
            p,
        ))
        .await
        .ok()
        .flatten()
        .map(|row| (row_i64(&row, "calls"), row_i64(&row, "tokens")))
        .unwrap_or((0, 0))
    };
    let compare = json!({
        "day": {
            "kind": "day",
            "calls": metric_delta(today_calls, prev_day_calls),
            "tokens": metric_delta(today_tokens, prev_day_tokens),
        },
        "range": {
            "kind": compare_range_kind(days),
            "calls": metric_delta(range_calls, prev_range_calls),
            "tokens": metric_delta(range_tokens, prev_range_tokens),
        },
    });

    let by_user_sql = format!(
        r#"
SELECT l.subject_id,
       COUNT(*)::bigint AS calls,
       COALESCE(SUM(l.input_tokens + l.output_tokens), 0)::bigint AS tokens,
       COALESCE(SUM(l.input_tokens), 0)::bigint AS input_tokens,
       COALESCE(SUM(l.output_tokens), 0)::bigint AS output_tokens,
       u.username,
       u.display_name,
       COALESCE(u.is_admin, false) AS is_admin,
       COALESCE(u.is_owner, false) AS is_owner
FROM tapp_ai_cost_ledger l
LEFT JOIN users u ON u.id = l.subject_id
{where_l}
GROUP BY l.subject_id, u.username, u.display_name, u.is_admin, u.is_owner
ORDER BY tokens DESC, calls DESC, l.subject_id ASC
LIMIT 50
"#
    );

    let by_source_sql = format!(
        r#"
SELECT COALESCE(NULLIF(TRIM(source), ''), 'unknown') AS source,
       COUNT(*)::bigint AS calls,
       COALESCE(SUM(input_tokens + output_tokens), 0)::bigint AS tokens,
       COALESCE(SUM(input_tokens), 0)::bigint AS input_tokens,
       COALESCE(SUM(output_tokens), 0)::bigint AS output_tokens
FROM tapp_ai_cost_ledger
{where_bare}
GROUP BY COALESCE(NULLIF(TRIM(source), ''), 'unknown')
ORDER BY tokens DESC, calls DESC, source ASC
LIMIT 30
"#
    );

    let by_model_sql = format!(
        r#"
SELECT model,
       COALESCE(MAX(provider), '') AS provider,
       COUNT(*)::bigint AS calls,
       COALESCE(SUM(input_tokens + output_tokens), 0)::bigint AS tokens,
       COALESCE(SUM(input_tokens), 0)::bigint AS input_tokens,
       COALESCE(SUM(output_tokens), 0)::bigint AS output_tokens
FROM tapp_ai_cost_ledger
{where_bare}
GROUP BY model
ORDER BY tokens DESC, calls DESC, model ASC
LIMIT 50
"#
    );

    let by_user_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &by_user_sql,
            params.clone(),
        ))
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("ai usage by_user failed: {}", e);
            vec![]
        });

    let by_model_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &by_model_sql,
            params.clone(),
        ))
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("ai usage by_model failed: {}", e);
            vec![]
        });

    let by_source_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            &by_source_sql,
            params.clone(),
        ))
        .await
        .unwrap_or_else(|e| {
            tracing::warn!("ai usage by_source failed: {}", e);
            vec![]
        });

    let by_user: Vec<Value> = by_user_rows
        .iter()
        .map(|row| {
            let subject_id = row_i32(row, "subject_id");
            json!({
                "subject_id": subject_id,
                "username": row.try_get::<Option<String>>("", "username").ok().flatten(),
                "display_name": row.try_get::<Option<String>>("", "display_name").ok().flatten(),
                "is_admin": row.try_get::<bool>("", "is_admin").unwrap_or(false),
                "is_owner": row.try_get::<bool>("", "is_owner").unwrap_or(false),
                "calls": row_i64(row, "calls"),
                "tokens": row_i64(row, "tokens"),
                "input_tokens": row_i64(row, "input_tokens"),
                "output_tokens": row_i64(row, "output_tokens"),
            })
        })
        .collect();

    let by_model: Vec<Value> = by_model_rows
        .iter()
        .map(|row| {
            json!({
                "model": row_string(row, "model"),
                "provider": row_string(row, "provider"),
                "calls": row_i64(row, "calls"),
                "tokens": row_i64(row, "tokens"),
                "input_tokens": row_i64(row, "input_tokens"),
                "output_tokens": row_i64(row, "output_tokens"),
            })
        })
        .collect();

    let by_source: Vec<Value> = by_source_rows
        .iter()
        .map(|row| {
            json!({
                "source": row_string(row, "source"),
                "calls": row_i64(row, "calls"),
                "tokens": row_i64(row, "tokens"),
                "input_tokens": row_i64(row, "input_tokens"),
                "output_tokens": row_i64(row, "output_tokens"),
            })
        })
        .collect();

    // Filter option lists (day-scoped; the picker dimension itself is unconstrained).
    let mut model_opt_where =
        String::from("WHERE occurred_at::date >= $1 AND occurred_at::date <= $2");
    let mut model_opt_params: Vec<SeaValue> = vec![SeaValue::from(from), SeaValue::from(to_day)];
    let mut next = 3u32;
    if let Some(sid) = subject_filter {
        model_opt_where.push_str(&format!(" AND subject_id = ${next}"));
        model_opt_params.push(SeaValue::Int(Some(sid)));
        next += 1;
    }
    if let Some(ref src) = source_filter {
        model_opt_where.push_str(&format!(" AND {SOURCE_EXPR} = ${next}"));
        model_opt_params.push(SeaValue::String(Some(src.clone())));
    }
    let filter_model_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT DISTINCT model FROM tapp_ai_cost_ledger {model_opt_where} ORDER BY model ASC LIMIT 100"
            ),
            model_opt_params,
        ))
        .await
        .unwrap_or_default();
    let filter_models: Vec<String> = filter_model_rows
        .iter()
        .map(|r| row_string(r, "model"))
        .filter(|m| !m.is_empty())
        .collect();

    let mut user_opt_where =
        String::from("WHERE l.occurred_at::date >= $1 AND l.occurred_at::date <= $2");
    let mut user_opt_params: Vec<SeaValue> = vec![SeaValue::from(from), SeaValue::from(to_day)];
    let mut next = 3u32;
    if let Some(ref model) = model_filter {
        user_opt_where.push_str(&format!(" AND l.model = ${next}"));
        user_opt_params.push(SeaValue::String(Some(model.clone())));
        next += 1;
    }
    if let Some(ref src) = source_filter {
        user_opt_where.push_str(&format!(" AND {SOURCE_EXPR_L} = ${next}"));
        user_opt_params.push(SeaValue::String(Some(src.clone())));
    }
    let filter_user_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                r#"
SELECT DISTINCT l.subject_id, u.username, u.display_name
FROM tapp_ai_cost_ledger l
LEFT JOIN users u ON u.id = l.subject_id
{user_opt_where}
ORDER BY l.subject_id ASC
LIMIT 100
"#
            ),
            user_opt_params,
        ))
        .await
        .unwrap_or_default();
    let filter_users: Vec<Value> = filter_user_rows
        .iter()
        .map(|row| {
            json!({
                "subject_id": row_i32(row, "subject_id"),
                "username": row.try_get::<Option<String>>("", "username").ok().flatten(),
                "display_name": row.try_get::<Option<String>>("", "display_name").ok().flatten(),
            })
        })
        .collect();

    let mut source_opt_where =
        String::from("WHERE occurred_at::date >= $1 AND occurred_at::date <= $2");
    let mut source_opt_params: Vec<SeaValue> = vec![SeaValue::from(from), SeaValue::from(to_day)];
    let mut next = 3u32;
    if let Some(sid) = subject_filter {
        source_opt_where.push_str(&format!(" AND subject_id = ${next}"));
        source_opt_params.push(SeaValue::Int(Some(sid)));
        next += 1;
    }
    if let Some(ref model) = model_filter {
        source_opt_where.push_str(&format!(" AND model = ${next}"));
        source_opt_params.push(SeaValue::String(Some(model.clone())));
    }
    let filter_source_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT DISTINCT {SOURCE_EXPR} AS source FROM tapp_ai_cost_ledger {source_opt_where} ORDER BY source ASC LIMIT 50"
            ),
            source_opt_params,
        ))
        .await
        .unwrap_or_default();
    let filter_sources: Vec<String> = filter_source_rows
        .iter()
        .map(|r| row_string(r, "source"))
        .filter(|s| !s.is_empty())
        .collect();

    (
        StatusCode::OK,
        Json(json!({
            "success": true,
            "from": from.format("%Y-%m-%d").to_string(),
            "to": to_day.format("%Y-%m-%d").to_string(),
            "timezone": tz_label,
            "days": days,
            "today": {
                "calls": today_calls,
                "tokens": today_tokens,
            },
            "range": {
                "calls": range_calls,
                "tokens": range_tokens,
                "input_tokens": range_input,
                "output_tokens": range_output,
                "users": by_user.len() as i64,
                "models": by_model.len() as i64,
            },
            "compare": compare,
            "daily": filled,
            "by_user": by_user,
            "by_model": by_model,
            "by_source": by_source,
            "filter_options": {
                "users": filter_users,
                "models": filter_models,
                "sources": filter_sources,
            },
            "filters": {
                "subject_id": subject_filter,
                "source": source_filter,
                "model": model_filter,
            },
            "source": "tapp_ai_cost_ledger",
            // Explicit: unlike visitor analytics, staff (admin/owner) are INCLUDED.
            "staff_included": true,
            "notes": "Full-site AI usage from tapp_ai_cost_ledger: all users including admin/owner; text, image, and speech calls are recorded. Tokens may be estimates.",
        })),
    )
}

#[cfg(test)]
mod tests {
    use super::super::intake_helpers::resolve_analytics_window;
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn days_clamp_bounds_match_analytics() {
        assert_eq!(DEFAULT_SUMMARY_DAYS, 7);
        assert_eq!(MAX_SUMMARY_DAYS, 365);
        assert_eq!(999i64.clamp(1, MAX_SUMMARY_DAYS), 365);
    }

    #[test]
    fn where_builders_add_optional_params() {
        assert!(bare_where(false, false, false).contains("$2"));
        assert!(bare_where(true, false, false).contains("subject_id = $3"));
        assert!(bare_where(true, true, false).contains("model = $4"));
        assert!(bare_where(true, true, true).contains("source"));
        assert!(aliased_where(true, true, true).contains("l.model = $4"));
        assert!(aliased_where(false, false, true).contains("$3"));
    }

    #[test]
    fn base_params_order() {
        let from = NaiveDate::from_ymd_opt(2026, 7, 1).unwrap();
        let today = NaiveDate::from_ymd_opt(2026, 7, 7).unwrap();
        let p = base_params(from, today, Some(3), Some("gpt-test"), Some("merope"));
        assert_eq!(p.len(), 5);
    }

    #[test]
    fn custom_from_to_swaps_and_spans() {
        let (from, to, days) =
            resolve_analytics_window(None, Some("2026-07-10"), Some("2026-07-01"));
        assert_eq!(from.format("%Y-%m-%d").to_string(), "2026-07-01");
        assert_eq!(to.format("%Y-%m-%d").to_string(), "2026-07-10");
        assert_eq!(days, 10);
    }
}
use myriad_error::AppError;
