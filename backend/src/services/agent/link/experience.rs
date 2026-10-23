//! What a child does for the site owner, made part of her (`experience`),
//! and the Work memories a child's run starts from (`work.recall`).
//!
//! Work on a child (pi, and the computer use it drives) runs there, without
//! her persona, as the site's own Work does. Before it starts it asks her
//! host for the few Work memories that fit; once it ends it tells her host
//! what it did. The host learns from that as from its own Work: memories of
//! the owner alone, kept as coming from the device. A run the owner started
//! is also noted in her diary, and one the owner or the host started reaches
//! her as a Work outcome, so her mood moves and she may say something. A
//! scheduled run is memory only.
//!
//! The child keeps an experience until `experience.taken` names it, which
//! goes only once it is written here. A record of each taken id (a month)
//! answers a resend without writing it twice.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};

use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::services::agent::memory::work_memory;
use crate::services::runtime_registry::{self as registry, RegistryIdentity};

use super::connection::Outgoing;
use super::contract::CONTRACT;

const NAMESPACE: &str = "her_link_experience";
/// How long a taken id is remembered: a child that was away longer has
/// long since been told.
const RETENTION_SECS: i64 = 30 * 24 * 60 * 60;
/// Work memories a run on a child starts from: the site's own Work's four.
pub(super) const RECALLED: usize = 4;
/// Characters of each recalled memory, as the site's own Work reads them.
pub(super) const RECALLED_CHARS: usize = 1_000;
/// What an experience may carry into memory.
const MAX_OUTPUTS: usize = 64;
const MAX_TURNS: usize = 16;
const MAX_CAPABILITIES: usize = 64;
/// Memories a run may say it was given: the start's four and one where it
/// was stuck, with room to spare.
const MAX_RECALLED: usize = 8;
/// What reaches her as the outcome: as long as a site notification's body.
const OUTCOME_CHARS: usize = 160;

/// Who started a run on the child.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Started {
    /// The owner asked for it on the child.
    Owner,
    /// Her host asked the child to do it.
    Host,
    /// One of the child's scheduled tasks.
    Schedule,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum Status {
    Completed,
    Failed,
    Cancelled,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Completed => "completed",
            Status::Failed => "failed",
            Status::Cancelled => "cancelled",
        }
    }

    /// The Work outcome she is told of, as for the site's own Work.
    fn event_key(self) -> &'static str {
        match self {
            Status::Completed => "agent.task_completed",
            Status::Failed => "agent.task_failed",
            Status::Cancelled => "agent.task_cancelled",
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct Output {
    pub(super) id: String,
    pub(super) value: Value,
    /// What became of the step is not known.
    #[serde(default)]
    pub(super) unknown: bool,
}

/// One `experience` body.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Experience {
    pub(super) kind: String,
    pub(super) started: Started,
    pub(super) status: Status,
    pub(super) input: String,
    #[allow(dead_code)]
    pub(super) ended_at: i64,
    #[serde(default)]
    pub(super) reply: Option<String>,
    #[serde(default)]
    #[allow(dead_code)]
    pub(super) title: Option<String>,
    #[serde(default)]
    pub(super) conversation: Vec<Value>,
    #[serde(default)]
    pub(super) outputs: Vec<Output>,
    #[serde(default)]
    pub(super) capabilities: Vec<String>,
    /// The request of her host's it ran (`started: host`).
    #[serde(default)]
    #[allow(dead_code)]
    pub(super) act_id: Option<String>,
    /// Her proposal it ran, when the owner accepted it on the child to run there.
    #[serde(default)]
    pub(super) proposal_id: Option<String>,
    /// The memories `work.recalled` gave the run, and what came of them.
    #[serde(default)]
    pub(super) recalled: Vec<Recalled>,
}

/// One memory a run on the child was given (`experience.recalled`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Recalled {
    pub(super) id: String,
    /// The tool the run was stuck on when it was given.
    #[serde(default)]
    pub(super) stuck: Option<String>,
    /// That tool failed again after it.
    #[serde(default)]
    pub(super) failed_again: bool,
}

/// A memory id as the host makes them: what a child sends back is only
/// ever looked up among the owner's own, never trusted as more.
fn valid_memory_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

impl Experience {
    /// Something the host can learn from: a Work run, with what was asked.
    pub(super) fn readable(&self) -> bool {
        self.kind == "work"
            && !self.input.trim().is_empty()
            && self.input.chars().count() <= CONTRACT.limits.max_text_chars
    }

    /// The run as Work memory reads it, for the owner, from `device_id`,
    /// where it is the experience `id`.
    pub(super) fn run(&self, owner: i32, device_id: &str, id: &str) -> work_memory::WorkRun {
        let outputs = self
            .outputs
            .iter()
            .take(MAX_OUTPUTS)
            .map(|output| {
                let mut value = output.value.clone();
                // A step whose outcome is not known is no failure to learn from.
                if output.unknown && value.is_object() {
                    value["execution_unknown"] = true.into();
                }
                (output.id.chars().take(64).collect(), value)
            })
            .collect();
        work_memory::WorkRun {
            user_id: owner,
            user_input: self.input.clone(),
            conversation: self.conversation.iter().take(MAX_TURNS).cloned().collect(),
            step_outputs: outputs,
            capabilities: self
                .capabilities
                .iter()
                .take(MAX_CAPABILITIES)
                .map(|capability| capability.chars().take(128).collect())
                .collect(),
            success: self.status == Status::Completed,
            reply: self.reply.clone().filter(|reply| !reply.trim().is_empty()),
            origin: Some(device_id.to_string()),
            record: work_memory::RunRecord {
                experience: Some(id.to_string()),
                ..Default::default()
            },
            shown: self
                .recalled
                .iter()
                .take(MAX_RECALLED)
                .filter(|shown| valid_memory_id(&shown.id))
                .map(|shown| work_memory::Shown {
                    id: shown.id.clone(),
                    stuck: shown
                        .stuck
                        .as_ref()
                        .map(|tool| tool.chars().take(128).collect()),
                    failed_again: shown.failed_again,
                })
                .collect(),
        }
    }

    /// What she is told happened, as a site notification's body says it.
    fn outcome(&self) -> String {
        let said = self
            .reply
            .as_deref()
            .map(str::trim)
            .filter(|reply| !reply.is_empty())
            .unwrap_or(self.input.trim());
        said.chars().take(OUTCOME_CHARS).collect()
    }
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Taken {
    id: String,
}

fn record_id(device_id: &str, id: &str) -> String {
    format!("{device_id}:{id}")
}

/// Experiences being written now, by device and id: a resend meanwhile is
/// answered once the first is written.
static WRITING: LazyLock<Mutex<HashSet<String>>> = LazyLock::new(|| Mutex::new(HashSet::new()));

pub(super) fn taken_frame(id: &str) -> Outgoing {
    Outgoing::latest("experience.taken", json!({ "id": id }))
}

pub(super) fn recalled_frame(request_id: &str, memories: &[(String, String)]) -> Outgoing {
    let (ids, texts): (Vec<&String>, Vec<&String>) =
        memories.iter().map(|(id, text)| (id, text)).unzip();
    Outgoing::latest(
        "work.recalled",
        json!({ "requestId": request_id, "memories": texts, "ids": ids }),
    )
}

/// Whether this device's experience `id` was already written.
pub(super) async fn was_taken(db: &DatabaseConnection, device_id: &str, id: &str) -> bool {
    match registry::get::<Taken>(db, NAMESPACE, &record_id(device_id, id)).await {
        Ok(found) => found.is_some_and(|taken| taken.id == id),
        Err(error) => {
            tracing::warn!(%error, device = %device_id, "[link] could not read an experience record");
            false
        }
    }
}

/// Starts writing the experience unless it is being written already: false
/// when another copy of it is on its way.
pub(super) fn begin(device_id: &str, id: &str) -> bool {
    WRITING
        .lock()
        .map(|mut writing| writing.insert(record_id(device_id, id)))
        .unwrap_or(false)
}

fn end(device_id: &str, id: &str) {
    if let Ok(mut writing) = WRITING.lock() {
        writing.remove(&record_id(device_id, id));
    }
}

/// Writes what the child did into her, then records the id as taken and
/// tells the device (on whichever link it has open then). After that, a
/// run the owner or the host started reaches her as a Work outcome.
pub(super) async fn write(
    db: DatabaseConnection,
    owner: i32,
    device_id: String,
    id: String,
    experience: Experience,
) {
    let written = write_once(&db, owner, &device_id, &id, &experience).await;
    end(&device_id, &id);
    match written {
        Ok(()) => {
            tracing::info!(
                device = %device_id,
                started = ?experience.started,
                status = ?experience.status,
                "[link] experience taken"
            );
            // Her proposal the owner had the app run ends as the run did.
            if let Some(proposal) = experience
                .proposal_id
                .as_deref()
                .filter(|_| experience.started == Started::Owner)
            {
                if let Some((status, summary)) =
                    super::act::outcome(experience.status.as_str(), experience.reply.as_deref())
                {
                    let _ = crate::services::agent::consciousness::IntentStore::new(db.clone())
                        .transition(proposal, owner, status, None, None, summary)
                        .await;
                }
            }
            if let Some(tx) = super::experience_sender(owner, &device_id) {
                let _ = tx.send(taken_frame(&id)).await;
            }
            if matches!(experience.started, Started::Owner | Started::Host) {
                crate::services::agent::merope::spawn_ingest(
                    owner,
                    experience.status.event_key(),
                    experience.outcome(),
                );
            }
        }
        Err(error) => {
            // Not taken: the child keeps it and sends it again on its next link.
            tracing::warn!(%error, device = %device_id, "[link] could not write an experience");
        }
    }
}

async fn write_once(
    db: &DatabaseConnection,
    owner: i32,
    device_id: &str,
    id: &str,
    experience: &Experience,
) -> Result<(), sea_orm::DbErr> {
    // What they had her do, as the site's own Work notes it when it starts.
    if experience.started == Started::Owner {
        crate::services::agent::merope::note_chat_diary(db, owner, &experience.input).await;
    }
    // A run stopped part way teaches nothing, as on the site and the Mac.
    if experience.status != Status::Cancelled {
        let run = experience.run(owner, device_id, id);
        let attribution = crate::services::ai_cost_ledger::AiLedgerAttribution::site(
            owner,
            "agent",
            "agent.memory",
        );
        crate::services::ai_cost_ledger::with_ai_ledger_attribution(
            attribution,
            work_memory::learn_from_run(db, run),
        )
        .await;
    }
    registry::put(
        db,
        NAMESPACE,
        &record_id(device_id, id),
        RegistryIdentity {
            subject_id: Some(owner),
            owner_id: Some(owner),
            tapp_id: None,
            runtime_id: Some(device_id),
        },
        &Taken { id: id.to_string() },
        crate::clock::now().timestamp() + RETENTION_SECS,
    )
    .await
}

/// The owner's Work memories that fit `query`, as the site's own Work
/// recalls them (four, in front of the owner alone), each with its id. When
/// the run is `stuck`, `query` is how, and it is the one lesson or pattern
/// that failure names, other than those it was given (`shown`).
pub(super) async fn recall(
    db: &DatabaseConnection,
    owner: i32,
    query: &str,
    stuck: bool,
    shown: &[String],
) -> Vec<(String, String)> {
    let found = if stuck {
        work_memory::recall_when_stuck(db, owner, query, shown)
            .await
            .map(|found| found.into_iter().collect())
    } else {
        work_memory::recall_for_run(db, owner, query, RECALLED).await
    };
    match found {
        Ok(memories) => memories
            .into_iter()
            .map(|memory| {
                (
                    memory.id,
                    memory.content.chars().take(RECALLED_CHARS).collect(),
                )
            })
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "[link] Work memory recall failed");
            Vec::new()
        }
    }
}
