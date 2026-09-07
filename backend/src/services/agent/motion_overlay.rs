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
) -> (ChatMotionDelivery, MotionRefinementGuard) {
    spawn_chat_motion_refinement_with(context, tx, crate::services::agent::merope::refine_motion)
}

pub(super) fn spawn_chat_motion_refinement_with<F, Fut>(
    mut context: crate::services::agent::merope::MotionContext,
    tx: tokio::sync::mpsc::Sender<AgentProgressEvent>,
    mut refine: F,
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
    let task = tokio::spawn(async move {
        loop {
            tokio::select! {
                biased;
                _ = tx.closed() => break,
                update = updates.changed() => if update.is_err() { break },
            }
            let text = updates.borrow_and_update().clone();
            context.response_text = text;
            // One in flight per round. New evidence replaces the pending value,
            // but doesn't repeatedly cancel a model that is about to finish.
            let result = tokio::select! {
                biased;
                _ = tx.closed() => break,
                result = refine(context.clone()) => result,
            };
            if let Some(mut performance) = result {
                // Spoken beats must use grounded phrase timing. Unanchored cues
                // from an old window must not replay the immediate local beat.
                performance.plan.cues.clear();
                if try_publish_motion(&tx, performance) {
                    did_publish.store(
                        MotionPublication::Refined as u8,
                        std::sync::atomic::Ordering::Release,
                    );
                }
            }
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
    // The body carries the floor so a non-streaming client still gets acting;
    // awaiting Lite here used to put its whole timeout in front of the reply.
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
            task_success: None,
            rig_state: None,
            motion_style: "even".into(),
        }
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
