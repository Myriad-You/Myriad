//! The pieces of Work she hands off in the web panel's one conversation.
//!
//! The panel has a single conversation with her (Chat). What she hands off
//! there becomes a piece, numbered in that conversation, exactly as in a
//! private IM chat (`channel_work::works`): the same lines she writes, the
//! same view of every piece, the same rules for where a line goes. Only the
//! way back differs: the panel follows each piece's own Work run, shows its
//! plan, confirmations and questions on that piece, and they are answered
//! there. Each piece runs with their granted permissions, through the same
//! gate as any run.
//!
//! A piece is kept under `{chat session}#{n}` with its own Work session; how
//! it stands is read from that session's task and run.

use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::services::agent::delegate::HandOff;
use crate::services::agent::types::{ChannelChat, TaskState, TaskStatus};
use crate::services::agent::{Agent, AgentInteractionMode};
use crate::services::channel_work::works::{self, Act, Piece, Standing};
use crate::services::runtime_registry::{self as registry, RegistryIdentity};
use myriad_agent_rules::channel::{PendingKind, PendingOption, PendingPrompt};

const NS: &str = "agent_panel_pieces";
const KEPT_FOR: chrono::Duration = chrono::Duration::days(30);
const RESULT_CHARS: usize = 2000;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Kept {
    /// The piece's own Work session.
    session_id: String,
    asked: String,
    started_at: i64,
    #[serde(default)]
    stopped: bool,
    /// The run it was last started or answered with.
    #[serde(default)]
    run_id: Option<String>,
}

struct Kept1 {
    number: u32,
    key: String,
    kept: Kept,
}

fn identity(user_id: i32) -> RegistryIdentity<'static> {
    RegistryIdentity {
        subject_id: Some(user_id),
        owner_id: Some(user_id),
        tapp_id: None,
        runtime_id: None,
    }
}

async fn kept(db: &DatabaseConnection, user_id: i32, chat: &str) -> Vec<Kept1> {
    let rows = registry::list(db, NS, Some(user_id), None)
        .await
        .unwrap_or_default();
    let mut kept: Vec<Kept1> = rows
        .into_iter()
        .filter_map(|row| {
            let (of, number) = works::split_lane(&row.record_id)?;
            (of == chat).then_some(())?;
            Some(Kept1 {
                number,
                key: row.record_id.clone(),
                kept: serde_json::from_value(row.payload).ok()?,
            })
        })
        .collect();
    kept.sort_by_key(|piece| piece.number);
    kept
}

async fn keep(db: &DatabaseConnection, user_id: i32, key: &str, kept: &Kept) {
    let until = (crate::clock::now() + KEPT_FOR).timestamp();
    if let Err(error) = registry::put(db, NS, key, identity(user_id), kept, until).await {
        tracing::warn!(%error, "[Panel] could not keep a piece");
    }
}

/// The task under way in a piece's session, if any.
fn live_task<'a>(tasks: &'a [TaskState], session_id: &str) -> Option<&'a TaskState> {
    tasks.iter().find(|task| {
        myriad_agent_rules::is_cancellable_task_status(&task.status)
            && myriad_agent_rules::session_id_from_lane_id(task.lane_id.as_deref()).as_deref()
                == Some(session_id)
    })
}

/// A parked task question as the channel's prompt, so her answer is read
/// by the same rules.
fn prompt_of(task: &TaskState) -> Option<PendingPrompt> {
    let question = task.pending_question.as_ref()?;
    let question_type = serde_json::to_value(&question.question_type)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_else(|| "free_text".into());
    Some(PendingPrompt {
        id: String::new(),
        kind: PendingKind::Answer {
            task_id: task.task_id.clone(),
            question_id: question.question_id.clone(),
            question_type,
        },
        question: question.question.clone(),
        options: question
            .options
            .iter()
            .flatten()
            .map(|option| PendingOption {
                value: option.value.clone(),
                label: option.label.clone(),
            })
            .collect(),
        expires_at_unix: None,
    })
}

struct Now {
    number: u32,
    key: String,
    kept: Kept,
    piece: Piece,
    task: Option<TaskState>,
    run_id: Option<String>,
    /// Ended without getting it done: its last turn failed.
    failed: bool,
}

async fn now(db: &DatabaseConnection, user_id: i32, chat: &str) -> Vec<Now> {
    let kept = kept(db, user_id, chat).await;
    if kept.is_empty() {
        return Vec::new();
    }
    let tasks = Agent::new(db.clone()).await.get_user_tasks(user_id).await;
    let mut pieces = Vec::with_capacity(kept.len());
    for Kept1 { number, key, kept } in kept {
        let task = live_task(&tasks, &kept.session_id).cloned();
        let running =
            crate::services::agent::run_hub::executing_run_for_session(user_id, &kept.session_id)
                .await;
        let mut failed = false;
        let standing = if kept.stopped {
            Standing::Stopped
        } else if let Some(prompt) = task
            .as_ref()
            .filter(|task| task.status == TaskStatus::WaitingForInput)
            .and_then(prompt_of)
        {
            Standing::Waiting {
                question: myriad_agent_rules::channel::format_pending_prompt(&prompt),
            }
        } else if task.is_some() || running.is_some() {
            Standing::Doing {
                progress: task.as_ref().map(|task| task.progress),
            }
        } else {
            let ended = came_back(db, &kept.session_id).await;
            failed = ended.as_ref().is_some_and(|ended| ended.failed);
            Standing::Done {
                result: ended.map(|ended| ended.text),
            }
        };
        let piece = Piece {
            number,
            asked: kept.asked.clone(),
            started_at: Some(kept.started_at),
            standing,
        };
        let run_id = running.or_else(|| kept.run_id.clone());
        pieces.push(Now {
            number,
            key,
            kept,
            piece,
            task,
            run_id,
            failed,
        });
    }
    pieces
}

struct Ended {
    text: String,
    failed: bool,
}

/// How its session ended: the last reply, or the turn that failed (kept as
/// a system line marked as an error).
async fn came_back(db: &DatabaseConnection, session_id: &str) -> Option<Ended> {
    use crate::models::entities::agent_messages;
    use sea_orm::{ColumnTrait, EntityTrait, QueryFilter, QueryOrder};
    let last = agent_messages::Entity::find()
        .filter(agent_messages::Column::SessionId.eq(session_id))
        .filter(agent_messages::Column::Role.is_in(["assistant", "system"]))
        .order_by_desc(agent_messages::Column::CreatedAt)
        .one(db)
        .await
        .ok()??;
    let failed = last
        .metadata
        .as_ref()
        .and_then(|metadata| metadata.get("error"))
        .and_then(Value::as_bool)
        .unwrap_or(false);
    if last.role != "assistant" && !failed {
        return None;
    }
    Some(Ended {
        text: last.content.chars().take(RESULT_CHARS).collect(),
        failed,
    })
}

/// What she sees of the pieces of this conversation, as in a private chat.
pub(crate) async fn view(db: &DatabaseConnection, user_id: i32, chat: &str) -> ChannelChat {
    let at = crate::clock::now().timestamp();
    let shown = works::shown(
        now(db, user_id, chat)
            .await
            .into_iter()
            .map(|piece| piece.piece)
            .collect(),
        at,
    );
    ChannelChat {
        works: shown.iter().map(|piece| works::line(piece, at)).collect(),
        waiting: shown
            .iter()
            .filter(|piece| matches!(piece.standing, Standing::Waiting { .. }))
            .count(),
        came_while_away: None,
    }
}

/// The pieces of this conversation for the panel to show and follow.
pub(crate) async fn listed(db: &DatabaseConnection, user_id: i32, chat: &str) -> Vec<Value> {
    now(db, user_id, chat)
        .await
        .into_iter()
        .map(|now| {
            let (state, detail) = match &now.piece.standing {
                Standing::Doing { progress } => ("doing", json!({ "progress": progress })),
                Standing::Waiting { .. } => (
                    "waiting",
                    json!({
                        "taskId": now.task.as_ref().map(|task| &task.task_id),
                        "question": now.task.as_ref().and_then(|task| task.pending_question.as_ref()),
                    }),
                ),
                Standing::Done { result } => (
                    if now.failed { "failed" } else { "done" },
                    json!({ "result": result }),
                ),
                Standing::Stopped => ("stopped", json!({})),
            };
            json!({
                "number": now.number,
                "asked": now.kept.asked,
                "startedAt": now.kept.started_at,
                "sessionId": now.kept.session_id,
                "runId": now.run_id,
                "state": state,
                "detail": detail,
                "key": now.key,
            })
        })
        .collect()
}

async fn start(
    db: &DatabaseConnection,
    user_id: i32,
    session_id: &str,
    input: String,
) -> Result<String, String> {
    let run = crate::services::agent::run::start_for_user(
        db.clone(),
        user_id,
        crate::services::agent::run::ProcessRequest {
            input,
            context: Some(crate::services::agent::run::ProcessContext {
                mode: Some(AgentInteractionMode::Work),
                session_id: Some(session_id.to_string()),
                handed_off: true,
                ..Default::default()
            }),
        },
    )
    .await
    .map_err(|error| error.to_json().to_string())?;
    Ok(run.run_id().to_string())
}

/// Carry out her lines from a panel turn, in the order she wrote them.
///
/// Boxed: her turn runs inside a run, and a piece starts a run of its own, so
/// the run's future would otherwise contain itself.
pub(crate) fn carry_out<'a>(
    db: &'a DatabaseConnection,
    user_id: i32,
    chat: &'a str,
    hand_offs: &'a [HandOff],
    their_words: &'a str,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send + 'a>> {
    Box::pin(carry_out_in_order(
        db,
        user_id,
        chat,
        hand_offs,
        their_words,
    ))
}

async fn carry_out_in_order(
    db: &DatabaseConnection,
    user_id: i32,
    chat: &str,
    hand_offs: &[HandOff],
    their_words: &str,
) {
    for hand_off in hand_offs {
        let pieces = now(db, user_id, chat).await;
        let plain: Vec<Piece> = pieces.iter().map(|now| now.piece.clone()).collect();
        let of = |work: u32| pieces.iter().find(|now| now.number == work);
        match works::act(hand_off, &plain) {
            Act::Start(asked) => {
                let number = works::next_number(pieces.iter().map(|now| now.number));
                let Ok(session_id) = crate::services::agent::sessions::ensure_session(
                    db,
                    None,
                    user_id,
                    AgentInteractionMode::Work,
                )
                .await
                else {
                    continue;
                };
                let key = works::lane_key(chat, number);
                let mut kept = Kept {
                    session_id: session_id.clone(),
                    asked: asked.clone(),
                    started_at: crate::clock::now().timestamp(),
                    ..Default::default()
                };
                keep(db, user_id, &key, &kept).await;
                match start(
                    db,
                    user_id,
                    &session_id,
                    works::work_input(&asked, their_words),
                )
                .await
                {
                    Ok(run_id) => {
                        kept.run_id = Some(run_id);
                        keep(db, user_id, &key, &kept).await;
                    }
                    Err(error) => tracing::warn!(%error, "[Panel] a piece did not start"),
                }
            }
            Act::Answer(work, answer) => {
                let Some(now) = of(work) else { continue };
                let Some((task, prompt)) = now
                    .task
                    .as_ref()
                    .and_then(|task| prompt_of(task).map(|prompt| (task, prompt)))
                else {
                    continue;
                };
                let answer = works::her_choice(&prompt, &answer);
                match myriad_agent_rules::channel::decide_pending_reply(
                    &prompt,
                    &answer,
                    crate::clock::now().timestamp(),
                ) {
                    myriad_agent_rules::channel::PendingDecision::Resume { answer, .. } => {
                        let question_id = task
                            .pending_question
                            .as_ref()
                            .map(|question| question.question_id.clone())
                            .unwrap_or_default();
                        match crate::services::agent::run::answer_for_user(
                            db.clone(),
                            user_id,
                            task.task_id.clone(),
                            question_id,
                            answer,
                            Some(now.kept.session_id.clone()),
                        )
                        .await
                        {
                            Ok(run) => {
                                let mut kept = now.kept.clone();
                                kept.run_id = Some(run.run_id().to_string());
                                keep(db, user_id, &now.key, &kept).await;
                            }
                            Err(error) => {
                                tracing::warn!(error = %error.to_json(), "[Panel] answer not taken")
                            }
                        }
                    }
                    // Not an answer to it: kept for the piece; its question
                    // stays on it for them.
                    _ => {
                        let _ = crate::services::agent::executor::enqueue_steering(
                            db,
                            &task.task_id,
                            works::work_input(&answer, their_words),
                        )
                        .await;
                    }
                }
            }
            Act::Steer(work, text) => {
                let Some(task) = of(work).and_then(|now| now.task.as_ref()) else {
                    tracing::warn!("[Panel] no task under way to take more for this piece");
                    continue;
                };
                let _ = crate::services::agent::executor::enqueue_steering(
                    db,
                    &task.task_id,
                    works::work_input(&text, their_words),
                )
                .await;
            }
            Act::Continue(work, text) => {
                let Some(now) = of(work) else { continue };
                if let Ok(run_id) = start(
                    db,
                    user_id,
                    &now.kept.session_id,
                    works::work_input(&text, their_words),
                )
                .await
                {
                    let mut kept = now.kept.clone();
                    kept.run_id = Some(run_id);
                    keep(db, user_id, &now.key, &kept).await;
                }
            }
            Act::Stop(work) => {
                stop(db, user_id, chat, work).await;
            }
            Act::Nothing => {
                tracing::info!(?hand_off, "[Panel] her line is for no piece under way");
            }
        }
    }
}

/// Stop one piece, and only it. False when there is no such piece.
pub(crate) async fn stop(db: &DatabaseConnection, user_id: i32, chat: &str, number: u32) -> bool {
    let Some(piece) = kept(db, user_id, chat)
        .await
        .into_iter()
        .find(|piece| piece.number == number)
    else {
        return false;
    };
    if let Some(run_id) =
        crate::services::agent::run_hub::executing_run_for_session(user_id, &piece.kept.session_id)
            .await
        && let Ok(Some(run)) =
            crate::services::agent::run_hub::get_run_for_user(&run_id, user_id).await
    {
        run.abort_execution().await;
    }
    let tasks = Agent::new(db.clone()).await.get_user_tasks(user_id).await;
    for task in tasks.iter().filter(|task| {
        myriad_agent_rules::is_cancellable_task_status(&task.status)
            && myriad_agent_rules::session_id_from_lane_id(task.lane_id.as_deref()).as_deref()
                == Some(piece.kept.session_id.as_str())
    }) {
        crate::services::agent::run::cancel_task_and_wake(db, user_id, &task.task_id).await;
    }
    let mut kept = piece.kept;
    kept.stopped = true;
    keep(db, user_id, &piece.key, &kept).await;
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kept_piece_reads_back_as_kept() {
        let kept = Kept {
            session_id: "ses_w".into(),
            asked: "查天气".into(),
            started_at: 7,
            stopped: false,
            run_id: Some("run_1".into()),
        };
        let back: Kept = serde_json::from_value(serde_json::to_value(&kept).unwrap()).unwrap();
        assert_eq!(back.session_id, "ses_w");
        assert_eq!(back.run_id.as_deref(), Some("run_1"));
        // Written before a field existed, it still reads.
        let old: Kept =
            serde_json::from_value(json!({ "sessionId": "s", "asked": "a", "startedAt": 1 }))
                .unwrap();
        assert!(!old.stopped);
    }
}
