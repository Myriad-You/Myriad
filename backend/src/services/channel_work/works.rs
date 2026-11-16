//! The pieces of Work she hands off in one private chat.
//!
//! Each piece is a lane of its own, kept under `{chat}#{n}`: its own Work
//! session, run, pending prompt and outbox, so delivery, retries and
//! recovery work per piece exactly as they did for the chat's one Work. The
//! chat's own record keeps her chat with them. Several can be under way at
//! once; how many is hers to weigh, from how long each has taken.
//!
//! What decides where something goes is the shared core's
//! (`myriad_merope::handoff`: `route`, `act`, `labelled`, `line`, …), the
//! same rules every end runs; keeping, running and delivering pieces is
//! here.

use super::*;
use crate::services::agent::types::TaskStatus;
pub(crate) use myriad_merope::handoff::{
    Act, Begun, Facts, HandOff, Piece, Route, Standing, act, labelled, line, next_number, route,
    shown, stands, work_input,
};

const LANE: char = '#';

pub(crate) fn lane_key(chat: &str, number: u32) -> String {
    format!("{chat}{LANE}{number}")
}

/// The chat and number of a piece's key; none for a chat's own key.
pub(crate) fn split_lane(key: &str) -> Option<(&str, u32)> {
    let (chat, number) = key.rsplit_once(LANE)?;
    Some((chat, number.parse().ok()?))
}

/// The chat a key belongs to, a piece's or its own.
pub(crate) fn chat_of(key: &str) -> &str {
    split_lane(key).map_or(key, |(chat, _)| chat)
}

/// A yes/no question, such as the confirmation before a write.
pub(crate) fn is_confirmation(prompt: &PendingPrompt) -> bool {
    matches!(
        &prompt.kind,
        PendingKind::Answer { question_type, .. }
            if matches!(question_type.as_str(), "confirmation" | "confirm")
    )
}

/// What a resumed task is given: a confirmation takes `yes` or `no` and
/// nothing else, whatever words it was confirmed in.
pub(crate) fn task_answer(answer: String, confirmed: Option<bool>) -> String {
    match confirmed {
        Some(true) => "yes".into(),
        Some(false) => "no".into(),
        None => answer,
    }
}

/// Her answer to a piece's choice question, as the choice she means.
pub(crate) fn her_choice(prompt: &PendingPrompt, text: &str) -> String {
    if is_confirmation(prompt) {
        return confirmation_reply(prompt, text);
    }
    myriad_merope::handoff::her_choice(&prompt.options, text)
}

/// A reply to a confirmation, hers or theirs: the number it is listed under
/// (`1. Yes`), read as any choice number is (`choice_by_number`), is that
/// yes or no. Nothing else is read into it, so a no, a maybe or
/// `yesterday` never reads as yes; the words go as written, and only a
/// plain yes or no takes.
pub(crate) fn confirmation_reply(prompt: &PendingPrompt, text: &str) -> String {
    myriad_agent_rules::channel::choice_by_number(&prompt.options, text).map_or_else(
        || text.trim().to_string(),
        |index| prompt.options[index].value.clone(),
    )
}

/// A piece's question carries its number in its id, and so in its buttons.
pub(crate) fn stamp(number: u32, mut prompt: PendingPrompt) -> PendingPrompt {
    ensure_pending_id(&mut prompt);
    if number_of_prompt(&prompt.id).is_none() {
        prompt.id = format!("{number}-{}", prompt.id);
    }
    prompt
}

pub(crate) fn number_of_prompt(id: &str) -> Option<u32> {
    id.split_once('-')?.0.parse().ok()
}

/// One piece as kept.
pub(super) struct Lane {
    pub(super) number: u32,
    pub(super) key: String,
    pub(super) stored: StoredSession,
}

/// The pieces of this chat as kept, in order.
pub(super) async fn lanes(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    chat: &str,
) -> Vec<Lane> {
    let rows = shared_registry::list(db, platform.session_ns(), Some(user_id), None)
        .await
        .unwrap_or_default();
    let mut lanes: Vec<Lane> = rows
        .into_iter()
        .filter_map(|row| {
            let (of, number) = split_lane(&row.record_id)?;
            if of != chat {
                return None;
            }
            Some(Lane {
                number,
                key: row.record_id.clone(),
                stored: serde_json::from_value(row.payload).ok()?,
            })
        })
        .collect();
    lanes.sort_by_key(|lane| lane.number);
    lanes
}

/// How each piece of this chat stands now, with its lane.
pub(super) async fn pieces(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    chat: &str,
) -> Vec<(Lane, Piece)> {
    let lanes = lanes(db, platform, user_id, chat).await;
    if lanes.is_empty() {
        return Vec::new();
    }
    let tasks = Agent::new(db.clone()).await.get_user_tasks(user_id).await;
    let mut pieces = Vec::with_capacity(lanes.len());
    for lane in lanes {
        let live = tasks.iter().find(|task| {
            is_cancellable_task_status(&task.status)
                && session_id_from_lane_id(task.lane_id.as_deref()).as_deref()
                    == Some(lane.stored.session_id.as_str())
        });
        let facts = Facts {
            stopped: lane.stored.stopped,
            question: still_asked(db, platform, user_id, &lane.key)
                .await
                .map(|pending| format_pending_prompt(&pending.prompt)),
            // A restart leaves its run on a question the chat never heard.
            unheard_question: live.is_some_and(|task| {
                task.status == TaskStatus::WaitingForInput && task.pending_question.is_some()
            }),
            under_way: live.is_some() || is_active(&lane.key).await,
            progress: live.map(|task| task.progress),
            ..Facts::default()
        };
        let mut standing = stands(&facts);
        let mut ended_at = None;
        if matches!(standing, Standing::Done { .. }) {
            // Every run of it is kept as its last (`start_delivery`); only a
            // stop lets go of it.
            standing = if lane.stored.last_run_id.is_none() {
                stands(&Facts {
                    begun: not_begun(lane.stored.started_at),
                    ..facts
                })
            } else {
                let back = came_back(db, &lane.stored.session_id).await;
                ended_at = back.at;
                Standing::Done {
                    result: back.result,
                }
            };
        }
        let piece = Piece {
            number: lane.number,
            asked: lane.stored.asked.clone(),
            started_at: lane.stored.started_at,
            ended_at,
            standing,
        };
        pieces.push((lane, piece));
    }
    pieces
}

/// The question kept for a piece, while its task still asks it. A wait
/// that runs out (`work_loop::recover`) fails the task and takes its
/// question away; the one kept on the chat goes with it, so the piece does
/// not stand waiting on an answer nothing will take.
async fn still_asked(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    key: &str,
) -> Option<StoredPending> {
    let pending = load_pending(db, platform, key).await?;
    if let PendingKind::Answer {
        task_id,
        question_id,
        ..
    } = &pending.prompt.kind
        && let Some(task) =
            crate::services::agent::executor::get_task_for_user(task_id, user_id).await
        && !(task.status == TaskStatus::WaitingForInput
            && task
                .pending_question
                .as_ref()
                .is_some_and(|question| &question.question_id == question_id))
    {
        clear_pending(db, platform, key).await;
        return None;
    }
    Some(pending)
}

/// How long a piece handed off may go without its run before it is one
/// that never began.
const BEGINS_WITHIN: i64 = 30;

/// A piece no run was ever started for: handed off a moment ago, or one
/// whose run never began.
pub(crate) fn not_begun(started_at: Option<i64>) -> Begun {
    let now = crate::clock::now().timestamp();
    if started_at.is_some_and(|at| now - at < BEGINS_WITHIN) {
        Begun::NotYet
    } else {
        Begun::Never
    }
}

/// What came of a piece's Work session.
#[derive(Default)]
struct Back {
    /// What the helper last said, when it said the last word.
    result: Option<String>,
    /// When, Unix seconds.
    at: Option<i64>,
}

async fn came_back(db: &DatabaseConnection, session_id: &str) -> Back {
    let history = crate::services::agent::sessions::load_session_history(db, session_id, 2, false)
        .await
        .unwrap_or_default();
    let Some(last) = history.last() else {
        return Back::default();
    };
    Back {
        result: (last.role == "assistant").then(|| last.content.clone()),
        at: last
            .created_at
            .as_deref()
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .map(|at| at.timestamp()),
    }
}

/// A new piece: its number, its own Work session, kept before it starts.
pub(super) async fn open(
    db: &DatabaseConnection,
    sink: &ChannelSink,
    user_id: i32,
    chat: &str,
    asked: &str,
) -> Option<Lane> {
    let platform = sink.platform();
    let number = next_number(
        lanes(db, platform, user_id, chat)
            .await
            .iter()
            .map(|lane| lane.number),
    )?;
    let session_id = crate::services::agent::sessions::ensure_session(
        db,
        None,
        user_id,
        AgentInteractionMode::Work,
    )
    .await
    .ok()?;
    let key = lane_key(chat, number);
    let stored = StoredSession {
        session_id,
        binding: Some(sink.binding.clone()),
        address: Some(sink.transport.address()),
        asked: asked.to_string(),
        started_at: Some(crate::clock::now().timestamp()),
        ..Default::default()
    };
    put_session(db, platform, user_id, &key, stored.clone())
        .await
        .ok()?;
    Some(Lane {
        number,
        key,
        stored,
    })
}

/// Stop one piece, and only it: its run, its tasks, what it had waiting.
pub(super) async fn stop(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    lane: &Lane,
) {
    stop_delivery(&lane.key).await;
    cancel_session_tasks(db, user_id, &lane.stored.session_id).await;
    clear_pending(db, platform, &lane.key).await;
    clear_outbound(db, platform, &lane.key).await;
    if let Some(mut stored) = load_session(db, platform, &lane.key).await {
        stored.stopped = true;
        stored.last_run_id = None;
        stored.last_event_seq = 0;
        let _ = put_session(db, platform, user_id, &lane.key, stored).await;
    }
}

/// Stop every piece and forget them: a new conversation numbers from 1.
pub(super) async fn forget_all(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    chat: &str,
) {
    for lane in lanes(db, platform, user_id, chat).await {
        stop(db, platform, user_id, &lane).await;
        let _ = shared_registry::take::<StoredSession>(db, platform.session_ns(), &lane.key).await;
    }
}

/// What a piece sends, labelled when another piece of the chat is not done.
pub(super) async fn label_for(
    db: &DatabaseConnection,
    platform: ChannelPlatform,
    user_id: i32,
    key: &str,
    content: &str,
) -> String {
    let Some((chat, number)) = split_lane(key) else {
        return content.to_string();
    };
    let mut asked = String::new();
    let mut others = false;
    for lane in lanes(db, platform, user_id, chat).await {
        if lane.number == number {
            asked = lane.stored.asked;
            continue;
        }
        if !others && !lane.stored.stopped {
            others =
                is_active(&lane.key).await || load_pending(db, platform, &lane.key).await.is_some();
        }
    }
    labelled(number, &asked, others, content)
}

/// Carry out her lines, in the order she wrote them.
#[allow(clippy::too_many_arguments)]
pub(super) async fn carry_out(
    db: &DatabaseConnection,
    user_id: i32,
    hand_offs: &[HandOff],
    their_words: &str,
    images: Option<Value>,
    sink: &ChannelSink,
    chat: &str,
) {
    let platform = sink.platform();
    for hand_off in hand_offs {
        let pieces = pieces(db, platform, user_id, chat).await;
        let plain: Vec<Piece> = pieces.iter().map(|(_, piece)| piece.clone()).collect();
        let lane_of = |work: u32| pieces.iter().find(|(lane, _)| lane.number == work);
        match act(hand_off, &plain) {
            Act::Start { text: instruction } => {
                let Some(lane) = open(db, sink, user_id, chat, &instruction).await else {
                    let _ = sink.send_text("没能开始这件事，请稍后再试。").await;
                    continue;
                };
                start_work_run(
                    db.clone(),
                    user_id,
                    lane.stored.session_id.clone(),
                    &work_input(&instruction, their_words),
                    images.clone(),
                    sink.clone(),
                    &lane.key,
                )
                .await;
            }
            Act::Answer { work, text: answer } => {
                let Some((lane, _)) = lane_of(work) else {
                    continue;
                };
                let Some(pending) = load_pending(db, platform, &lane.key).await else {
                    continue;
                };
                let answer = her_choice(&pending.prompt, &answer);
                answer_pending(
                    db,
                    user_id,
                    &lane.key,
                    lane.stored.session_id.clone(),
                    pending,
                    &answer,
                    sink.clone(),
                )
                .await;
            }
            Act::Steer { work, text } => {
                let Some((lane, _)) = lane_of(work) else {
                    continue;
                };
                steer(
                    db,
                    user_id,
                    &lane.stored.session_id,
                    &work_input(&text, their_words),
                )
                .await;
            }
            Act::Continue { work, text } => {
                let Some((lane, _)) = lane_of(work) else {
                    continue;
                };
                start_work_run(
                    db.clone(),
                    user_id,
                    lane.stored.session_id.clone(),
                    &work_input(&text, their_words),
                    images.clone(),
                    sink.clone(),
                    &lane.key,
                )
                .await;
            }
            Act::PickUp { work, text } => {
                if let Some((lane, _)) = lane_of(work) {
                    pick_up(db, user_id, lane, &work_input(&text, their_words), sink).await;
                }
            }
            Act::Stop { work } => {
                if let Some((lane, _)) = lane_of(work) {
                    stop(db, platform, user_id, lane).await;
                }
            }
            Act::Nothing => {
                info!(?hand_off, "[Channel] her line is for no piece under way");
            }
        }
    }
}

/// A piece a restart cut off goes on from where it was saved: what she says
/// answers the question its run was left on.
async fn pick_up(
    db: &DatabaseConnection,
    user_id: i32,
    lane: &Lane,
    text: &str,
    sink: &ChannelSink,
) {
    let tasks = Agent::new(db.clone()).await.get_user_tasks(user_id).await;
    let Some((task_id, question_id)) = tasks.into_iter().find_map(|task| {
        let question = task.pending_question.as_ref()?;
        (task.status == TaskStatus::WaitingForInput
            && session_id_from_lane_id(task.lane_id.as_deref()).as_deref()
                == Some(lane.stored.session_id.as_str()))
        .then(|| (task.task_id.clone(), question.question_id.clone()))
    }) else {
        warn!("[Channel] nothing left waiting to pick this piece up");
        return;
    };
    match crate::services::agent::run::answer_for_user(
        db.clone(),
        user_id,
        task_id,
        question_id,
        text.to_string(),
        Some(lane.stored.session_id.clone()),
    )
    .await
    {
        Ok(run) => {
            start_delivery(
                run,
                db.clone(),
                user_id,
                lane.key.clone(),
                sink.clone(),
                text.to_string(),
            )
            .await;
        }
        Err(error) => warn!(error = %error.to_json(), "[Channel] a cut-off piece did not pick up"),
    }
}

/// More for a running piece: into its tool loop as it is (see `work_loop`).
async fn steer(db: &DatabaseConnection, user_id: i32, session_id: &str, text: &str) -> bool {
    let tasks = Agent::new(db.clone()).await.get_user_tasks(user_id).await;
    let Some(task) = tasks.into_iter().find(|task| {
        is_cancellable_task_status(&task.status)
            && session_id_from_lane_id(task.lane_id.as_deref()).as_deref() == Some(session_id)
    }) else {
        warn!("[Channel] no task under way to take more for this piece");
        return false;
    };
    crate::services::agent::executor::enqueue_steering(db, &task.task_id, text.to_string())
        .await
        .is_ok()
}

/// The answer to a parked question: it resumes, is asked again (what they
/// said kept for the piece), or has expired.
pub(super) async fn answer_pending(
    db: &DatabaseConnection,
    user_id: i32,
    key: &str,
    session_id: String,
    pending: StoredPending,
    input: &str,
    sink: ChannelSink,
) {
    let platform = sink.platform();
    if pending
        .expected_user_id
        .is_some_and(|expected| expected != user_id)
    {
        let _ = sink.send_text(PENDING_STALE_REPLY).await;
        return;
    }
    let reply = if is_confirmation(&pending.prompt) {
        confirmation_reply(&pending.prompt, input)
    } else {
        input.to_string()
    };
    match decide_pending_reply(&pending.prompt, &reply, crate::clock::now().timestamp()) {
        PendingDecision::Reask { reply } => {
            // Not an answer, but meant for this piece: it goes on with it.
            // Not past a confirmation: what waits on it would take these
            // words as more to do once they confirm.
            if split_lane(key).is_some()
                && !is_confirmation(&pending.prompt)
                && let PendingKind::Answer { task_id, .. } = &pending.prompt.kind
            {
                let _ = crate::services::agent::executor::enqueue_steering(
                    db,
                    task_id,
                    input.to_string(),
                )
                .await;
            }
            let reply = label_for(db, platform, user_id, key, &reply).await;
            let _ = sink.send_prompt(&reply, &pending.prompt).await;
        }
        PendingDecision::Expired { reply } => {
            clear_pending(db, platform, key).await;
            // Its task's question goes too, so it is not read as one a
            // restart left behind.
            if let PendingKind::Answer {
                task_id,
                question_id,
                ..
            } = &pending.prompt.kind
            {
                let _ = crate::services::agent::work_loop::expire_question(
                    db,
                    task_id,
                    user_id,
                    question_id,
                )
                .await;
            }
            let _ = sink.send_text(&reply).await;
        }
        PendingDecision::Resume {
            kind,
            answer,
            confirmed,
        } => {
            let answer = task_answer(answer, confirmed);
            if take_pending_if_id(db, platform, key, &pending.prompt.id)
                .await
                .is_none()
            {
                let _ = sink.send_text(PENDING_STALE_REPLY).await;
                return;
            }
            resume_pending(
                db.clone(),
                session_id,
                key,
                user_id,
                kind,
                answer,
                sink,
                input,
                pending,
            )
            .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_piece_has_a_key_of_its_own_in_its_chat() {
        assert_eq!(lane_key("telegram:42", 3), "telegram:42#3");
        assert_eq!(split_lane("telegram:42#3"), Some(("telegram:42", 3)));
        assert_eq!(split_lane("telegram:42"), None);
        assert_eq!(chat_of("telegram:42#3"), "telegram:42");
        assert_eq!(chat_of("telegram:42"), "telegram:42");
        assert_eq!(next_number([]), Some(1));
        assert_eq!(next_number([1, 3, 2]), Some(4));
    }

    #[test]
    fn a_piece_whose_run_never_began_did_not_get_done() {
        let now = crate::clock::now().timestamp();
        let nothing_came = |started_at| {
            stands(&Facts {
                begun: not_begun(started_at),
                ..Facts::default()
            })
        };
        assert_eq!(nothing_came(Some(now - 5)), Standing::Starting);
        assert!(matches!(
            nothing_came(Some(now - BEGINS_WITHIN - 1)),
            Standing::Failed { why: Some(_) }
        ));
        assert!(matches!(nothing_came(None), Standing::Failed { .. }));
    }

    #[test]
    fn a_question_carries_its_piece_in_its_buttons() {
        let prompt = PendingPrompt {
            id: String::new(),
            kind: PendingKind::Answer {
                task_id: "t".into(),
                question_id: "q".into(),
                question_type: "confirmation".into(),
            },
            question: "发吗？".into(),
            options: Vec::new(),
            expires_at_unix: None,
        };
        let stamped = stamp(3, prompt);
        assert_eq!(number_of_prompt(&stamped.id), Some(3));
        assert_eq!(stamp(3, stamped.clone()).id, stamped.id);
        let rows = myriad_agent_rules::channel::telegram_inline_keyboard(&stamped);
        let data = &rows[0][0].callback_data;
        let bound = myriad_agent_rules::channel::parse_telegram_callback(data).unwrap();
        assert_eq!(number_of_prompt(&bound.prompt_id), Some(3));
        assert_eq!(
            telegram_callback_action(&stamped, data),
            TelegramCallbackAction::Resume("是".into())
        );
        // A chat's own question (from before pieces) has no number.
        assert_eq!(number_of_prompt("a1b2c3d4"), None);
    }

    #[test]
    fn her_answer_to_a_choice_is_the_choice_she_means() {
        let prompt = PendingPrompt {
            id: String::new(),
            kind: PendingKind::Answer {
                task_id: "t".into(),
                question_id: "q".into(),
                question_type: "single_choice".into(),
            },
            question: "要几点？".into(),
            options: ["18:00", "19:00"]
                .map(|time| PendingOption {
                    value: time.into(),
                    label: time.into(),
                })
                .to_vec(),
            expires_at_unix: None,
        };
        // Her choice is its number alone or its words as listed; anything
        // else is asked again, never guessed.
        let taken = |said: &str| match decide_pending_reply(&prompt, &her_choice(&prompt, said), 0)
        {
            PendingDecision::Resume { answer, .. } => Some(answer),
            _ => None,
        };
        assert_eq!(taken("2").as_deref(), Some("19:00"));
        assert_eq!(taken("19:00").as_deref(), Some("19:00"));
        assert_eq!(taken("19:00。").as_deref(), Some("19:00"));
        for said in ["2（19:00）", "订 19:00 的", "19点", "18:00 或 19:00", "01"] {
            assert_eq!(taken(said), None, "{said}");
        }
    }

    #[test]
    fn a_confirmation_is_answered_yes_or_no() {
        // The Work loop takes nothing else for a write's confirmation.
        assert_eq!(task_answer("是".into(), Some(true)), "yes");
        assert_eq!(task_answer("否".into(), Some(false)), "no");
        assert_eq!(task_answer("19:00".into(), None), "19:00");
        let confirm = PendingPrompt {
            id: String::new(),
            kind: PendingKind::Answer {
                task_id: "t".into(),
                question_id: "q".into(),
                question_type: "confirmation".into(),
            },
            question: "发吗？".into(),
            options: Vec::new(),
            expires_at_unix: None,
        };
        assert!(is_confirmation(&confirm));
        let PendingDecision::Resume {
            answer, confirmed, ..
        } = decide_pending_reply(&confirm, "好", 0)
        else {
            panic!("a yes resumes it");
        };
        assert_eq!(task_answer(answer, confirmed), "yes");
        assert!(!is_confirmation(&PendingPrompt {
            kind: PendingKind::Clarify {
                original_input: "x".into(),
            },
            ..confirm
        }));
    }
}
