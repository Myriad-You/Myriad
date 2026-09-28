//! Admission for transport ingress, before spawning pairing/Work adapters.
//! Accepted work owns its permit; disconnecting a transport does not cancel it.
use std::sync::{Arc, LazyLock};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

pub(crate) const MAX_GATEWAY_MESSAGE_BYTES: usize = 1024 * 1024;
const MAX_TASKS: usize = 128;
const MAX_CHANNEL_TASKS: usize = 64;
const MAX_PAYLOAD_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy)]
pub(crate) enum Channel {
    Feishu,
    Telegram,
    Discord,
    Qq,
    OneBot,
}

pub(crate) struct IngressPermit {
    _channel: OwnedSemaphorePermit,
    _task: OwnedSemaphorePermit,
    _bytes: OwnedSemaphorePermit,
}

struct Budget {
    tasks: Arc<Semaphore>,
    bytes: Arc<Semaphore>,
    channels: [Arc<Semaphore>; 5],
    max_bytes: usize,
}

impl Budget {
    fn new(tasks: usize, channel_tasks: usize, bytes: usize) -> Self {
        Self {
            tasks: Arc::new(Semaphore::new(tasks)),
            bytes: Arc::new(Semaphore::new(bytes)),
            channels: std::array::from_fn(|_| Arc::new(Semaphore::new(channel_tasks))),
            max_bytes: bytes,
        }
    }

    fn try_acquire(&self, channel: Channel, bytes: usize) -> Option<IngressPermit> {
        if bytes > self.max_bytes {
            return None;
        }
        let channel = self.channels[channel as usize]
            .clone()
            .try_acquire_owned()
            .ok()?;
        let task = self.tasks.clone().try_acquire_owned().ok()?;
        let bytes = self
            .bytes
            .clone()
            .try_acquire_many_owned(bytes.max(1) as u32)
            .ok()?;
        Some(IngressPermit {
            _channel: channel,
            _task: task,
            _bytes: bytes,
        })
    }

    async fn acquire(&self, channel: Channel, bytes: usize) -> Option<IngressPermit> {
        if bytes > self.max_bytes {
            return None;
        }
        let channel = self.channels[channel as usize]
            .clone()
            .acquire_owned()
            .await
            .ok()?;
        let task = self.tasks.clone().acquire_owned().await.ok()?;
        let bytes = self
            .bytes
            .clone()
            .acquire_many_owned(bytes.max(1) as u32)
            .await
            .ok()?;
        Some(IngressPermit {
            _channel: channel,
            _task: task,
            _bytes: bytes,
        })
    }
}

static BUDGET: LazyLock<Budget> =
    LazyLock::new(|| Budget::new(MAX_TASKS, MAX_CHANNEL_TASKS, MAX_PAYLOAD_BYTES));

/// Nonblocking for gateways: saturation must not suspend heartbeat processing.
pub(crate) fn try_acquire(channel: Channel, payload_bytes: usize) -> Option<IngressPermit> {
    BUDGET.try_acquire(channel, payload_bytes)
}

/// Only the single Telegram long-poll loop waits here; no task is spawned yet.
/// The caller selects cancellation while retaining the unacknowledged batch.
pub(crate) async fn acquire(channel: Channel, payload_bytes: usize) -> Option<IngressPermit> {
    BUDGET.acquire(channel, payload_bytes).await
}

/// Count allocated JSON space, including spare array/string capacity. Objects
/// conservatively charge a full BTree node per member, rather than wire bytes.
/// Callers also account for owned credentials and any parsed event copy.
pub(crate) fn json_bytes(value: &serde_json::Value) -> usize {
    use serde_json::Value;
    let inline = std::mem::size_of::<Value>();
    let heap = match value {
        Value::String(text) => text.capacity(),
        Value::Array(values) => values
            .iter()
            .fold(values.capacity().saturating_mul(inline), |bytes, child| {
                bytes.saturating_add(json_bytes(child).saturating_sub(inline))
            }),
        Value::Object(values) => values.iter().fold(0usize, |bytes, (key, child)| {
            bytes
                .saturating_add(1024)
                .saturating_add(key.capacity())
                .saturating_add(json_bytes(child))
        }),
        _ => 0,
    };
    inline.saturating_add(heap)
}

/// Gateway reconnect delay after `attempt` consecutive transient failures:
/// 2, 4, 8, 16 s, then 30 s. Shared by every bot worker so their
/// reconnect behaviour cannot drift apart.
pub fn reconnect_backoff(attempt: u32) -> std::time::Duration {
    let secs = if attempt >= 6 {
        30
    } else {
        1u64 << attempt.min(5)
    };
    std::time::Duration::from_secs(secs.min(30))
}

#[cfg(test)]
mod tests {
    #[test]
    fn reconnect_backoff_grows_then_caps() {
        let secs: Vec<u64> = (1..=7)
            .map(|n| super::reconnect_backoff(n).as_secs())
            .collect();
        assert_eq!(secs, vec![2, 4, 8, 16, 30, 30, 30]);
    }

    use super::*;

    #[tokio::test]
    async fn bounded_admission_releases_slots_and_bytes_when_accepted_work_finishes() {
        let budget = Arc::new(Budget::new(3, 2, 12));
        let a = budget.try_acquire(Channel::Feishu, 4).unwrap();
        let b = budget.try_acquire(Channel::Feishu, 4).unwrap();
        assert!(budget.try_acquire(Channel::Feishu, 1).is_none());
        assert!(budget.try_acquire(Channel::Discord, 5).is_none());
        let c = budget.try_acquire(Channel::Telegram, 4).unwrap();
        assert!(budget.try_acquire(Channel::Qq, 1).is_none());
        let (release, done) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let _permit = a;
            let _ = done.await;
        });
        assert!(budget.try_acquire(Channel::Feishu, 1).is_none());
        release.send(()).unwrap();
        task.await.unwrap();
        assert!(budget.try_acquire(Channel::Qq, 4).is_some());
        drop((b, c));
        assert_eq!(budget.tasks.available_permits(), 3);
        assert_eq!(budget.bytes.available_permits(), 12);
    }

    #[tokio::test]
    async fn cancelling_a_waiter_releases_partially_acquired_permits() {
        let budget = Arc::new(Budget::new(2, 2, 4));
        let held = budget.try_acquire(Channel::Feishu, 4).unwrap();
        let waiting = {
            let budget = budget.clone();
            tokio::spawn(async move { budget.acquire(Channel::Telegram, 4).await })
        };
        tokio::task::yield_now().await;
        assert_eq!(budget.tasks.available_permits(), 0);
        waiting.abort();
        let _ = waiting.await;
        assert_eq!(budget.tasks.available_permits(), 1);
        drop(held);
        assert!(budget.try_acquire(Channel::Telegram, 4).is_some());
        assert!(budget.try_acquire(Channel::Qq, 5).is_none());
    }
    #[test]
    fn dense_json_and_spare_string_capacity_cannot_bypass_payload_admission() {
        let budget = Budget::new(3, 3, 128);
        let dense = serde_json::Value::Array(vec![serde_json::Value::Null; 20]);
        assert!(
            budget
                .try_acquire(Channel::Discord, json_bytes(&dense))
                .is_none()
        );
        let mut text = String::with_capacity(256);
        text.push('x');
        assert!(
            budget
                .try_acquire(
                    Channel::Feishu,
                    json_bytes(&serde_json::Value::String(text))
                )
                .is_none()
        );
    }
}
