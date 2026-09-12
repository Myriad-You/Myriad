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
}

impl WearStreamFilter {
    fn new(user_input: String) -> Self {
        Self {
            user_input,
            raw: String::new(),
            emitted: 0,
            fired_wear: false,
            fired_music: false,
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

impl Agent {
    /// 真正的流式 Chat 回复：使用 `analyze_stream_parts` 从 AI 模型逐 token 输出
    ///
    /// 构建包含人格 + 对话历史的 prompt，调用流式 AI 接口，
    /// 每个 token 实时推送给前端。AI 不可用时回退到模拟流式。
    pub(crate) async fn stream_chat_response(
        &self,
        request: &UserRequest,
        planner_reply: &str,
        progress_tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
    ) -> String {
        let analyzer = match crate::services::agent::merope::create_speaking_analyzer().await {
            Some(a) => a,
            None => {
                // AI 不可用，回退到模拟流式
                Self::stream_text_as_tokens_with_finish(progress_tx, planner_reply, false).await;
                return planner_reply.to_string();
            }
        };

        match self
            .stream_chat_response_with_analyzer(request, progress_tx, analyzer, None)
            .await
        {
            Ok(reply) => reply,
            Err(error) => {
                // Work 路径的兼容降级：Planner 已经产生了可读回复。
                tracing::warn!(%error, "[Agent] Streaming chat response failed, falling back to planner reply");
                Self::stream_text_as_tokens_with_finish(progress_tx, planner_reply, false).await;
                planner_reply.to_string()
            }
        }
    }

    /// Chat 模式的唯一模型入口。严格 Lite 不可用时直接失败，绝不借用
    /// Standard / Pro，否则“只聊天”会悄悄变成另一条 Work 费用路径。
    pub(crate) async fn stream_strict_lite_chat_response(
        &self,
        request: &UserRequest,
        progress_tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
        speech_delivery: Option<ChatDelivery>,
    ) -> Result<String, String> {
        let analyzer = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(None)
            .await
            .ok_or_else(|| "Lite model is not configured for Chat mode".to_string())?;
        self.stream_chat_response_with_analyzer(request, progress_tx, analyzer, speech_delivery)
            .await
    }

    pub(crate) async fn strict_lite_chat_response(
        &self,
        request: &UserRequest,
    ) -> Result<String, String> {
        let analyzer = crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(None)
            .await
            .ok_or_else(|| "Lite model is not configured for Chat mode".to_string())?;
        let prompt = self.chat_response_prompt(request).await;
        let response = analyzer
            .analyze(&prompt)
            .await
            .map_err(|error| error.to_string())?;
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
        let mut merope_block = crate::services::agent::merope::speaking_prompt_plain(
            &crate::services::agent::merope::speaking_prompt_with_query(
                request.user_id,
                Some(request.raw_input.as_str()),
            )
            .await,
        );
        if request.context.as_ref().is_some_and(|context| {
            context.interaction_mode == crate::services::agent::AgentInteractionMode::Chat
        }) {
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
            let player = crate::services::agent::chat_music::format_chat_player_section(music);
            if merope_block.is_empty() {
                merope_block = player;
            } else {
                merope_block = format!("{merope_block}\n\n{player}");
            }
        }
        let history = request
            .context
            .as_ref()
            .and_then(|context| context.conversation_history.as_deref())
            .unwrap_or(&[]);

        let custom = request
            .context
            .as_ref()
            .and_then(|context| context.custom_data.as_ref());
        let perception = crate::services::agent::chat_prompt::format_chat_scene(
            custom.and_then(|data| data.get("perception")),
            custom.and_then(|data| data.get("pageContent")),
            &request.raw_input,
        );
        crate::services::agent::chat_prompt::build_chat_lite_prompt_with_perception(
            &soul,
            &merope_block,
            history,
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
        match analyzer
            .analyze_stream_parts(&prompt, |delta| {
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
            .await
        {
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
                if let Some(delivery) = speech_delivery.as_ref() {
                    delivery
                        .lock()
                        .unwrap_or_else(|p| p.into_inner())
                        .finish(progress_tx);
                }
                Ok(full_text.trim().to_owned())
            }
            Ok(_) => Err("Chat model returned an empty response".to_string()),
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
