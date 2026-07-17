//! Append-only per-call AI cost ledger.
//!
//! `tapp_quota_usage` answers "how much budget is left today"; this ledger
//! answers "which Tapp spent what, when, on which provider/model". Entries are
//! written for every governed AI call (completed, failed or cancelled) and are
//! never reset. Token counts are the same length/4 estimates the quota system
//! uses; `cost_micro_usd` stays NULL until a pricing source exists.

use axum::{
    extract::{Query, State},
    http::StatusCode,
    Extension, Json,
};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value as SeaValue};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::middleware::auth::Claims;

use super::common::parse_user_id;

pub(crate) struct AiCostLedgerEntry<'a> {
    pub subject_id: i32,
    pub owner_id: i32,
    pub tapp_id: &'a str,
    pub task_id: &'a str,
    /// "runtime" for sandbox-created AI Tasks, "internal:<caller>" for
    /// governed host adapters (scheduler, declared builtins, ...).
    pub source: &'a str,
    pub operation: &'a str,
    pub provider: &'a str,
    pub model: &'a str,
    pub input_tokens: i32,
    pub output_tokens: i32,
    pub status: &'a str,
    pub error_code: Option<&'a str>,
}

/// Best-effort insert: the ledger must never fail the AI task itself.
pub(crate) async fn record_ai_cost(db: &DatabaseConnection, entry: AiCostLedgerEntry<'_>) {
    let result = db
        .execute(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"
                INSERT INTO tapp_ai_cost_ledger
                    (subject_id, owner_id, tapp_id, task_id, source, operation,
                     provider, model, input_tokens, output_tokens,
                     tokens_estimated, status, error_code)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, TRUE, $11, $12)
            "#,
            vec![
                SeaValue::Int(Some(entry.subject_id)),
                SeaValue::Int(Some(entry.owner_id)),
                SeaValue::String(Some(Box::new(entry.tapp_id.to_string()))),
                SeaValue::String(Some(Box::new(entry.task_id.to_string()))),
                SeaValue::String(Some(Box::new(entry.source.to_string()))),
                SeaValue::String(Some(Box::new(entry.operation.to_string()))),
                SeaValue::String(Some(Box::new(entry.provider.to_string()))),
                SeaValue::String(Some(Box::new(entry.model.to_string()))),
                SeaValue::Int(Some(entry.input_tokens.max(0))),
                SeaValue::Int(Some(entry.output_tokens.max(0))),
                SeaValue::String(Some(Box::new(entry.status.to_string()))),
                SeaValue::String(entry.error_code.map(|code| Box::new(code.to_string()))),
            ],
        ))
        .await;
    if let Err(error) = result {
        tracing::error!(
            %error,
            tapp_id = entry.tapp_id,
            task_id = entry.task_id,
            "[TAPP] Failed to append AI cost ledger entry"
        );
    }
}

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
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
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
                SeaValue::String(Some(Box::new(tapp_id.clone()))),
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
        .query_all(Statement::from_sql_and_values(
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
        .query_all(Statement::from_sql_and_values(
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
