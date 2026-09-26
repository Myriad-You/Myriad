//! A run's events as an ordered stream of envelopes: what already happened,
//! then what happens, until the run ends. Every channel reads a run this way.

use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use futures::stream::Stream;
use serde_json::{Value, json};

use crate::services::agent::AgentProgressEvent;
use crate::services::agent::run_hub::{AgentRun, AgentRunEnvelope};

pub(crate) fn agent_run_event_is_terminal(event: &AgentProgressEvent) -> bool {
    match event {
        AgentProgressEvent::TaskCompleted { response, .. } => {
            response.get("streamTerminal").and_then(Value::as_bool) == Some(true)
                || response.pointer("/task/status").and_then(Value::as_str)
                    != Some("waiting_for_input")
        }
        AgentProgressEvent::Error { .. } => true,
        _ => false,
    }
}

/// Ensure session-message metadata always carries top-level run/task ids for reattach.
/// Merges into an existing JSON object (e.g. ApiResponse value) without dropping fields.
pub(crate) fn session_metadata_with_run_identity(
    base: Option<Value>,
    run_id: &str,
    task_id: &str,
) -> Value {
    let mut meta = match base {
        Some(Value::Object(map)) => Value::Object(map),
        Some(other) => json!({ "data": other }),
        None => json!({}),
    };
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("runId".to_string(), json!(run_id));
        obj.insert("taskId".to_string(), json!(task_id));
        // snake_case aliases `run_id` / `task_id`
        obj.insert("run_id".to_string(), json!(run_id));
        obj.insert("task_id".to_string(), json!(task_id));
        if !obj.contains_key("task") {
            obj.insert(
                "task".to_string(),
                json!({ "taskId": task_id, "status": "running" }),
            );
        }
    }
    meta
}

pub(crate) fn agent_run_envelopes(
    run: Arc<AgentRun>,
) -> impl Stream<Item = crate::services::agent::run_hub::AgentRunEnvelope> {
    futures::stream::unfold(Some(RunEnvelopeFeed::Boot(run)), |feed| async move {
        let feed = feed?;
        return run_envelope_step(feed).await;
    })
}

pub(crate) enum RunEnvelopeFeed {
    Boot(Arc<AgentRun>),
    Live(RunEnvelopeLive),
}

pub(crate) struct RunEnvelopeLive {
    run: Arc<AgentRun>,
    receiver: tokio::sync::broadcast::Receiver<AgentRunEnvelope>,
    last_sequence: u64,
    registry_poll: tokio::time::Interval,
    pending: VecDeque<AgentRunEnvelope>,
    stop_after_pending: bool,
}

pub(crate) async fn run_envelope_step(
    feed: RunEnvelopeFeed,
) -> Option<(AgentRunEnvelope, Option<RunEnvelopeFeed>)> {
    let mut live = match feed {
        RunEnvelopeFeed::Boot(run) => {
            // 先订阅再读取快照；sequence 去重消除两者之间的竞态。
            let receiver = run.subscribe();
            let (history, last_sequence, already_completed) = run.snapshot().await;
            let mut pending = VecDeque::new();
            if !history
                .iter()
                .any(|envelope| matches!(&envelope.event, AgentProgressEvent::RunStarted { .. }))
            {
                pending.push_back(AgentRunEnvelope {
                    sequence: 0,
                    event: AgentProgressEvent::RunStarted {
                        run_id: run.run_id().to_string(),
                        session_id: run.session_id().map(str::to_string),
                    },
                });
            }
            pending.extend(history);
            let mut registry_poll = tokio::time::interval(Duration::from_secs(2));
            registry_poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            RunEnvelopeLive {
                run,
                receiver,
                last_sequence,
                registry_poll,
                pending,
                stop_after_pending: already_completed,
            }
        }
        RunEnvelopeFeed::Live(live) => live,
    };
    if let Some(envelope) = live.pending.pop_front() {
        let terminal = agent_run_event_is_terminal(&envelope.event);
        if terminal {
            return Some((envelope, None));
        }
        return Some((envelope, Some(RunEnvelopeFeed::Live(live))));
    }
    if live.stop_after_pending {
        return None;
    }
    loop {
        tokio::select! {
            received = live.receiver.recv() => match received {
                Ok(envelope) if envelope.sequence > live.last_sequence => {
                    live.last_sequence = envelope.sequence;
                    let terminal = agent_run_event_is_terminal(&envelope.event);
                    return Some((
                        envelope,
                        if terminal {
                            None
                        } else {
                            Some(RunEnvelopeFeed::Live(live))
                        },
                    ));
                }
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                    // 不关流：从内存快照补发遗漏事件，避免前端必须重开连接。
                    live.receiver = live.run.subscribe();
                    let snapshot = live.run.snapshot().await;
                    let (history, snap_seq, completed) = snapshot;
                    for envelope in history {
                        if envelope.sequence <= live.last_sequence {
                            continue;
                        }
                        live.last_sequence = envelope.sequence;
                        let terminal = agent_run_event_is_terminal(&envelope.event);
                        live.pending.push_back(envelope);
                        if terminal {
                            live.stop_after_pending = true;
                            break;
                        }
                    }
                    live.last_sequence = live.last_sequence.max(snap_seq);
                    if completed {
                        live.stop_after_pending = true;
                    }
                    if let Some(envelope) = live.pending.pop_front() {
                        let terminal = agent_run_event_is_terminal(&envelope.event);
                        return Some((
                            envelope,
                            if terminal || (live.pending.is_empty() && live.stop_after_pending)
                            {
                                None
                            } else {
                                Some(RunEnvelopeFeed::Live(live))
                            },
                        ));
                    }
                    if live.stop_after_pending {
                        return None;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Closed) => return None,
            },
            _ = live.registry_poll.tick() => {
                let refreshed = live.run.refresh_from_registry().await;
                for envelope in refreshed {
                    if envelope.sequence <= live.last_sequence {
                        continue;
                    }
                    live.last_sequence = envelope.sequence;
                    let terminal = agent_run_event_is_terminal(&envelope.event);
                    live.pending.push_back(envelope);
                    if terminal {
                        live.stop_after_pending = true;
                        break;
                    }
                }
                if let Some(envelope) = live.pending.pop_front() {
                    let terminal = agent_run_event_is_terminal(&envelope.event);
                    return Some((
                        envelope,
                        if terminal {
                            None
                        } else {
                            Some(RunEnvelopeFeed::Live(live))
                        },
                    ));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::run_hub::create_run_for_test as create_run;
    use tokio_stream::StreamExt;

    fn progress(message: &str) -> AgentProgressEvent {
        AgentProgressEvent::Progress {
            progress: 10,
            completed_steps: 0,
            total_steps: 1,
            message: message.to_string(),
        }
    }

    fn completed(message: &str) -> AgentProgressEvent {
        AgentProgressEvent::TaskCompleted {
            task_id: "task".to_string(),
            success: true,
            response: Box::new(json!({ "success": true, "message": message })),
        }
    }

    #[test]
    fn task_completed_is_terminal_unless_waiting_for_input() {
        assert!(agent_run_event_is_terminal(&completed("done")));
        assert!(!agent_run_event_is_terminal(
            &AgentProgressEvent::TaskCompleted {
                task_id: "task".into(),
                success: true,
                response: Box::new(json!({
                    "success": true,
                    "task": { "status": "waiting_for_input" }
                })),
            }
        ));
        assert!(agent_run_event_is_terminal(&AgentProgressEvent::Error {
            task_id: None,
            message: "boom".into(),
            code: "test".into(),
        }));
        assert!(!agent_run_event_is_terminal(&progress("working")));
    }

    async fn collect_until_end(run: Arc<AgentRun>) -> Vec<AgentRunEnvelope> {
        let mut stream = std::pin::pin!(agent_run_envelopes(run));
        let mut out = Vec::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
        loop {
            match tokio::time::timeout_at(deadline, stream.next()).await {
                Ok(Some(envelope)) => out.push(envelope),
                Ok(None) => return out,
                Err(_) => panic!(
                    "agent_run_envelopes did not end; got {} event(s)",
                    out.len()
                ),
            }
        }
    }

    #[tokio::test]
    async fn completed_history_replays_then_ends() {
        let run = crate::services::agent::run_hub::create_run_for_test(
            8101,
            Some("envelope-completed".into()),
        )
        .await;
        run.publish(progress("working")).await;
        run.publish(completed("done")).await;

        let events = collect_until_end(run).await;
        assert!(
            matches!(
                events.first().map(|e| &e.event),
                Some(AgentProgressEvent::RunStarted { .. })
            ),
            "first event: {:?}",
            events.first().map(|e| &e.event)
        );
        assert!(
            matches!(
                events.last().map(|e| &e.event),
                Some(AgentProgressEvent::TaskCompleted { .. })
            ),
            "last event: {:?}",
            events.last().map(|e| &e.event)
        );
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.event, AgentProgressEvent::Progress { message, .. } if message == "working"))
        );
        let sequences: Vec<u64> = events.iter().map(|e| e.sequence).collect();
        let mut sorted = sequences.clone();
        sorted.sort_unstable();
        assert_eq!(sequences, sorted, "replay must keep sequence order");
    }

    #[tokio::test]
    async fn missing_run_started_is_synthesized_then_history_follows() {
        let run = AgentRun::new_for_test("envelope-synth", 8102);
        run.publish(progress("no-start")).await;
        run.publish(completed("done")).await;

        let events = collect_until_end(run).await;
        assert!(matches!(
            &events[0].event,
            AgentProgressEvent::RunStarted {
                run_id,
                ..
            } if run_id == "envelope-synth"
        ));
        assert_eq!(events[0].sequence, 0);
        assert!(
            events
                .iter()
                .any(|e| matches!(&e.event, AgentProgressEvent::Progress { message, .. } if message == "no-start"))
        );
        assert!(matches!(
            events.last().map(|e| &e.event),
            Some(AgentProgressEvent::TaskCompleted { .. })
        ));
    }

    #[tokio::test]
    async fn terminal_event_closes_the_stream() {
        let run = crate::services::agent::run_hub::create_run_for_test(
            8103,
            Some("envelope-terminal".into()),
        )
        .await;
        let mut stream = std::pin::pin!(agent_run_envelopes(run.clone()));
        let started = stream.next().await.expect("RunStarted");
        assert!(matches!(
            started.event,
            AgentProgressEvent::RunStarted { .. }
        ));

        run.publish(completed("stop")).await;
        let terminal = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("terminal should arrive")
            .expect("stream open for terminal");
        assert!(matches!(
            terminal.event,
            AgentProgressEvent::TaskCompleted { .. }
        ));

        let ended = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("stream must end after terminal, not wait on the 2s registry poll");
        assert!(ended.is_none());
    }

    #[tokio::test]
    async fn waiting_for_input_does_not_close_the_stream() {
        let run = crate::services::agent::run_hub::create_run_for_test(
            8104,
            Some("envelope-wait".into()),
        )
        .await;
        let mut stream = std::pin::pin!(agent_run_envelopes(run.clone()));
        let _ = stream.next().await;

        run.publish(AgentProgressEvent::TaskCompleted {
            task_id: "task".into(),
            success: true,
            response: Box::new(json!({
                "success": true,
                "message": "need input",
                "task": { "status": "waiting_for_input" }
            })),
        })
        .await;
        let waiting = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("waiting event")
            .expect("stream stays open");
        assert!(matches!(
            waiting.event,
            AgentProgressEvent::TaskCompleted { .. }
        ));

        let next = tokio::time::timeout(Duration::from_millis(200), stream.next()).await;
        assert!(
            next.is_err(),
            "waiting_for_input must keep the live select open"
        );

        run.publish(completed("after-wait")).await;
        let terminal = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("completion after wait")
            .expect("stream still open");
        assert!(matches!(
            terminal.event,
            AgentProgressEvent::TaskCompleted { success: true, .. }
        ));
    }

    #[tokio::test]
    async fn lagged_subscriber_replays_from_snapshot() {
        let run =
            crate::services::agent::run_hub::create_run_for_test(8105, Some("envelope-lag".into()))
                .await;
        let mut stream = std::pin::pin!(agent_run_envelopes(run.clone()));
        let started = stream.next().await.expect("RunStarted");
        assert!(matches!(
            started.event,
            AgentProgressEvent::RunStarted { .. }
        ));
        let last_before_flood = started.sequence;

        for i in 0..520 {
            run.publish(progress(&format!("lag-{i}"))).await;
        }

        let mut caught_up = 0usize;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
        loop {
            match tokio::time::timeout_at(deadline, stream.next()).await {
                Ok(Some(envelope)) => {
                    assert!(
                        envelope.sequence > last_before_flood,
                        "catch-up must skip already-yielded sequence {}",
                        envelope.sequence
                    );
                    if matches!(
                        &envelope.event,
                        AgentProgressEvent::Progress { message, .. } if message.starts_with("lag-")
                    ) {
                        caught_up += 1;
                    }
                    if caught_up >= 100 {
                        break;
                    }
                }
                Ok(None) => panic!("stream ended during lag catch-up after {caught_up} events"),
                Err(_) => panic!("lag catch-up stalled after {caught_up} events"),
            }
        }
        assert!(caught_up >= 100);
    }

    #[tokio::test]
    async fn error_event_closes_the_stream() {
        let run = crate::services::agent::run_hub::create_run_for_test(
            8106,
            Some("envelope-error".into()),
        )
        .await;
        let mut stream = std::pin::pin!(agent_run_envelopes(run.clone()));
        let _ = stream.next().await;

        run.publish(AgentProgressEvent::Error {
            task_id: None,
            message: "boom".into(),
            code: "test".into(),
        })
        .await;
        let error = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("error should arrive")
            .expect("stream open for error");
        assert!(matches!(error.event, AgentProgressEvent::Error { .. }));

        let ended = tokio::time::timeout(Duration::from_millis(500), stream.next())
            .await
            .expect("stream must end after Error");
        assert!(ended.is_none());
    }
}
