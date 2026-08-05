//! Durable inbound Activity receipts.
//!
//! A database receipt is the sole cross-process/restart authority: its
//! `(signer, activity_id, inbox_scope)` key is unique, the raw request digest
//! is bound to that key, and the row is completed in the same transaction as
//! the inbound DB effects.
//!
//! Callers must perform signature and trust checks before opening the
//! transaction.  A policy rejection therefore never creates an accepted
//! receipt. A transient handler error rolls back the claim with the handler
//! writes, so a fresh request can claim it again. A permanent handler error is
//! recorded as `rejected` (never as accepted).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseTransaction, Statement};
use sha2::{Digest, Sha256};

use crate::federation::types::{normalize_activity_id, normalize_actor_url};

/// A stable key used by the receipt table and by completion updates.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReceiptKey {
    pub signer: String,
    pub activity_id: String,
    pub inbox_scope: String,
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
    /// Deterministic malformed/unauthorized state after trust checks.
    Rejected,
}

/// Build a receipt identity after signature + trust checks.
///
/// ActivityPub ids are normalized with the existing URL policy.  Activities
/// without an id still get durable digest-based deduplication, but they cannot
/// conflict with a separately identified activity.
pub fn receipt_key(signer: &str, activity_id: &str, inbox_scope: &str, body: &[u8]) -> ReceiptKey {
    let digest = body_digest(body);
    let activity_id = normalize_activity_id(activity_id);
    let activity_id = if activity_id.is_empty() {
        format!("urn:myriad:inbox-body:{digest}")
    } else {
        activity_id
    };
    let normalized_signer = normalize_actor_url(signer);
    ReceiptKey {
        signer: if normalized_signer.is_empty() {
            signer_from_opaque(signer)
        } else {
            normalized_signer
        },
        activity_id,
        inbox_scope: inbox_scope.trim().to_ascii_lowercase(),
        body_digest: digest,
    }
}

fn signer_from_opaque(raw: &str) -> String {
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
                   (signer, activity_id, inbox_scope, body_digest, status, created_at)
               VALUES ($1, $2, $3, $4, 'processing', NOW())
               ON CONFLICT (signer, activity_id, inbox_scope) DO NOTHING
               RETURNING signer"#,
            [
                key.signer.clone().into(),
                key.activity_id.clone().into(),
                key.inbox_scope.clone().into(),
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
            r#"SELECT body_digest, status, outcome_status, error_message
               FROM federation_inbox_receipts
               WHERE signer = $1 AND activity_id = $2 AND inbox_scope = $3
               FOR UPDATE"#,
            [
                key.signer.clone().into(),
                key.activity_id.clone().into(),
                key.inbox_scope.clone().into(),
            ],
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
        // `processing` is never committed: claim, handler writes, and outcome
        // share one transaction. Seeing it here means an invariant was broken
        // by manual data changes or incompatible code, so do not execute.
        "processing" => Err("committed processing inbox receipt violates atomicity".into()),
        other => Err(format!("unknown inbox receipt status: {other}")),
    }
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
        ReceiptOutcome::Rejected => "rejected",
    };
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"UPDATE federation_inbox_receipts
           SET status = $5, outcome_status = $6, error_message = $7,
               completed_at = NOW()
           WHERE signer = $1 AND activity_id = $2 AND inbox_scope = $3
             AND body_digest = $4
             AND status = 'processing'"#,
            [
                key.signer.clone().into(),
                key.activity_id.clone().into(),
                key.inbox_scope.clone().into(),
                key.body_digest.clone().into(),
                state.into(),
                (status as i16).into(),
                message.map(str::to_owned).into(),
            ],
        ))
        .await
        .map_err(|e| format!("finish inbound receipt: {e}"))?;
    if result.rows_affected() != 1 {
        return Err(format!(
            "finish inbound receipt affected {} rows, expected exactly one",
            result.rows_affected()
        ));
    }
    Ok(())
}

/// A tiny model used by focused unit tests. It stores only committed rows,
/// matching PostgreSQL visibility: an in-transaction `processing` insert is
/// never visible to a later claim unless the atomicity invariant is broken.
#[cfg(test)]
#[derive(Default)]
struct ReceiptModel {
    rows: std::collections::HashMap<(String, String, String), (String, &'static str)>,
}

#[cfg(test)]
impl ReceiptModel {
    fn claim(&mut self, key: &ReceiptKey) -> ReceiptClaim {
        let identity = (
            key.signer.clone(),
            key.activity_id.clone(),
            key.inbox_scope.clone(),
        );
        match self.rows.get(&identity).cloned() {
            None => ReceiptClaim::Execute(key.clone()),
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
            Some((_, state)) => panic!("invalid committed receipt state in model: {state}"),
        }
    }

    fn finish(&mut self, key: &ReceiptKey, outcome: ReceiptOutcome) {
        let state = match outcome {
            ReceiptOutcome::Accepted => "accepted",
            ReceiptOutcome::Rejected => "rejected",
        };
        self.rows.insert(
            (
                key.signer.clone(),
                key.activity_id.clone(),
                key.inbox_scope.clone(),
            ),
            (key.body_digest.clone(), state),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key_for_scope(id: &str, scope: &str, body: &[u8]) -> ReceiptKey {
        receipt_key("https://A.example/users/alice/", id, scope, body)
    }

    fn key(id: &str, body: &[u8]) -> ReceiptKey {
        key_for_scope(id, "user:1", body)
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
        model.finish(&first, ReceiptOutcome::Accepted);
        let conflict = key("https://peer.example/a/2", b"two");
        assert!(matches!(
            model.claim(&conflict),
            ReceiptClaim::Conflict { .. }
        ));
    }

    #[test]
    fn opaque_signer_fallback_uses_the_original_input() {
        let key = receipt_key("  /  ", "opaque-activity", "user:1", b"body");
        assert_eq!(key.signer, "/");
    }

    #[test]
    fn rejection_is_not_accepted() {
        let mut model = ReceiptModel::default();
        let rejected = key("https://peer.example/a/3", b"bad");
        assert!(matches!(model.claim(&rejected), ReceiptClaim::Execute(_)));
        model.finish(&rejected, ReceiptOutcome::Rejected);
        assert!(matches!(
            model.claim(&rejected),
            ReceiptClaim::Rejected { .. }
        ));
    }

    #[test]
    fn same_activity_delivered_to_different_local_inboxes_executes_per_scope() {
        let mut model = ReceiptModel::default();
        let alice = key_for_scope("https://peer.example/a/4", "user:1", b"public activity");
        let bob = key_for_scope("https://peer.example/a/4", "user:2", b"public activity");

        assert!(matches!(model.claim(&alice), ReceiptClaim::Execute(_)));
        model.finish(&alice, ReceiptOutcome::Accepted);
        assert!(matches!(model.claim(&bob), ReceiptClaim::Execute(_)));
        model.finish(&bob, ReceiptOutcome::Accepted);
        assert_eq!(model.claim(&alice), ReceiptClaim::AlreadyAccepted);
        assert_eq!(model.claim(&bob), ReceiptClaim::AlreadyAccepted);
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
        // Reusing the durable model represents a fresh process reading the
        // previously committed database row.
        assert_eq!(
            before_restart.claim(&activity),
            ReceiptClaim::AlreadyAccepted
        );
    }
}
