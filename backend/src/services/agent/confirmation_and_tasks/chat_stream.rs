// Chat stream + WearStreamFilter.

use super::super::agent_header::*;
use super::super::response_agent;
use super::super::types::*;

type ChatDelivery =
    std::sync::Arc<std::sync::Mutex<super::super::motion_overlay::ChatMotionDelivery>>;

#[cfg(test)]
mod chat_director_tests {
    use super::super::super::motion_overlay::{
        motion_refinement_tests::context, spawn_chat_motion_refinement_with,
    };
    use super::*;

    #[tokio::test]
    async fn chat_director_stalled_model_cannot_delay_prose_or_stream_completion() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(1);
        let entered = std::sync::Arc::new(tokio::sync::Notify::new());
        let signal = entered.clone();
        let (delivery, guard) =
            spawn_chat_motion_refinement_with(context(), tx.clone(), move |_| {
                let signal = signal.clone();
                async move {
                    signal.notify_one();
                    std::future::pending().await
                }
            });
        let delivery = std::sync::Arc::new(std::sync::Mutex::new(delivery));
        // A single-slot transport is full after each prose emission. Both the
        // immediate floor and the director observer must return without waits.
        for token in ["第一句话。", "下一句不等导演。", "末句照常到达。"] {
            tokio::time::timeout(
                std::time::Duration::from_secs(1),
                emit_chat_delta(
                    &tx,
                    crate::services::analyzer::StreamDelta::Text(token.into()),
                    Some(&delivery),
                ),
            )
            .await
            .unwrap();
            assert!(
                matches!(rx.recv().await, Some(AgentProgressEvent::SummaryToken { token: actual, done: false }) if actual == token)
            );
        }
        tokio::time::timeout(std::time::Duration::from_secs(1), entered.notified())
            .await
            .unwrap();
        let finish = response_agent::finish_stream(&tx);
        let consume = async {
            assert!(matches!(
                rx.recv().await,
                Some(AgentProgressEvent::ThinkingToken { done: true, .. })
            ));
            assert!(matches!(
                rx.recv().await,
                Some(AgentProgressEvent::SummaryToken { done: true, .. })
            ));
        };
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            tokio::join!(finish, consume);
        })
        .await
        .unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), guard.stop())
            .await
            .unwrap();
    }
}

/// Prose reaches the transport first. Observing it cannot wait on a model or
/// an action send, even when the director's mailbox/transport is congested.
async fn emit_chat_delta(
    tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
    delta: crate::services::analyzer::StreamDelta,
    delivery: Option<&ChatDelivery>,
) {
    let spoken = match &delta {
        crate::services::analyzer::StreamDelta::Text(text) if delivery.is_some() => {
            Some(text.clone())
        }
        _ => None,
    };
    response_agent::emit_stream_delta(tx, delta).await;
    if let (Some(delivery), Some(text)) = (delivery, spoken) {
        delivery
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .observe(&text, tx);
    }
}

struct WearStreamFilter {
    user_input: String,
    raw: String,
    emitted: usize,
    fired_wear: bool,
    fired_music: bool,
    /// She said she would host a turtle soup.
    started_game: bool,
}

impl WearStreamFilter {
    fn new(user_input: String) -> Self {
        Self {
            user_input,
            raw: String::new(),
            emitted: 0,
            fired_wear: false,
            fired_music: false,
            started_game: false,
        }
    }
}

impl WearStreamFilter {
    fn push(
        &mut self,
        token: &str,
    ) -> (
        String,
        Option<myriad_merope::WearDirective>,
        Option<crate::services::agent::chat_music::ChatMusicAction>,
    ) {
        self.raw.push_str(token);
        self.emit_held(true)
    }

    fn flush(
        &mut self,
    ) -> (
        String,
        Option<myriad_merope::WearDirective>,
        Option<crate::services::agent::chat_music::ChatMusicAction>,
    ) {
        let (spoken, wear, music) = self.emit_held(false);
        let wear = if wear.is_some() {
            wear
        } else {
            let visible = crate::services::agent::chat_music::peel_chat_live_reply(&self.raw).0;
            self.take_wear(myriad_merope::wear_directive_after_reply(
                &self.user_input,
                &visible,
                None,
            ))
        };
        (spoken, wear, music)
    }

    fn emit_held(
        &mut self,
        hold: bool,
    ) -> (
        String,
        Option<myriad_merope::WearDirective>,
        Option<crate::services::agent::chat_music::ChatMusicAction>,
    ) {
        let (after_wear, wear) = myriad_merope::split_chat_wear_directive(&self.raw);
        let (spoken, music) =
            crate::services::agent::chat_music::split_chat_music_directive(&after_wear);
        let (spoken, started) = crate::services::agent::merope::soup::split_start(&spoken);
        self.started_game |= started;
        // A hand-off line is for the channel, not to be seen or heard.
        let (spoken, _) = crate::services::agent::delegate::split(&spoken);
        let visible = if hold {
            crate::services::agent::chat_music::hold_incomplete_live_marker(&spoken)
        } else {
            spoken.as_str()
        };
        (
            self.delta(visible),
            self.take_wear(wear),
            self.take_music(music),
        )
    }

    fn take_wear(
        &mut self,
        directive: Option<myriad_merope::WearDirective>,
    ) -> Option<myriad_merope::WearDirective> {
        let directive = directive?;
        if self.fired_wear {
            return None;
        }
        self.fired_wear = true;
        Some(directive)
    }

    fn take_music(
        &mut self,
        action: Option<crate::services::agent::chat_music::ChatMusicAction>,
    ) -> Option<crate::services::agent::chat_music::ChatMusicAction> {
        let action = action?;
        if self.fired_music {
            return None;
        }
        self.fired_music = true;
        Some(action)
    }

    fn delta(&mut self, spoken: &str) -> String {
        if spoken.len() < self.emitted {
            self.emitted = spoken.len();
            return String::new();
        }
        if !spoken.is_char_boundary(self.emitted) {
            self.emitted = spoken.len();
            return String::new();
        }
        let out = spoken[self.emitted..].to_string();
        self.emitted = spoken.len();
        out
    }
}

fn spawn_chat_music_control(
    action: crate::services::agent::chat_music::ChatMusicAction,
    tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
) {
    tokio::spawn(async move {
        let _ = tx
            .send(AgentProgressEvent::MusicControl {
                action: action.as_str().to_string(),
            })
            .await;
    });
}

fn spawn_model_outfit_overlay(
    db: sea_orm::DatabaseConnection,
    user_id: i32,
    session_id: Option<String>,
    directive: myriad_merope::WearDirective,
    tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
) {
    let Some(session_id) = session_id.filter(|id| !id.is_empty()) else {
        return;
    };
    tokio::spawn(async move {
        let Some(outfit_id) = crate::services::agent::merope::apply_model_wear_directive(
            &db,
            user_id,
            &session_id,
            &directive,
        )
        .await
        else {
            return;
        };
        let _ = tx
            .send(AgentProgressEvent::OutfitOverlay { outfit_id })
            .await;
    });
}

const EMPTY_REPLY: &str = "Chat model returned an empty response";
/// The provider dropped the reply before it was finished.
const CUT_REPLY: &str = "Chat model reply was cut off before it finished";

/// Whether only the finished reply reaches anyone: a group or an IM chat,
/// where a reply is sent whole. On the panel it is shown as it is written.
fn sent_whole(request: &UserRequest) -> bool {
    request
        .context
        .as_ref()
        .is_some_and(|context| context.venue.is_some() || context.channel_chat.is_some())
}

fn is_cut(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<crate::services::analyzer::StreamCut>()
        .is_some()
}

/// This turn is a live voice call.
fn on_call(request: &UserRequest) -> bool {
    request
        .context
        .as_ref()
        .and_then(|context| context.custom_data.as_ref())
        .and_then(|data| data.get("voice"))
        .and_then(|voice| voice.as_str())
        == Some("realtime")
}

/// Her voice in chat. It thinks little: measured on the chat suite, the first
/// word came in about 1.9 s instead of 6 s with replies of the same kind.
async fn chat_analyzer() -> Result<crate::services::analyzer::AiAnalyzer, String> {
    crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(None)
        .await
        .map(crate::services::analyzer::AiAnalyzer::with_light_thinking)
        .ok_or_else(|| "Lite model is not configured for Chat mode".to_string())
}

/// The images attached to this chat message (none in a group turn).
fn images_of(request: &UserRequest) -> &[crate::services::analyzer::ImageInput] {
    request
        .context
        .as_ref()
        .map(|context| context.images.as_slice())
        .unwrap_or(&[])
}

impl Agent {
    /// Chat 模式的唯一模型入口。严格 Lite 不可用时直接失败，绝不借用
    /// Standard / Pro，否则“只聊天”会悄悄变成另一条 Work 费用路径。
    pub(crate) async fn stream_strict_lite_chat_response(
        &self,
        request: &UserRequest,
        progress_tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
        speech_delivery: Option<ChatDelivery>,
    ) -> Result<String, String> {
        let analyzer = chat_analyzer().await?;
        let first = self
            .stream_chat_response_with_analyzer(
                request,
                progress_tx,
                analyzer,
                speech_delivery.clone(),
            )
            .await;
        // The model now and then answers with nothing at all, or the provider
        // drops a reply sent whole before it is finished. Nothing reached them,
        // so asking once more is safe.
        if !matches!(&first, Err(error) if error == EMPTY_REPLY || error == CUT_REPLY) {
            return first;
        }
        tracing::warn!(
            user_id = request.user_id,
            error = first.as_ref().err().map(String::as_str).unwrap_or(""),
            "[Chat] no whole reply; asking once more"
        );
        let analyzer = chat_analyzer().await?;
        self.stream_chat_response_with_analyzer(request, progress_tx, analyzer, speech_delivery)
            .await
    }

    pub(crate) async fn strict_lite_chat_response(
        &self,
        request: &UserRequest,
    ) -> Result<String, String> {
        let analyzer = chat_analyzer().await?;
        let prompt = self.chat_response_prompt(request).await;
        // The same request as the streamed path, only not relayed. Nothing
        // was shown, so a reply the provider dropped halfway is asked again.
        let mut response = analyzer
            .analyze_stream_parts_with_images(&prompt, images_of(request), |_| async { true })
            .await;
        if response.as_ref().is_err_and(is_cut) {
            tracing::warn!(user_id = request.user_id, "[Chat] reply cut off; asking once more");
            response = analyzer
                .analyze_stream_parts_with_images(&prompt, images_of(request), |_| async { true })
                .await;
        }
        let response = response.map_err(|error| error.to_string())?;
        let (mut response, started_game) =
            crate::services::agent::merope::soup::split_start(response.trim());
        if started_game {
            if let Some(opening) = crate::services::agent::merope::soup::start(request).await {
                response = format!("{response}\n\n{opening}");
            }
        }
        let response = response.trim();
        if response.is_empty() {
            Err("Chat model returned an empty response".to_string())
        } else {
            Ok(response.to_string())
        }
    }

    async fn chat_response_prompt(&self, request: &UserRequest) -> String {
        let soul = crate::services::agent::identity::get_speaking_soul()
            .await
            .unwrap_or_default();
        let soul: String = soul.chars().take(2000).collect();
        let venue = request
            .context
            .as_ref()
            .and_then(|context| context.venue.clone());
        let sections = if let Some(venue) = venue.as_deref() {
            crate::services::agent::merope::speaking_prompt_in_group(
                request.user_id,
                request.raw_input.as_str(),
                venue,
            )
            .await
        } else {
            crate::services::agent::merope::speaking_prompt_with_query(
                request.user_id,
                Some(request.raw_input.as_str()),
            )
            .await
        };
        let mut merope_block = crate::services::agent::merope::speaking_prompt_plain(&sections);
        // A group has no wardrobe of hers to change and no player of theirs to
        // run: those sections are for a private chat.
        if venue.is_none()
            && request.context.as_ref().is_some_and(|context| {
                context.interaction_mode == crate::services::agent::AgentInteractionMode::Chat
            })
        {
            let session_id = request
                .context
                .as_ref()
                .and_then(|context| context.session_id.as_deref())
                .unwrap_or("");
            if let Some(wardrobe) = crate::services::agent::merope::chat_wardrobe_section(
                &self.db,
                request.user_id,
                session_id,
            )
            .await
            {
                if merope_block.is_empty() {
                    merope_block = wardrobe;
                } else {
                    merope_block = format!("{merope_block}\n\n{wardrobe}");
                }
            }
            let music = request
                .context
                .as_ref()
                .and_then(|context| context.custom_data.as_ref())
                .and_then(|data| data.get("musicStatus"));
            let mut player = crate::services::agent::chat_music::format_chat_player_section(music);
            if let Some(line) = crate::services::agent::merope::doing::current()
                .and_then(|doing| crate::services::agent::merope::doing::player_line(&doing, music))
            {
                player.push('\n');
                player.push_str(line);
            }
            // A turtle soup on in this conversation, with their message
            // judged; or how she would start one.
            let game = match crate::services::agent::merope::soup::this_turn(request).await {
                Some(section) => Some(section),
                None => {
                    crate::services::agent::merope::soup::offer_line(request).map(str::to_string)
                }
            };
            if let Some(game) = game {
                player.push_str("\n\n");
                player.push_str(&game);
            }
            // A private IM chat: she can hand work off.
            if let Some(chat) = request
                .context
                .as_ref()
                .and_then(|context| context.channel_chat.as_ref())
            {
                player.push_str("\n\n");
                player.push_str(&crate::services::agent::delegate::section(chat));
            }
            if merope_block.is_empty() {
                merope_block = player;
            } else {
                merope_block = format!("{merope_block}\n\n{player}");
            }
        }
        let supplied = request
            .context
            .as_ref()
            .and_then(|context| context.conversation_history.as_deref())
            .unwrap_or(&[]);
        if venue.is_some() {
            // Nobody asked her: she chose to say something.
            if let Some(why) = request
                .context
                .as_ref()
                .and_then(|context| context.chime.as_deref())
            {
                merope_block = format!(
                    "{merope_block}\n\n## Chiming in\nNobody addressed you: you are joining the group's talk on your own because {why}. Say one or two short lines to the group, as yourself; do not make it a speech.",
                    why = why.trim().trim_end_matches('.')
                );
            }
            // A group turn: the history is the group's transcript, other
            // people's words included, and nothing she said in private.
            return crate::services::agent::chat_prompt::build_group_chat_prompt(
                &soul,
                &merope_block,
                supplied,
                &request.raw_input,
            );
        }
        let history = crate::services::agent::merope::with_said_unprompted(
            &self.db,
            request.user_id,
            supplied,
        )
        .await;
        // How long they were away, when it was a while.
        if let Some(block) = supplied
            .iter()
            .rev()
            .find(|message| message.role == "user")
            .and_then(|message| message.created_at.as_deref())
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .and_then(|at| {
                crate::services::agent::merope::format_since_section(
                    request
                        .timestamp
                        .signed_duration_since(at.with_timezone(&chrono::Utc))
                        .num_minutes(),
                )
            })
        {
            merope_block = format!("{merope_block}\n\n{block}");
        }
        if on_call(request) {
            merope_block = format!(
                "{merope_block}\n\n## On a call\nThis reply is spoken aloud on a live voice call: talk, do not write. No lists, no markup, short sentences."
            );
        }

        let custom = request
            .context
            .as_ref()
            .and_then(|context| context.custom_data.as_ref());
        let mut perception = crate::services::agent::chat_prompt::format_chat_scene(
            custom.and_then(|data| data.get("perception")),
            custom.and_then(|data| data.get("pageContent")),
            &request.raw_input,
        );
        // Which part of the site they are on, even without the page itself.
        if let Some(route) = request
            .context
            .as_ref()
            .and_then(|context| context.current_route.as_deref())
            .map(str::trim)
            .filter(|route| !route.is_empty() && route.len() <= 200)
        {
            let line = format!("They are on the page {route} of this site.");
            perception = if perception.trim().is_empty() {
                line
            } else {
                format!("{perception}\n{line}")
            };
        }
        let attached = crate::services::agent::chat_attachments::format_attached(
            custom,
            images_of(request).len(),
        );
        if !attached.is_empty() {
            perception = if perception.trim().is_empty() {
                attached
            } else {
                format!("{perception}\n\n{attached}")
            };
        }
        crate::services::agent::chat_prompt::build_chat_lite_prompt_with_perception(
            &soul,
            &merope_block,
            &history,
            &request.raw_input,
            &perception,
        )
    }

    async fn stream_chat_response_with_analyzer(
        &self,
        request: &UserRequest,
        progress_tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
        analyzer: crate::services::analyzer::AiAnalyzer,
        speech_delivery: Option<ChatDelivery>,
    ) -> Result<String, String> {
        let prompt = self.chat_response_prompt(request).await;

        let tx = progress_tx.clone();
        let wear = std::sync::Arc::new(std::sync::Mutex::new(WearStreamFilter::new(
            request.raw_input.clone(),
        )));
        let db = self.db.clone();
        let user_id = request.user_id;
        let session_id = request
            .context
            .as_ref()
            .and_then(|context| context.session_id.clone());
        let streamed = analyzer
            .analyze_stream_parts_with_images(&prompt, images_of(request), |delta| {
                let tx = tx.clone();
                let speech_delivery = speech_delivery.clone();
                let wear = wear.clone();
                let db = db.clone();
                let session_id = session_id.clone();
                async move {
                    let delta = match delta {
                        crate::services::analyzer::StreamDelta::Text(token) => {
                            let (spoken, directive, music) = wear
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner())
                                .push(&token);
                            if let Some(directive) = directive {
                                spawn_model_outfit_overlay(
                                    db.clone(),
                                    user_id,
                                    session_id.clone(),
                                    directive,
                                    tx.clone(),
                                );
                            }
                            if let Some(music) = music {
                                spawn_chat_music_control(music, tx.clone());
                            }
                            if spoken.is_empty() {
                                return true;
                            }
                            crate::services::analyzer::StreamDelta::Text(spoken)
                        }
                        other => other,
                    };
                    emit_chat_delta(&tx, delta, speech_delivery.as_ref()).await;
                    true
                }
            })
            .await;
        // The provider dropped the reply halfway. Sent whole, nothing reached
        // anyone yet: ask again. On the panel they already saw it being
        // written: keep what they saw rather than take it back.
        let streamed = match streamed {
            Err(error) if is_cut(&error) => {
                let partial = error
                    .downcast_ref::<crate::services::analyzer::StreamCut>()
                    .map(|cut| cut.partial.clone())
                    .unwrap_or_default();
                if sent_whole(request) || partial.trim().is_empty() {
                    return Err(CUT_REPLY.to_string());
                }
                tracing::warn!(user_id, "[Chat] reply cut off halfway; keeping what was shown");
                Ok(partial)
            }
            other => other,
        };
        match streamed {
            Ok(full_text) if !full_text.trim().is_empty() => {
                let (leftover, directive, music) = wear
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .flush();
                if let Some(directive) = directive {
                    spawn_model_outfit_overlay(
                        self.db.clone(),
                        user_id,
                        session_id,
                        directive,
                        progress_tx.clone(),
                    );
                }
                if let Some(music) = music {
                    spawn_chat_music_control(music, progress_tx.clone());
                }
                if !leftover.is_empty() {
                    emit_chat_delta(
                        progress_tx,
                        crate::services::analyzer::StreamDelta::Text(leftover),
                        speech_delivery.as_ref(),
                    )
                    .await;
                }
                let started_game = wear
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .started_game;
                let (mut full_text, _) =
                    crate::services::agent::merope::soup::split_start(&full_text);
                // She said she would think one up: the puzzle follows her words.
                if started_game {
                    if let Some(opening) =
                        crate::services::agent::merope::soup::start(request).await
                    {
                        let opening = format!("\n\n{opening}");
                        full_text.push_str(&opening);
                        emit_chat_delta(
                            progress_tx,
                            crate::services::analyzer::StreamDelta::Text(opening),
                            speech_delivery.as_ref(),
                        )
                        .await;
                    }
                }
                if let Some(delivery) = speech_delivery.as_ref() {
                    delivery
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .finish(progress_tx);
                }
                Ok(full_text.trim().to_owned())
            }
            Ok(_) => Err(EMPTY_REPLY.to_string()),
            Err(error) => Err(error.to_string()),
        }
    }

    /// 将已有文本分块推送为 SummaryToken 事件，模拟流式输出
    ///
    /// 将文本按句/标点拆分为自然片段，逐个发送给前端，
    /// 让用户看到"AI 在打字"的效果而非一次性出现全部内容。
    pub(crate) async fn stream_text_as_tokens(
        tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
        text: &str,
    ) {
        Self::stream_text_as_tokens_with_finish(tx, text, true).await;
    }

    async fn stream_text_as_tokens_with_finish(
        tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
        text: &str,
        finish_stream: bool,
    ) {
        // 按自然断点切分（标点、换行）
        let mut chunks = Vec::new();
        let mut current = String::new();
        for ch in text.chars() {
            current.push(ch);
            // 在句号、逗号、换行、感叹号、问号等处断开
            if matches!(
                ch,
                '。' | '，' | '！' | '？' | '\n' | '；' | '：' | '.' | ',' | '!' | '?' | ';' | ':'
            ) || current.len() > 40
            {
                chunks.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            chunks.push(current);
        }

        for chunk in &chunks {
            let _ = tx
                .send(AgentProgressEvent::SummaryToken {
                    token: chunk.clone(),
                    done: false,
                })
                .await;
            // 极短延迟让前端有时间渲染，避免所有 token 在同一帧到达
            tokio::time::sleep(tokio::time::Duration::from_millis(15)).await;
        }
        if finish_stream {
            response_agent::finish_stream(tx).await;
        }
    }
}
