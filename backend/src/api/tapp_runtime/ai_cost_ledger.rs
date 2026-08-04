//! HTTP surface for the append-only AI cost ledger.
//!
//! Write path lives in [`crate::services::ai_cost_ledger`]; this module only
//! serves the host UI journal endpoint.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value as SeaValue};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::error::HttpError;
use crate::middleware::auth::Claims;

use super::common::parse_user_id;

#[derive(Debug, Deserialize)]
pub struct LedgerQuery {
    /// Restrict to one Tapp installation.
    pub tapp_id: Option<String>,
    /// Max entries to return (1..=200, default 50).
    pub limit: Option<u32>,
}

/// GET /api/tapp/ai/v2/ledger
///
/// Host UI endpoint: the authenticated user reads their own per-call AI
/// spending journal plus per-Tapp totals. This is deliberately not part of the
/// sandbox SDK surface — Tapps see quota snapshots, not the account book.
pub async fn ai_cost_ledger(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<LedgerQuery>,
) -> Result<Json<Value>, HttpError> {
    let subject_id = parse_user_id(&claims)?;
    let limit = i64::from(query.limit.unwrap_or(50).clamp(1, 200));

    let ledger_error = || {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({
                "error": "Failed to read AI cost ledger",
                "code": "AI_COST_LEDGER_ERROR"
            })),
        )
    };

    let (entry_sql, totals_sql, mut values): (&str, &str, Vec<SeaValue>) = match &query.tapp_id {
        Some(tapp_id) => (
            r#"
                SELECT id, occurred_at, tapp_id, task_id, source, operation,
                       provider, model, input_tokens, output_tokens,
                       tokens_estimated, cost_micro_usd, status, error_code
                FROM tapp_ai_cost_ledger
                WHERE subject_id = $1 AND tapp_id = $2
                ORDER BY occurred_at DESC, id DESC
                LIMIT $3
            "#,
            r#"
                SELECT tapp_id,
                       COUNT(*)::BIGINT AS calls,
                       COALESCE(SUM(input_tokens), 0)::BIGINT AS input_tokens,
                       COALESCE(SUM(output_tokens), 0)::BIGINT AS output_tokens,
                       SUM(cost_micro_usd)::BIGINT AS cost_micro_usd
                FROM tapp_ai_cost_ledger
                WHERE subject_id = $1 AND tapp_id = $2
                GROUP BY tapp_id
            "#,
            vec![
                SeaValue::Int(Some(subject_id)),
                SeaValue::String(Some(tapp_id.clone())),
            ],
        ),
        None => (
            r#"
                SELECT id, occurred_at, tapp_id, task_id, source, operation,
                       provider, model, input_tokens, output_tokens,
                       tokens_estimated, cost_micro_usd, status, error_code
                FROM tapp_ai_cost_ledger
                WHERE subject_id = $1
                ORDER BY occurred_at DESC, id DESC
                LIMIT $2
            "#,
            r#"
                SELECT tapp_id,
                       COUNT(*)::BIGINT AS calls,
                       COALESCE(SUM(input_tokens), 0)::BIGINT AS input_tokens,
                       COALESCE(SUM(output_tokens), 0)::BIGINT AS output_tokens,
                       SUM(cost_micro_usd)::BIGINT AS cost_micro_usd
                FROM tapp_ai_cost_ledger
                WHERE subject_id = $1
                GROUP BY tapp_id
                ORDER BY calls DESC
            "#,
            vec![SeaValue::Int(Some(subject_id))],
        ),
    };
    let totals_values = values.clone();
    values.push(SeaValue::BigInt(Some(limit)));

    let entry_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            entry_sql,
            values,
        ))
        .await
        .map_err(|error| {
            tracing::error!(%error, "[TAPP] Failed to query AI cost ledger entries");
            ledger_error()
        })?;
    let totals_rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            totals_sql,
            totals_values,
        ))
        .await
        .map_err(|error| {
            tracing::error!(%error, "[TAPP] Failed to query AI cost ledger totals");
            ledger_error()
        })?;

    let entries: Vec<Value> = entry_rows
        .iter()
        .filter_map(|row| {
            Some(json!({
                "id": row.try_get::<i64>("", "id").ok()?,
                "occurredAt": row
                    .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "occurred_at")
                    .ok()?
                    .to_rfc3339(),
                "tappId": row.try_get::<String>("", "tapp_id").ok()?,
                "taskId": row.try_get::<String>("", "task_id").ok()?,
                "source": row.try_get::<String>("", "source").ok()?,
                "operation": row.try_get::<String>("", "operation").ok()?,
                "provider": row.try_get::<String>("", "provider").ok()?,
                "model": row.try_get::<String>("", "model").ok()?,
                "inputTokens": row.try_get::<i32>("", "input_tokens").ok()?,
                "outputTokens": row.try_get::<i32>("", "output_tokens").ok()?,
                "tokensEstimated": row.try_get::<bool>("", "tokens_estimated").ok()?,
                "costMicroUsd": row.try_get::<Option<i64>>("", "cost_micro_usd").ok()?,
                "status": row.try_get::<String>("", "status").ok()?,
                "errorCode": row.try_get::<Option<String>>("", "error_code").ok()?,
            }))
        })
        .collect();
    let totals: Vec<Value> = totals_rows
        .iter()
        .filter_map(|row| {
            Some(json!({
                "tappId": row.try_get::<String>("", "tapp_id").ok()?,
                "calls": row.try_get::<i64>("", "calls").ok()?,
                "inputTokens": row.try_get::<i64>("", "input_tokens").ok()?,
                "outputTokens": row.try_get::<i64>("", "output_tokens").ok()?,
                "costMicroUsd": row.try_get::<Option<i64>>("", "cost_micro_usd").ok()?,
            }))
        })
        .collect();

    Ok(Json(json!({
        "success": true,
        "entries": entries,
        "totals": totals
    })))
}
