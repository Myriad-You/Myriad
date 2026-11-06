//! Append-only per-call AI cost ledger (write path).
//!
//! `ai_quota_usage` answers "how much budget is left today"; this ledger
//! answers "which caller spent what, when, on which provider/model". Entries are
//! written for governed AI tasks (Tapp / scheduler) via [`record_ai_cost`], and
//! for every other site-wide path from `AiAnalyzer` / image / speech hooks.
//! When a task-local [`AiLedgerAttribution`] is active, that source/user is
//! used; otherwise the hook still writes an `internal` row so usage cannot
//! vanish. Governed tasks wrap their provider call in
//! [`with_ai_ledger_suppressed`] to avoid double-count. **Admin subjects are
//! included** — this is full-site usage, not visitor-stats style staff
//! exclusion. Token counts are the same length/4 estimates the quota system
//! uses (images/speech use the helpers below); `cost_micro_usd` stays NULL
//! until a pricing source exists.
//!
//! HTTP list endpoint stays in the API layer; only the best-effort append lives
//! here so AI task execution does not import `crate::api`.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, Value as SeaValue};

/// Task-local attribution for non-governed paths that call `AiAnalyzer` / image /
/// speech directly (Agent, report generation, prompt tools, …). Governed tasks
/// already call [`record_ai_cost`] explicitly and should wrap the provider call
/// in [`with_ai_ledger_suppressed`] (avoids double-count).
#[derive(Debug, Clone)]
pub struct AiLedgerAttribution {
    pub subject_id: i32,
    pub owner_id: i32,
    pub source: String,
    pub operation: String,
    /// The Tapp installation that made the call; site callers (Agent,
    /// reports, the persona, …) have none and are told apart by `source`.
    pub tapp_id: Option<String>,
    pub task_id: String,
}

impl AiLedgerAttribution {
    /// Site-wide caller that is not a Tapp install: no `tapp_id`.
    pub fn site(subject_id: i32, source: impl Into<String>, operation: impl Into<String>) -> Self {
        let source = source.into();
        let operation = operation.into();
        Self {
            subject_id,
            owner_id: subject_id,
            tapp_id: None,
            task_id: operation.clone(),
            source,
            operation,
        }
    }
}

/// Running token total for one logical unit of work.
///
/// A caller that reserves AI quota up front needs to know what the work
/// actually cost in order to settle the reservation, but the spend is spread
/// across nested `AiAnalyzer` calls it never sees — an agent turn covers
/// planning, per-step handlers, dynamic-step analysis and response generation.
/// The meter is task-local and is not snapshotted onto spawned work (memory extraction).
/// Rather than thread counters through every call site, the
/// meter rides the same task-local scope the ledger already uses and is
/// incremented from the one place every call passes through.
///
/// Counts match ledger usage: provider-reported tokens when available, otherwise
/// length/4 estimates. Durable Work reservations may conservatively charge more.
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
    static WORK_USAGE_METER: AiUsageMeter;
    static AI_LEDGER_SUPPRESSED: bool;
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

/// Work accounting coexists with the outer daily-quota meter.
pub(crate) async fn with_work_usage_meter<F: Future>(meter: AiUsageMeter, fut: F) -> F::Output {
    WORK_USAGE_METER.scope(meter, fut).await
}

pub(crate) fn work_tokens() -> u64 {
    WORK_USAGE_METER
        .try_with(AiUsageMeter::total_tokens)
        .unwrap_or(0)
}

/// Skip hook writes inside `fut` because the caller records the task itself.
pub async fn with_ai_ledger_suppressed<F, T>(fut: F) -> T
where
    F: Future<Output = T>,
{
    AI_LEDGER_SUPPRESSED.scope(true, fut).await
}

/// Attribute nested analyzer / image / speech calls to a site-wide source.
pub async fn with_site_ai_ledger<F, T>(subject_id: i32, source: &str, operation: &str, fut: F) -> T
where
    F: Future<Output = T>,
{
    with_ai_ledger_attribution(
        AiLedgerAttribution::site(subject_id, source, operation),
        fut,
    )
    .await
}

/// Map a site-owner lookup onto a billing subject. Errors stay errors and
/// must not become another real user id.
pub fn site_owner_id_from_lookup(lookup: Result<Option<i32>, String>) -> Result<i32, String> {
    match lookup {
        Ok(Some(id)) => Ok(id),
        Ok(None) => Err("AI billing owner is not configured".to_string()),
        Err(error) => Err(error),
    }
}

/// Durable site owner. Lookup/decode failure is not user `1`.
pub async fn resolve_site_owner_id() -> Result<i32, String> {
    let lookup = match crate::services::process_db::database() {
        Ok(db) => crate::services::principal::site_owner_id(&db)
            .await
            .map_err(|_| "AI billing owner is unavailable".to_string()),
        Err(_) => Err("AI billing owner is unavailable".to_string()),
    };
    site_owner_id_from_lookup(lookup)
}

fn ledger_write_enabled() -> bool {
    !AI_LEDGER_SUPPRESSED
        .try_with(|suppressed| *suppressed)
        .unwrap_or(false)
}

fn fallback_attribution() -> AiLedgerAttribution {
    AiLedgerAttribution::site(0, "internal", "unattributed")
}

fn current_attribution() -> Option<AiLedgerAttribution> {
    AI_LEDGER_ATTRIBUTION.try_with(|c| c.clone()).ok()
}

/// Attribution active on this task, for work that is about to be spawned.
///
/// A detached task starts with empty task-locals, so background work would
/// otherwise vanish from the cost ledger. Callers re-install the snapshot with
/// [`with_ai_ledger_attribution`] inside the spawned future. The usage meter is
/// deliberately not part of the snapshot: spawned work outlives the turn that
/// would settle it.
pub fn current_ai_attribution() -> Option<AiLedgerAttribution> {
    current_attribution()
}

/// Length/4 estimate used by the text analyzer hook.
pub fn estimate_text_tokens(chars: usize) -> i32 {
    i32::try_from(chars / 4).unwrap_or(i32::MAX)
}

/// Prompt length/4 plus a size-based placeholder for one generated image.
pub fn estimate_image_tokens(prompt: &str, width: u32, height: u32) -> (i32, i32) {
    let input = estimate_text_tokens(prompt.len());
    let pixels = u64::from(width).saturating_mul(u64::from(height));
    let output = i32::try_from((pixels / 256).clamp(256, 16_384)).unwrap_or(16_384);
    (input, output)
}

/// File TTS: charge the spoken text on both sides until audio pricing exists.
pub fn estimate_tts_tokens(text: &str) -> (i32, i32) {
    let tokens = estimate_text_tokens(text.len()).max(1);
    (tokens, tokens)
}

/// File STT: audio bytes/100 in, transcript length/4 out.
pub fn estimate_stt_tokens(audio_bytes: usize, transcript: &str) -> (i32, i32) {
    let input = i32::try_from(audio_bytes / 100).unwrap_or(i32::MAX).max(1);
    let output = estimate_text_tokens(transcript.len());
    (input, output)
}

/// Best-effort ledger write from analyzer text hooks (`estimate_text_tokens`).
///
/// Writes even without a task-local attribution (`source=internal`) so a
/// forgotten wrapper cannot hide spend. Governed Tapp tasks suppress this.
pub async fn record_ai_call_from_attribution(
    provider: &str,
    model: &str,
    input_chars: usize,
    output_chars: usize,
    status: &str,
    error_code: Option<&str>,
) {
    record_ai_tokens_from_attribution(
        provider,
        model,
        estimate_text_tokens(input_chars),
        estimate_text_tokens(output_chars),
        status,
        error_code,
    )
    .await;
}

/// Same hook with caller-supplied token estimates (images, speech).
/// `fut`, its calls counted under `step` by the tests' spend counter;
/// outside tests just `fut`.
pub(crate) async fn as_step<F: std::future::Future>(step: &str, fut: F) -> F::Output {
    #[cfg(test)]
    return spent::during(step, fut).await;
    #[cfg(not(test))]
    {
        let _ = step;
        fut.await
    }
}

pub async fn record_ai_tokens_from_attribution(
    provider: &str,
    model: &str,
    input_tokens: i32,
    output_tokens: i32,
    status: &str,
    error_code: Option<&str>,
) {
    let input_tokens = input_tokens.max(0);
    let output_tokens = output_tokens.max(0);

    #[cfg(test)]
    spent::add(model, input_tokens, output_tokens);

    // Metered even without attribution: planner / nested work must still settle.
    let _ = AI_USAGE_METER.try_with(|meter| {
        meter.add(u64::from(input_tokens as u32) + u64::from(output_tokens as u32));
    });

    let _ = WORK_USAGE_METER.try_with(|meter| {
        meter.add(u64::from(input_tokens as u32) + u64::from(output_tokens as u32))
    });

    if !ledger_write_enabled() {
        return;
    }
    let attr = current_attribution().unwrap_or_else(fallback_attribution);
    let Ok(db) = crate::services::process_db::database() else {
        return;
    };
    record_ai_cost(
        &db,
        AiCostLedgerEntry {
            subject_id: attr.subject_id,
            owner_id: attr.owner_id,
            tapp_id: attr.tapp_id.as_deref(),
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
    /// The Tapp installation, when a Tapp made the call.
    pub tapp_id: Option<&'a str>,
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
                INSERT INTO ai_cost_ledger
                    (subject_id, owner_id, tapp_id, task_id, source, operation,
                     provider, model, input_tokens, output_tokens,
                     tokens_estimated, status, error_code)
                VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, TRUE, $11, $12)
            "#,
            vec![
                SeaValue::Int(Some(entry.subject_id)),
                SeaValue::Int(Some(entry.owner_id)),
                SeaValue::String(entry.tapp_id.map(str::to_string)),
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
            tapp_id = ?entry.tapp_id,
            task_id = entry.task_id,
            "[AI ledger] Failed to append AI cost ledger entry"
        );
    }
}

/// What every model call in this test process spent, by model, with the
/// same estimates the ledger keeps: the memory benches report it, so what a
/// run costs is known from the last run instead of guessed.
#[cfg(test)]
pub(crate) mod spent {
    use std::collections::BTreeMap;
    use std::sync::Mutex;

    /// Calls, input tokens, output tokens.
    type Spent = BTreeMap<String, (u64, u64, u64)>;
    static SPENT: Mutex<Spent> = Mutex::new(BTreeMap::new());
    /// The same, by what each call was for.
    static BY_STEP: Mutex<Spent> = Mutex::new(BTreeMap::new());

    tokio::task_local! {
        static STEP: String;
    }

    /// `fut`, its calls counted as `step` unless an outer step is set.
    pub(super) async fn during<F: std::future::Future>(step: &str, fut: F) -> F::Output {
        if STEP.try_with(|_| ()).is_ok() {
            fut.await
        } else {
            STEP.scope(step.to_string(), fut).await
        }
    }

    fn count(into: &Mutex<Spent>, key: String, input_tokens: i32, output_tokens: i32) {
        let mut spent = into.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let entry = spent.entry(key).or_default();
        entry.0 += 1;
        entry.1 += input_tokens as u64;
        entry.2 += output_tokens as u64;
    }

    pub(super) fn add(model: &str, input_tokens: i32, output_tokens: i32) {
        count(&SPENT, model.to_string(), input_tokens, output_tokens);
        let step = STEP
            .try_with(|step| step.clone())
            .ok()
            .or_else(|| super::current_ai_attribution().map(|attribution| attribution.operation))
            .unwrap_or_else(|| "unlabeled".to_string());
        count(&BY_STEP, step, input_tokens, output_tokens);
    }

    /// What was spent since the last call, by model, by step, and in all.
    pub(crate) fn take() -> serde_json::Value {
        let taken = |from: &Mutex<Spent>| {
            std::mem::take(&mut *from.lock().unwrap_or_else(|poisoned| poisoned.into_inner()))
        };
        let (spent, by_step) = (taken(&SPENT), taken(&BY_STEP));
        let total = spent
            .values()
            .fold((0, 0, 0), |sum, one| (sum.0 + one.0, sum.1 + one.1, sum.2 + one.2));
        let line = |(calls, input, output): (u64, u64, u64)| {
            serde_json::json!({"calls": calls, "input_tokens": input, "output_tokens": output})
        };
        let lines = |spent: Spent| {
            spent
                .into_iter()
                .map(|(key, one)| (key, line(one)))
                .collect::<serde_json::Map<_, _>>()
        };
        serde_json::json!({
            "estimated": "length/4 per call, as the ledger counts",
            "by_model": lines(spent),
            "by_step": lines(by_step),
            "total": line(total),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::{
        AiCostLedgerEntry, AiLedgerAttribution, AiUsageMeter, estimate_image_tokens,
        estimate_stt_tokens, estimate_text_tokens, estimate_tts_tokens, ledger_write_enabled,
        record_ai_call_from_attribution, with_ai_ledger_attribution, with_ai_ledger_suppressed,
        with_ai_usage_meter, with_site_ai_ledger,
    };
    use super::{record_ai_tokens_from_attribution, with_work_usage_meter};

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
        // Metering is independent of attribution scope.
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
                    tapp_id: None,
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
    async fn work_meter_keeps_outer_quota_accounting() {
        let outer = AiUsageMeter::new();
        let inner = AiUsageMeter::new();
        with_ai_usage_meter(
            outer.clone(),
            with_work_usage_meter(inner.clone(), async {
                record_ai_tokens_from_attribution("openai", "fixture", 123, 45, "completed", None)
                    .await;
            }),
        )
        .await;
        assert_eq!(outer.total_tokens(), 168);
        assert_eq!(inner.total_tokens(), 168);
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
            tapp_id: Some("com.example.app"),
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
        assert_eq!(entry.tapp_id, Some("com.example.app"));
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
            tapp_id: None,
            task_id: "t1".into(),
        };
        let seen = with_ai_ledger_attribution(attr, async {
            super::current_attribution().map(|a| a.subject_id)
        })
        .await;
        assert_eq!(seen, Some(42));
        assert!(super::current_attribution().is_none());
    }

    #[test]
    fn site_attribution_uses_source_bucket() {
        let attr = AiLedgerAttribution::site(7, "merope", "onboarding");
        assert_eq!(attr.subject_id, 7);
        assert_eq!(attr.owner_id, 7);
        assert_eq!(attr.source, "merope");
        assert_eq!(attr.operation, "onboarding");
        assert_eq!(attr.tapp_id, None, "the persona is not a Tapp");
        assert_eq!(attr.task_id, "onboarding");
    }

    #[test]
    fn image_and_speech_estimates_are_nonzero() {
        assert_eq!(estimate_text_tokens(400), 100);
        let (input, output) = estimate_image_tokens("abcd", 1024, 1024);
        assert_eq!(input, 1);
        assert_eq!(output, 4096);
        let (tts_in, tts_out) = estimate_tts_tokens("hello");
        assert!(tts_in >= 1);
        assert_eq!(tts_in, tts_out);
        let (stt_in, stt_out) = estimate_stt_tokens(2_000, "abcd");
        assert_eq!(stt_in, 20);
        assert_eq!(stt_out, 1);
    }

    #[tokio::test]
    async fn suppress_disables_ledger_writes_but_meter_still_counts() {
        assert!(ledger_write_enabled());
        let meter = AiUsageMeter::new();
        with_ai_usage_meter(meter.clone(), async {
            with_ai_ledger_suppressed(async {
                assert!(!ledger_write_enabled());
                record_ai_call_from_attribution("openai", "m", 4_000, 0, "completed", None).await;
            })
            .await;
            assert!(ledger_write_enabled());
        })
        .await;
        assert_eq!(meter.total_tokens(), 1_000);
    }

    #[tokio::test]
    async fn site_helper_installs_attribution() {
        let seen = with_site_ai_ledger(3, "playground", "generate", async {
            super::current_attribution().map(|a| (a.subject_id, a.source, a.tapp_id))
        })
        .await;
        assert_eq!(
            seen,
            Some((3, "playground".into(), None))
        );
    }

    #[test]
    fn owner_lookup_failure_is_not_user_one() {
        assert_eq!(
            super::site_owner_id_from_lookup(Err("db down".into())).unwrap_err(),
            "db down"
        );
        assert!(
            super::site_owner_id_from_lookup(Ok(None))
                .unwrap_err()
                .contains("not configured")
        );
        assert_ne!(super::site_owner_id_from_lookup(Err("db down".into())).ok(), Some(1));
        assert_ne!(super::site_owner_id_from_lookup(Ok(None)).ok(), Some(1));
        assert_eq!(super::site_owner_id_from_lookup(Ok(Some(42))).unwrap(), 42);
    }
}
