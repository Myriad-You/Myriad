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
        // A sticker line is for the chat app, not to be seen or heard.
        let (spoken, _) = myriad_merope::stickers::split_sticker_directive(&spoken);
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
    user_id: i32,
    session_id: Option<&str>,
    tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
) {
    let Some(event) =
        crate::services::agent::chat_music::control_event(action, user_id, session_id)
    else {
        return;
    };
    tokio::spawn(async move {
        let _ = tx.send(event).await;
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
/// Her own voice for this turn (see `services::omni_voice`): where the panel
/// that will play it asked for it, and Lite can say it. Otherwise the
/// panel's stream ends at once, and it reads her words aloud as before.
async fn her_voice(request: &UserRequest) -> Option<HerVoice> {
    let custom = request.context.as_ref()?.custom_data.as_ref()?;
    let token = custom.get("voiceOut")?.as_str()?;
    let out = crate::services::omni_voice::voice_out(token, request.user_id)?;
    let omni = crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .merope_omni_voice()
        .ok()?;
    // Said aloud to her: she hears it as said, beside the words written down.
    let heard = custom
        .get("voiceIn")
        .and_then(|token| token.as_str())
        .and_then(|token| crate::services::omni_voice::take_heard(token, request.user_id));
    Some(HerVoice { omni, out, heard })
}

/// Her own voice for a turn: Lite as Omni, where the sound goes, and what
/// she heard said, if it was said aloud.
struct HerVoice {
    omni: crate::config::OmniVoice,
    out: crate::services::omni_voice::VoiceOut,
    heard: Option<String>,
}

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
        // Put together once: asking again is the same turn, and putting it
        // together judges soups and reads everything a second time.
        let prompt = self.chat_response_prompt(request).await;
        // Said aloud in her own voice, where the panel will play it; asked
        // again, the words alone, read aloud as before.
        let voice = her_voice(request).await;
        let first = self
            .stream_chat_response_with_analyzer(
                request,
                &prompt,
                progress_tx,
                analyzer,
                speech_delivery.clone(),
                voice,
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
        self.stream_chat_response_with_analyzer(
            request,
            &prompt,
            progress_tx,
            analyzer,
            speech_delivery,
            None,
        )
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
            tracing::warn!(
                user_id = request.user_id,
                "[Chat] reply cut off; asking once more"
            );
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

    pub(crate) async fn chat_response_prompt(&self, request: &UserRequest) -> String {
        let soul = crate::services::agent::identity::get_speaking_soul()
            .await
            .unwrap_or_default();
        let venue = request
            .context
            .as_ref()
            .and_then(|context| context.venue.clone());
        let sections = if let Some(venue) = venue.as_deref() {
            crate::services::agent::merope::speaking_prompt_in_group(
                request.user_id,
                request.raw_input.as_str(),
                venue,
                request
                    .context
                    .as_ref()
                    .and_then(|context| context.speaker.as_deref()),
            )
            .await
        } else {
            crate::services::agent::merope::speaking_prompt_with_query(
                request.user_id,
                Some(request.raw_input.as_str()),
            )
            .await
        };
        // How she is this moment goes last, nearest their words: what she
        // answers from, not one more thing among the rest.
        let (moment, sections): (Vec<String>, Vec<String>) = sections
            .into_iter()
            .partition(|section| myriad_merope::speaking::is_her_moment(section));
        let moment = moment.join("\n\n");
        let with_moment = |block: String| {
            if moment.is_empty() {
                block
            } else if block.is_empty() {
                moment.clone()
            } else {
                format!("{block}\n\n{moment}")
            }
        };
        let mut merope_block = crate::services::agent::merope::speaking_prompt_plain(&sections);
        // A group has no wardrobe of hers to change and no player of theirs to
        // run, and neither has a chat app: those sections are for a private
        // chat on the site, where they are really there.
        if venue.is_none()
            && request.context.as_ref().is_some_and(|context| {
                context.interaction_mode == crate::services::agent::AgentInteractionMode::Chat
            })
        {
            let in_app = request
                .context
                .as_ref()
                .is_some_and(|context| context.channel_chat.is_some());
            let session_id = request
                .context
                .as_ref()
                .and_then(|context| context.session_id.as_deref())
                .unwrap_or("");
            let mut blocks: Vec<String> = Vec::new();
            if !in_app {
                if let Some(wardrobe) = crate::services::agent::merope::chat_wardrobe_section(
                    &self.db,
                    request.user_id,
                    session_id,
                )
                .await
                {
                    blocks.push(wardrobe);
                }
                let music = request
                    .context
                    .as_ref()
                    .and_then(|context| context.custom_data.as_ref())
                    .and_then(|data| data.get("musicStatus"));
                let mut player =
                    crate::services::agent::chat_music::format_chat_player_section(music);
                if let Some(line) =
                    crate::services::agent::merope::doing::current().and_then(|doing| {
                        crate::services::agent::merope::doing::player_line(&doing, music)
                    })
                {
                    player.push('\n');
                    player.push_str(line);
                }
                blocks.push(player);
                // Songs she could put on for them, where there is a player.
                if music.is_some() {
                    if let Some(section) = crate::services::agent::chat_music::offer_songs(
                        &self.db,
                        request.user_id,
                        session_id,
                    )
                    .await
                    {
                        blocks.push(section);
                    }
                }
            }
            // A turtle soup on in this conversation, with their message
            // judged; or how she would start one.
            let game = match crate::services::agent::merope::soup::this_turn(request).await {
                Some(section) => Some(section),
                None => crate::services::agent::merope::soup::offer(request).await,
            };
            blocks.extend(game);
            // A private IM chat: she can hand work off.
            if let Some(chat) = request
                .context
                .as_ref()
                .and_then(|context| context.channel_chat.as_ref())
            {
                blocks.push(crate::services::agent::delegate::section(chat));
                blocks.push(myriad_merope::speaking::chat_app_section().to_string());
                // How they type to her, once there is enough of it.
                if let Some(room) = request
                    .context
                    .as_ref()
                    .and_then(|context| context.conversation_history.as_deref())
                    .and_then(crate::services::agent::merope::their_typing)
                {
                    blocks.push(myriad_merope::talk_shape::describe(&room, "How they type"));
                }
                // No player there: a song goes as its link.
                if let Some(songs) =
                    crate::services::agent::chat_music::offer_song_links(&self.db).await
                {
                    blocks.push(songs);
                }
                // Stickers of her, to send there.
                if let Some(stickers) = crate::services::agent::merope::stickers::section(
                    &self.db,
                    &crate::services::agent::merope::stickers::key(request),
                    None,
                    &request.raw_input,
                )
                .await
                {
                    blocks.push(stickers);
                }
            }
            if !merope_block.is_empty() {
                blocks.insert(0, merope_block);
            }
            merope_block = blocks.join("\n\n");
        }
        let supplied = request
            .context
            .as_ref()
            .and_then(|context| context.conversation_history.as_deref())
            .unwrap_or(&[]);
        // What of her own time she has already told them in this talk.
        if let Some(doing) = crate::services::agent::merope::doing::current() {
            let hers: Vec<&str> = supplied
                .iter()
                .filter(|message| message.role == "assistant")
                .flat_map(|message| message.content.lines())
                .collect();
            let about = crate::services::agent::merope::doing::now_line(&doing, chrono::Utc::now());
            let told = myriad_merope::speaking::already_told(&about, &hers);
            if let Some(section) = myriad_merope::speaking::format_already_told_section(&told) {
                merope_block = if merope_block.is_empty() {
                    section
                } else {
                    format!("{merope_block}\n\n{section}")
                };
            }
        }
        // A habit of hers in this talk, when one shows: known, not ordered.
        let her_replies: Vec<&str> = supplied
            .iter()
            .filter(|message| message.role == "assistant")
            .map(|message| message.content.as_str())
            .collect();
        if let Some(section) = myriad_merope::speaking::format_habits_section(&her_replies) {
            merope_block = if merope_block.is_empty() {
                section
            } else {
                format!("{merope_block}\n\n{section}")
            };
        }
        // Coming back after a while: how she greeted them the last times
        // they did, so a greeting of hers she says every time is known to her.
        let came_back = supplied
            .last()
            .and_then(|message| message.created_at.as_deref())
            .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
            .is_some_and(|at| {
                crate::services::agent::merope::clock::now() - at.with_timezone(&chrono::Utc)
                    >= chrono::Duration::hours(3)
            });
        if venue.is_none() && came_back {
            let openers =
                crate::services::agent::merope::her_openers(&self.db, request.user_id).await;
            if let Some(section) = myriad_merope::speaking::format_openers_section(&openers) {
                merope_block = if merope_block.is_empty() {
                    section
                } else {
                    format!("{merope_block}\n\n{section}")
                };
            }
        }
        // Words and memes in the talk that she looked up before: she knows
        // them, here as anywhere.
        let talk: String = supplied
            .iter()
            .rev()
            .take(12)
            .map(|message| message.content.as_str())
            .chain(std::iter::once(request.raw_input.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        if let Some(section) =
            crate::services::agent::merope::memes::section_for(&self.db, &talk).await
        {
            merope_block = if merope_block.is_empty() {
                section
            } else {
                format!("{merope_block}\n\n{section}")
            };
        }
        if venue.is_some() {
            // A turtle soup on in this group, with this line judged; or how
            // she would start one for the group.
            let game = match crate::services::agent::merope::soup::this_turn(request).await {
                Some(section) => Some(section),
                None => crate::services::agent::merope::soup::offer(request).await,
            };
            if let Some(game) = game {
                merope_block = format!("{merope_block}\n\n{game}");
            }
            merope_block = format!(
                "{merope_block}\n\n{}",
                myriad_merope::speaking::chat_app_section()
            );
            if let Some(room) = request
                .context
                .as_ref()
                .and_then(|context| context.room.as_ref())
            {
                merope_block = format!(
                    "{merope_block}\n\n{}",
                    myriad_merope::talk_shape::describe(room, "How people type here")
                );
            }
            // What she made of the talk before answering: she answers from it.
            if let Some(sense) = request
                .context
                .as_ref()
                .and_then(|context| context.making_sense.as_deref())
            {
                merope_block = format!("{merope_block}\n\n{sense}");
            }
            // She only now sees the line she answers.
            if let Some(ago) = request
                .context
                .as_ref()
                .and_then(|context| context.late.as_deref())
            {
                merope_block = format!(
                    "{merope_block}\n\n{}",
                    myriad_merope::joining::seeing_it_late(ago)
                );
            }
            if let Some(differs) = request
                .context
                .as_ref()
                .and_then(|context| context.differs.as_deref())
            {
                merope_block = format!("{merope_block}\n\n{differs}");
            }
            // Stickers of her: hers, and this group's own.
            if let Some(stickers) = crate::services::agent::merope::stickers::section(
                &self.db,
                &crate::services::agent::merope::stickers::key(request),
                venue.as_deref(),
                &request.raw_input,
            )
            .await
            {
                merope_block = format!("{merope_block}\n\n{stickers}");
            }
            // Nobody called her by name: she chose to say something.
            if let Some(why) = request
                .context
                .as_ref()
                .and_then(|context| context.chime.as_deref())
            {
                merope_block = format!(
                    "{merope_block}\n\n{}",
                    myriad_merope::joining::speaking_up_section(why.trim())
                );
            }
            // A group turn: the history is the group's transcript, other
            // people's words included, and nothing she said in private.
            return crate::services::agent::chat_prompt::build_group_chat_prompt(
                &soul,
                &with_moment(merope_block),
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
            &with_moment(merope_block),
            &history,
            &request.raw_input,
            &perception,
        )
    }

    async fn stream_chat_response_with_analyzer(
        &self,
        request: &UserRequest,
        prompt: &str,
        progress_tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
        analyzer: crate::services::analyzer::AiAnalyzer,
        speech_delivery: Option<ChatDelivery>,
        voice: Option<HerVoice>,
    ) -> Result<String, String> {
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
        let said_anything = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let heard = said_anything.clone();
        let on_delta = |delta| {
            heard.store(true, std::sync::atomic::Ordering::Relaxed);
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
                            spawn_chat_music_control(
                                music,
                                user_id,
                                session_id.as_deref(),
                                tx.clone(),
                            );
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
        };
        let mut streamed = None;
        if let Some(HerVoice { omni, out, heard }) = &voice {
            let spoken = crate::services::omni_voice::speak(
                omni,
                prompt,
                images_of(request),
                heard.as_deref(),
                |piece| {
                    let text = match piece {
                        crate::services::omni_voice::OmniDelta::Text(text) => Some(text),
                        crate::services::omni_voice::OmniDelta::Audio(sound) => {
                            // Heard is said: no going back to the words alone.
                            said_anything.store(true, std::sync::atomic::Ordering::Relaxed);
                            out.send(&sound);
                            None
                        }
                    };
                    let shown = text
                        .map(|text| on_delta(crate::services::analyzer::StreamDelta::Text(text)));
                    async move {
                        match shown {
                            Some(shown) => shown.await,
                            None => true,
                        }
                    }
                },
            )
            .await;
            // Nothing of it reached them: say it the usual way instead.
            match spoken {
                Err(error) if !said_anything.load(std::sync::atomic::Ordering::Relaxed) => {
                    tracing::warn!(
                        user_id,
                        %error,
                        "[Chat] her own voice did not answer; the words alone"
                    );
                }
                spoken => streamed = Some(spoken),
            }
        }
        drop(voice);
        let streamed = match streamed {
            Some(streamed) => streamed,
            None => {
                analyzer
                    .analyze_stream_parts_with_images(&prompt, images_of(request), on_delta)
                    .await
            }
        };
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
                tracing::warn!(
                    user_id,
                    "[Chat] reply cut off halfway; keeping what was shown"
                );
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
                if let Some(music) = music {
                    spawn_chat_music_control(
                        music,
                        user_id,
                        session_id.as_deref(),
                        progress_tx.clone(),
                    );
                }
                if let Some(directive) = directive {
                    spawn_model_outfit_overlay(
                        self.db.clone(),
                        user_id,
                        session_id,
                        directive,
                        progress_tx.clone(),
                    );
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
}

#[cfg(test)]
#[path = "chat_stream_replay.rs"]
mod replay;

#[cfg(test)]
mod prompt_size {
    use super::*;

    /// How long one private chat turn's prompt is on the site, part by part,
    /// read only: MEROPE_PROBE_USER (default 1), MEROPE_PROBE_SAID (default a
    /// greeting), MEROPE_PROBE_DUMP=<file> to keep the prompt. Recall cannot
    /// mark what it brought up as used on a read-only connection, so what it
    /// would bring is missing here and counted apart.
    #[tokio::test]
    #[ignore = "reads the site's database and asks its model"]
    async fn one_chat_turn_on_the_site() {
        let db = crate::services::agent::semantic_eval::load_configured_lite().await;
        crate::services::process_db::set_process_database(db.clone());
        let user_id: i32 = std::env::var("MEROPE_PROBE_USER")
            .ok()
            .and_then(|id| id.parse().ok())
            .unwrap_or(1);
        let said = std::env::var("MEROPE_PROBE_SAID").unwrap_or_else(|_| "你好呀".to_string());
        let (session, _) = crate::services::agent::merope::store::latest_open_session(&db, user_id)
            .await
            .unwrap()
            .expect("a conversation");
        let history =
            crate::services::agent::sessions::load_session_history(&db, &session, 20, true)
                .await
                .unwrap();
        let request = UserRequest {
            raw_input: said.clone(),
            timestamp: chrono::Utc::now(),
            user_id,
            context: Some(RequestContext {
                interaction_mode: AgentInteractionMode::Chat,
                session_id: Some(session),
                conversation_history: Some(history.clone()),
                custom_data: Some(serde_json::json!({
                    "musicStatus": {
                        "isPlaying": true,
                        "currentSong": { "name": "晴天", "artist": "周杰伦" },
                    }
                })),
                ..Default::default()
            }),
        };
        crate::services::agent::merope::remembering::begin(user_id, "private", &said);
        let began = std::time::Instant::now();
        let prompt = Agent::new(db.clone())
            .await
            .chat_response_prompt(&request)
            .await;
        println!("put together in {:?}", began.elapsed());
        let history_chars: usize = history.iter().map(|m| m.content.chars().count()).sum();
        let mut parts: Vec<(usize, String)> = Vec::new();
        let mut title = "(before any heading)".to_string();
        let mut chars = 0;
        for line in prompt.lines() {
            if line.starts_with("## ") || line.starts_with("# ") {
                parts.push((chars, title));
                title = line.chars().take(70).collect();
                chars = 0;
            }
            chars += line.chars().count() + 1;
        }
        parts.push((chars, title));
        for (chars, title) in &parts {
            println!("{chars:>6}  {title}");
        }
        println!(
            "{:>6}  total ({} parts; history {} messages, {history_chars} chars)",
            prompt.chars().count(),
            parts.len(),
            history.len()
        );
        if let Ok(path) = std::env::var("MEROPE_PROBE_DUMP") {
            std::fs::write(path, &prompt).unwrap();
        }
    }
}
