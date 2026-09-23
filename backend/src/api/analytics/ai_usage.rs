//! Admin AI usage summary from `tapp_ai_cost_ledger`.
//!
//! Breaks down site-wide AI calls (text, image, speech) by calendar day, user
//! (`subject_id`), model, and source. Complements the per-user journal at
//! `GET /api/tapp/ai/v2/ledger`.

use axum::{Json, extract::Query, http::StatusCode};
use chrono::Duration;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement, Value as SeaValue,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashMap;

use super::intake_helpers::{
    DEFAULT_SUMMARY_DAYS, MAX_SUMMARY_DAYS, analytics_tz_label, compare_range_kind, metric_delta,
    resolve_analytics_window,
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

const SOURCE_EXPR: &str = "COALESCE(NULLIF(TRIM(source), ''), 'unknown')";

/// Day-range WHERE on the bare ledger (`$1..=$2`) plus optional
/// subject/model/source filters bound from `$3` in that order.
fn bare_where(subject: bool, model: bool, source: bool) -> String {
    ledger_where("", subject, model, source)
}

/// [`bare_where`] with a column prefix such as `l.`.
fn ledger_where(p: &str, subject: bool, model: bool, source: bool) -> String {
    let mut s = format!("WHERE {p}occurred_at::date >= $1 AND {p}occurred_at::date <= $2");
    let mut idx = 3u32;
    if subject {
        s.push_str(&format!(" AND {p}subject_id = ${idx}"));
        idx += 1;
    }
    if model {
        s.push_str(&format!(" AND {p}model = ${idx}"));
        idx += 1;
    }
    if source {
        s.push_str(&format!(
            " AND COALESCE(NULLIF(TRIM({p}source), ''), 'unknown') = ${idx}"
        ));
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

/// One pass over the filtered ledger from the start of the previous range to
/// the end of the window: daily rows, the current-range totals with both
/// comparisons, and the top user/model/source breakdowns, tagged by `kind`.
/// `$1..=$2` is the scanned span, filters follow, then `current_from`,
/// `prev_day` and `prev_range_to` (see [`summary_params`]).
fn summary_sql(subject: bool, model: bool, source: bool) -> String {
    let where_scan = bare_where(subject, model, source);
    let next = 3 + u32::from(subject) + u32::from(model) + u32::from(source);
    let (cur, prev_day, prev_to) = (next, next + 1, next + 2);
    format!(
        r#"
WITH scoped AS (
    SELECT occurred_at::date AS day, subject_id, model, provider,
           {SOURCE_EXPR} AS source, input_tokens, output_tokens,
           occurred_at::date >= ${cur} AS cur
    FROM tapp_ai_cost_ledger
    {where_scan}
), grouped AS (
    SELECT CASE
               WHEN GROUPING(day) = 0 THEN 'day'
               WHEN GROUPING(subject_id) = 0 THEN 'user'
               WHEN GROUPING(model) = 0 THEN 'model'
               WHEN GROUPING(source) = 0 THEN 'source'
               ELSE 'totals'
           END AS kind,
           day, subject_id, model, source,
           COALESCE(MAX(provider) FILTER (WHERE cur), '') AS provider,
           COUNT(*) FILTER (WHERE cur)::bigint AS calls,
           COALESCE(SUM(input_tokens + output_tokens) FILTER (WHERE cur), 0)::bigint AS tokens,
           COALESCE(SUM(input_tokens) FILTER (WHERE cur), 0)::bigint AS input_tokens,
           COALESCE(SUM(output_tokens) FILTER (WHERE cur), 0)::bigint AS output_tokens,
           COUNT(*) FILTER (WHERE day = ${prev_day})::bigint AS prev_day_calls,
           COALESCE(SUM(input_tokens + output_tokens) FILTER (WHERE day = ${prev_day}), 0)::bigint
               AS prev_day_tokens,
           COUNT(*) FILTER (WHERE day <= ${prev_to})::bigint AS prev_range_calls,
           COALESCE(SUM(input_tokens + output_tokens) FILTER (WHERE day <= ${prev_to}), 0)::bigint
               AS prev_range_tokens
    FROM scoped
    GROUP BY GROUPING SETS ((day), (subject_id), (model), (source), ())
), ranked AS (
    SELECT grouped.*,
           row_number() OVER (
               PARTITION BY kind
               ORDER BY tokens DESC, calls DESC, subject_id ASC, model ASC, source ASC
           ) AS rank
    FROM grouped
    WHERE kind = 'totals' OR calls > 0
)
SELECT r.kind, r.day::text AS day, r.subject_id, r.model, r.source, r.provider,
       r.calls, r.tokens, r.input_tokens, r.output_tokens,
       r.prev_day_calls, r.prev_day_tokens, r.prev_range_calls, r.prev_range_tokens,
       u.username, u.display_name,
       COALESCE(u.is_admin, false) AS is_admin,
       COALESCE(u.is_owner, false) AS is_owner
FROM ranked r
LEFT JOIN users u ON r.kind = 'user' AND u.id = r.subject_id
WHERE r.kind IN ('day', 'totals')
   OR (r.kind IN ('user', 'model') AND r.rank <= 50)
   OR (r.kind = 'source' AND r.rank <= 30)
ORDER BY r.kind, r.rank
"#
    )
}

/// Main-statement parameters in [`summary_sql`] order.
struct SummaryWindow {
    scan_from: chrono::NaiveDate,
    to_day: chrono::NaiveDate,
    current_from: chrono::NaiveDate,
    prev_day: chrono::NaiveDate,
    prev_range_to: chrono::NaiveDate,
}

fn summary_params(
    window: &SummaryWindow,
    subject_id: Option<i32>,
    model: Option<&str>,
    source: Option<&str>,
) -> Vec<SeaValue> {
    let mut values = base_params(window.scan_from, window.to_day, subject_id, model, source);
    values.extend([
        SeaValue::from(window.current_from),
        SeaValue::from(window.prev_day),
        SeaValue::from(window.prev_range_to),
    ]);
    values
}

/// Picker options ignore their own dimension but keep the other filters.
fn picker_statement(
    select_from: &str,
    order_limit: &str,
    from: chrono::NaiveDate,
    to_day: chrono::NaiveDate,
    subject_id: Option<i32>,
    model: Option<&str>,
    source: Option<&str>,
) -> Statement {
    let where_sql = ledger_where(
        "l.",
        subject_id.is_some(),
        model.is_some(),
        source.is_some(),
    );
    Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        format!("{select_from} {where_sql} {order_limit}"),
        base_params(from, to_day, subject_id, model, source),
    )
}

fn usage_totals(row: &sea_orm::QueryResult) -> Result<Value, DbErr> {
    Ok(json!({
        "calls": row.try_get::<i64>("", "calls")?,
        "tokens": row.try_get::<i64>("", "tokens")?,
        "input_tokens": row.try_get::<i64>("", "input_tokens")?,
        "output_tokens": row.try_get::<i64>("", "output_tokens")?,
    }))
}

/// GET /api/analytics/ai-usage?days=7&subject_id=&model=&source=
/// or ?from=YYYY-MM-DD&to=YYYY-MM-DD
pub async fn get_ai_usage_summary(
    crate::extract::Db(db): crate::extract::Db,
    Query(q): Query<AiUsageSummaryQuery>,
) -> (StatusCode, Json<Value>) {
    match ai_usage_summary(&db, q).await {
        Ok(body) => (StatusCode::OK, Json(body)),
        Err(error) => {
            tracing::warn!(%error, "ai usage summary failed");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(AppError::fail_json("db_error")),
            )
        }
    }
}

async fn ai_usage_summary(db: &DatabaseConnection, q: AiUsageSummaryQuery) -> Result<Value, DbErr> {
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
    let (model_ref, source_ref) = (model_filter.as_deref(), source_filter.as_deref());

    // 环比：窗口末日 vs 前一日；当前区间 vs 等长上一区间。
    let prev_range_to = from - Duration::days(1);
    let window = SummaryWindow {
        scan_from: prev_range_to - Duration::days(days - 1),
        to_day,
        current_from: from,
        prev_day: to_day - Duration::days(1),
        prev_range_to,
    };
    let summary = Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        summary_sql(
            subject_filter.is_some(),
            model_filter.is_some(),
            source_filter.is_some(),
        ),
        summary_params(&window, subject_filter, model_ref, source_ref),
    );
    let model_picker = picker_statement(
        "SELECT DISTINCT l.model FROM tapp_ai_cost_ledger l",
        "ORDER BY l.model ASC LIMIT 100",
        from,
        to_day,
        subject_filter,
        None,
        source_ref,
    );
    let user_picker = picker_statement(
        "SELECT DISTINCT l.subject_id, u.username, u.display_name \
         FROM tapp_ai_cost_ledger l LEFT JOIN users u ON u.id = l.subject_id",
        "ORDER BY l.subject_id ASC LIMIT 100",
        from,
        to_day,
        None,
        model_ref,
        source_ref,
    );
    let source_picker = picker_statement(
        "SELECT DISTINCT COALESCE(NULLIF(TRIM(l.source), ''), 'unknown') AS source \
         FROM tapp_ai_cost_ledger l",
        "ORDER BY source ASC LIMIT 50",
        from,
        to_day,
        subject_filter,
        model_ref,
        None,
    );
    // Picker predicates differ from the main filter, so they cannot reuse its
    // rows; all four statements are independent and share one latency wave.
    let (summary_rows, model_rows, user_rows, source_rows) = tokio::try_join!(
        db.query_all_raw(summary),
        db.query_all_raw(model_picker),
        db.query_all_raw(user_picker),
        db.query_all_raw(source_picker),
    )?;

    let mut by_day: HashMap<String, Value> = HashMap::new();
    let mut totals = None;
    let (mut by_user, mut by_model, mut by_source) = (Vec::new(), Vec::new(), Vec::new());
    for row in &summary_rows {
        let kind: String = row.try_get("", "kind")?;
        match kind.as_str() {
            "day" => {
                let day: String = row.try_get("", "day")?;
                let mut value = usage_totals(row)?;
                value["day"] = json!(day);
                by_day.insert(day, value);
            }
            "totals" => totals = Some(row),
            "user" => {
                let mut value = usage_totals(row)?;
                value["subject_id"] = json!(row.try_get::<i32>("", "subject_id")?);
                value["username"] = json!(row.try_get::<Option<String>>("", "username")?);
                value["display_name"] = json!(row.try_get::<Option<String>>("", "display_name")?);
                value["is_admin"] = json!(row.try_get::<bool>("", "is_admin")?);
                value["is_owner"] = json!(row.try_get::<bool>("", "is_owner")?);
                by_user.push(value);
            }
            "model" => {
                let mut value = usage_totals(row)?;
                value["model"] = json!(row.try_get::<String>("", "model")?);
                value["provider"] = json!(row.try_get::<String>("", "provider")?);
                by_model.push(value);
            }
            "source" => {
                let mut value = usage_totals(row)?;
                value["source"] = json!(row.try_get::<String>("", "source")?);
                by_source.push(value);
            }
            other => {
                return Err(DbErr::Custom(format!(
                    "unexpected ai usage row kind {other}"
                )));
            }
        }
    }
    let totals = totals.ok_or_else(|| DbErr::Custom("ai usage totals row missing".into()))?;
    let range_calls: i64 = totals.try_get("", "calls")?;
    let range_tokens: i64 = totals.try_get("", "tokens")?;
    let range_input: i64 = totals.try_get("", "input_tokens")?;
    let range_output: i64 = totals.try_get("", "output_tokens")?;
    let prev_day_calls: i64 = totals.try_get("", "prev_day_calls")?;
    let prev_day_tokens: i64 = totals.try_get("", "prev_day_tokens")?;
    let prev_range_calls: i64 = totals.try_get("", "prev_range_calls")?;
    let prev_range_tokens: i64 = totals.try_get("", "prev_range_tokens")?;

    let mut filled = Vec::new();
    let mut cursor = from;
    while cursor <= to_day {
        let key = cursor.format("%Y-%m-%d").to_string();
        filled.push(by_day.remove(&key).unwrap_or_else(|| {
            json!({
                "day": key,
                "calls": 0,
                "input_tokens": 0,
                "output_tokens": 0,
                "tokens": 0,
            })
        }));
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

    let mut filter_models = Vec::with_capacity(model_rows.len());
    for row in &model_rows {
        let model: String = row.try_get("", "model")?;
        if !model.is_empty() {
            filter_models.push(model);
        }
    }
    let filter_users = user_rows
        .iter()
        .map(|row| {
            Ok(json!({
                "subject_id": row.try_get::<i32>("", "subject_id")?,
                "username": row.try_get::<Option<String>>("", "username")?,
                "display_name": row.try_get::<Option<String>>("", "display_name")?,
            }))
        })
        .collect::<Result<Vec<Value>, DbErr>>()?;
    let mut filter_sources = Vec::with_capacity(source_rows.len());
    for row in &source_rows {
        let source: String = row.try_get("", "source")?;
        if !source.is_empty() {
            filter_sources.push(source);
        }
    }

    Ok(json!({
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
    }))
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
        let sql = summary_sql(true, false, true);
        assert!(sql.contains("subject_id = $3") && sql.contains("= $4"));
        assert!(sql.contains(">= $5 AS cur") && sql.contains("day = $6"));
        assert!(sql.contains("day <= $7"));
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

    #[tokio::test]
    async fn one_ledger_pass_matches_filters_and_fails_whole_on_error() {
        let Ok(url) = std::env::var("ANALYTICS_TEST_DATABASE_URL") else {
            return;
        };
        let isolated = crate::db::IsolatedSchema::migrated(&url, "ai_usage_test").await;
        let db = isolated.db.clone();
        db.execute_unprepared(
            r#"
INSERT INTO users (id, username) VALUES (1, 'u1');
INSERT INTO tapp_ai_cost_ledger
    (occurred_at, subject_id, owner_id, tapp_id, task_id, source, operation, provider, model,
     input_tokens, output_tokens, status)
VALUES (NOW(), 1, 1, 't', 'k', 'agent', 'chat', 'p', 'm1', 10, 5, 'ok'),
       (NOW(), 0, 0, 't', 'k', ' ', 'chat', 'p', 'm2', 1, 1, 'ok'),
       (NOW() - INTERVAL '1 day', 1, 1, 't', 'k', 'agent', 'chat', 'p', 'm1', 7, 0, 'ok'),
       (NOW() - INTERVAL '10 days', 1, 1, 't', 'k', 'agent', 'chat', 'p', 'm1', 3, 0, 'ok');
"#,
        )
        .await
        .unwrap();
        let query = |model: Option<&str>| AiUsageSummaryQuery {
            days: Some(7),
            from: None,
            to: None,
            subject_id: None,
            model: model.map(str::to_string),
            source: None,
        };
        let all = ai_usage_summary(&db, query(None)).await.unwrap();
        assert_eq!(all["range"]["calls"], 3);
        assert_eq!(all["range"]["tokens"], 24);
        assert_eq!(all["today"]["calls"], 2);
        assert_eq!(all["compare"]["range"]["calls"]["previous"], 1);
        assert_eq!(all["by_user"][0]["username"], "u1");
        assert_eq!(all["by_source"].as_array().unwrap().len(), 2);
        let m1 = ai_usage_summary(&db, query(Some("m1"))).await.unwrap();
        assert_eq!(m1["range"]["calls"], 2);
        // The model picker ignores its own filter.
        assert_eq!(
            m1["filter_options"]["models"],
            serde_json::json!(["m1", "m2"])
        );

        db.execute_unprepared("ALTER TABLE users RENAME TO users_off")
            .await
            .unwrap();
        assert!(ai_usage_summary(&db, query(None)).await.is_err());
        db.execute_unprepared("ALTER TABLE users_off RENAME TO users")
            .await
            .unwrap();
        isolated.drop().await;
    }
}
use myriad_error::AppError;
