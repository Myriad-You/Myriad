//! A private IM message is a chat with her.
//!
//! She answers as herself (Chat): with her memory of them, her own time and
//! views, and the photos they send. When they ask her to get something done
//! she hands it off, and the channel starts it as their Work in the Work
//! session it has always used, so questions, confirmations, delivery and
//! recovery work as before. She can keep talking while it runs, and the next
//! time they talk she knows what she handed off and how it came back.
//!
//! Her replies are best effort, like a group reply: a restart mid-turn loses
//! that one reply. Work keeps its own ledger.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures::StreamExt;
use myriad_agent_rules::channel::{ChannelImageRef, split_channel_text};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};
use tracing::warn;

use crate::services::agent::run::{ProcessContext, ProcessRequest};
use crate::services::agent::types::ChannelChat;
use crate::services::agent::{AgentInteractionMode, AgentProgressEvent};

use super::{ChannelSink, cache_inbound_images, is_active, load_session, put_session};

/// Longest she takes over one reply before giving up on it.
const TURN_DEADLINE: Duration = Duration::from_secs(90);
const TYPING_EVERY: Duration = Duration::from_secs(4);
/// What she handed off still counts as recent this long.
const HANDED_OFF_FOR: chrono::Duration = chrono::Duration::hours(24);
const MODEL_IMAGE_EDGE: u32 = 1024;
/// She is in talk with someone this long after they last wrote to her, or
/// she to them, anywhere (the site, a group, any app they paired): she sees
/// what they send at once.
const IN_TALK: Duration = Duration::from_secs(10 * 60);

/// Messages of a chat she has not seen yet, read together once she does.
struct Unseen {
    texts: Vec<String>,
    images: Vec<ChannelImageRef>,
}

static UNSEEN: LazyLock<Mutex<HashMap<String, Unseen>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
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
fn take_in(session_key: &str, input: &str, images: &[ChannelImageRef]) -> bool {
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
/// `merope::timing`; she is not held off by sleep here, as nothing would
/// keep the message across a restart); what they send meanwhile she reads
/// with it, and answers once.
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
    use crate::services::agent::merope::timing;
    if !take_in(session_key, input, images) {
        return;
    }
    let seeing = Seeing(session_key);
    let talking = LAST_REPLY
        .lock()
        .ok()
        .and_then(|last| last.get(&user_id).copied())
        .is_some_and(|at| at.elapsed() < IN_TALK)
        || wrote_lately(&db, user_id).await;
    let at = timing::where_she_is(talking, false);
    let wait = timing::until_read(at, input);
    tracing::info!(
        ?at,
        seconds = wait.as_secs(),
        "[Channel] she will see the message"
    );
    tokio::time::sleep(wait).await;
    let Some(seen) = UNSEEN
        .lock()
        .ok()
        .and_then(|mut unseen| unseen.remove(session_key))
    else {
        return;
    };
    drop(seeing);
    let input = seen
        .texts
        .iter()
        .map(|text| text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    answer(
        db,
        user_id,
        work_session_id,
        &input,
        &seen.images,
        sink,
        session_key,
    )
    .await;
    if let Ok(mut last) = LAST_REPLY.lock() {
        if last.len() >= CHATS_KEPT {
            last.retain(|_, at| at.elapsed() < IN_TALK);
        }
        last.insert(user_id, Instant::now());
    }
}

/// Whether this person wrote to her anywhere in the last few minutes.
async fn wrote_lately(db: &DatabaseConnection, user_id: i32) -> bool {
    crate::services::agent::merope::store::get_or_create_state(db, user_id)
        .await
        .ok()
        .and_then(|state| state.last_user_message_at)
        .and_then(|at| {
            (chrono::Utc::now() - at.with_timezone(&chrono::Utc))
                .to_std()
                .ok()
        })
        .is_some_and(|age| age < IN_TALK)
}

#[allow(clippy::too_many_arguments)]
async fn answer(
    db: DatabaseConnection,
    user_id: i32,
    work_session_id: String,
    input: &str,
    images: &[ChannelImageRef],
    sink: ChannelSink,
    session_key: &str,
) {
    sink.send_typing().await;
    let began = Instant::now();
    let platform = sink.platform();
    let Some(chat_session_id) = chat_session(&db, platform, user_id, session_key).await else {
        let _ = sink.send_text("无法保存通道会话，请稍后重试。").await;
        return;
    };
    let cached = match cache_inbound_images(&db, user_id, &sink.transport, images).await {
        Ok(data) => data,
        Err(message) => {
            let _ = sink.send_text(&message).await;
            return;
        }
    };
    let busy = is_active(session_key).await;
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
                    handed_off: handed_off(&db, &work_session_id, busy).await,
                    busy,
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
    let Some(response) = await_reply(run, &sink).await else {
        return;
    };
    let message = response
        .get("message")
        .and_then(Value::as_str)
        .map(str::trim)
        .unwrap_or("");
    // Most turns go as one message, and a few in a row when something grabs
    // her, typing each first.
    let most = myriad_merope::talk_shape::messages_this_turn(
        myriad_agent_rules::channel::as_messages(message).len(),
        rand::random::<f64>(),
    );
    for (index, line) in myriad_agent_rules::channel::as_messages_at_most(message, most)
        .into_iter()
        .enumerate()
    {
        // Typing takes as long as it takes; the first she was already at
        // while she thought.
        let typing = crate::services::agent::merope::timing::typing(&line);
        let left = if index == 0 {
            typing.saturating_sub(began.elapsed())
        } else {
            typing
        };
        if !left.is_zero() {
            sink.send_typing().await;
            tokio::time::sleep(left).await;
        }
        for chunk in split_channel_text(&line, sink.text_limit()) {
            if sink.send_text(&chunk).await.is_err() {
                return;
            }
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
    // What she handed off starts as their own Work, unless one is running.
    let instruction = response
        .pointer("/data/handOff/instruction")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|instruction| !instruction.is_empty());
    if let Some(instruction) = instruction.filter(|_| !busy) {
        let work_input = work_input(instruction, input);
        super::start_work_run(
            db,
            user_id,
            work_session_id,
            &work_input,
            cached,
            sink,
            session_key,
        )
        .await;
    }
}

/// Send the sticker she chose, making it first if it is a new one.
async fn send_sticker(db: &DatabaseConnection, sink: &ChannelSink, chosen: &Value) {
    use crate::services::agent::merope::stickers;
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

/// What the helper is asked: her instruction, with their own words.
fn work_input(instruction: &str, their_words: &str) -> String {
    let their_words = their_words.trim();
    if their_words.is_empty() || instruction.contains(their_words) {
        instruction.to_string()
    } else {
        format!("{instruction}\n\n（对方原话：{their_words}）")
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

/// What she handed off last and how it came back, if lately.
async fn handed_off(db: &DatabaseConnection, work_session_id: &str, busy: bool) -> Option<String> {
    if work_session_id.is_empty() {
        return None;
    }
    let history =
        crate::services::agent::sessions::load_session_history(db, work_session_id, 2, false)
            .await
            .ok()?;
    let recent = |at: Option<&str>| {
        at.and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .is_some_and(|at| chrono::Utc::now().signed_duration_since(at) < HANDED_OFF_FOR)
    };
    let asked = history
        .iter()
        .rev()
        .find(|message| message.role == "user")?;
    if !recent(asked.created_at.as_deref()) {
        return None;
    }
    let asked: String = asked.content.chars().take(300).collect();
    let answered = history
        .last()
        .filter(|message| message.role == "assistant")
        .map(|message| message.content.chars().take(800).collect::<String>());
    Some(match (busy, answered) {
        (true, _) | (false, None) => {
            format!("You handed off: {asked}\nIt is still being done.")
        }
        (false, Some(answered)) => format!("You handed off: {asked}\nIt came back: {answered}"),
    })
}

/// The cached attachments, with a small copy of each image the model can
/// look at. The copies are taken out of the request before anything is kept.
async fn with_model_images(cached: Option<&Value>) -> Option<Value> {
    let mut data = cached?.clone();
    let Some(attachments) = data.get_mut("attachments").and_then(Value::as_array_mut) else {
        return Some(data);
    };
    for attachment in attachments.iter_mut() {
        let Some(url) = attachment
            .get("url")
            .and_then(Value::as_str)
            .map(str::to_string)
        else {
            continue;
        };
        let Ok(image) = super::transport::load_channel_image_bytes(&url).await else {
            continue;
        };
        let bytes = image.bytes;
        if let Ok(Some(small)) = tokio::task::spawn_blocking(move || model_copy(&bytes)).await {
            attachment["image"] = json!(small);
        }
    }
    Some(data)
}

/// A JPEG data URL no longer than `MODEL_IMAGE_EDGE` on its long side.
fn model_copy(bytes: &[u8]) -> Option<String> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = if image.width().max(image.height()) > MODEL_IMAGE_EDGE {
        image.resize(
            MODEL_IMAGE_EDGE,
            MODEL_IMAGE_EDGE,
            image::imageops::FilterType::Triangle,
        )
    } else {
        image
    };
    let mut out = Vec::new();
    image
        .to_rgb8()
        .write_to(
            &mut std::io::Cursor::new(&mut out),
            image::ImageFormat::Jpeg,
        )
        .ok()?;
    Some(format!("data:image/jpeg;base64,{}", STANDARD.encode(out)))
}

async fn await_reply(
    run: std::sync::Arc<crate::services::agent::run_hub::AgentRun>,
    sink: &ChannelSink,
) -> Option<Value> {
    let mut events = Box::pin(crate::services::agent::run::agent_run_envelopes(run));
    let mut typing = tokio::time::interval(TYPING_EVERY);
    let deadline = tokio::time::sleep(TURN_DEADLINE);
    tokio::pin!(deadline);
    loop {
        tokio::select! {
            _ = &mut deadline => return None,
            _ = typing.tick() => sink.send_typing().await,
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
mod tests {
    use super::*;

    #[test]
    fn the_helper_hears_their_own_words_too() {
        assert_eq!(
            work_input("查一下明天东京的天气", "明天东京会下雨吗？帮我看看"),
            "查一下明天东京的天气\n\n（对方原话：明天东京会下雨吗？帮我看看）"
        );
        assert_eq!(work_input("帮我看看", "帮我看看"), "帮我看看");
    }

    #[test]
    fn photos_are_shrunk_for_the_model() {
        let mut big = image::RgbImage::new(3000, 1500);
        big.put_pixel(0, 0, image::Rgb([255, 0, 0]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgb8(big)
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        let url = model_copy(&png).unwrap();
        let bytes = STANDARD
            .decode(url.strip_prefix("data:image/jpeg;base64,").unwrap())
            .unwrap();
        let small = image::load_from_memory(&bytes).unwrap();
        assert_eq!((small.width(), small.height()), (1024, 512));
        assert!(model_copy(b"not an image").is_none());
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
}
