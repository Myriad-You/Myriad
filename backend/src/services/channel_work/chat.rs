//! A private IM message is a chat with her.
//!
//! She answers as herself (Chat): with her memory of them, her own time and
//! views, and the photos they send. When they ask her to get something done
//! she hands it off, and the channel starts it as their Work, a piece of its
//! own (see `works`), so questions, confirmations, delivery and recovery work
//! for each as they did for one. She can keep talking while pieces run, hand
//! off another, pass on more for one or stop it, and each time they talk she
//! sees every piece: what it is, how far it got, what it waits for.
//!
//! Her replies are best effort, like a group reply: a restart mid-turn loses
//! that one reply. Work keeps its own ledger. What came while she was asleep
//! is kept until she wakes, restart or not, where she can answer later.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use futures::StreamExt;
use myriad_agent_rules::channel::{ChannelImageRef, split_channel_text};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::Value;
use tracing::warn;

use crate::services::agent::chat_prompt::{CUT_OFF_PART_SENT, CUT_OFF_UNSENT};
use crate::services::agent::run::{ProcessContext, ProcessRequest};
use crate::services::agent::delegate::HandOff;
use crate::services::agent::types::ChannelChat;
use crate::services::agent::{AgentInteractionMode, AgentProgressEvent};

use super::chat_images::with_model_images;
use super::chat_waiting::{can_answer_later, forget_waiting, keep_waiting};
use super::{ChannelSink, cache_inbound_images, load_session, put_session};

/// Longest she takes over one reply before giving up on it.
/// How long she shows typing while she thinks.
const TURN_DEADLINE: Duration = Duration::from_secs(90);
/// How long a slow reply is still worth sending: late, as anyone answers late.
const LATE_REPLY: Duration = Duration::from_secs(600);
const TYPING_EVERY: Duration = Duration::from_secs(4);
/// She is in talk with someone this long after they last wrote to her, or
/// she to them, anywhere (the site, a group, any app they paired): she sees
/// what they send at once.
const IN_TALK: Duration = Duration::from_secs(10 * 60);

/// Messages of a chat she has not seen yet, read together once she does.
pub(super) struct Unseen {
    pub(super) texts: Vec<String>,
    pub(super) images: Vec<ChannelImageRef>,
}

pub(super) static UNSEEN: LazyLock<Mutex<HashMap<String, Unseen>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// What came lately in each chat, numbered in the order it came, whether or
/// not a turn has taken it up yet: each turn knows up to which it saw, and
/// what came after is what came while she answered (see `came_meanwhile`).
static CAME: LazyLock<Mutex<HashMap<String, std::collections::VecDeque<(u64, String)>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
static CAME_COUNT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Lines kept per chat for that.
const CAME_KEPT: usize = 32;
/// When she last answered each person in a private chat, by site user.
static LAST_REPLY: LazyLock<Mutex<HashMap<i32, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const CHATS_KEPT: usize = 4096;

/// Whatever happens to the turn, what it was to read is let go, so the
/// next message is seen afresh.
struct Seeing<'a>(&'a str);

impl Drop for Seeing<'_> {
    fn drop(&mut self) {
        if let Ok(mut unseen) = UNSEEN.lock() {
            unseen.remove(self.0);
        }
    }
}

/// Keep a message she has not seen: true when it is the first unseen one
/// in the chat, and so the one to wait on; the rest she reads with it.
pub(super) fn take_in(session_key: &str, input: &str, images: &[ChannelImageRef]) -> bool {
    if let Ok(mut came) = CAME.lock()
        && (came.len() < CHATS_KEPT || came.contains_key(session_key))
    {
        let number = CAME_COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
        let lines = came.entry(session_key.to_string()).or_default();
        lines.push_back((number, input.to_string()));
        while lines.len() > CAME_KEPT {
            lines.pop_front();
        }
    }
    let Ok(mut unseen) = UNSEEN.lock() else {
        return false;
    };
    if let Some(waiting) = unseen.get_mut(session_key) {
        waiting.texts.push(input.to_string());
        waiting.images.extend_from_slice(images);
        return false;
    }
    if unseen.len() >= CHATS_KEPT {
        return false;
    }
    unseen.insert(
        session_key.to_string(),
        Unseen {
            texts: vec![input.to_string()],
            images: images.to_vec(),
        },
    );
    true
}

/// A message comes in a private chat. She sees it when she would (see
/// `merope::timing`): asleep, when she wakes, where she can answer then,
/// with what came kept across a restart; otherwise it wakes her. What they
/// send meanwhile she reads with it, and answers once.
#[allow(clippy::too_many_arguments)]
pub(super) async fn start_chat_turn(
    db: DatabaseConnection,
    user_id: i32,
    work_session_id: String,
    input: &str,
    images: &[ChannelImageRef],
    sink: ChannelSink,
    session_key: &str,
) {
    use crate::services::agent::merope::group::timing;
    let may_sleep = can_answer_later(sink.platform());
    // What waits must be answerable after a restart: where, and paired with whom.
    if may_sleep {
        super::remember_sink(&db, user_id, session_key, &sink).await;
    }
    // Where she can answer later, what she has not answered survives a
    // restart: asleep or awake, before she reads it or while she answers.
    if !take_in(session_key, input, images) {
        // Read with the first; kept with it.
        if may_sleep {
            keep_waiting(&db, session_key, user_id, &work_session_id).await;
        }
        return;
    }
    let talking = LAST_REPLY
        .lock()
        .ok()
        .and_then(|last| last.get(&user_id).copied())
        .is_some_and(|at| at.elapsed() < IN_TALK)
        || wrote_lately(&db, user_id).await;
    let at = timing::where_she_is(talking, may_sleep);
    if may_sleep {
        keep_waiting(&db, session_key, user_id, &work_session_id).await;
    }
    let wait = timing::until_read(at, input);
    tracing::info!(
        ?at,
        seconds = wait.as_secs(),
        "[Channel] she will see the message"
    );
    // She waits apart from the chat's lock, so what they send meanwhile comes
    // in and is read with it; she answers holding it.
    let session_key = session_key.to_string();
    tokio::spawn(async move {
        let seeing = Seeing(&session_key);
        tokio::time::sleep(wait).await;
        let Some(seen) = UNSEEN
            .lock()
            .ok()
            .and_then(|mut unseen| unseen.remove(&session_key))
        else {
            return;
        };
        // What they send after this came while she waits her turn in the
        // chat and while she answers.
        let saw = saw_upto(&session_key);
        drop(seeing);
        let input = seen
            .texts
            .iter()
            .map(|text| text.trim())
            .filter(|text| !text.is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        super::runtime::with_chat_lock(
            &session_key,
            answer(
                db.clone(),
                user_id,
                &input,
                &seen.images,
                sink,
                &session_key,
                saw,
            ),
        )
        .await;
        // Answered (or given up on): no longer waiting. What came while she
        // answered is waiting on its own turn, kept again as it came in.
        if !UNSEEN
            .lock()
            .is_ok_and(|unseen| unseen.contains_key(&session_key))
        {
            forget_waiting(&db, &session_key).await;
        }
        if let Ok(mut last) = LAST_REPLY.lock() {
            if last.len() >= CHATS_KEPT {
                last.retain(|_, at| at.elapsed() < IN_TALK);
            }
            last.insert(user_id, Instant::now());
        }
    });
}

/// The number of the latest line that came in this chat (0 for none): what
/// she sees when she looks, what comes after came while she answered.
fn saw_upto(session_key: &str) -> u64 {
    CAME.lock()
        .ok()
        .and_then(|came| {
            came.get(session_key)
                .and_then(|lines| lines.back().map(|(number, _)| *number))
        })
        .unwrap_or(0)
}

/// What they sent in this chat after the line numbered `saw`, the last she
/// saw before answering, oldest first: it is answered on its own turn after
/// this one.
fn came_meanwhile(session_key: &str, saw: u64) -> Vec<String> {
    CAME.lock()
        .ok()
        .and_then(|came| {
            came.get(session_key).map(|lines| {
                lines
                    .iter()
                    .filter(|(number, _)| *number > saw)
                    .map(|(_, text)| text.clone())
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// Whether this person wrote to her anywhere in the last few minutes.
async fn wrote_lately(db: &DatabaseConnection, user_id: i32) -> bool {
    crate::services::agent::merope::get_or_create_state(db, user_id)
        .await
        .ok()
        .and_then(|state| state.last_user_message_at)
        .and_then(|at| {
            (crate::clock::now() - at.with_timezone(&chrono::Utc))
                .to_std()
                .ok()
        })
        .is_some_and(|age| age < IN_TALK)
}

#[allow(clippy::too_many_arguments)]
async fn answer(
    db: DatabaseConnection,
    user_id: i32,
    input: &str,
    images: &[ChannelImageRef],
    sink: ChannelSink,
    session_key: &str,
    saw: u64,
) {
    let platform = sink.platform();
    // She left the site while this waited for her (asleep, about to read
    // it): held for when she is back, not started and refused (D3).
    if super::held::away() {
        super::held::hold(&db, &sink, session_key, user_id, input, images).await;
        return;
    }
    sink.send_typing().await;
    let began = Instant::now();
    let Some(chat_session_id) = chat_session(&db, platform, user_id, session_key).await else {
        let _ = sink.send_text("无法保存通道会话，请稍后重试。").await;
        return;
    };
    let chat_session_id_for_mark = chat_session_id.clone();
    let cached = match cache_inbound_images(&db, user_id, &sink.transport, images).await {
        Ok(data) => data,
        Err(message) => {
            let _ = sink.send_text(&message).await;
            return;
        }
    };
    let now = crate::clock::now().timestamp();
    let pieces = super::works::shown(
        super::works::pieces(&db, platform, user_id, session_key)
            .await
            .into_iter()
            .map(|(_, piece)| piece)
            .collect(),
        now,
    );
    let room = their_typing(&db, &chat_session_id).await;
    let input_text = if input.trim().is_empty() && cached.is_some() {
        "（发来了图片）".to_string()
    } else {
        input.to_string()
    };
    let run = match crate::services::agent::run::start_for_user(
        db.clone(),
        user_id,
        ProcessRequest {
            input: input_text,
            context: Some(ProcessContext {
                mode: Some(AgentInteractionMode::Chat),
                session_id: Some(chat_session_id),
                custom_data: with_model_images(cached.as_ref()).await,
                channel_chat: Some(ChannelChat {
                    works: pieces
                        .iter()
                        .map(|piece| super::works::line(piece, now))
                        .collect(),
                    waiting: pieces
                        .iter()
                        .filter(|piece| matches!(piece.standing, super::works::Standing::Waiting { .. }))
                        .count(),
                    came_while_away: super::held::note_for(session_key),
                }),
                ..Default::default()
            }),
        },
    )
    .await
    {
        Ok(run) => run,
        Err(error) => {
            let body = error.to_json();
            // A move began as she was about to answer: held, not refused.
            if super::held::is_away_refusal(&body) {
                super::held::hold(&db, &sink, session_key, user_id, input, images).await;
                return;
            }
            let message = body
                .get("message")
                .and_then(Value::as_str)
                .or_else(|| body.get("error").and_then(Value::as_str))
                .unwrap_or("她暂时回不了话，请稍后再试。")
                .to_string();
            warn!(error = %message, "channel chat start failed");
            let _ = sink.send_text(&message).await;
            return;
        }
    };
    let run_id = run.run_id().to_string();
    let Some(response) = await_reply(run, &sink).await else {
        // Whatever she ended up with did not go out: she should know.
        mark_delivery(&db, &chat_session_id_for_mark, &run_id, CUT_OFF_UNSENT).await;
        return;
    };
    let message = response
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    // Most turns go as one message, and a few in a row when something grabs
    // her, typed the way they type to her, typing each first.
    let lines = myriad_agent_rules::channel::as_messages(message);
    let most = myriad_merope::talk_shape::messages_this_turn(lines.len(), rand::random::<f64>());
    let mut sent_any = false;
    // What they sent while she wrote it, already weighed.
    let mut looked = 0;
    for (index, line) in myriad_merope::talk_shape::goes_out_as(&lines, most, room.as_ref())
        .into_iter()
        .enumerate()
    {
        // Typing takes as long as it takes; the first she was already at
        // while she thought.
        let typing = crate::services::agent::merope::group::timing::typing(&line);
        let left = if index == 0 {
            typing.saturating_sub(began.elapsed())
        } else {
            typing
        };
        if !left.is_zero() {
            sink.send_typing().await;
            tokio::time::sleep(left).await;
        }
        // The talk may have moved on while she wrote: before anything has
        // gone, she looks at what came and keeps it back if it no longer
        // fits; once she is under way, their speaking stops her there.
        let came = came_meanwhile(session_key, saw);
        if came.len() > looked {
            if sent_any {
                mark_delivery(&db, &chat_session_id_for_mark, &run_id, CUT_OFF_PART_SENT).await;
                return;
            }
            let came_now: Vec<(String, String)> = came[looked..]
                .iter()
                .map(|text| ("them".to_string(), text.clone()))
                .collect();
            if !crate::services::agent::merope::draft::still_fits(
                user_id, input, message, &came_now,
            )
            .await
            {
                mark_delivery(&db, &chat_session_id_for_mark, &run_id, CUT_OFF_UNSENT).await;
                return;
            }
            looked = came.len();
        }
        for chunk in split_channel_text(&line, sink.text_limit()) {
            if sink.send_text(&chunk).await.is_err() {
                let kind = if sent_any {
                    CUT_OFF_PART_SENT
                } else {
                    CUT_OFF_UNSENT
                };
                mark_delivery(&db, &chat_session_id_for_mark, &run_id, kind).await;
                return;
            }
            sent_any = true;
        }
    }
    // A sticker she chose goes after her words; one she is making, when it
    // is made, while she carries on.
    if let Some(chosen) = response.pointer("/data/sticker").cloned() {
        let (db, sink) = (db.clone(), sink.clone());
        tokio::spawn(async move {
            send_sticker(&db, &sink, &chosen).await;
        });
    }
    // What she handed off starts as a piece of its own; what she passed on
    // for a piece goes to it.
    let hand_offs: Vec<HandOff> = response
        .pointer("/data/handOffs")
        .cloned()
        .and_then(|value| serde_json::from_value(value).ok())
        .unwrap_or_default();
    if !hand_offs.is_empty() {
        super::works::carry_out(&db, user_id, &hand_offs, input, cached, &sink, session_key).await;
    }
}

/// Send the sticker she chose, making it first if it is a new one.
async fn send_sticker(db: &DatabaseConnection, sink: &ChannelSink, chosen: &Value) {
    use crate::services::agent::merope::group::stickers;
    if chosen.get("make").is_some() {
        sink.send_typing().await;
    }
    let Some(sticker) = stickers::resolve(db, chosen, None).await else {
        return;
    };
    let Some((png, _)) = stickers::picture(&sticker).await else {
        return;
    };
    match sink.transport.send_sticker(png).await {
        Ok(()) => stickers::sent(db, &sticker).await,
        Err(_) => warn!("channel sticker not sent"),
    }
}

/// Her chat session with them in this conversation, made on first use.
pub(super) async fn chat_session(
    db: &DatabaseConnection,
    platform: crate::services::channel_platform::ChannelPlatform,
    user_id: i32,
    session_key: &str,
) -> Option<String> {
    let mut stored = load_session(db, platform, session_key).await?;
    if let Some(id) = stored.chat_session_id.clone().filter(|id| !id.is_empty()) {
        return Some(id);
    }
    let id = crate::services::agent::sessions::ensure_session(
        db,
        None,
        user_id,
        AgentInteractionMode::Chat,
    )
    .await
    .ok()?;
    stored.chat_session_id = Some(id.clone());
    put_session(db, platform, user_id, session_key, stored)
        .await
        .ok()?;
    Some(id)
}

/// Their lines in this chat lately, as many as are looked at for how they
/// type to her.
const THEIR_LINES_LOOKED_AT: u64 = 40;

/// How they type to her in this chat, once they have said enough of it:
/// what each of their turns went in as, a line a message.
async fn their_typing(
    db: &DatabaseConnection,
    chat_session_id: &str,
) -> Option<myriad_merope::talk_shape::Shape> {
    let history = crate::services::agent::sessions::load_session_history(
        db,
        chat_session_id,
        THEIR_LINES_LOOKED_AT,
        false,
    )
    .await
    .ok()?;
    let at = |message: &crate::services::agent::ConversationMessage| {
        message
            .created_at
            .as_deref()
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .map_or(0, |at| at.timestamp())
    };
    let lines: Vec<(&str, i64, &str)> = history
        .iter()
        .filter(|message| message.role == "user")
        .flat_map(|message| {
            let at = at(message);
            message.content.lines().map(move |line| ("them", at, line))
        })
        .collect();
    myriad_merope::talk_shape::room_of(&lines)
}

/// What became of her reply in the chat app, kept on her line of it: she
/// reads it back as said only if it went out.
async fn mark_delivery(db: &DatabaseConnection, chat_session_id: &str, run_id: &str, kind: &str) {
    let marked = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_messages \
             SET metadata = (COALESCE(metadata::jsonb, '{}'::jsonb) || jsonb_build_object($3::text, $4::text))::json \
             WHERE session_id = $1 AND role = 'assistant' AND metadata ->> 'runId' = $2",
            [
                chat_session_id.into(),
                run_id.into(),
                crate::services::agent::chat_prompt::CUT_OFF_KEY.into(),
                kind.into(),
            ],
        ))
        .await;
    if let Err(error) = marked {
        warn!(%error, kind, "her reply did not go out, and that was not kept");
    }
}

async fn await_reply(
    run: std::sync::Arc<crate::services::agent::run_hub::AgentRun>,
    sink: &ChannelSink,
) -> Option<Value> {
    let mut events = Box::pin(crate::services::agent::run::agent_run_envelopes(run));
    let mut typing = tokio::time::interval(TYPING_EVERY);
    let deadline = tokio::time::sleep(LATE_REPLY);
    tokio::pin!(deadline);
    let began = Instant::now();
    loop {
        tokio::select! {
            _ = &mut deadline => return None,
            // Typing shows while she thinks; after that she is just slow.
            _ = typing.tick() => if began.elapsed() < TURN_DEADLINE {
                sink.send_typing().await;
            },
            envelope = events.next() => match envelope?.event {
                AgentProgressEvent::TaskCompleted { success, response, .. } => {
                    return success.then_some(*response);
                }
                AgentProgressEvent::Error { .. } => return None,
                _ => {}
            },
        }
    }
}

#[cfg(test)]
mod seeing_tests {
    use super::*;

    #[test]
    fn what_they_send_before_she_looks_is_read_together() {
        let key = "test:seeing:1";
        assert!(take_in(key, "在吗", &[]));
        assert!(!take_in(key, "问你个事", &[]));
        assert!(!take_in(key, "", &[]));
        let seeing = Seeing(key);
        let seen = UNSEEN.lock().unwrap().remove(key).unwrap();
        assert_eq!(seen.texts, ["在吗", "问你个事", ""]);
        drop(seeing);
        // Seen: the next message is waited on afresh.
        assert!(take_in(key, "人呢", &[]));
        // A turn that ends any way lets go of what it was to read.
        drop(Seeing(key));
        assert!(UNSEEN.lock().unwrap().get(key).is_none());
    }

    #[test]
    fn what_came_while_she_answered_is_what_came_after_she_saw() {
        let key = "test:came:1";
        take_in(key, "在吗", &[]);
        take_in(key, "问你个事", &[]);
        let saw = saw_upto(key);
        assert!(came_meanwhile(key, saw).is_empty());
        take_in(key, "算了不用了", &[]);
        assert_eq!(came_meanwhile(key, saw), ["算了不用了"]);
        // A later turn seeing it does not take it from this one.
        let later = saw_upto(key);
        assert_eq!(came_meanwhile(key, saw), ["算了不用了"]);
        assert!(came_meanwhile(key, later).is_empty());
        UNSEEN.lock().unwrap().remove(key);
    }
}
