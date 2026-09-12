// Motion overlay for Agent turns: local floor, Lite refinement, attach to result.

use super::types::*;

fn request_rig_state(request: &UserRequest) -> Option<myriad_merope::RigStateSummary> {
    request
        .context
        .as_ref()
        .and_then(|context| context.rig_state.clone())
}

pub(super) async fn round_motion_style(
    request: &UserRequest,
    mood: Option<&crate::services::agent::merope::MoodTransition>,
) -> String {
    let Some(mood) = mood else {
        return request_rig_state(request)
            .map(|summary| summary.motion_style)
            .filter(|style| myriad_merope::RIG_STATE_MOTION_STYLES.contains(&style.as_str()))
            .unwrap_or_else(|| "even".to_string());
    };
    crate::services::agent::merope::resolve_round_motion_style(
        request_rig_state(request).as_ref(),
        mood.after.round() as i32,
        mood.arousal_after.round() as i32,
    )
    .await
}

pub(super) fn motion_context(
    request: &UserRequest,
    user_id: i32,
    phase: crate::services::agent::merope::MotionPhase,
    mood: crate::services::agent::merope::MoodTransition,
    motion_style: String,
    response_text: Option<String>,
    task_success: Option<bool>,
) -> crate::services::agent::merope::MotionContext {
    crate::services::agent::merope::MotionContext {
        user_id,
        phase,
        mood,
        activity: phase.activity().to_string(),
        user_text: request.raw_input.clone(),
        response_text,
        previous_phrases: Vec::new(),
        task_success,
        rig_state: request_rig_state(request),
        motion_style,
    }
}

pub(super) fn utterance_index_in_session(request: &UserRequest) -> u32 {
    request
        .context
        .as_ref()
        .and_then(|ctx| ctx.conversation_history.as_ref())
        .map(|history| {
            history
                .iter()
                .filter(|message| message.role == "user")
                .count()
                .saturating_sub(1) as u32
        })
        .unwrap_or(0)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub(super) enum MotionPublication {
    None,
    Refined,
}

pub(super) struct MotionRefinementGuard {
    task: tokio::task::JoinHandle<()>,
    published: std::sync::Arc<std::sync::atomic::AtomicU8>,
}

impl MotionRefinementGuard {
    /// The reply is complete, not necessarily its playback. Keep already-started
    /// refinement alive for the bounded playback window without retaining the
    /// request/lane or delaying terminal. The client closes it when playback ends.
    pub(super) fn finish_for_playback(
        mut self,
        live: super::playback_direction::PlaybackDirection,
    ) {
        tokio::spawn(async move {
            let completed = tokio::select! {
                biased;
                _ = live.cancelled() => true,
                _ = tokio::time::sleep(std::time::Duration::from_secs(60)) => false,
                _ = &mut self.task => true,
            };
            self.task.abort();
            if completed {
                live.finish();
            } else {
                live.close();
            }
        });
    }

    #[cfg(test)]
    fn has_refinement(&self) -> bool {
        self.published.load(std::sync::atomic::Ordering::Acquire) != MotionPublication::None as u8
    }

    pub(super) async fn stop(mut self) -> MotionPublication {
        self.task.abort();
        // Abort is a request, not a completion barrier. A concurrent send can
        // still finish before cancellation is observed; inspect publication
        // only after the task has stopped, before choosing the landing floor.
        let _ = (&mut self.task).await;
        match self.published.load(std::sync::atomic::Ordering::Acquire) {
            0 => MotionPublication::None,
            _ => MotionPublication::Refined,
        }
    }
}

impl Drop for MotionRefinementGuard {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) async fn publish_local_motion(
    context: &crate::services::agent::merope::MotionContext,
    progress_tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
) -> bool {
    if let Some(performance) = crate::services::agent::merope::local_directive(context) {
        return progress_tx
            .send(AgentProgressEvent::PerformancePlan { performance })
            .await
            .is_ok();
    }
    false
}

/// Work's one asynchronous reaction refinement.
pub(super) fn spawn_motion_refinement(
    context: crate::services::agent::merope::MotionContext,
    progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
) -> MotionRefinementGuard {
    spawn_motion_refinement_with(
        context,
        progress_tx,
        crate::services::agent::merope::refine_motion,
    )
}

fn spawn_motion_refinement_with<F, Fut>(
    context: crate::services::agent::merope::MotionContext,
    progress_tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    refine: F,
) -> MotionRefinementGuard
where
    F: FnOnce(crate::services::agent::merope::MotionContext) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Option<crate::services::agent::merope::PerformanceDirective>>
        + Send
        + 'static,
{
    let published = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(
        MotionPublication::None as u8,
    ));
    let did_publish = published.clone();
    let task = tokio::spawn(async move {
        if progress_tx.is_closed() {
            return;
        }
        if let Some(performance) = refine(context).await {
            if progress_tx
                .send(AgentProgressEvent::PerformancePlan { performance })
                .await
                .is_ok()
            {
                did_publish.store(
                    MotionPublication::Refined as u8,
                    std::sync::atomic::Ordering::Release,
                );
            }
        }
    });
    MotionRefinementGuard { task, published }
}

/// Motion is expendable under transport pressure; reserve room for prose.
/// Never await an action send from the speaking path or queue stale actions.
pub(super) fn try_publish_motion(
    tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>,
    performance: crate::services::agent::merope::PerformanceDirective,
) -> bool {
    let Ok(permit) = tx.try_reserve() else {
        tracing::debug!("[MeropeMotion] Optional motion dropped: transport unavailable");
        return false;
    };
    if tx.capacity() == 0 {
        tracing::debug!("[MeropeMotion] Optional motion dropped: reserved prose capacity");
        return false;
    }
    permit.send(AgentProgressEvent::PerformancePlan { performance });
    true
}

/// The speaking path owns only a bounded observer and a latest-value mailbox.
/// The round owns the task guard: dropping a cancelled round aborts the model.
pub(super) struct ChatMotionDelivery {
    context: crate::services::agent::merope::MotionContext,
    preview: crate::services::agent::merope::motion_preview::MotionPreview,
    latest: tokio::sync::watch::Sender<Option<String>>,
    floor_sent: bool,
}

impl ChatMotionDelivery {
    pub fn observe(&mut self, text: &str, tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>) {
        if let Some(text) = self.preview.push(text) {
            self.publish(text, tx);
        }
    }

    pub fn finish(&mut self, tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>) {
        if let Some(text) = self.preview.finish() {
            self.publish(text, tx);
        }
    }

    fn publish(&mut self, text: String, tx: &tokio::sync::mpsc::Sender<AgentProgressEvent>) {
        if !self.floor_sent {
            self.context.response_text = Some(text.clone());
            if let Some(performance) =
                crate::services::agent::merope::local_directive(&self.context)
            {
                self.floor_sent = try_publish_motion(tx, performance);
            }
        }
        // send_replace is synchronous; a slow director sees the latest window,
        // not an unbounded backlog of requests. No watch borrow crosses await.
        self.latest.send_replace(Some(text));
    }
}

pub(super) fn spawn_chat_motion_refinement(
    context: crate::services::agent::merope::MotionContext,
    tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    live: Option<super::playback_direction::PlaybackDirection>,
) -> (ChatMotionDelivery, MotionRefinementGuard) {
    spawn_chat_motion_refinement_in(
        context,
        tx,
        crate::services::agent::merope::refine_motion,
        live,
    )
}

#[cfg(test)]
pub(super) fn spawn_chat_motion_refinement_with<F, Fut>(
    context: crate::services::agent::merope::MotionContext,
    tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    refine: F,
) -> (ChatMotionDelivery, MotionRefinementGuard)
where
    F: FnMut(crate::services::agent::merope::MotionContext) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Option<crate::services::agent::merope::PerformanceDirective>>
        + Send
        + 'static,
{
    spawn_chat_motion_refinement_in(context, tx, refine, None)
}

fn spawn_chat_motion_refinement_in<F, Fut>(
    mut context: crate::services::agent::merope::MotionContext,
    tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    mut refine: F,
    live: Option<super::playback_direction::PlaybackDirection>,
) -> (ChatMotionDelivery, MotionRefinementGuard)
where
    F: FnMut(crate::services::agent::merope::MotionContext) -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Option<crate::services::agent::merope::PerformanceDirective>>
        + Send
        + 'static,
{
    context.phase = crate::services::agent::merope::MotionPhase::Delivery;
    context.activity = context.phase.activity().to_owned();
    let (latest, mut updates) = tokio::sync::watch::channel(None);
    let delivery = ChatMotionDelivery {
        context: context.clone(),
        preview: Default::default(),
        latest,
        floor_sent: false,
    };
    let published = std::sync::Arc::new(std::sync::atomic::AtomicU8::new(
        MotionPublication::None as u8,
    ));
    let did_publish = published.clone();
    // Subscribe before spawning so feedback arriving before the worker's first
    // scheduled poll is not marked as already seen by a late subscription.
    let mut observations = live.as_ref().map(|slot| slot.observations());
    let task = tokio::spawn(async move {
        let mut generation_open = true;
        let mut generated = None;
        let mut analyzed = String::new();
        let mut calls = 0;
        let mut playback_observed = false;
        let cancelled = async {
            if let Some(live) = &live {
                live.cancelled().await;
            } else {
                tx.closed().await;
            }
        };
        tokio::pin!(cancelled);
        loop {
            tokio::select! {
                biased;
                _ = &mut cancelled => break,
                update = updates.changed(), if generation_open => {
                    generation_open = update.is_ok();
                    generated = updates.borrow_and_update().clone();
                    if !generation_open && live.is_none() { break; }
                },
                _ = async {
                    if let Some(rx) = &mut observations {
                        let _ = rx.changed().await;
                        rx.borrow_and_update();
                    } else {
                        std::future::pending::<()>().await;
                    }
                } => {},
            }
            context.response_text = generated.clone();
            if let Some(observation) = live.as_ref().and_then(|slot| slot.observation()) {
                playback_observed = true;
                context.rig_state = myriad_merope::sanitize_rig_state(&observation.rig);
                context.response_text = Some(observation.upcoming_text);
            } else if playback_observed {
                // Once playback has been observed, stale telemetry must not
                // revert the director to already-generated, possibly spoken text.
                continue;
            }
            let text = context.response_text.as_deref().unwrap_or("").trim();
            // Cursor/pose feedback refreshes evidence, not the model bill.
            // A suffix of an examined window is already covered. Only new
            // upcoming content can request another single-flight refinement.
            if text.is_empty() || analyzed.contains(text) {
                continue;
            }
            if calls >= 12 {
                break;
            }
            calls += 1;
            analyzed = text.to_owned();
            // One in flight per round. New evidence replaces the pending value,
            // but doesn't repeatedly cancel a model that is about to finish.
            let result = tokio::select! {
                biased;
                _ = &mut cancelled => break,
                result = refine(context.clone()) => result,
            };
            if let Some(mut performance) = result {
                // Spoken beats must use grounded phrase timing. Unanchored cues
                // from an old window must not replay the immediate local beat.
                performance.plan.cues.clear();
                if let Some(observation) = live.as_ref().and_then(|slot| slot.observation()) {
                    performance.phrases = myriad_merope::grounded_speech_phrases(
                        &serde_json::to_value(&performance.phrases).unwrap_or_default(),
                        Some(&observation.upcoming_text),
                    );
                    if observation.upcoming_text.trim().is_empty() {
                        continue;
                    }
                } else if playback_observed {
                    continue;
                }
                if myriad_merope::plan_is_empty(&performance.plan) && performance.phrases.is_empty()
                {
                    continue;
                }
                let issued = performance.phrases.clone();
                let sent = if let Some(live) = &live {
                    live.publish(performance)
                } else {
                    try_publish_motion(&tx, performance)
                };
                if sent {
                    for phrase in issued {
                        context
                            .previous_phrases
                            .retain(|old| old.text != phrase.text);
                        context.previous_phrases.push(phrase);
                    }
                    let excess = context.previous_phrases.len().saturating_sub(6);
                    context.previous_phrases.drain(..excess);
                    did_publish.store(
                        MotionPublication::Refined as u8,
                        std::sync::atomic::Ordering::Release,
                    );
                }
            }
        }
        if let Some(live) = &live {
            live.finish();
        }
    });
    (delivery, MotionRefinementGuard { task, published })
}

pub(super) async fn attach_motion_to_result(
    result: Result<AgentResponse, String>,
    request: &UserRequest,
    mood: Option<crate::services::agent::merope::MoodTransition>,
    motion_style: &str,
) -> Result<AgentResponse, String> {
    let mut response = result?;
    let Some(mood) = mood else {
        return Ok(response);
    };
    let task_success = response.task.as_ref().and_then(|task| match task.status {
        TaskStatus::Completed => Some(true),
        TaskStatus::Failed | TaskStatus::Cancelled => Some(false),
        _ => None,
    });
    let phase = if task_success.is_some() {
        crate::services::agent::merope::MotionPhase::Outcome
    } else {
        crate::services::agent::merope::MotionPhase::Delivery
    };
    let context = motion_context(
        request,
        request.user_id,
        phase,
        mood,
        motion_style.to_string(),
        Some(response.message.clone()),
        task_success,
    );
    // The body carries the floor so a non-streaming client still gets acting.
    // Do not await Lite here — that would put its timeout in front of the reply.
    response.performance = crate::services::agent::merope::local_directive(&context);
    Ok(response)
}

#[cfg(test)]
pub(super) mod motion_refinement_tests {
    use super::*;
    use crate::services::agent::merope::{MoodTransition, MotionContext, MotionPhase};
    use std::sync::Arc;
    use std::time::Duration;

    pub(in crate::services::agent) fn context() -> MotionContext {
        MotionContext {
            user_id: 1,
            phase: MotionPhase::Reaction,
            mood: MoodTransition {
                before: 70.0,
                after: 70.0,
                arousal_before: 48.0,
                arousal_after: 48.0,
                band_before: "calm".into(),
                band_after: "calm".into(),
                delta: 0.0,
                cause: "test".into(),
                revision: 1,
            },
            activity: "thinking".into(),
            user_text: "tell me about that".into(),
            response_text: None,
            previous_phrases: Vec::new(),
            task_success: None,
            rig_state: None,
            motion_style: "even".into(),
        }
    }

    #[tokio::test]
    async fn playback_direction_new_evidence_has_a_per_round_call_budget() {
        use super::super::playback_direction::{PlaybackDirection, PlaybackObservation};
        let (tx, _rx) = tokio::sync::mpsc::channel(4);
        let live = PlaybackDirection::default();
        let (started, mut calls) = tokio::sync::mpsc::unbounded_channel();
        let (_delivery, guard) = spawn_chat_motion_refinement_in(
            context(),
            tx,
            move |_| {
                started.send(()).unwrap();
                async { None }
            },
            Some(live.clone()),
        );
        for index in 0..12 {
            live.observe(PlaybackObservation {
                upcoming_text: format!("新句段{index}。"),
                rig: serde_json::json!({}),
            });
            assert_eq!(
                tokio::time::timeout(Duration::from_secs(1), calls.recv())
                    .await
                    .unwrap(),
                Some(())
            );
        }
        live.observe(PlaybackObservation {
            upcoming_text: "第十三次不再调用。".into(),
            rig: serde_json::json!({}),
        });
        assert!(
            tokio::time::timeout(Duration::from_secs(1), live.read_after(0))
                .await
                .unwrap()
                .closed
        );
        assert_eq!(calls.recv().await, None);
        guard.stop().await;
    }

    #[tokio::test]
    async fn playback_direction_uses_live_text_drops_spoken_results_and_carries_intentions() {
        use super::super::playback_direction::{PlaybackDirection, PlaybackObservation};
        let (tx, _rx) = tokio::sync::mpsc::channel(16);
        let live = PlaybackDirection::default();
        let observe = |text: &str| {
            live.observe(PlaybackObservation {
                upcoming_text: text.into(),
                rig: serde_json::json!({"expression":"subdued", "speaking":true}),
            })
        };
        observe("也许可以试试。不过先解释清楚。");
        let (started, mut contexts) = tokio::sync::mpsc::unbounded_channel();
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let blocked = gate.clone();
        let (mut delivery, guard) = spawn_chat_motion_refinement_in(
            context(),
            tx.clone(),
            move |context| {
                let started = started.clone();
                let blocked = blocked.clone();
                async move {
                    started.send(context.clone()).unwrap();
                    blocked.acquire().await.unwrap().forget();
                    let mut result =
                        crate::services::agent::merope::local_directive(&context).unwrap();
                    result.phrases = myriad_merope::grounded_speech_phrases(
                        &serde_json::json!([
                            {"text":"也许可以试试。","intent":"hesitate"},
                            {"text":"不过先解释清楚。","intent":"explain"},
                            {"text":"你觉得呢？","intent":"check-in"}
                        ]),
                        context.response_text.as_deref(),
                    );
                    Some(result)
                }
            },
            Some(live.clone()),
        );
        delivery.observe("这是早已生成但现场已经说完的旧内容。", &tx);
        let first = tokio::time::timeout(Duration::from_secs(1), contexts.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            first.response_text.as_deref(),
            Some("也许可以试试。不过先解释清楚。")
        );
        assert_eq!(first.rig_state.unwrap().expression, "subdued");
        observe("不过先解释清楚。"); // First phrase commits while inference is in flight.
        gate.add_permits(1);
        let result = tokio::time::timeout(Duration::from_secs(1), live.read_after(0))
            .await
            .unwrap();
        let phrases = result.performance.unwrap().phrases;
        assert_eq!(phrases.len(), 1);
        assert_eq!(phrases[0].intent, "explain");
        assert!(
            tokio::time::timeout(Duration::from_millis(30), contexts.recv())
                .await
                .is_err(),
            "a suffix/pose update must not ask the same model question again"
        );
        drop(delivery);
        guard.finish_for_playback(live.clone());
        observe("不过先解释清楚。你觉得呢？"); // New queue evidence after prose terminal.
        let next = tokio::time::timeout(Duration::from_secs(1), contexts.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(next.previous_phrases, phrases);
        assert_eq!(
            next.response_text.as_deref(),
            Some("不过先解释清楚。你觉得呢？")
        );
        live.close();
    }

    #[tokio::test]
    async fn playback_direction_survives_text_completion_but_not_playback_cancel() {
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let live = super::super::playback_direction::PlaybackDirection::default();
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let blocked = gate.clone();
        let entered = Arc::new(tokio::sync::Notify::new());
        let signal = entered.clone();
        let (mut delivery, guard) = spawn_chat_motion_refinement_in(
            context(),
            tx.clone(),
            move |context| {
                let blocked = blocked.clone();
                let signal = signal.clone();
                async move {
                    signal.notify_one();
                    blocked.acquire().await.unwrap().forget();
                    crate::services::agent::merope::local_directive(&context)
                }
            },
            Some(live.clone()),
        );
        delivery.observe("这句话还没有播放完。", &tx);
        tokio::time::timeout(Duration::from_secs(1), entered.notified())
            .await
            .unwrap();
        drop(delivery); // Text generation has finished.
        guard.finish_for_playback(live.clone()); // Returns without the model.
        gate.add_permits(1);
        let state = tokio::time::timeout(Duration::from_secs(1), live.read_after(0))
            .await
            .unwrap();
        assert_eq!(state.version, 1);
        assert!(state.performance.is_some());
        live.close();
        assert!(!live.publish(state.performance.unwrap()));
    }

    #[tokio::test]
    async fn playback_direction_cancel_aborts_a_stalled_model_without_waiting_for_deadline() {
        let (tx, _rx) = tokio::sync::mpsc::channel(8);
        let live = super::super::playback_direction::PlaybackDirection::default();
        let dropped = Arc::new(std::sync::atomic::AtomicBool::new(false));
        struct DropFlag(Arc<std::sync::atomic::AtomicBool>);
        impl Drop for DropFlag {
            fn drop(&mut self) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let flag = dropped.clone();
        let entered = Arc::new(tokio::sync::Notify::new());
        let signal = entered.clone();
        let (mut delivery, guard) = spawn_chat_motion_refinement_in(
            context(),
            tx.clone(),
            move |_| {
                let flag = DropFlag(flag.clone());
                let signal = signal.clone();
                async move {
                    let _flag = flag;
                    signal.notify_one();
                    std::future::pending().await
                }
            },
            Some(live.clone()),
        );
        delivery.observe("还在播放。", &tx);
        tokio::time::timeout(Duration::from_secs(1), entered.notified())
            .await
            .unwrap();
        guard.finish_for_playback(live.clone());
        live.close();
        tokio::time::timeout(Duration::from_secs(1), async {
            while !dropped.load(std::sync::atomic::Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(live.read_after(0).await.version, 0);
    }

    #[tokio::test]
    async fn chat_director_coalesces_updates_and_never_runs_two_calls_at_once() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let (started, mut calls) = tokio::sync::mpsc::unbounded_channel();
        let gate = Arc::new(tokio::sync::Semaphore::new(0));
        let blocked = gate.clone();
        let (mut delivery, guard) =
            spawn_chat_motion_refinement_with(context(), tx.clone(), move |context| {
                let started = started.clone();
                let blocked = blocked.clone();
                async move {
                    started
                        .send(context.response_text.clone().unwrap())
                        .unwrap();
                    blocked.acquire().await.unwrap().forget();
                    crate::services::agent::merope::local_directive(&context)
                }
            });
        delivery.observe("第一句话。", &tx);
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), calls.recv())
                .await
                .unwrap()
                .unwrap(),
            "第一句话。"
        );
        for _ in 0..100 {
            delivery.observe("中间句子。", &tx);
        }
        delivery.observe("这是最新的一句。", &tx);
        assert!(calls.try_recv().is_err());
        gate.add_permits(1);
        let second = tokio::time::timeout(Duration::from_secs(1), calls.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(second.ends_with("这是最新的一句。"));
        assert!(second.chars().count() <= 900);
        assert!(calls.try_recv().is_err());
        // Stopping is a cancellation barrier, not a wait on the second model.
        tokio::time::timeout(Duration::from_secs(1), guard.stop())
            .await
            .unwrap();
        while rx.try_recv().is_ok() {}
        gate.add_permits(1);
        tokio::task::yield_now().await;
        assert!(rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn chat_director_full_transport_drops_actions_and_reserves_text_capacity() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(2);
        let performance = crate::services::agent::merope::local_directive(&context()).unwrap();
        assert!(try_publish_motion(&tx, performance.clone()));
        assert!(!try_publish_motion(&tx, performance.clone()));
        tx.try_send(AgentProgressEvent::SummaryToken {
            token: "正文".into(),
            done: false,
        })
        .unwrap();
        assert!(!try_publish_motion(&tx, performance.clone()));
        assert!(matches!(
            rx.recv().await,
            Some(AgentProgressEvent::PerformancePlan { .. })
        ));
        assert!(
            matches!(rx.recv().await, Some(AgentProgressEvent::SummaryToken { token, .. }) if token == "正文")
        );
        assert!(
            rx.try_recv().is_err(),
            "dropped actions must not appear later"
        );
        drop(rx);
        assert!(!try_publish_motion(&tx, performance));
    }

    #[tokio::test]
    async fn chat_director_model_failure_does_not_prevent_the_next_window() {
        let (tx, _rx) = tokio::sync::mpsc::channel(32);
        let (started, mut calls) = tokio::sync::mpsc::unbounded_channel();
        let (mut delivery, guard) =
            spawn_chat_motion_refinement_with(context(), tx.clone(), move |context| {
                let started = started.clone();
                async move {
                    started.send(context.response_text.unwrap()).unwrap();
                    None // Production timeout/error/continue are all no refinement.
                }
            });
        delivery.observe("第一句话。", &tx);
        tokio::time::timeout(Duration::from_secs(1), calls.recv())
            .await
            .unwrap()
            .unwrap();
        delivery.observe("第二句话。", &tx);
        assert!(tokio::time::timeout(Duration::from_secs(1), calls.recv())
            .await
            .unwrap()
            .unwrap()
            .ends_with("第二句话。"));
        assert!(!guard.has_refinement());
        guard.stop().await;
    }

    #[tokio::test]
    async fn chat_director_round_drop_aborts_inflight_work() {
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let entered = Arc::new(tokio::sync::Notify::new());
        let signal = entered.clone();
        let (mut delivery, guard) =
            spawn_chat_motion_refinement_with(context(), tx.clone(), move |_| {
                let signal = signal.clone();
                async move {
                    signal.notify_one();
                    std::future::pending().await
                }
            });
        delivery.observe("现在说话。", &tx);
        tokio::time::timeout(Duration::from_secs(1), entered.notified())
            .await
            .unwrap();
        drop(guard);
        delivery.observe("取消后的文本。", &tx);
        drop(delivery);
        drop(tx);
        // Only the immediate floor can remain. The worker drops its sender.
        let drain = async {
            while let Some(event) = rx.recv().await {
                let AgentProgressEvent::PerformancePlan { performance } = event else {
                    panic!("unexpected event")
                };
                assert!(performance.phrases.is_empty());
            }
        };
        tokio::time::timeout(Duration::from_secs(1), drain)
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn cancelled_refinement_cannot_publish_after_its_turn() {
        let (tx, mut events) = tokio::sync::mpsc::channel(4);
        let entered = Arc::new(tokio::sync::Notify::new());
        let signal = entered.clone();
        let guard = spawn_motion_refinement_with(context(), tx, move |_| async move {
            signal.notify_one();
            std::future::pending().await
        });
        tokio::time::timeout(Duration::from_secs(1), entered.notified())
            .await
            .unwrap();
        assert_eq!(guard.stop().await, MotionPublication::None);
        assert!(tokio::time::timeout(Duration::from_secs(1), events.recv())
            .await
            .unwrap()
            .is_none());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn stopping_at_publication_boundary_reports_exactly_what_was_sent() {
        for _ in 0..128 {
            let (tx, mut events) = tokio::sync::mpsc::channel(1);
            let guard = spawn_motion_refinement_with(context(), tx, |context| async move {
                crate::services::agent::merope::local_directive(&context)
            });
            tokio::task::yield_now().await;
            let published = guard.stop().await;
            assert_eq!(
                published == MotionPublication::Refined,
                events.recv().await.is_some()
            );
            assert!(events.recv().await.is_none());
        }
    }
}
