//! Server-authoritative daily quota for guest Tapp speech calls.
//!
//! Guest and Tapp identity come from the validated Runtime Grant. The existing
//! `tapp_quota_usage` ledger persists counters across iframe reloads and server
//! replicas. Guest reservations use session, HMAC-IP, and site-owner buckets so
//! replacing the signed guest cookie cannot bypass the finite anonymous budget.

use crate::config::DynamicConfig;
use crate::services::tapp_rate_limit::anonymous_subject_fingerprint;
use sea_orm::{ConnectionTrait, DatabaseConnection, DbBackend, Statement, TransactionTrait, Value};

const ANONYMOUS_LEDGER_USER_ID: i32 = 0;
const GUEST_IP_BUDGET_MULTIPLIER: i32 = 3;
const GUEST_SITE_BUDGET_MULTIPLIER: i32 = 100;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechOperation {
    Tts,
    Asr,
}

impl SpeechOperation {
    pub const fn quota_type(self) -> &'static str {
        match self {
            Self::Tts => "speech_tts_daily",
            Self::Asr => "speech_asr_daily",
        }
    }

    pub const fn config_limit(self, config: &DynamicConfig) -> i32 {
        match self {
            Self::Tts => config.guest_speech_daily_tts,
            Self::Asr => config.guest_speech_daily_asr,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpeechQuotaError {
    Exhausted { operation: SpeechOperation, limit: i32 },
    Ledger,
}

impl SpeechQuotaError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Exhausted { .. } => "SPEECH_DAILY_QUOTA_EXHAUSTED",
            Self::Ledger => "SPEECH_QUOTA_LEDGER_ERROR",
        }
    }

    pub const fn status(&self) -> axum::http::StatusCode {
        match self {
            Self::Exhausted { .. } => axum::http::StatusCode::TOO_MANY_REQUESTS,
            Self::Ledger => axum::http::StatusCode::SERVICE_UNAVAILABLE,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct SpeechQuotaBucket {
    subject_id: i32,
    ledger_tapp_id: String,
    quota_type: String,
    limit: i32,
}

fn quota_type(operation: SpeechOperation, owner_id: i32, scope: &str) -> String {
    format!("{}:owner:{owner_id}:{scope}", operation.quota_type())
}

fn guest_quota_buckets(
    subject_id: i32,
    owner_id: i32,
    tapp_id: &str,
    operation: SpeechOperation,
    limit: i32,
    anonymous_scope: Option<&str>,
) -> Vec<SpeechQuotaBucket> {
    let digest = anonymous_subject_fingerprint(anonymous_scope.unwrap_or("unresolved"));
    vec![
        SpeechQuotaBucket {
            subject_id,
            ledger_tapp_id: tapp_id.to_string(),
            quota_type: quota_type(operation, owner_id, "guest-session"),
            limit,
        },
        SpeechQuotaBucket {
            subject_id: ANONYMOUS_LEDGER_USER_ID,
            ledger_tapp_id: tapp_id.to_string(),
            quota_type: quota_type(operation, owner_id, &format!("ip:{}", &digest[..12])),
            limit: limit.saturating_mul(GUEST_IP_BUDGET_MULTIPLIER),
        },
        SpeechQuotaBucket {
            subject_id: ANONYMOUS_LEDGER_USER_ID,
            ledger_tapp_id: "__anonymous_speech_site__".to_string(),
            quota_type: quota_type(operation, owner_id, "guest-site"),
            limit: limit.saturating_mul(GUEST_SITE_BUDGET_MULTIPLIER),
        },
    ]
}

async fn ensure_row<C: ConnectionTrait>(
    db: &C,
    bucket: &SpeechQuotaBucket,
) -> Result<(), SpeechQuotaError> {
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        INSERT INTO tapp_quota_usage
            (tapp_id, user_id, quota_type, used, "limit", period_start, period_end, updated_at)
        VALUES ($1, $2, $3, 0, $4, date_trunc('day', NOW()),
                date_trunc('day', NOW()) + interval '1 day', NOW())
        ON CONFLICT (user_id, tapp_id, quota_type, period_start)
        DO UPDATE SET "limit" = EXCLUDED."limit"
        "#,
        vec![
            Value::String(Some(bucket.ledger_tapp_id.clone())),
            Value::Int(Some(bucket.subject_id)),
            Value::String(Some(bucket.quota_type.clone())),
            Value::Int(Some(bucket.limit)),
        ],
    ))
    .await
    .map(|_| ())
    .map_err(|error| {
        tracing::error!(error = %error, "Failed to initialize speech quota row");
        SpeechQuotaError::Ledger
    })
}

async fn used_for_update<C: ConnectionTrait>(
    db: &C,
    bucket: &SpeechQuotaBucket,
) -> Result<i32, SpeechQuotaError> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DbBackend::Postgres,
            r#"
            SELECT used FROM tapp_quota_usage
            WHERE user_id = $1 AND tapp_id = $2 AND quota_type = $3
              AND period_start = date_trunc('day', NOW())
            FOR UPDATE
            "#,
            vec![
                Value::Int(Some(bucket.subject_id)),
                Value::String(Some(bucket.ledger_tapp_id.clone())),
                Value::String(Some(bucket.quota_type.clone())),
            ],
        ))
        .await
        .map_err(|error| {
            tracing::error!(error = %error, "Failed to read speech quota row");
            SpeechQuotaError::Ledger
        })?
        .ok_or(SpeechQuotaError::Ledger)?;
    row.try_get::<i32>("", "used")
        .map_err(|_| SpeechQuotaError::Ledger)
}

async fn increment<C: ConnectionTrait>(
    db: &C,
    bucket: &SpeechQuotaBucket,
) -> Result<(), SpeechQuotaError> {
    db.execute_raw(Statement::from_sql_and_values(
        DbBackend::Postgres,
        r#"
        UPDATE tapp_quota_usage SET used = used + 1, updated_at = NOW()
        WHERE user_id = $1 AND tapp_id = $2 AND quota_type = $3
          AND period_start = date_trunc('day', NOW())
        "#,
        vec![
            Value::Int(Some(bucket.subject_id)),
            Value::String(Some(bucket.ledger_tapp_id.clone())),
            Value::String(Some(bucket.quota_type.clone())),
        ],
    ))
    .await
    .map(|_| ())
    .map_err(|error| {
        tracing::error!(error = %error, "Failed to reserve speech quota");
        SpeechQuotaError::Ledger
    })
}

/// Atomically reserve one guest speech call across session, IP, and site-owner
/// buckets. Provider failures consume the reservation to bound retry cost.
pub async fn reserve_guest_speech_call(
    db: &DatabaseConnection,
    subject_id: i32,
    owner_id: i32,
    tapp_id: &str,
    operation: SpeechOperation,
    limit: i32,
    anonymous_scope: Option<&str>,
) -> Result<(), SpeechQuotaError> {
    let limit = limit.max(0);
    let buckets = guest_quota_buckets(
        subject_id,
        owner_id,
        tapp_id,
        operation,
        limit,
        anonymous_scope,
    );
    let txn = db.begin().await.map_err(|_| SpeechQuotaError::Ledger)?;

    for bucket in &buckets {
        ensure_row(&txn, bucket).await?;
    }
    for bucket in &buckets {
        if used_for_update(&txn, bucket).await? >= bucket.limit {
            return Err(SpeechQuotaError::Exhausted {
                operation,
                limit: bucket.limit,
            });
        }
    }
    for bucket in &buckets {
        increment(&txn, bucket).await?;
    }
    txn.commit().await.map_err(|_| SpeechQuotaError::Ledger)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn speech_operations_have_independent_quota_types_and_limits() {
        let config = DynamicConfig::default();
        assert_ne!(SpeechOperation::Tts.quota_type(), SpeechOperation::Asr.quota_type());
        assert!(SpeechOperation::Tts.config_limit(&config) > 0);
        assert!(SpeechOperation::Asr.config_limit(&config) > 0);
    }

    #[test]
    fn guest_quota_has_session_ip_and_site_buckets_without_raw_ip() {
        let buckets = guest_quota_buckets(-7, 1, "radio", SpeechOperation::Tts, 20, Some("203.0.113.8"));
        assert_eq!(buckets.len(), 3);
        assert_eq!(buckets[0].limit, 20);
        assert_eq!(buckets[1].limit, 60);
        assert_eq!(buckets[2].limit, 2000);
        assert!(buckets.iter().all(|bucket| !bucket.quota_type.contains("203.0.113.8")));
        assert_eq!(buckets[2].ledger_tapp_id, "__anonymous_speech_site__");
    }

    #[test]
    fn exhausted_quota_has_stable_client_error() {
        let error = SpeechQuotaError::Exhausted { operation: SpeechOperation::Tts, limit: 10 };
        assert_eq!(error.code(), "SPEECH_DAILY_QUOTA_EXHAUSTED");
        assert_eq!(error.status(), axum::http::StatusCode::TOO_MANY_REQUESTS);
    }
}
