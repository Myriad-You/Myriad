//! Persistent, server-authoritative AI quota ledger for Tapp runtimes.
//!
//! Domain implementation lives in services so `ai_tasks` / governed paths do not
//! own HTTP error tuples for reserve/settle/release/usage.

use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use sea_orm::{
    ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait, Value as SeaValue,
};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

use crate::services::permission_service::UserRole;
use crate::GLOBAL_DYNAMIC_CONFIG;

#[derive(Debug, Clone, Copy)]
struct AiQuotaLimits {
    calls: i32,
    tokens: i32,
    cooldown_seconds: i32,
    unlimited: bool,
}

#[derive(Debug, Clone)]
pub struct AiQuotaReservation {
    buckets: Vec<AiQuotaBucket>,
    reserved_tokens: i32,
    unlimited: bool,
}

#[derive(Debug, Clone)]
struct AiQuotaBucket {
    subject_id: i32,
    ledger_tapp_id: String,
    calls_type: String,
    tokens_type: String,
}

#[derive(Debug, Clone)]
struct AiQuotaBucketLimits {
    bucket: AiQuotaBucket,
    calls: i32,
    tokens: i32,
    enforce_cooldown: bool,
    anonymous: bool,
}

/// Knobs for a single [`reserve_ai_quota_with_options`] call.
#[derive(Debug, Clone, Copy, Default)]
pub struct AiQuotaReserveOptions {
    /// Skip the per-role cooldown gate for this reservation.
    ///
    /// Cooldown exists to space out *new* demand. A continuation — the user
    /// confirming a sensitive step we asked about, or answering a question the
    /// agent raised — is the tail of a turn they already paid a call for, and
    /// arrives exactly as fast as a human can click. With the default
    /// `user_ai_cooldown_seconds = 5` / `guest_ai_cooldown_seconds = 10`, gating
    /// it means "plan → confirm" reliably answers `AI_COOLDOWN_ACTIVE` instead
    /// of doing the work the user just approved.
    ///
    /// The call charge and the token check still apply, and the cooldown clock
    /// is still advanced by the reservation — a continuation does spend AI, so
    /// the *next* fresh turn is spaced from it.
    pub skip_cooldown: bool,
}

const ANONYMOUS_LEDGER_USER_ID: i32 = 0;
const GUEST_IP_BUDGET_MULTIPLIER: i32 = 3;
const GUEST_SITE_BUDGET_MULTIPLIER: i32 = 100;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsageCounter {
    /// `None` means unlimited; JSON encodes this as null instead of Infinity.
    pub limit: Option<i32>,
    pub used: i32,
    pub remaining: Option<i32>,
    pub resets_at: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiCooldownStatus {
    pub required_seconds: i32,
    pub remaining_seconds: i32,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiUsageSnapshot {
    pub calls: AiUsageCounter,
    pub tokens: AiUsageCounter,
    pub cooldown: AiCooldownStatus,
    pub restricted: bool,
    pub restriction_reason: Option<String>,
    pub unlimited: bool,
    pub role: UserRole,
}

/// Domain errors for quota ledger operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AiQuotaError {
    Ledger { message: String },
    Cooldown { remaining_seconds: i64 },
    DailyCallLimit { anonymous: bool },
    DailyTokenLimit { anonymous: bool },
}

impl AiQuotaError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Ledger { .. } => "AI_QUOTA_LEDGER_ERROR",
            Self::Cooldown { .. } => "AI_COOLDOWN_ACTIVE",
            Self::DailyCallLimit { anonymous: false } => "AI_DAILY_CALL_LIMIT",
            Self::DailyCallLimit { anonymous: true } => "AI_ANONYMOUS_DAILY_CALL_LIMIT",
            Self::DailyTokenLimit { anonymous: false } => "AI_DAILY_TOKEN_LIMIT",
            Self::DailyTokenLimit { anonymous: true } => "AI_ANONYMOUS_DAILY_TOKEN_LIMIT",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Ledger { message } => message.clone(),
            Self::Cooldown { remaining_seconds } => {
                format!("AI cooldown active; retry after {remaining_seconds} seconds")
            }
            Self::DailyCallLimit { .. } => "Daily AI call limit reached".to_string(),
            Self::DailyTokenLimit { .. } => {
                "Daily AI token budget is insufficient for this request".to_string()
            }
        }
    }

    /// HTTP status class: 429 for budget/cooldown, 500 for ledger faults.
    pub fn is_client_limit(&self) -> bool {
        !matches!(self, Self::Ledger { .. })
    }
}

/// Recognise a stringified [`AiQuotaError`] that is a client budget rejection.
///
/// Paths that surface quota failures as plain `String` (the agent returns
/// `Result<_, String>` throughout) would otherwise report "out of budget" as a
/// 500. [`Display`](std::fmt::Display) writes `CODE: message`, so the leading
/// code is a stable discriminator. Ledger faults are deliberately excluded —
/// those really are server errors.
pub fn is_client_limit_message(message: &str) -> bool {
    const CLIENT_LIMIT_CODES: &[&str] = &[
        "AI_COOLDOWN_ACTIVE",
        "AI_DAILY_CALL_LIMIT",
        "AI_ANONYMOUS_DAILY_CALL_LIMIT",
        "AI_DAILY_TOKEN_LIMIT",
        "AI_ANONYMOUS_DAILY_TOKEN_LIMIT",
    ];
    CLIENT_LIMIT_CODES
        .iter()
        .any(|code| message.starts_with(code))
}

impl std::fmt::Display for AiQuotaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.code(), self.message())
    }
}

impl std::error::Error for AiQuotaError {}

fn ledger_error(message: impl Into<String>) -> AiQuotaError {
    AiQuotaError::Ledger {
        message: message.into(),
    }
}

/// HMAC fingerprint for anonymous guest quota scopes (IP / unresolved).
fn anonymous_subject_fingerprint(value: &str) -> String {
    let secret = std::env::var("JWT_SECRET")
        .unwrap_or_else(|_| "myriad-development-anonymous-quota-v1".to_string());
    let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes())
        .expect("HMAC accepts arbitrary key lengths");
    mac.update(b"myriad-tapp-anonymous-quota-v1\0");
    mac.update(value.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

async fn limits_for_role(role: UserRole) -> AiQuotaLimits {
    if role == UserRole::Admin {
        return AiQuotaLimits {
            calls: i32::MAX,
            tokens: i32::MAX,
            cooldown_seconds: 0,
            unlimited: true,
        };
    }

    let config = GLOBAL_DYNAMIC_CONFIG.read().await;
    match role {
        UserRole::User => AiQuotaLimits {
            calls: config.user_ai_daily_calls.max(0),
            tokens: config.user_ai_daily_tokens.max(0),
            cooldown_seconds: config.user_ai_cooldown_seconds.max(0),
            unlimited: false,
        },
        UserRole::Guest => AiQuotaLimits {
            calls: config.guest_ai_daily_calls.max(0),
            tokens: config.guest_ai_daily_tokens.max(0),
            cooldown_seconds: config.guest_ai_cooldown_seconds.max(0),
            unlimited: false,
        },
        UserRole::Admin => unreachable!(),
    }
}

fn quota_type(kind: &str, owner_id: i32) -> String {
    format!("ai_{kind}:owner:{owner_id}")
}

fn anonymous_quota_type(kind: &str, owner_id: i32, scope: &str) -> String {
    format!("ai_{kind}:owner:{owner_id}:{scope}")
}

fn guest_quota_buckets(
    subject_id: i32,
    owner_id: i32,
    tapp_id: &str,
    limits: AiQuotaLimits,
    anonymous_scope: Option<&str>,
) -> Vec<AiQuotaBucketLimits> {
    let mut buckets = vec![AiQuotaBucketLimits {
        bucket: AiQuotaBucket {
            subject_id,
            ledger_tapp_id: tapp_id.to_string(),
            calls_type: quota_type("calls", owner_id),
            tokens_type: quota_type("tokens", owner_id),
        },
        calls: limits.calls,
        tokens: limits.tokens,
        enforce_cooldown: true,
        anonymous: false,
    }];

    let digest = anonymous_subject_fingerprint(anonymous_scope.unwrap_or("unresolved"));
    let ip_scope = format!("ip:{}", &digest[..12]);
    buckets.push(AiQuotaBucketLimits {
        bucket: AiQuotaBucket {
            subject_id: ANONYMOUS_LEDGER_USER_ID,
            ledger_tapp_id: tapp_id.to_string(),
            calls_type: anonymous_quota_type("calls", owner_id, &ip_scope),
            tokens_type: anonymous_quota_type("tokens", owner_id, &ip_scope),
        },
        calls: limits.calls.saturating_mul(GUEST_IP_BUDGET_MULTIPLIER),
        tokens: limits.tokens.saturating_mul(GUEST_IP_BUDGET_MULTIPLIER),
        enforce_cooldown: false,
        anonymous: true,
    });
    buckets.push(AiQuotaBucketLimits {
        bucket: AiQuotaBucket {
            subject_id: ANONYMOUS_LEDGER_USER_ID,
            ledger_tapp_id: "__anonymous_ai_site__".to_string(),
            calls_type: anonymous_quota_type("calls", owner_id, "guest-site"),
            tokens_type: anonymous_quota_type("tokens", owner_id, "guest-site"),
        },
        calls: limits.calls.saturating_mul(GUEST_SITE_BUDGET_MULTIPLIER),
        tokens: limits.tokens.saturating_mul(GUEST_SITE_BUDGET_MULTIPLIER),
        enforce_cooldown: false,
        anonymous: true,
    });
    buckets
}

fn period_end() -> String {
    let tomorrow = Utc::now().date_naive().succ_opt().unwrap_or_default();
    tomorrow
        .and_hms_opt(0, 0, 0)
        .map(|value| DateTime::<Utc>::from_naive_utc_and_offset(value, Utc).to_rfc3339())
        .unwrap_or_else(|| Utc::now().to_rfc3339())
}

fn insert_quota_sql() -> &'static str {
    r#"
        INSERT INTO tapp_quota_usage
            (tapp_id, user_id, quota_type, used, "limit", period_start, period_end, updated_at)
        VALUES
            ($1, $2, $3, 0, $4, date_trunc('day', NOW()),
             date_trunc('day', NOW()) + interval '1 day', NOW())
        ON CONFLICT (user_id, tapp_id, quota_type, period_start)
        DO UPDATE SET "limit" = EXCLUDED."limit"
    "#
}

async fn ensure_quota_row<C: ConnectionTrait>(
    db: &C,
    subject_id: i32,
    tapp_id: &str,
    quota_type: &str,
    limit: i32,
) -> Result<(), AiQuotaError> {
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        insert_quota_sql(),
        vec![
            SeaValue::String(Some(tapp_id.to_string())),
            SeaValue::Int(Some(subject_id)),
            SeaValue::String(Some(quota_type.to_string())),
            SeaValue::Int(Some(limit)),
        ],
    ))
    .await
    .map_err(|error| {
        tracing::error!(error = %error, "[TAPP] Failed to initialize AI quota row");
        ledger_error("Failed to initialize AI quota ledger")
    })?;
    Ok(())
}

async fn read_row_for_update<C: ConnectionTrait>(
    db: &C,
    subject_id: i32,
    tapp_id: &str,
    quota_type: &str,
) -> Result<(i32, DateTime<Utc>), AiQuotaError> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"
                SELECT used, updated_at
                FROM tapp_quota_usage
                WHERE user_id = $1 AND tapp_id = $2 AND quota_type = $3
                  AND period_start = date_trunc('day', NOW())
                FOR UPDATE
            "#,
            vec![
                SeaValue::Int(Some(subject_id)),
                SeaValue::String(Some(tapp_id.to_string())),
                SeaValue::String(Some(quota_type.to_string())),
            ],
        ))
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "[TAPP] Failed to lock AI quota row");
            ledger_error("Failed to read AI quota ledger")
        })?
        .ok_or_else(|| ledger_error("AI quota row is missing"))?;

    let used = row
        .try_get::<i32>("", "used")
        .map_err(|_| ledger_error("AI quota usage is invalid"))?;
    let updated_at = row
        .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "updated_at")
        .map(|value| value.with_timezone(&Utc))
        .map_err(|_| ledger_error("AI quota timestamp is invalid"))?;
    Ok((used, updated_at))
}

async fn increment_row<C: ConnectionTrait>(
    db: &C,
    subject_id: i32,
    tapp_id: &str,
    quota_type: &str,
    amount: i32,
    touch: bool,
) -> Result<(), AiQuotaError> {
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        if touch {
            r#"
                UPDATE tapp_quota_usage
                SET used = GREATEST(0, used + $4), updated_at = NOW()
                WHERE user_id = $1 AND tapp_id = $2 AND quota_type = $3
                  AND period_start = date_trunc('day', NOW())
            "#
        } else {
            r#"
                UPDATE tapp_quota_usage
                SET used = GREATEST(0, used + $4)
                WHERE user_id = $1 AND tapp_id = $2 AND quota_type = $3
                  AND period_start = date_trunc('day', NOW())
            "#
        },
        vec![
            SeaValue::Int(Some(subject_id)),
            SeaValue::String(Some(tapp_id.to_string())),
            SeaValue::String(Some(quota_type.to_string())),
            SeaValue::Int(Some(amount)),
        ],
    ))
    .await
    .map_err(|error| {
        tracing::error!(error = %error, "[TAPP] Failed to update AI quota row");
        ledger_error("Failed to update AI quota ledger")
    })?;
    Ok(())
}

/// Seconds still owed on the cooldown, or `None` when the call may proceed.
///
/// `enforced` folds together the bucket's own `enforce_cooldown` (guest IP and
/// site-wide buckets never gate on it — one shared clock across users would let
/// anyone stall everyone) and the per-call
/// [`AiQuotaReserveOptions::skip_cooldown`]. Note this only decides the *check*:
/// the reservation still touches `updated_at`, so the clock keeps running for
/// whoever comes next.
fn cooldown_remaining(
    enforced: bool,
    calls_used: i32,
    elapsed_seconds: i64,
    cooldown_seconds: i32,
) -> Option<i64> {
    if !enforced || calls_used <= 0 {
        return None;
    }
    let required = i64::from(cooldown_seconds);
    (elapsed_seconds < required).then(|| required - elapsed_seconds)
}

pub async fn reserve_ai_quota(
    db: &DatabaseConnection,
    role: UserRole,
    subject_id: i32,
    owner_id: i32,
    tapp_id: &str,
    estimated_tokens: usize,
    anonymous_scope: Option<&str>,
) -> Result<AiQuotaReservation, AiQuotaError> {
    reserve_ai_quota_with_options(
        db,
        role,
        subject_id,
        owner_id,
        tapp_id,
        estimated_tokens,
        anonymous_scope,
        AiQuotaReserveOptions::default(),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub async fn reserve_ai_quota_with_options(
    db: &DatabaseConnection,
    role: UserRole,
    subject_id: i32,
    owner_id: i32,
    tapp_id: &str,
    estimated_tokens: usize,
    anonymous_scope: Option<&str>,
    options: AiQuotaReserveOptions,
) -> Result<AiQuotaReservation, AiQuotaError> {
    let limits = limits_for_role(role).await;
    if limits.unlimited {
        return Ok(AiQuotaReservation {
            buckets: Vec::new(),
            reserved_tokens: 0,
            unlimited: true,
        });
    }

    let estimated_tokens = i32::try_from(estimated_tokens).unwrap_or(i32::MAX).max(0);
    let cooldown_seconds = limits.cooldown_seconds;
    let bucket_limits = if role == UserRole::Guest {
        guest_quota_buckets(subject_id, owner_id, tapp_id, limits, anonymous_scope)
    } else {
        vec![AiQuotaBucketLimits {
            bucket: AiQuotaBucket {
                subject_id,
                ledger_tapp_id: tapp_id.to_string(),
                calls_type: quota_type("calls", owner_id),
                tokens_type: quota_type("tokens", owner_id),
            },
            calls: limits.calls,
            tokens: limits.tokens,
            enforce_cooldown: true,
            anonymous: false,
        }]
    };
    let txn = db
        .begin()
        .await
        .map_err(|_| ledger_error("Failed to start AI quota transaction"))?;

    for limits in &bucket_limits {
        ensure_quota_row(
            &txn,
            limits.bucket.subject_id,
            &limits.bucket.ledger_tapp_id,
            &limits.bucket.calls_type,
            limits.calls,
        )
        .await?;
        ensure_quota_row(
            &txn,
            limits.bucket.subject_id,
            &limits.bucket.ledger_tapp_id,
            &limits.bucket.tokens_type,
            limits.tokens,
        )
        .await?;
        let (calls_used, last_call_at) = read_row_for_update(
            &txn,
            limits.bucket.subject_id,
            &limits.bucket.ledger_tapp_id,
            &limits.bucket.calls_type,
        )
        .await?;
        let (tokens_used, _) = read_row_for_update(
            &txn,
            limits.bucket.subject_id,
            &limits.bucket.ledger_tapp_id,
            &limits.bucket.tokens_type,
        )
        .await?;

        let cooldown_elapsed = Utc::now()
            .signed_duration_since(last_call_at)
            .num_seconds()
            .max(0);
        if let Some(remaining_seconds) = cooldown_remaining(
            limits.enforce_cooldown && !options.skip_cooldown,
            calls_used,
            cooldown_elapsed,
            cooldown_seconds,
        ) {
            return Err(AiQuotaError::Cooldown { remaining_seconds });
        }
        if calls_used >= limits.calls {
            return Err(AiQuotaError::DailyCallLimit {
                anonymous: limits.anonymous,
            });
        }
        if estimated_tokens > limits.tokens.saturating_sub(tokens_used) {
            return Err(AiQuotaError::DailyTokenLimit {
                anonymous: limits.anonymous,
            });
        }
    }

    for limits in &bucket_limits {
        increment_row(
            &txn,
            limits.bucket.subject_id,
            &limits.bucket.ledger_tapp_id,
            &limits.bucket.calls_type,
            1,
            limits.enforce_cooldown,
        )
        .await?;
        increment_row(
            &txn,
            limits.bucket.subject_id,
            &limits.bucket.ledger_tapp_id,
            &limits.bucket.tokens_type,
            estimated_tokens,
            false,
        )
        .await?;
    }
    txn.commit()
        .await
        .map_err(|_| ledger_error("Failed to commit AI quota reservation"))?;

    Ok(AiQuotaReservation {
        buckets: bucket_limits
            .into_iter()
            .map(|limits| limits.bucket)
            .collect(),
        reserved_tokens: estimated_tokens,
        unlimited: false,
    })
}

pub async fn settle_ai_quota(
    db: &DatabaseConnection,
    reservation: &AiQuotaReservation,
    actual_tokens: usize,
) -> Result<(), AiQuotaError> {
    if reservation.unlimited {
        return Ok(());
    }
    let actual = i32::try_from(actual_tokens).unwrap_or(i32::MAX).max(0);
    let delta = (i64::from(actual) - i64::from(reservation.reserved_tokens))
        .clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32;
    for bucket in &reservation.buckets {
        increment_row(
            db,
            bucket.subject_id,
            &bucket.ledger_tapp_id,
            &bucket.tokens_type,
            delta,
            false,
        )
        .await?;
    }
    Ok(())
}

pub async fn release_ai_token_reservation(
    db: &DatabaseConnection,
    reservation: &AiQuotaReservation,
) -> Result<(), AiQuotaError> {
    if reservation.unlimited || reservation.reserved_tokens == 0 {
        return Ok(());
    }
    for bucket in &reservation.buckets {
        increment_row(
            db,
            bucket.subject_id,
            &bucket.ledger_tapp_id,
            &bucket.tokens_type,
            -reservation.reserved_tokens,
            false,
        )
        .await?;
    }
    Ok(())
}

/// Roll back a reservation when the task itself was never registered. Provider
/// failures still consume one call, but a cross-replica registration race or
/// registry outage must not charge for work that never started.
pub async fn rollback_ai_quota_reservation(
    db: &DatabaseConnection,
    reservation: &AiQuotaReservation,
) -> Result<(), AiQuotaError> {
    if reservation.unlimited {
        return Ok(());
    }
    let transaction = db
        .begin()
        .await
        .map_err(|_| ledger_error("Failed to start AI quota rollback"))?;
    for bucket in &reservation.buckets {
        increment_row(
            &transaction,
            bucket.subject_id,
            &bucket.ledger_tapp_id,
            &bucket.calls_type,
            -1,
            false,
        )
        .await?;
        if reservation.reserved_tokens > 0 {
            increment_row(
                &transaction,
                bucket.subject_id,
                &bucket.ledger_tapp_id,
                &bucket.tokens_type,
                -reservation.reserved_tokens,
                false,
            )
            .await?;
        }
    }
    transaction
        .commit()
        .await
        .map_err(|_| ledger_error("Failed to commit AI quota rollback"))?;
    Ok(())
}

async fn read_usage_value(
    db: &DatabaseConnection,
    subject_id: i32,
    tapp_id: &str,
    quota_type: &str,
) -> Result<Option<(i32, DateTime<Utc>)>, AiQuotaError> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"
                SELECT used, updated_at
                FROM tapp_quota_usage
                WHERE user_id = $1 AND tapp_id = $2 AND quota_type = $3
                  AND period_start = date_trunc('day', NOW())
            "#,
            vec![
                SeaValue::Int(Some(subject_id)),
                SeaValue::String(Some(tapp_id.to_string())),
                SeaValue::String(Some(quota_type.to_string())),
            ],
        ))
        .await
        .map_err(|_| ledger_error("Failed to read AI quota usage"))?;
    row.map(|row| {
        let used = row
            .try_get::<i32>("", "used")
            .map_err(|_| ledger_error("AI quota usage is invalid"))?;
        let updated = row
            .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "updated_at")
            .map(|value| value.with_timezone(&Utc))
            .map_err(|_| ledger_error("AI quota timestamp is invalid"))?;
        Ok((used, updated))
    })
    .transpose()
}

pub async fn get_ai_usage(
    db: &DatabaseConnection,
    role: UserRole,
    subject_id: i32,
    owner_id: i32,
    tapp_id: &str,
) -> Result<AiUsageSnapshot, AiQuotaError> {
    let limits = limits_for_role(role).await;
    let resets_at = period_end();
    if limits.unlimited {
        return Ok(AiUsageSnapshot {
            calls: AiUsageCounter {
                limit: None,
                used: 0,
                remaining: None,
                resets_at: resets_at.clone(),
            },
            tokens: AiUsageCounter {
                limit: None,
                used: 0,
                remaining: None,
                resets_at,
            },
            cooldown: AiCooldownStatus {
                required_seconds: 0,
                remaining_seconds: 0,
            },
            restricted: false,
            restriction_reason: None,
            unlimited: true,
            role,
        });
    }

    let calls = read_usage_value(db, subject_id, tapp_id, &quota_type("calls", owner_id)).await?;
    let tokens = read_usage_value(db, subject_id, tapp_id, &quota_type("tokens", owner_id)).await?;
    let calls_used = calls.as_ref().map_or(0, |value| value.0);
    let tokens_used = tokens.as_ref().map_or(0, |value| value.0);
    let remaining_seconds = calls
        .map(|(_, updated_at)| {
            let elapsed = Utc::now()
                .signed_duration_since(updated_at)
                .num_seconds()
                .max(0);
            (i64::from(limits.cooldown_seconds) - elapsed).max(0) as i32
        })
        .unwrap_or(0);
    let restriction_reason = if calls_used >= limits.calls {
        Some("daily_calls".to_string())
    } else if tokens_used >= limits.tokens {
        Some("daily_tokens".to_string())
    } else if remaining_seconds > 0 {
        Some("cooldown".to_string())
    } else {
        None
    };

    Ok(AiUsageSnapshot {
        calls: AiUsageCounter {
            limit: Some(limits.calls),
            used: calls_used,
            remaining: Some(limits.calls.saturating_sub(calls_used)),
            resets_at: resets_at.clone(),
        },
        tokens: AiUsageCounter {
            limit: Some(limits.tokens),
            used: tokens_used,
            remaining: Some(limits.tokens.saturating_sub(tokens_used)),
            resets_at,
        },
        cooldown: AiCooldownStatus {
            required_seconds: limits.cooldown_seconds,
            remaining_seconds,
        },
        restricted: restriction_reason.is_some(),
        restriction_reason,
        unlimited: false,
        role,
    })
}

#[cfg(test)]
mod tests {
    use super::{
        cooldown_remaining, guest_quota_buckets, is_client_limit_message, quota_type, AiQuotaError,
        AiQuotaLimits, AiQuotaReserveOptions,
    };

    #[test]
    fn cooldown_gates_a_fresh_call_but_not_the_first_of_the_day() {
        // Default config: user 5s, guest 10s.
        assert_eq!(cooldown_remaining(true, 1, 2, 5), Some(3));
        assert_eq!(cooldown_remaining(true, 1, 5, 5), None);
        assert_eq!(cooldown_remaining(true, 1, 9, 5), None);
        // Nothing spent yet today — nothing to space out from.
        assert_eq!(cooldown_remaining(true, 0, 0, 5), None);
        // Cooldown disabled by config.
        assert_eq!(cooldown_remaining(true, 3, 0, 0), None);
    }

    #[test]
    fn continuations_are_not_blocked_by_the_cooldown_they_just_started() {
        // The bug: `process` plans, returns a confirmation and settles (call +1,
        // clock starts). The user clicks confirm ~2s later. Reserving that
        // continuation as a fresh turn answers AI_COOLDOWN_ACTIVE instead of
        // running the work they just approved.
        let elapsed_since_plan = 2;
        assert_eq!(
            cooldown_remaining(true, 1, elapsed_since_plan, 5),
            Some(3),
            "a brand-new turn this soon is still spaced out"
        );

        // What `AgentTurnBudget::run_continuation` passes down.
        let continuation = AiQuotaReserveOptions {
            skip_cooldown: true,
        };
        let bucket_enforces = true;
        assert_eq!(
            cooldown_remaining(
                bucket_enforces && !continuation.skip_cooldown,
                1,
                elapsed_since_plan,
                5
            ),
            None,
            "confirm / resume continue a turn the user already paid for"
        );
    }

    #[test]
    fn skip_cooldown_is_opt_in() {
        assert!(!AiQuotaReserveOptions::default().skip_cooldown);
    }

    #[test]
    fn anonymous_guest_buckets_never_own_the_cooldown_clock() {
        let limits = AiQuotaLimits {
            calls: 10,
            tokens: 5_000,
            cooldown_seconds: 10,
            unlimited: false,
        };
        let buckets = guest_quota_buckets(-42, 1, "com.example.app", limits, Some("203.0.113.8"));
        assert!(buckets[0].enforce_cooldown, "per-subject bucket gates");
        for shared in &buckets[1..] {
            assert!(
                cooldown_remaining(shared.enforce_cooldown, 5, 0, 10).is_none(),
                "a shared bucket's clock would let one guest stall every other"
            );
        }
    }

    #[test]
    fn quota_key_isolated_by_install_owner() {
        assert_ne!(quota_type("calls", 1), quota_type("calls", 2));
        assert_ne!(quota_type("calls", 1), quota_type("tokens", 1));
    }

    #[test]
    fn guest_budgets_include_hashed_ip_and_global_site_caps() {
        let limits = AiQuotaLimits {
            calls: 10,
            tokens: 5_000,
            cooldown_seconds: 10,
            unlimited: false,
        };
        let buckets = guest_quota_buckets(-42, 1, "com.example.app", limits, Some("203.0.113.8"));

        assert_eq!(buckets.len(), 3);
        assert_eq!(buckets[0].calls, 10);
        assert_eq!(buckets[1].calls, 30);
        assert_eq!(buckets[2].calls, 1_000);
        assert_eq!(buckets[2].bucket.ledger_tapp_id, "__anonymous_ai_site__");
        assert!(!buckets[1].bucket.calls_type.contains("203.0.113.8"));
        assert_ne!(
            buckets[1].bucket.calls_type,
            guest_quota_buckets(-99, 1, "com.example.app", limits, Some("203.0.113.9"))[1]
                .bucket
                .calls_type
        );
    }

    #[test]
    fn error_codes_preserve_api_contract() {
        assert_eq!(
            AiQuotaError::Cooldown {
                remaining_seconds: 3
            }
            .code(),
            "AI_COOLDOWN_ACTIVE"
        );
        assert_eq!(
            AiQuotaError::DailyCallLimit { anonymous: false }.code(),
            "AI_DAILY_CALL_LIMIT"
        );
        assert_eq!(
            AiQuotaError::DailyCallLimit { anonymous: true }.code(),
            "AI_ANONYMOUS_DAILY_CALL_LIMIT"
        );
        assert_eq!(
            AiQuotaError::DailyTokenLimit { anonymous: true }.code(),
            "AI_ANONYMOUS_DAILY_TOKEN_LIMIT"
        );
        assert!(AiQuotaError::DailyCallLimit { anonymous: false }.is_client_limit());
        assert!(!AiQuotaError::Ledger {
            message: "x".into()
        }
        .is_client_limit());
    }

    #[test]
    fn client_limit_messages_are_recognised_from_their_display_form() {
        // Callers that only have `Result<_, String>` classify on this.
        for error in [
            AiQuotaError::Cooldown {
                remaining_seconds: 5,
            },
            AiQuotaError::DailyCallLimit { anonymous: false },
            AiQuotaError::DailyCallLimit { anonymous: true },
            AiQuotaError::DailyTokenLimit { anonymous: false },
            AiQuotaError::DailyTokenLimit { anonymous: true },
        ] {
            assert!(
                is_client_limit_message(&error.to_string()),
                "{} must be a client limit",
                error.code()
            );
            assert!(error.is_client_limit());
        }
    }

    #[test]
    fn ledger_faults_and_unrelated_errors_stay_server_errors() {
        let ledger = AiQuotaError::Ledger {
            message: "db down".into(),
        };
        assert!(!is_client_limit_message(&ledger.to_string()));
        assert!(!is_client_limit_message("Unknown capability_id: foo"));
        assert!(!is_client_limit_message(""));
    }
}
