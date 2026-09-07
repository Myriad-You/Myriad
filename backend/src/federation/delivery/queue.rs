//! Claim, lease, process, and spawn the outbound federation delivery worker.

use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, QueryResult, Statement,
    TransactionSession, TransactionTrait,
};
use serde::Serialize;
use std::time::Duration;
use uuid::Uuid;

use crate::federation::types::get_base_url;

use super::dispatch::{
    deliver_activity, is_unrecoverable_key_load_error, load_user_keypair_ensuring,
    signing_identity_for_activity,
};

pub(crate) async fn get_username_by_id(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<String, String> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT username FROM users WHERE id = $1 LIMIT 1",
            [user_id.into()],
        ))
        .await
        .map_err(|e| {
            tracing::error!("DB error: {}", e);
            "Database error".to_string()
        })?
        .ok_or_else(|| "User not found".to_string())?;

    Ok(row.try_get("", "username").unwrap_or_default())
}

/// Immediate enqueue result (send path observability — before HTTP delivery).
#[derive(Debug, Clone, Serialize, Default)]
pub struct DeliveryEnqueueInfo {
    /// Rows inserted into federation_delivery_queue
    pub queued: u32,
    /// Remote peers we intended to reach (members / channel peer)
    pub remote_targets: u32,
    /// Members present but missing remote_actors / empty inbox
    #[serde(default, skip_serializing_if = "is_zero_u32")]
    pub unresolved: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
}

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}

/// Fan-out enqueue stats for room activities.
#[derive(Debug, Clone, Default)]
pub struct FanoutResult {
    pub enqueued: u32,
    pub remote_with_inbox: u32,
    pub skipped_empty_inbox: u32,
    pub unresolved_members: u32,
}

impl FanoutResult {
    pub fn to_enqueue_info(&self) -> DeliveryEnqueueInfo {
        let mut info = DeliveryEnqueueInfo {
            queued: self.enqueued,
            remote_targets: self.remote_with_inbox
                + self.skipped_empty_inbox
                + self.unresolved_members,
            unresolved: self.unresolved_members + self.skipped_empty_inbox,
            warning: None,
        };
        if self.enqueued == 0 && info.remote_targets > 0 {
            info.warning =
                Some("no_delivery_queued: remote members lack inbox or remote_actors row".into());
        } else if self.skipped_empty_inbox > 0 || self.unresolved_members > 0 {
            info.warning = Some(format!(
                "partial_enqueue: queued={} empty_inbox={} unresolved={}",
                self.enqueued, self.skipped_empty_inbox, self.unresolved_members
            ));
        }
        info
    }
}

/// Delivery queue batch outcome (worker metrics).
#[derive(Debug, Clone, Default)]
pub struct DeliveryBatchStats {
    pub claimed: u32,
    pub reclaimed: u32,
    pub delivered: u32,
    pub dead: u32,
    pub retried: u32,
    pub lease_lost: u32,
    pub relationships_revoked: u32,
    /// Queue rows killed by a revocation sweep (domain-wide, not just this batch).
    pub deliveries_cancelled: u32,
}

const DELIVERY_LEASE_SECS: i64 = 10 * 60;
const DELIVERY_LEASE_HEARTBEAT_SECS: u64 = 60;

/// Unreachable delivery attempts required before a domain is considered gone.
///
/// A count alone is a poor signal: the worker drains 20 rows per 15s tick, so a
/// fan-out to one peer can burn an arbitrary count during a single restart. The
/// count is therefore only half the gate — see the streak window below.
const DELIVERY_FAILURE_REVOCATION_THRESHOLD: i32 = 20;

/// The failure streak must also have lasted this long, unbroken.
///
/// `max_attempts` defaults to 12, so no single queue row can reach the count
/// threshold by itself; revocation needs sustained traffic that keeps failing
/// across a full week. Any successful delivery resets both halves of the gate.
const DELIVERY_FAILURE_REVOCATION_MIN_STREAK_SECS: i64 = 7 * 24 * 60 * 60;

pub(crate) fn should_revoke_relationships(consecutive_failures: i32, streak_secs: i64) -> bool {
    consecutive_failures >= DELIVERY_FAILURE_REVOCATION_THRESHOLD
        && streak_secs >= DELIVERY_FAILURE_REVOCATION_MIN_STREAK_SECS
}

/// Derived from the thresholds so the wording can never drift from the policy.
pub(crate) fn domain_revocation_reason() -> String {
    format!(
        "cancelled: federation relationship revoked after {} unreachable delivery attempts over {}+ days with no successful delivery",
        DELIVERY_FAILURE_REVOCATION_THRESHOLD,
        DELIVERY_FAILURE_REVOCATION_MIN_STREAK_SECS / 86_400
    )
}

struct DeliveryLeaseHeartbeat {
    task: tokio::task::JoinHandle<()>,
}

impl Drop for DeliveryLeaseHeartbeat {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn spawn_delivery_lease_heartbeat(
    db: DatabaseConnection,
    queue_id: i32,
    lease_token: Uuid,
) -> DeliveryLeaseHeartbeat {
    let task = tokio::spawn(async move {
        let mut interval =
            tokio::time::interval(Duration::from_secs(DELIVERY_LEASE_HEARTBEAT_SECS));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        // Tokio's first tick is immediate; the claim already established a full
        // lease, so wait for the first real heartbeat interval.
        interval.tick().await;
        loop {
            interval.tick().await;
            match renew_delivery_lease(&db, queue_id, lease_token).await {
                Ok(true) => {
                    tracing::debug!(queue_id, "renewed federation delivery lease");
                }
                Ok(false) => {
                    tracing::warn!(
                        queue_id,
                        "federation delivery lease heartbeat lost ownership"
                    );
                    break;
                }
                Err(error) => {
                    // Keep trying while the current lease is still valid. The
                    // outbound HTTP request itself is bounded to 30 seconds, so
                    // a transient DB outage cannot normally outlive this lease.
                    tracing::warn!(
                        queue_id,
                        error = %error,
                        "failed to renew federation delivery lease"
                    );
                }
            }
        }
    });
    DeliveryLeaseHeartbeat { task }
}

pub(crate) async fn renew_delivery_lease(
    db: &impl ConnectionTrait,
    queue_id: i32,
    lease_token: Uuid,
) -> Result<bool, DbErr> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_delivery_queue
               SET lease_expires_at = NOW() + make_interval(secs => $1::double precision),
                   last_attempt_at = NOW()
               WHERE id = $2 AND status = 'delivering' AND lease_token = $3"#,
            [
                DELIVERY_LEASE_SECS.into(),
                queue_id.into(),
                lease_token.into(),
            ],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

pub(crate) async fn claim_next_delivery(
    db: &impl ConnectionTrait,
    lease_token: Uuid,
) -> Result<Option<QueryResult>, DbErr> {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        // MATERIALIZED keeps FOR UPDATE SKIP LOCKED from being inlined away (PG12+).
        r#"WITH selected AS MATERIALIZED (
               SELECT id, status AS prev_status, attempts AS prev_attempts,
                      EXTRACT(EPOCH FROM (NOW() - last_attempt_at))::BIGINT
                          AS prior_lease_age_secs
               FROM federation_delivery_queue
               WHERE (status = 'pending'
                      AND (next_retry_at IS NULL OR next_retry_at <= NOW()))
                  OR (status = 'delivering' AND (
                      lease_expires_at <= NOW()
                      OR (lease_token IS NULL
                          AND (last_attempt_at IS NULL
                               OR last_attempt_at < NOW() - INTERVAL '10 minutes'))
                  ))
               ORDER BY created_at ASC
               LIMIT 1
               FOR UPDATE SKIP LOCKED
           ),
           claimed AS (
               UPDATE federation_delivery_queue dq
               SET status = 'delivering',
                   last_attempt_at = NOW(),
                   lease_token = $1,
                   lease_expires_at = NOW() + make_interval(secs => $2::double precision),
                   attempts = CASE
                       WHEN s.prev_status = 'delivering' THEN s.prev_attempts + 1
                       ELSE s.prev_attempts
                   END
               FROM selected s
               WHERE dq.id = s.id
               RETURNING dq.id, dq.activity_id, dq.target_inbox, dq.target_domain,
                         dq.attempts, dq.max_attempts, s.prev_status,
                         s.prior_lease_age_secs
           )
           SELECT c.id, c.activity_id, c.target_inbox, c.target_domain,
                  c.attempts, c.max_attempts, c.prev_status,
                  c.prior_lease_age_secs,
                  a.activity_id AS ap_activity_id, a.activity_type, a.object_json, a.user_id
           FROM claimed c
           JOIN federation_activities a ON a.id = c.activity_id"#,
        [lease_token.into(), DELIVERY_LEASE_SECS.into()],
    ))
    .await
}

pub(crate) async fn mark_delivery_delivered_if_owned(
    db: &impl ConnectionTrait,
    queue_id: i32,
    lease_token: Uuid,
) -> Result<bool, DbErr> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE federation_delivery_queue SET status = 'delivered', last_attempt_at = NOW(), lease_token = NULL, lease_expires_at = NULL WHERE id = $1 AND status = 'delivering' AND lease_token = $2",
            [queue_id.into(), lease_token.into()],
        ))
        .await?;
    Ok(result.rows_affected() == 1)
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum RemoteDeliveryFailureDisposition {
    Dead,
    RetryAfter(i64),
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct RemoteDeliveryFailureSettlement {
    pub applied: bool,
    /// False for permanent peer rejections — the peer answered, so the attempt
    /// carries no reachability signal and leaves the streak untouched.
    pub counted_toward_streak: bool,
    pub consecutive_failures: i32,
    /// Seconds since the current unbroken failure streak began.
    pub streak_secs: i64,
    pub relationships_revoked: bool,
    pub follows_removed: u64,
    pub channels_closed: u64,
    pub room_members_removed: u64,
    pub transfers_cancelled: u64,
    pub deliveries_cancelled: u64,
    /// `(user_id, cancelled_rows)` for every owner whose queued deliveries the
    /// sweep killed, so the caller can notify them once the txn has committed.
    pub cancelled_owners: Vec<(i32, i64)>,
}

async fn lock_delivery_instance(
    db: &impl ConnectionTrait,
    target_domain: &str,
) -> Result<i32, DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"INSERT INTO federation_instances (domain, failure_count, created_at)
           VALUES ($1, 0, NOW())
           ON CONFLICT (domain) DO NOTHING"#,
        [target_domain.into()],
    ))
    .await?;

    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT failure_count FROM federation_instances WHERE domain = $1 FOR UPDATE",
            [target_domain.into()],
        ))
        .await?
        .ok_or_else(|| DbErr::Custom("failed to lock federation instance".into()))?;
    Ok(row.try_get("", "failure_count").unwrap_or(0))
}

/// Settle a successful remote HTTP delivery and reset the domain failure streak.
///
/// The instance row is locked before the queue row is settled, matching the
/// failure path's lock order. A cancelled/reclaimed lease cannot reset health.
pub(crate) async fn settle_remote_delivery_success(
    db: &(impl ConnectionTrait + TransactionTrait),
    queue_id: i32,
    lease_token: Uuid,
    target_domain: &str,
) -> Result<bool, DbErr> {
    let txn = db.begin().await?;
    let _ = lock_delivery_instance(&txn, target_domain).await?;
    let applied = mark_delivery_delivered_if_owned(&txn, queue_id, lease_token).await?;
    if !applied {
        txn.rollback().await?;
        return Ok(false);
    }

    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"UPDATE federation_instances
           SET last_success_at = NOW(), failure_count = 0, failing_since = NULL
           WHERE domain = $1"#,
        [target_domain.into()],
    ))
    .await?;
    txn.commit().await?;
    Ok(true)
}

/// Settle one remote delivery failure.
///
/// `counts_toward_streak` separates *unreachable* from *rejected*. DNS failures,
/// connection/timeout errors and retryable HTTP statuses say the peer is not
/// answering, so they advance the domain's failure streak. A permanent 4xx says
/// the peer answered and declined this specific activity — it is left neutral:
/// it neither advances nor resets the streak, because a live peer rejecting one
/// object must never be able to tear down every relationship with its domain.
/// Local signing/DB/policy failures never reach this function at all.
///
/// Revocation requires both halves of the gate in [`should_revoke_relationships`]
/// (sustained count *and* elapsed streak). When it fires, active relationship
/// rows to the domain are removed (channels are closed to preserve messages) and
/// its **unfinished** deliveries are cancelled in the same transaction. Rows that
/// already reached a terminal state keep their original error — historical
/// dead-letters are not rewritten or made unretryable by a later outage.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn settle_remote_delivery_failure(
    db: &(impl ConnectionTrait + TransactionTrait),
    queue_id: i32,
    lease_token: Uuid,
    target_domain: &str,
    new_attempts: i32,
    error_message: &str,
    disposition: RemoteDeliveryFailureDisposition,
    counts_toward_streak: bool,
) -> Result<RemoteDeliveryFailureSettlement, DbErr> {
    let txn = db.begin().await?;
    let current_failures = lock_delivery_instance(&txn, target_domain).await?;

    let queue_update = match disposition {
        RemoteDeliveryFailureDisposition::Dead => {
            txn.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"UPDATE federation_delivery_queue
                   SET status = 'dead', attempts = $1, error_message = $2,
                       last_attempt_at = NOW(), next_retry_at = NULL,
                       lease_token = NULL, lease_expires_at = NULL
                   WHERE id = $3 AND status = 'delivering' AND lease_token = $4"#,
                [
                    new_attempts.into(),
                    error_message.into(),
                    queue_id.into(),
                    lease_token.into(),
                ],
            ))
            .await?
        }
        RemoteDeliveryFailureDisposition::RetryAfter(backoff_secs) => {
            txn.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                r#"UPDATE federation_delivery_queue
                   SET status = 'pending', attempts = $1, error_message = $2,
                       last_attempt_at = NOW(),
                       next_retry_at = NOW() + make_interval(secs => $4::double precision),
                       lease_token = NULL, lease_expires_at = NULL
                   WHERE id = $3 AND status = 'delivering' AND lease_token = $5"#,
                [
                    new_attempts.into(),
                    error_message.into(),
                    queue_id.into(),
                    backoff_secs.into(),
                    lease_token.into(),
                ],
            ))
            .await?
        }
    };

    if queue_update.rows_affected() != 1 {
        txn.rollback().await?;
        return Ok(RemoteDeliveryFailureSettlement::default());
    }

    // A peer that answered with a permanent rejection is demonstrably reachable.
    // Leave the streak exactly as it was: no increment, and no reset either.
    if !counts_toward_streak {
        let settlement = RemoteDeliveryFailureSettlement {
            applied: true,
            consecutive_failures: current_failures,
            ..Default::default()
        };
        txn.commit().await?;
        return Ok(settlement);
    }

    // `failing_since` is stamped on the first failure of a streak and cleared by
    // any success, so RETURNING always yields a non-negative age here.
    let failure_row = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_instances
               SET failure_count = failure_count + 1,
                   failing_since = COALESCE(failing_since, NOW())
               WHERE domain = $1
               RETURNING failure_count,
                         GREATEST(0, EXTRACT(EPOCH FROM (NOW() - failing_since))::BIGINT)
                             AS streak_secs"#,
            [target_domain.into()],
        ))
        .await?
        .ok_or_else(|| DbErr::Custom("failed to update federation failure count".into()))?;
    let consecutive_failures = failure_row.try_get("", "failure_count").unwrap_or(0);
    let streak_secs = failure_row.try_get("", "streak_secs").unwrap_or(0);
    let mut settlement = RemoteDeliveryFailureSettlement {
        applied: true,
        counted_toward_streak: true,
        consecutive_failures,
        streak_secs,
        ..Default::default()
    };

    if !should_revoke_relationships(consecutive_failures, streak_secs) {
        txn.commit().await?;
        return Ok(settlement);
    }

    // Serialize the teardown against every other domain's teardown. The
    // statements below sweep shared tables (room members, transfers) whose row
    // sets overlap across domains, and two concurrent seq scans can visit blocks
    // in different orders (`synchronize_seqscans` wraparound) — enough for a
    // deadlock. Taken only once the gate has passed, so the common failure path
    // never contends. The per-domain instance row is already locked and differs
    // between callers, so this cannot invert into a cycle.
    txn.execute_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtext('myriad:federation:domain_revocation')::BIGINT)"
            .to_string(),
    ))
    .await?;

    settlement.follows_removed = txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"DELETE FROM federation_follows f
               USING federation_remote_actors ra
               WHERE f.remote_actor_id = ra.id
                 AND LOWER(ra.domain) = LOWER($1)
                 AND f.status IN ('pending', 'accepted')"#,
            [target_domain.into()],
        ))
        .await?
        .rows_affected();

    settlement.channels_closed = txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_channels c
               SET status = 'closed', closed_at = COALESCE(closed_at, NOW())
               FROM federation_remote_actors ra
               WHERE c.remote_actor_id = ra.id
                 AND LOWER(ra.domain) = LOWER($1)
                 AND c.status IN ('pending', 'accepted', 'active')"#,
            [target_domain.into()],
        ))
        .await?
        .rows_affected();

    settlement.transfers_cancelled = txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_file_transfers ft
               SET status = 'cancelled'
               WHERE ft.status IN ('pending', 'in-progress', 'finalizing')
                 AND (
                   EXISTS (
                     SELECT 1
                     FROM federation_channels c
                     JOIN federation_remote_actors ra ON c.remote_actor_id = ra.id
                     WHERE c.channel_id = ft.channel_id
                       AND LOWER(ra.domain) = LOWER($1)
                   )
                   OR (
                     ft.room_id IS NOT NULL
                     AND EXISTS (
                       SELECT 1
                       FROM federation_rooms r
                       WHERE r.room_id = ft.room_id
                         AND (
                           EXISTS (
                             SELECT 1 FROM federation_remote_actors owner
                             WHERE owner.actor_url = r.owner_actor
                               AND LOWER(owner.domain) = LOWER($1)
                           )
                           OR LOWER(regexp_replace(
                                split_part(regexp_replace(BTRIM(r.home_server), '^https?://', '', 'i'), '/', 1),
                                ':[0-9]+$', ''
                              )) = LOWER(regexp_replace($1, ':[0-9]+$', ''))
                         )
                     )
                   )
                 )"#,
            [target_domain.into()],
        ))
        .await?
        .rows_affected();

    settlement.room_members_removed = txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"DELETE FROM federation_room_members rm
               WHERE (
                 rm.is_local = false
                 AND (
                     EXISTS (
                       SELECT 1 FROM federation_remote_actors ra
                       WHERE ra.actor_url = rm.actor_url
                         AND LOWER(ra.domain) = LOWER($1)
                     )
                     OR LOWER(regexp_replace(
                          split_part(regexp_replace(BTRIM(rm.actor_url), '^https?://', '', 'i'), '/', 1),
                          ':[0-9]+$', ''
                        )) = LOWER(regexp_replace($1, ':[0-9]+$', ''))
                 )
               )
               OR (
                 rm.is_local = true
                 AND EXISTS (
                   SELECT 1
                   FROM federation_rooms r
                   WHERE r.room_id = rm.room_id
                     AND (
                       EXISTS (
                         SELECT 1 FROM federation_remote_actors owner
                         WHERE owner.actor_url = r.owner_actor
                           AND LOWER(owner.domain) = LOWER($1)
                       )
                       OR LOWER(regexp_replace(
                            split_part(regexp_replace(BTRIM(r.home_server), '^https?://', '', 'i'), '/', 1),
                            ':[0-9]+$', ''
                          )) = LOWER(regexp_replace($1, ':[0-9]+$', ''))
                     )
                 )
               )"#,
            [target_domain.into()],
        ))
        .await?
        .rows_affected();

    // Only unfinished work is cancelled. Rows that already died — including ones
    // that died for local reasons such as key load or SSRF policy — keep their
    // real error, stay attributable, and stay retryable via
    // `retry_all_dead_for_user`, which skips anything prefixed `cancelled:`.
    // `idx_delivery_queue_target_domain` covers this predicate.
    let cancelled_rows = txn
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_delivery_queue dq
               SET status = 'dead',
                   error_message = CASE
                     WHEN dq.error_message ILIKE 'cancelled:%' THEN dq.error_message
                     WHEN dq.error_message IS NULL OR BTRIM(dq.error_message) = '' THEN $2
                     ELSE $2 || '; previous error: ' || dq.error_message
                   END,
                   last_attempt_at = NOW(), next_retry_at = NULL,
                   lease_token = NULL, lease_expires_at = NULL
               WHERE LOWER(dq.target_domain) = LOWER($1)
                 AND dq.status IN ('pending', 'delivering')
               RETURNING (
                   SELECT a.user_id FROM federation_activities a WHERE a.id = dq.activity_id
               ) AS user_id"#,
            [target_domain.into(), domain_revocation_reason().into()],
        ))
        .await?;

    settlement.deliveries_cancelled = cancelled_rows.len() as u64;
    let mut owners: std::collections::BTreeMap<i32, i64> = std::collections::BTreeMap::new();
    for row in &cancelled_rows {
        if let Ok(Some(user_id)) = row.try_get::<Option<i32>>("", "user_id") {
            if user_id > 0 {
                *owners.entry(user_id).or_default() += 1;
            }
        }
    }
    settlement.cancelled_owners = owners.into_iter().collect();

    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"UPDATE federation_instances
           SET failure_count = 0, failing_since = NULL
           WHERE domain = $1"#,
        [target_domain.into()],
    ))
    .await?;

    settlement.relationships_revoked = true;
    txn.commit().await?;
    Ok(settlement)
}

fn lease_update_applied(
    rows_affected: u64,
    stats: &mut DeliveryBatchStats,
    queue_id: i32,
    outcome: &str,
) -> bool {
    if rows_affected == 1 {
        true
    } else {
        stats.lease_lost += 1;
        tracing::warn!(
            queue_id,
            outcome,
            "ignored stale federation delivery outcome after lease ownership changed"
        );
        false
    }
}

/// Process a batch of pending deliveries and return full batch counters.
pub async fn process_delivery_queue_detailed(
    db: &DatabaseConnection,
    batch_size: u32,
) -> Result<DeliveryBatchStats, String> {
    let mut stats = DeliveryBatchStats::default();
    // Rows counted as `retried` earlier in this batch that a later revocation
    // then killed. Without this the batch log claims rows are waiting to retry
    // when the sweep has already dead-lettered them.
    let mut retried_by_domain: std::collections::HashMap<String, u32> =
        std::collections::HashMap::new();

    // Claim immediately before processing each item. Rows waiting behind a slow
    // peer remain pending and therefore cannot have a lease expire before work
    // even begins.
    for _ in 0..batch_size {
        let lease_token = Uuid::new_v4();
        let row = claim_next_delivery(db, lease_token)
            .await
            .map_err(|e| format!("Queue claim failed: {e}"))?;
        let Some(row) = row else {
            break;
        };

        let queue_id: i32 = row.try_get("", "id").unwrap_or(0);
        let target_inbox: String = row.try_get("", "target_inbox").unwrap_or_default();
        let target_domain: String = row.try_get("", "target_domain").unwrap_or_default();
        let attempts: i32 = row.try_get("", "attempts").unwrap_or(0);
        let max_attempts: i32 = row.try_get("", "max_attempts").unwrap_or(12);
        let prev_status: String = row.try_get("", "prev_status").unwrap_or_default();
        let prior_lease_age_secs: Option<i64> =
            row.try_get("", "prior_lease_age_secs").ok().flatten();
        let _ap_activity_id: String = row.try_get("", "ap_activity_id").unwrap_or_default();
        let activity_type: String = row.try_get("", "activity_type").unwrap_or_default();
        let object_json: serde_json::Value = row.try_get("", "object_json").unwrap_or_default();
        let user_id: i32 = row.try_get("", "user_id").unwrap_or(0);
        stats.claimed += 1;
        let _lease_heartbeat = spawn_delivery_lease_heartbeat(db.clone(), queue_id, lease_token);

        // Reclaimed stuck delivering: attempts already incremented in the claim UPDATE
        let reclaim = prev_status == "delivering";
        if reclaim {
            stats.reclaimed += 1;
            tracing::warn!(
                queue_id,
                attempts,
                prior_lease_age_secs,
                "reclaimed expired federation delivery lease; remote may have observed the prior attempt"
            );
        }
        if reclaim && attempts >= max_attempts {
            let err = "Exceeded max attempts after reclaim";
            let mark = db
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "UPDATE federation_delivery_queue SET status = 'dead', error_message = $1, last_attempt_at = NOW(), lease_token = NULL, lease_expires_at = NULL WHERE id = $2 AND status = 'delivering' AND lease_token = $3",
                    [err.into(), queue_id.into(), lease_token.into()],
                ))
                .await
                .map_err(|e| format!("Queue dead-letter update failed: {e}"))?;
            if !lease_update_applied(mark.rows_affected(), &mut stats, queue_id, "dead") {
                continue;
            }
            mark_delivery_dead(user_id, &activity_type, &target_domain, err).await;
            stats.dead += 1;
            continue;
        }

        // 投递前：目标实例信任策略检查（黑名单等）
        if let Err(reason) = crate::federation::trust::enforce_outbound(db, &target_domain).await {
            let mark = db
                .execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    r#"UPDATE federation_delivery_queue
                       SET status = 'dead', error_message = $1, last_attempt_at = NOW(),
                           lease_token = NULL, lease_expires_at = NULL
                       WHERE id = $2 AND status = 'delivering' AND lease_token = $3"#,
                    [reason.clone().into(), queue_id.into(), lease_token.into()],
                ))
                .await
                .map_err(|e| format!("Queue trust-policy update failed: {e}"))?;
            if !lease_update_applied(mark.rows_affected(), &mut stats, queue_id, "dead") {
                continue;
            }
            tracing::warn!(
                "🛑 Delivery blocked by trust policy: target={}, reason={}",
                target_inbox,
                reason
            );
            mark_delivery_dead(user_id, &activity_type, &target_domain, &reason).await;
            stats.dead += 1;
            continue;
        }

        // object_json 已经是完整的 Activity JSON（含 @context/type/id/actor/object），直接发送
        let base_url = get_base_url().await;
        let username = get_username_by_id(db, user_id).await.unwrap_or_default();

        let body_bytes = serde_json::to_vec(&object_json).unwrap_or_default();

        // Move is signed as the **old** actor. Prefer activity.actor origin for
        // keyId so peers verifying against the old actor document succeed even
        // after local base_url has switched to the new domain.
        let (sign_base, sign_username) =
            signing_identity_for_activity(&activity_type, &object_json, &base_url, &username);

        // 获取用户密钥对 — if missing, ensure once then reload so already-queued
        // deliveries recover without waiting for GET /users/{username}.
        // Prefer stored key_id from federation_keys (domain-move G retargets it);
        // recompute from sign_base only when missing. Move always signs with the
        // activity actor origin so old peers can verify against the departing doc.
        match load_user_keypair_ensuring(db, user_id, &username).await {
            Ok(loaded) => {
                match deliver_activity(
                    &loaded.keypair,
                    &sign_base,
                    &sign_username,
                    &target_inbox,
                    &target_domain,
                    &body_bytes,
                    loaded.stored_key_id.as_deref(),
                    &activity_type,
                )
                .await
                {
                    Ok(()) => {
                        // 投递成功 — only if still `delivering` (user cancel may
                        // have marked dead mid-flight; do not resurrect).
                        let applied = settle_remote_delivery_success(
                            db,
                            queue_id,
                            lease_token,
                            &target_domain,
                        )
                        .await
                        .map_err(|e| format!("Queue delivery completion failed: {e}"))?;
                        if !lease_update_applied(
                            u64::from(applied),
                            &mut stats,
                            queue_id,
                            "delivered",
                        ) {
                            tracing::info!(
                                queue_id = queue_id,
                                target = %target_inbox,
                                "Delivery HTTP ok but queue row no longer delivering (cancelled?); not marking delivered"
                            );
                            continue;
                        }
                        stats.delivered += 1;

                        tracing::debug!("📤 Delivered {} to {}", activity_type, target_inbox);
                    }
                    Err(delivery_error) => {
                        let new_attempts = attempts + 1;
                        if !delivery_error.counts_toward_remote_failure {
                            let error_message = delivery_error.message;
                            let mark = db
                                .execute_raw(Statement::from_sql_and_values(
                                    DatabaseBackend::Postgres,
                                    r#"UPDATE federation_delivery_queue
                                       SET status = 'dead', attempts = $1, error_message = $2,
                                           last_attempt_at = NOW(), next_retry_at = NULL,
                                           lease_token = NULL, lease_expires_at = NULL
                                       WHERE id = $3 AND status = 'delivering' AND lease_token = $4"#,
                                    [
                                        new_attempts.into(),
                                        error_message.clone().into(),
                                        queue_id.into(),
                                        lease_token.into(),
                                    ],
                                ))
                                .await
                                .map_err(|err| {
                                    format!("Queue local delivery failure update failed: {err}")
                                })?;
                            if !lease_update_applied(
                                mark.rows_affected(),
                                &mut stats,
                                queue_id,
                                "dead",
                            ) {
                                continue;
                            }
                            tracing::error!(
                                queue_id,
                                target = %target_inbox,
                                error = %error_message,
                                "Federation delivery failed before remote request; peer health unchanged"
                            );
                            mark_delivery_dead(
                                user_id,
                                &activity_type,
                                &target_domain,
                                &error_message,
                            )
                            .await;
                            stats.dead += 1;
                            continue;
                        }

                        let e = delivery_error.message;
                        // Permanent: 4xx from peer, OR 5xx body that is really
                        // not_found / not_member (legacy peers still return 500).
                        let permanent = crate::federation::errors::is_permanent_delivery_error(&e);
                        // A permanent rejection proves the peer answered, so it
                        // is not evidence of an unreachable domain.
                        let counts_toward_streak = !permanent;
                        let terminal = permanent || new_attempts >= max_attempts;
                        let backoff_secs = retry_backoff_secs(new_attempts);
                        let disposition = if terminal {
                            RemoteDeliveryFailureDisposition::Dead
                        } else {
                            RemoteDeliveryFailureDisposition::RetryAfter(backoff_secs)
                        };
                        let settlement = settle_remote_delivery_failure(
                            db,
                            queue_id,
                            lease_token,
                            &target_domain,
                            new_attempts,
                            &e,
                            disposition,
                            counts_toward_streak,
                        )
                        .await
                        .map_err(|err| format!("Queue remote failure settlement failed: {err}"))?;
                        if !lease_update_applied(
                            u64::from(settlement.applied),
                            &mut stats,
                            queue_id,
                            if terminal { "dead" } else { "pending" },
                        ) {
                            continue;
                        }

                        if settlement.relationships_revoked {
                            stats.relationships_revoked += 1;
                            stats.deliveries_cancelled +=
                                u32::try_from(settlement.deliveries_cancelled).unwrap_or(u32::MAX);
                            // Rows this batch already logged as retrying are dead now.
                            let reclassified =
                                retried_by_domain.remove(&target_domain).unwrap_or(0);
                            stats.retried = stats.retried.saturating_sub(reclassified);
                            stats.dead += reclassified;
                            tracing::warn!(
                                target_domain = %target_domain,
                                consecutive_failures = settlement.consecutive_failures,
                                streak_secs = settlement.streak_secs,
                                follows_removed = settlement.follows_removed,
                                channels_closed = settlement.channels_closed,
                                room_members_removed = settlement.room_members_removed,
                                transfers_cancelled = settlement.transfers_cancelled,
                                deliveries_cancelled = settlement.deliveries_cancelled,
                                notified_owners = settlement.cancelled_owners.len(),
                                "Revoked federation relationships after a sustained unreachable streak"
                            );
                            // The sweep is one bulk statement, so these owners
                            // never pass through the per-row dead-letter path.
                            for (owner_id, cancelled) in &settlement.cancelled_owners {
                                crate::federation::notify::notify_domain_relationship_revoked(
                                    *owner_id,
                                    &target_domain,
                                    *cancelled,
                                )
                                .await;
                            }
                        }

                        if terminal {
                            if permanent {
                                tracing::warn!(
                                    "💀 Delivery permanent failure to {}: {}",
                                    target_inbox,
                                    e
                                );
                            } else {
                                tracing::warn!(
                                    "💀 Delivery dead after {} attempts to {}: {}",
                                    new_attempts,
                                    target_inbox,
                                    e
                                );
                            }
                            // This row was already terminal before the sweep ran,
                            // so it kept its real error — report that, not the
                            // revocation boilerplate.
                            mark_delivery_dead(user_id, &activity_type, &target_domain, &e).await;
                            stats.dead += 1;
                        } else if settlement.relationships_revoked {
                            // Still `pending` when the sweep ran, so it was
                            // cancelled with the rest and its owner is already
                            // covered by the revocation notification above.
                            stats.dead += 1;
                        } else {
                            tracing::warn!(
                                "⚠️ Delivery failed (attempt {}/{}), retrying in {}s: {}",
                                new_attempts,
                                max_attempts,
                                backoff_secs,
                                e
                            );
                            stats.retried += 1;
                            *retried_by_domain.entry(target_domain.clone()).or_default() += 1;
                        }
                    }
                }
            }
            Err(e) => {
                tracing::error!("Failed to load keypair for user {}: {}", user_id, e);
                let new_attempts = attempts + 1;
                let err_msg = format!("Key load failed: {}", e);
                // Empty username cannot ensure — mark dead instead of spinning
                // retries. Decrypt failures back off (no re-generate) until max.
                let permanent = is_unrecoverable_key_load_error(&e);
                if permanent || new_attempts >= max_attempts {
                    let mark = db
                        .execute_raw(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            "UPDATE federation_delivery_queue SET status = 'dead', attempts = $1, error_message = $2, last_attempt_at = NOW(), lease_token = NULL, lease_expires_at = NULL WHERE id = $3 AND status = 'delivering' AND lease_token = $4",
                            [
                                new_attempts.into(),
                                err_msg.clone().into(),
                                queue_id.into(),
                                lease_token.into(),
                            ],
                        ))
                        .await
                        .map_err(|err| format!("Queue key dead-letter update failed: {err}"))?;
                    if !lease_update_applied(mark.rows_affected(), &mut stats, queue_id, "dead") {
                        continue;
                    }
                    if permanent {
                        tracing::error!(
                            user_id = user_id,
                            queue_id = queue_id,
                            error = %err_msg,
                            "federation key load permanent failure; marking dead"
                        );
                    }
                    mark_delivery_dead(user_id, &activity_type, &target_domain, &err_msg).await;
                    stats.dead += 1;
                } else {
                    // 密钥问题几乎不会自愈；按普通失败计数退避，避免 15s 热循环刷日志
                    let backoff_secs = retry_backoff_secs(new_attempts);
                    let mark = db
                        .execute_raw(Statement::from_sql_and_values(
                            DatabaseBackend::Postgres,
                            "UPDATE federation_delivery_queue SET status = 'pending', attempts = $1, error_message = $2, last_attempt_at = NOW(), next_retry_at = NOW() + make_interval(secs => $4::double precision), lease_token = NULL, lease_expires_at = NULL WHERE id = $3 AND status = 'delivering' AND lease_token = $5",
                            [
                                new_attempts.into(),
                                err_msg.into(),
                                queue_id.into(),
                                backoff_secs.into(),
                                lease_token.into(),
                            ],
                        ))
                        .await
                        .map_err(|err| format!("Queue key retry update failed: {err}"))?;
                    if !lease_update_applied(mark.rows_affected(), &mut stats, queue_id, "pending")
                    {
                        continue;
                    }
                    stats.retried += 1;
                }
            }
        }
    }

    Ok(stats)
}

async fn mark_delivery_dead(user_id: i32, activity_type: &str, target_domain: &str, error: &str) {
    crate::federation::notify::notify_delivery_failed(user_id, activity_type, target_domain, error)
        .await;
    // Also log at error level for operator dashboards / log aggregators
    tracing::error!(
        user_id = user_id,
        activity_type = activity_type,
        target_domain = target_domain,
        error = %error,
        "federation delivery dead letter"
    );
}

/// 启动投递队列后台循环
pub fn spawn_delivery_worker(db: DatabaseConnection) {
    tokio::spawn(async move {
        // Lab dual-instance: `MYRIAD_FEDERATION_DELIVERY_INTERVAL_SECS` (e.g. 2)
        // speeds full-chain harness without changing production default (15s).
        let secs = std::env::var("MYRIAD_FEDERATION_DELIVERY_INTERVAL_SECS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|n| *n >= 1 && *n <= 3600)
            .unwrap_or(15);
        let mut interval = tokio::time::interval(Duration::from_secs(secs));
        loop {
            interval.tick().await;
            match process_delivery_queue_detailed(&db, 20).await {
                Ok(s)
                    if s.delivered > 0
                        || s.dead > 0
                        || s.retried > 0
                        || s.reclaimed > 0
                        || s.lease_lost > 0
                        || s.relationships_revoked > 0 =>
                {
                    tracing::info!(
                        "📤 Delivery worker: claimed={} reclaimed={} delivered={} dead={} retried={} lease_lost={} relationships_revoked={} deliveries_cancelled={}",
                        s.claimed,
                        s.reclaimed,
                        s.delivered,
                        s.dead,
                        s.retried,
                        s.lease_lost,
                        s.relationships_revoked,
                        s.deliveries_cancelled
                    );
                }
                Err(e) => {
                    tracing::error!("Delivery worker error: {}", e);
                }
                _ => {} // 无待投递项，静默
            }
        }
    });
}

/// 重试退避：指数增长 + 抖动，上限 24 小时。
///
/// 没有抖动时，同一个远端实例宕机期间积压的**所有**投递会算出完全相同的
/// `next_retry_at`，于是每一轮都整齐地同时打过去 —— 对方刚恢复就被我们自己
/// 制造的尖峰再打一次。抖动把它们摊开。
///
/// 抖动取 ±25%：足以打散同批，又不会让退避语义走形。
pub(crate) fn retry_backoff_secs(attempts: i32) -> i64 {
    let base = 2i64
        .saturating_pow(attempts.clamp(0, 32) as u32)
        .min(86_400);
    // 低位退避（1~2 秒）加抖动没有意义，反而可能算出 0
    if base <= 2 {
        return base;
    }
    let spread = base / 4;
    let jitter = (rand::random::<u64>() % (2 * spread as u64 + 1)) as i64 - spread;
    (base + jitter).clamp(1, 86_400)
}
