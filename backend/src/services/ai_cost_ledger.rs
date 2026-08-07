//! Append-only per-call AI cost ledger (write path).
//!
//! `tapp_quota_usage` answers "how much budget is left today"; this ledger
//! answers "which caller spent what, when, on which provider/model". Entries are
//! written for governed AI tasks (Tapp / scheduler), and for other site-wide
//! paths (Arael agent, reports, …) when a task-local [`AiLedgerAttribution`] is
//! active. **Admin subjects are included** — this is full-site usage, not
//! visitor-stats style staff exclusion. Token counts are the same length/4
//! estimates the quota system uses; `cost_micro_usd` stays NULL until a pricing
//! source exists.
//!
//! HTTP list endpoint stays in the API layer; only the best-effort append lives
//! here so AI task execution does not import `crate::api`.

use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value as SeaValue};

/// Task-local attribution for non-governed paths that call `AiAnalyzer` directly
/// (Arael, report generation, prompt tools, …). Governed tasks already call
/// [`record_ai_cost`] explicitly and should **not** set this context (avoids double-count).
#[derive(Debug, Clone)]
pub struct AiLedgerAttribution {
    pub subject_id: i32,
    pub owner_id: i32,
    pub source: String,
    pub operation: String,
    pub tapp_id: String,
    pub task_id: String,
}

/// Running token total for one logical unit of work.
///
/// A caller that reserves AI quota up front needs to know what the work
/// actually cost in order to settle the reservation, but the spend is spread
/// across nested `AiAnalyzer` calls it never sees — an agent turn covers
/// planning, per-step handlers, dynamic-step analysis, response generation and
/// memory extraction. Rather than thread counters through every call site, the
/// meter rides the same task-local scope the ledger already uses and is
/// incremented from the one place every call passes through.
///
/// Counts are the same length/4 estimates written to the ledger, so a settled
/// reservation and the ledger rows agree.
#[derive(Debug, Clone, Default)]
pub struct AiUsageMeter {
    total_tokens: Arc<AtomicU64>,
}

impl AiUsageMeter {
    pub fn new() -> Self {
        Self::default()
    }

    /// Tokens charged inside the scope so far.
    pub fn total_tokens(&self) -> u64 {
        self.total_tokens.load(Ordering::Relaxed)
    }

    fn add(&self, tokens: u64) {
        self.total_tokens.fetch_add(tokens, Ordering::Relaxed);
    }
}

tokio::task_local! {
    static AI_LEDGER_ATTRIBUTION: AiLedgerAttribution;
    static AI_USAGE_METER: AiUsageMeter;
}

/// Run `fut` with ledger attribution for any nested `AiAnalyzer` calls.
pub async fn with_ai_ledger_attribution<F, T>(attr: AiLedgerAttribution, fut: F) -> T
where
    F: Future<Output = T>,
{
    AI_LEDGER_ATTRIBUTION.scope(attr, fut).await
}

/// Run `fut` with `meter` accumulating every nested `AiAnalyzer` call.
///
/// Kept independent of the attribution scope so an inner
/// [`with_ai_ledger_attribution`] — the executor re-scopes per recipe — narrows
/// attribution without detaching the meter from the outer unit of work.
pub async fn with_ai_usage_meter<F, T>(meter: AiUsageMeter, fut: F) -> T
where
    F: Future<Output = T>,
{
    AI_USAGE_METER.scope(meter, fut).await
}

fn current_attribution() -> Option<AiLedgerAttribution> {
    AI_LEDGER_ATTRIBUTION.try_with(|c| c.clone()).ok()
}

/// Best-effort ledger write when a task-local attribution is active (AiAnalyzer hooks).
pub async fn record_ai_call_from_attribution(
    provider: &str,
    model: &str,
    input_chars: usize,
    output_chars: usize,
    status: &str,
    error_code: Option<&str>,
) {
    let input_tokens = i32::try_from(input_chars / 4).unwrap_or(i32::MAX);
    let output_tokens = i32::try_from(output_chars / 4).unwrap_or(i32::MAX);

    // Metered before the attribution check: the planner runs outside any
    // attribution scope today, and a quota settlement must still see its spend.
    let _ = AI_USAGE_METER.try_with(|meter| {
        meter.add(u64::from(input_tokens.max(0) as u32) + u64::from(output_tokens.max(0) as u32));
    });

    let Some(attr) = current_attribution() else {
        return;
    };
    let Ok(db) = crate::services::tapp_registry::database().await else {
        return;
    };
    record_ai_cost(
        &db,
        AiCostLedgerEntry {
            subject_id: attr.subject_id,
            owner_id: attr.owner_id,
            tapp_id: &attr.tapp_id,
            task_id: &attr.task_id,
            source: &attr.source,
            operation: &attr.operation,
            provider,
            model,
            input_tokens,
            output_tokens,
            status,
            error_code,
        },
    )
    .await;
}

pub struct AiCostLedgerEntry<'a> {
    pub subject_id: i32,
    pub owner_id: i32,
    pub tapp_id: &'a str,
    pub task_id: &'a str,
    /// "runtime" · "scheduler" · "agent" · "reports" · "internal:<caller>" …
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
pub async fn record_ai_cost(db: &DatabaseConnection, entry: AiCostLedgerEntry<'_>) {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
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
                SeaValue::String(Some(entry.tapp_id.to_string())),
                SeaValue::String(Some(entry.task_id.to_string())),
                SeaValue::String(Some(entry.source.to_string())),
                SeaValue::String(Some(entry.operation.to_string())),
                SeaValue::String(Some(entry.provider.to_string())),
                SeaValue::String(Some(entry.model.to_string())),
                SeaValue::Int(Some(entry.input_tokens.max(0))),
                SeaValue::Int(Some(entry.output_tokens.max(0))),
                SeaValue::String(Some(entry.status.to_string())),
                SeaValue::String(entry.error_code.map(|code| code.to_string())),
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

#[cfg(test)]
mod tests {
    use super::{
        record_ai_call_from_attribution, with_ai_ledger_attribution, with_ai_usage_meter,
        AiCostLedgerEntry, AiLedgerAttribution, AiUsageMeter,
    };

    #[tokio::test]
    async fn meter_accumulates_across_nested_calls() {
        let meter = AiUsageMeter::new();
        with_ai_usage_meter(meter.clone(), async {
            // 4000 chars in, 400 out -> length/4 estimates of 1000 + 100.
            record_ai_call_from_attribution("openai", "m", 4_000, 400, "completed", None).await;
            record_ai_call_from_attribution("openai", "m", 800, 0, "failed", Some("E")).await;
        })
        .await;
        assert_eq!(meter.total_tokens(), 1_000 + 100 + 200);
    }

    #[tokio::test]
    async fn meter_counts_calls_made_outside_any_attribution_scope() {
        // The planner runs with no attribution installed; a settlement still has
        // to see that spend, so metering cannot be gated on attribution.
        let meter = AiUsageMeter::new();
        with_ai_usage_meter(meter.clone(), async {
            record_ai_call_from_attribution("gemini", "m", 4_000, 0, "completed", None).await;
        })
        .await;
        assert_eq!(meter.total_tokens(), 1_000);
    }

    #[tokio::test]
    async fn inner_attribution_scope_does_not_detach_the_meter() {
        // The executor re-scopes attribution per recipe inside a turn; the turn's
        // meter must keep counting through it.
        let meter = AiUsageMeter::new();
        with_ai_usage_meter(meter.clone(), async {
            record_ai_call_from_attribution("openai", "m", 4_000, 0, "completed", None).await;
            with_ai_ledger_attribution(
                AiLedgerAttribution {
                    subject_id: 1,
                    owner_id: 1,
                    source: "agent".into(),
                    operation: "inner".into(),
                    tapp_id: "__agent__".into(),
                    task_id: "recipe_1".into(),
                },
                async {
                    record_ai_call_from_attribution("openai", "m", 8_000, 0, "completed", None)
                        .await;
                },
            )
            .await;
        })
        .await;
        assert_eq!(meter.total_tokens(), 1_000 + 2_000);
    }

    #[tokio::test]
    async fn calls_outside_a_meter_scope_are_harmless() {
        record_ai_call_from_attribution("openai", "m", 100, 100, "completed", None).await;
    }

    #[test]
    fn entry_fields_are_borrowed_for_zero_copy_call_sites() {
        let entry = AiCostLedgerEntry {
            subject_id: 1,
            owner_id: 1,
            tapp_id: "com.example.app",
            task_id: "ait_1",
            source: "scheduler",
            operation: "generate",
            provider: "openai",
            model: "gpt-test",
            input_tokens: 10,
            output_tokens: 20,
            status: "completed",
            error_code: None,
        };
        assert_eq!(entry.tapp_id, "com.example.app");
        assert_eq!(entry.input_tokens, 10);
        assert!(entry.error_code.is_none());
    }

    #[tokio::test]
    async fn task_local_attribution_is_visible_inside_scope() {
        let attr = AiLedgerAttribution {
            subject_id: 42,
            owner_id: 1,
            source: "agent".into(),
            operation: "chat".into(),
            tapp_id: "__agent__".into(),
            task_id: "t1".into(),
        };
        let seen = with_ai_ledger_attribution(attr, async {
            super::current_attribution().map(|a| a.subject_id)
        })
        .await;
        assert_eq!(seen, Some(42));
        assert!(super::current_attribution().is_none());
    }
}
