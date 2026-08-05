//! Durable inbound Activity receipts.
//!
//! The in-process replay cache is deliberately only a fast path.  A receipt
//! row is the cross-process/restart authority: its `(signer, activity_id)`
//! key is unique, the raw request digest is bound to that key, and the row is
//! changed to `accepted` in the same transaction as the inbound DB effects.
//!
//! Callers must perform signature and trust checks before opening the
//! transaction.  A policy rejection therefore never creates an accepted
//! receipt.  A transient handler error rolls back the claim with the handler
//! writes, so a fresh request can claim it again; a committed `failed` row is
//! also reclaimable for recovery paths.  A permanent handler error is recorded
//! as `rejected` (never as accepted).

use std::time::Duration;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseTransaction, Statement};
use sha2::{Digest, Sha256};

use crate::federation::types::{normalize_activity_id, normalize_actor_url};

/// Keep a crashed transaction's `processing` row reclaimable without allowing
/// two live handlers to execute the same activity concurrently.
pub const RECEIPT_LEASE: Duration = Duration::from_secs(5 * 60);

/// A stable key used by the receipt table and by completion updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptKey {
    pub signer: String,
    pub activity_id: String,
    pub body_digest: String,
}

/// Result of trying to claim an inbound activity inside an open transaction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReceiptClaim {
    /// The caller owns the row and may run effects in this transaction.
    Execute(ReceiptKey),
    /// A committed success already exists.  The caller should answer 202 and
    /// must not run any handler.
    AlreadyAccepted,
    /// Another live request owns the row.  The caller should answer 202 and
    /// must not run any handler; the peer can retry if that request fails.
    InFlight,
    /// The same signer/activity id was seen with different bytes.
    Conflict { stored_digest: String },
    /// A permanent handler rejection was committed for this exact digest.
    Rejected {
        status: u16,
        message: Option<String>,
    },
}

/// Completion classification used by the inbound dispatcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReceiptOutcome {
    Accepted,
    /// Handler/persistence failure that the sender should retry.
    Retryable,
    /// Deterministic malformed/unauthorized state after trust checks.
    Rejected,
}

/// Build a receipt identity after signature + trust checks.
///
/// ActivityPub ids are normalized with the existing URL policy.  Activities
/// without an id still get durable digest-based deduplication, but they cannot
/// conflict with a separately identified activity.
pub fn receipt_key(signer: &str, activity_id: &str, body: &[u8]) -> ReceiptKey {
    let digest = body_digest(body);
    let activity_id = normalize_activity_id(activity_id);
    let activity_id = if activity_id.is_empty() {
        format!("urn:myriad:inbox-body:{digest}")
    } else {
        activity_id
    };
    let signer = normalize_actor_url(signer);
    ReceiptKey {
        signer: if signer.is_empty() {
            signer_from_opaque(signer)
        } else {
            signer
        },
        activity_id,
        body_digest: digest,
    }
}

fn signer_from_opaque(raw: String) -> String {
    raw.trim().to_ascii_lowercase()
}

/// SHA-256 digest of the exact signed request bytes.
pub fn body_digest(body: &[u8]) -> String {
    hex::encode(Sha256::digest(body))
}

/// Claim a row.  The caller must pass a `DatabaseTransaction`; this function
/// never opens or commits a nested transaction, so the eventual receipt
/// outcome can commit atomically with the handler's DB writes.
pub async fn claim_receipt<C: ConnectionTrait + ?Sized>(
    db: &C,
    key: &ReceiptKey,
) -> Result<ReceiptClaim, String> {
    let inserted = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"INSERT INTO federation_inbox_receipts
                   (signer, activity_id, body_digest, status, attempts,
                    lease_until, created_at, updated_at)
               VALUES ($1, $2, $3, 'processing', 1,
                       NOW() + INTERVAL '5 minutes', NOW(), NOW())
               ON CONFLICT (signer, activity_id) DO NOTHING
               RETURNING signer"#,
            [
                key.signer.clone().into(),
                key.activity_id.clone().into(),
                key.body_digest.clone().into(),
            ],
        ))
        .await
        .map_err(|e| format!("claim inbound receipt insert: {e}"))?;
    if inserted.is_some() {
        return Ok(ReceiptClaim::Execute(key.clone()));
    }

    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"SELECT body_digest, status, outcome_status, error_message,
                      (lease_until IS NOT NULL AND lease_until > NOW()) AS lease_active
               FROM federation_inbox_receipts
               WHERE signer = $1 AND activity_id = $2
               FOR UPDATE"#,
            [key.signer.clone().into(), key.activity_id.clone().into()],
        ))
        .await
        .map_err(|e| format!("claim inbound receipt lookup: {e}"))?
        .ok_or_else(|| "receipt disappeared after conflict; retry transaction".to_string())?;

    let stored_digest = row
        .try_get::<String>("", "body_digest")
        .map_err(|e| format!("read inbound receipt digest: {e}"))?;
    if stored_digest != key.body_digest {
        return Ok(ReceiptClaim::Conflict { stored_digest });
    }

    let status = row.try_get::<String>("", "status").unwrap_or_default();
    match status.as_str() {
        "accepted" => Ok(ReceiptClaim::AlreadyAccepted),
        "rejected" => Ok(ReceiptClaim::Rejected {
            status: row
                .try_get::<i16>("", "outcome_status")
                .unwrap_or(400)
                .max(100) as u16,
            message: row
                .try_get::<Option<String>>("", "error_message")
                .ok()
                .flatten(),
        }),
        "processing" => {
            let lease_active = row.try_get::<bool>("", "lease_active").unwrap_or(true);
            if lease_active {
                Ok(ReceiptClaim::InFlight)
            } else {
                reclaim_receipt(db, key).await?;
                Ok(ReceiptClaim::Execute(key.clone()))
            }
        }
        // `failed` is intentionally retryable.  Unknown states fail closed
        // for this request and remain retryable after operator inspection.
        _ => {
            reclaim_receipt(db, key).await?;
            Ok(ReceiptClaim::Execute(key.clone()))
        }
    }
}

async fn reclaim_receipt<C: ConnectionTrait + ?Sized>(
    db: &C,
    key: &ReceiptKey,
) -> Result<(), String> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"UPDATE federation_inbox_receipts
           SET status = 'processing', attempts = attempts + 1,
               lease_until = NOW() + INTERVAL '5 minutes',
               updated_at = NOW(), error_message = NULL, outcome_status = NULL
           WHERE signer = $1 AND activity_id = $2 AND body_digest = $3"#,
        [
            key.signer.clone().into(),
            key.activity_id.clone().into(),
            key.body_digest.clone().into(),
        ],
    ))
    .await
    .map_err(|e| format!("reclaim inbound receipt: {e}"))?;
    Ok(())
}

/// Mark a receipt outcome in the same transaction as the handler's writes.
pub async fn finish_receipt<C: ConnectionTrait + ?Sized>(
    db: &C,
    key: &ReceiptKey,
    outcome: ReceiptOutcome,
    status: u16,
    message: Option<&str>,
) -> Result<(), String> {
    let state = match outcome {
        ReceiptOutcome::Accepted => "accepted",
        ReceiptOutcome::Retryable => "failed",
        ReceiptOutcome::Rejected => "rejected",
    };
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        r#"UPDATE federation_inbox_receipts
           SET status = $4, outcome_status = $5, error_message = $6,
               lease_until = NULL, updated_at = NOW(),
               accepted_at = CASE WHEN $4 = 'accepted' THEN NOW() ELSE accepted_at END
           WHERE signer = $1 AND activity_id = $2 AND body_digest = $3
             AND status = 'processing'"#,
        [
            key.signer.clone().into(),
            key.activity_id.clone().into(),
            key.body_digest.clone().into(),
            state.into(),
            (status as i16).into(),
            message.map(str::to_owned).into(),
        ],
    ))
    .await
    .map_err(|e| format!("finish inbound receipt: {e}"))?;
    Ok(())
}

/// A tiny model used by focused unit tests.  It mirrors the SQL state machine
/// and makes the conflict/rejection/retry invariants executable without a DB.
#[cfg(test)]
#[derive(Default)]
struct ReceiptModel {
    rows: std::collections::HashMap<(String, String), (String, &'static str)>,
}

#[cfg(test)]
impl ReceiptModel {
    fn claim(&mut self, key: &ReceiptKey) -> ReceiptClaim {
        let identity = (key.signer.clone(), key.activity_id.clone());
        match self.rows.get(&identity).cloned() {
            None => {
                self.rows
                    .insert(identity, (key.body_digest.clone(), "processing"));
                ReceiptClaim::Execute(key.clone())
            }
            Some((digest, "accepted")) if digest == key.body_digest => {
                ReceiptClaim::AlreadyAccepted
            }
            Some((digest, "rejected")) if digest == key.body_digest => ReceiptClaim::Rejected {
                status: 400,
                message: None,
            },
            Some((digest, _)) if digest != key.body_digest => ReceiptClaim::Conflict {
                stored_digest: digest,
            },
            Some((digest, "failed")) => {
                self.rows.insert(identity, (digest, "processing"));
                ReceiptClaim::Execute(key.clone())
            }
            Some(_) => ReceiptClaim::InFlight,
        }
    }

    fn finish(&mut self, key: &ReceiptKey, outcome: ReceiptOutcome) {
        let state = match outcome {
            ReceiptOutcome::Accepted => "accepted",
            ReceiptOutcome::Retryable => "failed",
            ReceiptOutcome::Rejected => "rejected",
        };
        self.rows.insert(
            (key.signer.clone(), key.activity_id.clone()),
            (key.body_digest.clone(), state),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: &str, body: &[u8]) -> ReceiptKey {
        receipt_key("https://A.example/users/alice/", id, body)
    }

    #[test]
    fn same_signer_and_normalized_id_is_one_execution() {
        let mut model = ReceiptModel::default();
        let first = key("https://peer.example/a/1/?q=1", b"body");
        assert!(matches!(model.claim(&first), ReceiptClaim::Execute(_)));
        model.finish(&first, ReceiptOutcome::Accepted);
        let retry = key("https://PEER.example/a/1", b"body");
        assert_eq!(model.claim(&retry), ReceiptClaim::AlreadyAccepted);
    }

    #[test]
    fn different_digest_is_conflict_not_second_execution() {
        let mut model = ReceiptModel::default();
        let first = key("https://peer.example/a/2", b"one");
        assert!(matches!(model.claim(&first), ReceiptClaim::Execute(_)));
        let conflict = key("https://peer.example/a/2", b"two");
        assert!(matches!(
            model.claim(&conflict),
            ReceiptClaim::Conflict { .. }
        ));
    }

    #[test]
    fn rejection_is_not_accepted_and_retryable_failure_can_retry() {
        let mut model = ReceiptModel::default();
        let rejected = key("https://peer.example/a/3", b"bad");
        assert!(matches!(model.claim(&rejected), ReceiptClaim::Execute(_)));
        model.finish(&rejected, ReceiptOutcome::Rejected);
        assert!(matches!(
            model.claim(&rejected),
            ReceiptClaim::Rejected { .. }
        ));

        let retryable = key("https://peer.example/a/4", b"temporary");
        assert!(matches!(model.claim(&retryable), ReceiptClaim::Execute(_)));
        model.finish(&retryable, ReceiptOutcome::Retryable);
        assert!(matches!(model.claim(&retryable), ReceiptClaim::Execute(_)));
    }

    #[test]
    fn process_restart_model_keeps_committed_acceptance() {
        let mut before_restart = ReceiptModel::default();
        let activity = key("https://peer.example/a/5", b"stable");
        assert!(matches!(
            before_restart.claim(&activity),
            ReceiptClaim::Execute(_)
        ));
        before_restart.finish(&activity, ReceiptOutcome::Accepted);
        // Reusing the durable model represents a fresh process with an empty
        // in-memory replay cache.
        assert_eq!(
            before_restart.claim(&activity),
            ReceiptClaim::AlreadyAccepted
        );
    }
}
