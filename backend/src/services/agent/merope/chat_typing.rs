//! How she types to someone, wherever she types: she sees a message when
//! she would, a reply goes out as a few messages typed one by one, and what
//! they send meanwhile she looks at before it goes. Her habits in chat apps
//! are her habits on the site too (decisions 17 and 18).
//!
//! The site's timing, draft and talk-shape calls behind these habits are
//! kept here together, so they move to the core in one place.

use std::time::{Duration, Instant};

use myriad_merope::talk_shape::Shape;
use serde_json::json;
use tokio::sync::mpsc::Sender;

use crate::services::agent::chat_prompt::{CUT_OFF_KEY, CUT_OFF_UNSENT};
use crate::services::agent::turn::{CutOff, Draft};
use crate::services::agent::types::{AgentInteractionMode, AgentProgressEvent, UserRequest};
use crate::services::agent::{ConversationMessage, RequestContext};

/// She is in talk with someone this long after the last line between them.
const IN_TALK: Duration = Duration::from_secs(10 * 60);

/// Her reply as the messages it goes out as, typed the way people type to
/// her (`room`, or as people in chat apps usually do).
pub fn messages_of(reply: &str, room: Option<&Shape>) -> Vec<String> {
    let lines = myriad_agent_rules::channel::as_messages(reply);
    let most = myriad_merope::talk_shape::messages_this_turn(lines.len(), rand::random::<f64>());
    myriad_merope::talk_shape::goes_out_as(&lines, most, room)
}

/// How long typing `message` takes her, give or take.
pub fn typing(message: &str) -> Duration {
    super::timing::typing(message)
}

/// How long until she has seen `text` and read it: at once, near enough,
/// while they are talking; later when she is busy with something of her
/// own, or (where `may_sleep`) asleep.
pub fn until_seen(in_talk: bool, may_sleep: bool, text: &str) -> Duration {
    let at = super::timing::where_she_is(in_talk, may_sleep);
    let wait = super::timing::until_read(at, text);
    tracing::info!(
        ?at,
        seconds = wait.as_secs(),
        "[Merope] she will see the message"
    );
    wait
}

/// Whether her draft, written to `said`, still fits now that `came` arrived
/// while she wrote it (who, what, oldest first).
pub async fn still_fits(owner: i32, said: &str, draft: &str, came: &[(String, String)]) -> bool {
    super::draft::still_fits(owner, said, draft, came).await
}

/// A private chat typed on the site, a message at a time as in a chat app:
/// not said aloud, not in a group, not answered elsewhere (a chat app sends
/// its own way, a device has its own body).
pub(crate) fn in_messages(context: Option<&RequestContext>) -> bool {
    let Some(context) = context else {
        return false;
    };
    let aloud = context.custom_data.as_ref().is_some_and(|data| {
        data.get("voice").and_then(|voice| voice.as_str()) == Some("realtime")
            || data.get("voiceOut").is_some()
    });
    context.interaction_mode == AgentInteractionMode::Chat
        && context.venue.is_none()
        && context.channel_chat.is_none()
        && context.on_device.is_none()
        && !aloud
}

/// Whether they and she are talking just now: a line between them in the
/// last few minutes.
pub(crate) fn in_talk(history: &[ConversationMessage]) -> bool {
    let now = crate::clock::now();
    history.iter().rev().take(4).any(|message| {
        message
            .created_at
            .as_deref()
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .and_then(|at| (now - at.with_timezone(&chrono::Utc)).to_std().ok())
            .is_some_and(|age| age < IN_TALK)
    })
}

/// How long until she sees what they typed on the site. Not yet while she
/// sleeps: the panel waits on its reply, and nothing here keeps a message
/// for when she wakes the way a chat app's does (`channel_work::chat_waiting`).
pub(crate) fn until_seen_here(request: &UserRequest) -> Duration {
    let history = request
        .context
        .as_ref()
        .and_then(|context| context.conversation_history.as_deref())
        .unwrap_or(&[]);
    until_seen(in_talk(history), false, &request.raw_input)
}

/// Her draft of the last turn, that a message of theirs cut off on the
/// site: what becomes of it.
pub(crate) enum AfterCut {
    /// Not one typed a message at a time: kept as before.
    AsBefore,
    /// Kept for what it was.
    Kept,
    /// Nothing of it had gone and it still fits: it goes first in this turn.
    GoesFirst(Vec<String>),
}

/// They wrote `input` while her last reply on the site was being typed.
/// Nothing of it had gone: she looks at it again and sends it if it still
/// fits, or keeps it as never sent. Partway: it stops where it was, as
/// any reply cut off.
pub(crate) async fn after_cut(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    session_id: &str,
    cut: &CutOff,
    input: &str,
) -> AfterCut {
    let Some(Draft {
        said,
        reply,
        messages,
    }) = &cut.draft
    else {
        return AfterCut::AsBefore;
    };
    let whole = messages.join("\n");
    if !cut.text.is_empty() && cut.text != whole {
        return AfterCut::AsBefore;
    }
    let came = [("them".to_string(), input.to_string())];
    let goes = cut.text == whole || still_fits(user_id, said, reply, &came).await;
    let metadata = if goes {
        json!({ "runId": cut.run_id })
    } else {
        json!({ CUT_OFF_KEY: CUT_OFF_UNSENT, "runId": cut.run_id })
    };
    let text = if goes { whole.as_str() } else { reply.as_str() };
    if let Err(error) = crate::services::agent::sessions::persist_assistant_message(
        db,
        session_id,
        None,
        text,
        Some(metadata),
    )
    .await
    {
        tracing::warn!(%error, "[Merope] her cut-off draft was not kept");
    }
    if goes && cut.text.is_empty() {
        AfterCut::GoesFirst(messages.clone())
    } else {
        AfterCut::Kept
    }
}

/// Type `messages` out to them on the site, one at a time: typing shows,
/// typing takes as long as it takes (less what she already spent from
/// `began`, for the first), and each goes as a message of its own. A
/// message of theirs meanwhile cuts this off from outside (see `turn`).
pub(in crate::services::agent) async fn type_out(
    messages: &[String],
    began: Option<Instant>,
    first_of_turn: bool,
    tx: &Sender<AgentProgressEvent>,
    delivery: Option<&std::sync::Mutex<super::super::motion_overlay::ChatMotionDelivery>>,
) {
    for (index, message) in messages.iter().enumerate() {
        let typing = typing(message);
        let left = match began {
            Some(began) if index == 0 => typing.saturating_sub(began.elapsed()),
            _ => typing,
        };
        let _ = tx.send(AgentProgressEvent::Typing).await;
        tokio::time::sleep(left).await;
        let token = if index == 0 && first_of_turn {
            message.clone()
        } else {
            format!("\n{message}")
        };
        let _ = tx
            .send(AgentProgressEvent::SummaryToken {
                token: token.clone(),
                done: false,
            })
            .await;
        if let Some(delivery) = delivery {
            delivery
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .observe(&token, tx);
        }
    }
}

/// Lines a turn sends before its own: her last draft, still fitting (see
/// `after_cut`), by the run that sends them.
static GOES_FIRST: std::sync::LazyLock<
    std::sync::Mutex<std::collections::HashMap<String, Vec<String>>>,
> = std::sync::LazyLock::new(Default::default);

pub(crate) fn send_first(run_id: &str, messages: Vec<String>) {
    if let Ok(mut first) = GOES_FIRST.lock() {
        first.insert(run_id.to_string(), messages);
    }
}

pub(crate) fn take_first(run_id: &str) -> Vec<String> {
    GOES_FIRST
        .lock()
        .ok()
        .and_then(|mut first| first.remove(run_id))
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn typed_on_the_panel_but_not_aloud_in_a_group_or_elsewhere() {
        let chat = RequestContext {
            interaction_mode: AgentInteractionMode::Chat,
            ..Default::default()
        };
        assert!(in_messages(Some(&chat)));
        assert!(!in_messages(None));
        let work = RequestContext::default();
        assert!(!in_messages(Some(&work)));
        for aloud in [json!({ "voice": "realtime" }), json!({ "voiceOut": "t" })] {
            let context = RequestContext {
                custom_data: Some(aloud),
                ..chat.clone()
            };
            assert!(!in_messages(Some(&context)));
        }
        let group = RequestContext {
            venue: Some("telegram:-1".into()),
            ..chat.clone()
        };
        assert!(!in_messages(Some(&group)));
        let device = RequestContext {
            on_device: Some("mac".into()),
            ..chat
        };
        assert!(!in_messages(Some(&device)));
    }

    #[tokio::test(start_paused = true)]
    async fn each_message_is_typed_then_goes_on_its_own() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        let messages = vec!["嗯".to_string(), "周六可以".to_string()];
        let began = tokio::time::Instant::now();
        type_out(&messages, None, true, &tx, None).await;
        // Typing took time: about as long as typing them does.
        assert!(began.elapsed() >= Duration::from_secs(1));
        drop(tx);
        let mut seen = Vec::new();
        while let Some(event) = rx.recv().await {
            seen.push(match event {
                AgentProgressEvent::Typing => "typing".to_string(),
                AgentProgressEvent::SummaryToken { token, .. } => token,
                other => panic!("unexpected {other:?}"),
            });
        }
        assert_eq!(seen, ["typing", "嗯", "typing", "\n周六可以"]);
        // After a draft sent first, its own messages each start a line.
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        type_out(&messages[..1], None, false, &tx, None).await;
        drop(tx);
        rx.recv().await;
        assert!(matches!(
            rx.recv().await,
            Some(AgentProgressEvent::SummaryToken { token, .. }) if token == "\n嗯"
        ));
    }

    #[test]
    fn a_line_in_the_last_minutes_is_talk() {
        let line = |minutes: i64| ConversationMessage {
            role: "user".into(),
            content: "在吗".into(),
            created_at: Some(
                (crate::clock::now() - chrono::Duration::minutes(minutes)).to_rfc3339(),
            ),
        };
        assert!(in_talk(&[line(90), line(3)]));
        assert!(!in_talk(&[line(90), line(30)]));
        assert!(!in_talk(&[]));
    }
}
