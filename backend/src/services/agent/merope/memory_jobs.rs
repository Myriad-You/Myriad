//! Recoverable memory extraction, owned by the Persona driver lifecycle.
use super::{call, chat_remember, store::memory_jobs as queue, strangers};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum Payload {
    /// What one person said to her in one place since it was last gone over:
    /// every line, in order, with her replies so far. A line said while she
    /// was still answering the one before is not lost; it joins the next.
    Chat {
        user_text: String,
        reply: String,
        /// The newest line's.
        input_at: chrono::DateTime<chrono::FixedOffset>,
        present: crate::services::agent::memory::unified::Audience,
        turn: super::TurnContext,
        /// When each line folded in here was said; one held twice (when it
        /// lands, and again with her reply) is not repeated.
        #[serde(default)]
        lines: Vec<chrono::DateTime<chrono::FixedOffset>>,
    },
    Stranger {
        venue: String,
        stranger: strangers::Stranger,
        exchanges: Vec<myriad_merope::strangers::Exchange>,
        count: i64,
    },
}
impl Payload {
    /// Fold a later arrival for the same person and place into this one.
    /// Their lines join in order; her reply, when there is one, is added.
    pub(super) fn fold(&mut self, later: Payload) {
        let (
            Payload::Chat {
                user_text,
                reply,
                input_at,
                present,
                turn,
                lines,
            },
            Payload::Chat {
                user_text: added_text,
                reply: added_reply,
                input_at: added_at,
                turn: added_turn,
                lines: added_lines,
                ..
            },
        ) = (self, later)
        else {
            return;
        };
        let _ = present;
        if lines.is_empty() {
            lines.push(*input_at);
        }
        let added_lines = if added_lines.is_empty() {
            vec![added_at]
        } else {
            added_lines
        };
        if !added_lines.iter().all(|at| lines.contains(at)) {
            if !added_text.trim().is_empty() {
                if !user_text.is_empty() {
                    user_text.push('\n');
                }
                user_text.push_str(&added_text);
            }
            for at in added_lines {
                if !lines.contains(&at) {
                    lines.push(at);
                }
            }
            // What came just before their first line is still what came
            // before; where they are now is the latest.
            let before = turn.before.take();
            *turn = added_turn;
            turn.before = before.or(turn.before.take());
        }
        if !added_reply.trim().is_empty() && !reply.contains(added_reply.trim()) {
            if !reply.is_empty() {
                reply.push('\n');
            }
            reply.push_str(&added_reply);
        }
        *input_at = (*input_at).max(added_at);
    }

    /// Held as it landed, before she has answered.
    pub(super) fn awaits_reply(&self) -> bool {
        matches!(self, Payload::Chat { reply, .. } if reply.trim().is_empty())
    }
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
            ..
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
