//! A member's turn in a group, as her group mind asks it (D2c-8 A, D2c-9 A):
//! the group's own part of what she answers from (the game, her stickers, the
//! words she knows, the group's talk and how it reads) is put together in the
//! core; what she knows of this person and how she is, and everything a Chat
//! turn keeps around it, stay here, the same as for a private chat.
//!
//! A turn opens ([`begin`]): it is the member's Chat turn in their session in
//! the group (claimed as their live turn, their line kept, a lane taken, their
//! AI budget reserved), their words land on how she feels, and what she knows
//! of them is read. She is asked within it ([`say`]). It closes ([`end`])
//! with her words ready to go out; what of them went out is kept in their
//! session and taken into her memory, her thoughts after it run, the budget
//! is settled ([`kept`]). A turn the group mind leaves
//! (nothing to say, past its time) closes without anything kept of her.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use super::agent_footer::{AgentTurnBudget, AgentTurnKind};
use super::queue::LaneGuard;
use super::run::ProcessContext;
use super::types::{ConversationMessage, UserRequest};
use super::{Agent, AgentInteractionMode, CHAT_LANE_QUEUE};

/// A member's turn, as the group mind opens it.
pub struct Start {
    pub user_id: i32,
    /// The group, as sessions keep it (`telegram:-100123`).
    pub venue: String,
    pub session_id: String,
    /// What they said (or, saying something first, what stands in for it).
    pub input: String,
    /// The group's recent lines, oldest first.
    pub transcript: Vec<ConversationMessage>,
    pub speaker: String,
    pub chime: Option<String>,
    /// Saying something first: nobody's words to keep as theirs.
    pub first: bool,
}

/// An open turn: what she knows of them and how she is, as sections, and
/// what she is doing on her own right now, if anything.
pub struct Begun {
    pub turn: String,
    pub sections: Vec<String>,
    pub doing: Option<String>,
}

struct Open {
    db: DatabaseConnection,
    request: UserRequest,
    session_id: String,
    slot_id: u64,
    cancelled: Option<tokio::sync::oneshot::Receiver<()>>,
    lane: Option<LaneGuard>,
    budget: Option<AgentTurnBudget>,
    memory_input_at: Option<chrono::DateTime<chrono::FixedOffset>>,
}

static OPEN: LazyLock<Mutex<HashMap<String, Open>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn take(turn: &str) -> Option<Open> {
    OPEN.lock().ok()?.remove(turn)
}

fn put(turn: &str, open: Open) {
    if let Ok(mut all) = OPEN.lock() {
        all.insert(turn.to_string(), open);
    }
}

/// Open a member's turn; none when it may not be had (she is not here, their
/// line cannot be kept, the lanes are full, their budget is spent).
pub async fn begin(db: &DatabaseConnection, start: Start) -> Option<Begun> {
    // She is on no end while she moves, and not here while she lives on
    // another: every Chat turn is refused.
    crate::services::persona_sync::lease::refuse_while_away().ok()?;
    let user_id = start.user_id;
    let session_id = start.session_id.clone();
    let turn = format!("run_{}", uuid::Uuid::new_v4().simple());
    let claim = super::turn::claim_speaking_turn(user_id, &session_id, &turn, false).await;
    let finish = |slot_id| super::turn::finish_chat_turn(user_id, &session_id, slot_id);
    // Saying something first, the words that stand in for theirs were never
    // said by anyone: they are not kept as theirs.
    if !start.first
        && super::sessions::require_user_message_persisted(
            super::sessions::persist_user_message(db, &session_id, &start.input).await,
        )
        .is_err()
    {
        finish(claim.slot_id).await;
        return None;
    }
    let lane_key = super::queue::LaneQueue::make_lane_key(user_id, Some(&session_id));
    let mut cancelled = claim.cancelled;
    let acquired = tokio::select! {
        biased;
        _ = &mut cancelled => None,
        result = CHAT_LANE_QUEUE.acquire_timeout(
            &lane_key,
            std::time::Duration::from_secs(super::queue::LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS),
        ) => Some(result),
    };
    let Some(Ok(lane)) = acquired else {
        finish(claim.slot_id).await;
        return None;
    };
    let mut context = super::run::build_request_context(ProcessContext {
        mode: Some(AgentInteractionMode::Chat),
        session_id: Some(session_id.clone()),
        ..Default::default()
    });
    context.lane_key = Some(lane_key);
    context.session_id = Some(session_id.clone());
    context.conversation_history = Some(start.transcript.clone());
    context.venue = Some(start.venue.clone());
    context.chime = start.chime.clone();
    context.speaker = Some(start.speaker.clone());
    context.nobody_said = start.first;
    context.run_id = Some(turn.clone());
    let request = UserRequest {
        raw_input: start.input.clone(),
        timestamp: crate::clock::now(),
        user_id,
        context: Some(context),
    };
    let budget = match AgentTurnBudget::reserve(
        db,
        user_id,
        "agent.process_with_progress",
        turn.clone(),
        AgentTurnKind::Fresh,
    )
    .await
    {
        Ok(budget) => budget,
        Err(_) => {
            finish(claim.slot_id).await;
            return None;
        }
    };
    let (sections, memory_input_at) = budget
        .scope(async {
            super::consciousness::remember_turn_presence(&request);
            super::merope::mark_activity(db, user_id, "thinking").await;
            let memory_input_at = super::merope::note_user_turn(
                db,
                &request,
                super::motion_overlay::utterance_index_in_session(&request),
            )
            .await
            .map(|(_, at)| at);
            let sections = super::merope::speaking_prompt_in_group(
                user_id,
                &request.raw_input,
                &start.venue,
                Some(&start.speaker),
            )
            .await;
            (sections, memory_input_at)
        })
        .await;
    let doing = super::merope::doing::current()
        .map(|doing| super::merope::doing::now_line(&doing, crate::clock::now()));
    put(
        &turn,
        Open {
            db: db.clone(),
            request,
            session_id: session_id.clone(),
            slot_id: claim.slot_id,
            cancelled: Some(cancelled),
            lane: Some(lane),
            budget: Some(budget),
            memory_input_at,
        },
    );
    Some(Begun {
        turn,
        sections,
        doing,
    })
}

/// Ask her, within the open turn, what she says to the whole `prompt`: the
/// raw text (empty when she gave nothing), or how it failed.
pub async fn say(turn: &str, prompt: &str) -> Result<String, &'static str> {
    let Some(mut open) = take(turn) else {
        return Err("failed");
    };
    let agent = Agent::new(open.db.clone()).await;
    let budget = open.budget.take();
    let mut cancelled = open.cancelled.take();
    let asked = async {
        match &budget {
            Some(budget) => {
                budget
                    .scope(agent.stream_group_reply(&open.request, prompt))
                    .await
            }
            None => agent.stream_group_reply(&open.request, prompt).await,
        }
    };
    // A newer turn of theirs, or a stop, cuts this one off.
    let said = match cancelled.as_mut() {
        Some(cancelled) => tokio::select! {
            biased;
            _ = cancelled => Err("superseded"),
            said = asked => said,
        },
        None => asked.await,
    };
    open.budget = budget;
    open.cancelled = cancelled;
    put(turn, open);
    said
}

/// Run `work` within the open turn `turn`, when it is one: what it asks a
/// model is the member's turn's, as a game judged in their Chat turn was.
pub async fn within<F: std::future::Future>(turn: Option<&str>, work: F) -> F::Output {
    let Some(mut open) = turn.and_then(take) else {
        return work.await;
    };
    let budget = open.budget.take();
    let turn = turn.unwrap_or_default().to_string();
    put(&turn, open);
    let done = match &budget {
        Some(budget) => budget.scope(work).await,
        None => work.await,
    };
    if let Some(mut open) = take(&turn) {
        open.budget = budget;
        put(&turn, open);
    }
    done
}

/// Close the turn. With what she said (a sticker she picked as the chat app
/// sends it), it is kept and taken in, and her words as they stand are
/// given back; without, it is left, nothing of hers kept.
pub async fn end(turn: &str, said: Option<String>, sticker: Option<Value>) -> Option<String> {
    let mut open = take(turn)?;
    let db = open.db.clone();
    let request = open.request.clone();
    let user_id = request.user_id;
    let budget = open.budget.take();
    let reply = match said {
        None => {
            super::merope::mark_activity(&db, user_id, "idle").await;
            None
        }
        Some(said) => {
            let published = async {
                let (reply, _) = super::delegate::split(&said);
                super::process_chat::published_reply(&db, &request, &reply).await
            };
            Some(match &budget {
                Some(budget) => budget.scope(published).await,
                None => published.await,
            })
        }
    };
    drop(open.lane.take());
    super::turn::finish_chat_turn(user_id, &open.session_id, open.slot_id).await;
    match &reply {
        // What of it goes out is kept once it has ([`kept`]).
        Some(reply) => hold(
            turn,
            Said {
                db,
                request,
                session_id: open.session_id.clone(),
                memory_input_at: open.memory_input_at,
                budget,
                reply: reply.clone(),
                sticker,
            },
        ),
        None => {
            if let Some(budget) = budget {
                budget.settle(&db).await;
            }
        }
    }
    reply
}

/// A closed turn's words, waiting to be kept until it is known what of them
/// went out.
struct Said {
    db: DatabaseConnection,
    request: UserRequest,
    session_id: String,
    memory_input_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    budget: Option<AgentTurnBudget>,
    reply: String,
    sticker: Option<Value>,
}

static SAID: LazyLock<Mutex<HashMap<String, Said>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

/// How long closed words wait to hear what went out; a turn the group mind
/// dropped meanwhile (a restart) keeps nothing of them.
const SAID_WAITS: std::time::Duration = std::time::Duration::from_secs(15 * 60);

fn hold(turn: &str, said: Said) {
    if let Ok(mut all) = SAID.lock() {
        all.insert(turn.to_string(), said);
    }
    let turn = turn.to_string();
    tokio::spawn(async move {
        tokio::time::sleep(SAID_WAITS).await;
        kept(&turn, "", false).await;
    });
}

/// What of a closed turn went out: only that is kept in their session and
/// taken into her memory, and her thoughts after it run on it. Words that
/// never reached the group were not said: nothing of them is kept.
pub async fn kept(turn: &str, went_out: &str, sticker_went: bool) {
    let Some(said) = SAID.lock().ok().and_then(|mut all| all.remove(turn)) else {
        return;
    };
    let Said {
        db,
        request,
        session_id,
        memory_input_at,
        budget,
        reply,
        sticker,
    } = said;
    let user_id = request.user_id;
    let went_out = went_out.trim();
    let sticker = sticker.filter(|_| sticker_went);
    if went_out.is_empty() && sticker.is_none() {
        if let Some(budget) = budget {
            budget.settle(&db).await;
        }
        return;
    }
    // Her words as published, cut to what of them went out.
    let reply = if went_out.is_empty() || went_out == reply.trim() {
        reply
    } else {
        went_out.to_string()
    };
    let remember = async {
        if !reply.trim().is_empty() {
            super::merope::enqueue_chat_remember(
                &db,
                user_id,
                request.raw_input.clone(),
                reply.clone(),
                memory_input_at,
                super::merope::audience_for(&request),
                super::merope::turn_context(&request),
            )
            .await;
            super::merope::spawn_inner_after(db.clone(), &request, &reply);
            super::merope::spawn_curiosity(
                user_id,
                request.raw_input.clone(),
                reply.clone(),
                super::merope::audience_for(&request),
                super::merope::turn_context(&request),
            );
        }
        super::merope::mark_activity(&db, user_id, "idle").await;
    };
    match &budget {
        Some(budget) => budget.scope(remember).await,
        None => remember.await,
    }
    let data = super::process_chat::group_reply_data(&db, &request, &reply, sticker).await;
    let metadata = json!({
        "suggestions": Vec::<String>::new(),
        "dataDisplay": Value::Null,
        "frontendAction": Value::Null,
        "data": data,
        "runId": turn,
        "taskId": Value::Null,
        "task": Value::Null,
    });
    if let Err(error) =
        super::sessions::persist_assistant_message(&db, &session_id, None, &reply, Some(metadata))
            .await
    {
        tracing::warn!("[Group] Failed to persist her group reply: {error}");
    }
    if let Some(budget) = budget {
        budget.settle(&db).await;
    }
}
