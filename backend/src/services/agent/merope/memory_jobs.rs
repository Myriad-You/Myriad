//! Recoverable memory extraction, owned by the Persona driver lifecycle.
use super::{call, chat_remember, store::memory_jobs as queue, strangers};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum Payload {
    Chat {
        user_text: String,
        reply: String,
        input_at: chrono::DateTime<chrono::FixedOffset>,
        present: crate::services::agent::memory::unified::Audience,
        turn: super::TurnContext,
    },
    Stranger {
        venue: String,
        stranger: strangers::Stranger,
        exchanges: Vec<myriad_merope::strangers::Exchange>,
        count: i64,
    },
}
pub(super) enum Effect {
    Chat(chat_remember::ChatMemoryUpdates),
    Stranger {
        previous: Option<String>,
        note: Option<String>,
    },
    NoChange,
    Stale,
}
#[derive(Debug)]
pub(super) enum Failure {
    Model(call::Failure),
    Storage,
}
impl Failure {
    pub(super) fn retryable(&self) -> bool {
        match self {
            Self::Model(failure) => failure.retryable(),
            Self::Storage => true,
        }
    }
}
impl std::fmt::Display for Failure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Model(failure) => failure.fmt(f),
            Self::Storage => f.write_str("storage_failed"),
        }
    }
}
impl From<call::Failure> for Failure {
    fn from(value: call::Failure) -> Self {
        Self::Model(value)
    }
}

pub(super) fn key(parts: &[&str]) -> String {
    use sha2::{Digest, Sha256};
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    hex::encode(digest.finalize())
}

/// Four bounded attempts at a time. Shutdown cancels this driver; unacknowledged
/// leases become eligible again, including when the process dies mid-request.
pub(crate) async fn tick(db: DatabaseConnection) {
    if !super::is_enabled().await {
        return;
    }
    let mut claims = Vec::new();
    for _ in 0..4 {
        match queue::claim(&db).await {
            Ok(Some(claim)) => claims.push(claim),
            Ok(None) => break,
            Err(_) => {
                tracing::warn!(outcome = "claim_failed", "[Merope] memory queue");
                break;
            }
        }
    }
    let db = &db;
    futures::future::join_all(claims.into_iter().map(|claim| async move {
        let result = match tokio::time::timeout(Duration::from_secs(45), prepare(db, &claim)).await
        {
            Ok(result) => result,
            Err(_) => Err(Failure::Model(call::Failure::Timeout)),
        };
        let result = if super::is_enabled().await {
            result
        } else {
            Ok(Effect::Stale)
        };
        if queue::finish(db, &claim, result).await.is_err() {
            // Do not acknowledge an uncertain write. Transaction rollback or
            // a lost response is resolved by the receipt on the next claim.
            tracing::warn!(
                job = claim.id,
                outcome = "commit_failed",
                "[Merope] memory queue"
            );
        }
    }))
    .await;
}
async fn prepare(db: &DatabaseConnection, claim: &queue::Claim) -> Result<Effect, Failure> {
    if !super::is_enabled().await
        || !queue::current(db, &claim.job, false)
            .await
            .map_err(|_| Failure::Storage)?
    {
        return Ok(Effect::Stale);
    }
    match claim.job.data.as_ref().expect("claimed jobs carry work") {
        Payload::Chat {
            user_text,
            reply,
            input_at,
            present,
            turn,
        } => {
            chat_remember::extract(
                db,
                claim.job.owner,
                user_text,
                reply,
                *input_at,
                present,
                turn,
            )
            .await
        }
        Payload::Stranger {
            venue,
            stranger,
            exchanges,
            count,
        } => strangers::prepare_note(db, claim.job.owner, venue, stranger, exchanges, *count).await,
    }
}
