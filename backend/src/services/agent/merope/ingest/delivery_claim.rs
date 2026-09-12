//! Process-local delivery arbitration, matching the in-memory speak-intent queue.
//! The per-addressee guard spans compose → delivery → transcript. Successful
//! transport acceptance is remembered before any further await, even if the
//! transcript write fails. This is not a durable outbox or a multi-replica lock.

use crate::services::agent::consciousness::SpeakIntent;
use chrono::{DateTime, Utc};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

const MAX_USERS: usize = 4096;
const MAX_KEYS: usize = 256;
const IDLE_RETENTION: Duration = Duration::from_secs(30 * 60);

#[derive(Default)]
struct History {
    topics: HashMap<String, DateTime<Utc>>,
    sources: HashMap<String, DateTime<Utc>>,
}

struct UserSlot {
    lock: Arc<AsyncMutex<History>>,
    used: Instant,
}

#[derive(Default)]
pub(super) struct DeliveryCoordinator {
    users: Mutex<HashMap<i32, UserSlot>>,
}

pub(super) struct DeliveryClaim {
    history: OwnedMutexGuard<History>,
}

impl DeliveryCoordinator {
    pub async fn claim(&self, intent: &SpeakIntent) -> Option<DeliveryClaim> {
        let lock = {
            let mut users = self.users.lock().ok()?;
            users.retain(|_, slot| {
                Arc::strong_count(&slot.lock) > 1 || slot.used.elapsed() < IDLE_RETENTION
            });
            if !users.contains_key(&intent.user_id) && users.len() >= MAX_USERS {
                return None;
            }
            let slot = users.entry(intent.user_id).or_insert_with(|| UserSlot {
                lock: Arc::new(AsyncMutex::new(History::default())),
                used: Instant::now(),
            });
            slot.used = Instant::now();
            slot.lock.clone()
        };
        let mut history = lock.lock_owned().await;
        let now = Utc::now();
        history.topics.retain(|_, until| *until > now);
        history.sources.retain(|_, until| *until > now);
        if intent.expires_at <= now
            || history.topics.contains_key(&intent.topic)
            || history.sources.contains_key(&intent.source_event_id)
            || history.topics.len() + history.sources.len() > MAX_KEYS - 2
        {
            return None;
        }
        Some(DeliveryClaim { history })
    }
}

impl DeliveryClaim {
    pub fn delivered(&mut self, intent: &SpeakIntent, repeat_minutes: i64) {
        let until = Utc::now() + chrono::Duration::minutes(repeat_minutes);
        self.history.topics.insert(intent.topic.clone(), until);
        self.history
            .sources
            .insert(intent.source_event_id.clone(), until.max(intent.expires_at));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::consciousness::new_speak_intent;

    fn intent(user: i32, source: &str) -> SpeakIntent {
        new_speak_intent(
            user,
            source.into(),
            "agent.merope.greeting".into(),
            "hello".into(),
            Default::default(),
            None,
        )
    }

    #[tokio::test]
    async fn concurrent_drains_cannot_compose_or_deliver_the_same_topic_twice() {
        let coordinator = Arc::new(DeliveryCoordinator::default());
        let first = intent(1, "source-a");
        let mut claim = coordinator.claim(&first).await.unwrap();
        let next = intent(1, "source-b");
        let worker = {
            let coordinator = coordinator.clone();
            tokio::spawn(async move { coordinator.claim(&next).await.is_some() })
        };
        // Another addressee progresses while this user's model is still running.
        assert!(coordinator.claim(&intent(2, "source-a")).await.is_some());
        tokio::task::yield_now().await;
        assert!(!worker.is_finished());
        claim.delivered(&first, 15);
        drop(claim); // Also covers a failed transcript write after accepted speech.
        assert!(!worker.await.unwrap());
        let mut same_source_other_topic = first.clone();
        same_source_other_topic.topic = "agent.merope.report_ready".into();
        assert!(coordinator.claim(&same_source_other_topic).await.is_none());
    }

    #[tokio::test]
    async fn suppression_cancellation_and_transport_failure_do_not_consume_delivery() {
        let coordinator = DeliveryCoordinator::default();
        let event = intent(1, "source");
        drop(coordinator.claim(&event).await.unwrap());
        let mut claim = coordinator.claim(&event).await.unwrap();
        claim.delivered(&event, 15);
        assert!(claim.history.topics.contains_key(&event.topic));
        drop(claim);
        assert!(coordinator.claim(&event).await.is_none());
    }
}
