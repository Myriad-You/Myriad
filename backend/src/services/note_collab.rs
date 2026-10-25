//! 笔记协同：一篇文档一个广播房间。存在感 + 整篇快照，revision 做冲突。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use once_cell::sync::OnceCell;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

const ROOM_CAP: usize = 64;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NoteCollabEvent {
    #[serde(rename = "type")]
    pub kind: String,
    #[serde(default)]
    pub peer_id: String,
    #[serde(default)]
    pub user_id: i32,
    pub name: Option<String>,
    pub revision: Option<i64>,
    #[serde(default)]
    pub client_request_id: Option<String>,
    #[serde(default)]
    pub published_at: Option<i64>,
    pub cursor: Option<i32>,
    pub title: Option<String>,
    pub content_md: Option<String>,
    pub topic: Option<String>,
    pub image: Option<String>,
}

pub struct NoteCollabHub {
    rooms: Arc<Mutex<HashMap<i32, broadcast::Sender<NoteCollabEvent>>>>,
}

/// Owns one receiver and atomically removes its room when the final receiver
/// disappears. Dropping the task that owns this subscription performs cleanup.
pub struct NoteCollabSubscription {
    rooms: Arc<Mutex<HashMap<i32, broadcast::Sender<NoteCollabEvent>>>>,
    doc_id: i32,
    rx: Option<broadcast::Receiver<NoteCollabEvent>>,
}

impl NoteCollabSubscription {
    pub async fn recv(&mut self) -> Result<NoteCollabEvent, broadcast::error::RecvError> {
        self.rx.as_mut().expect("live subscription").recv().await
    }
}

impl Drop for NoteCollabSubscription {
    fn drop(&mut self) {
        // Serialize the final receiver drop with a concurrent subscription so
        // an abort cannot remove a room that a reconnect just joined.
        let mut rooms = self
            .rooms
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        drop(self.rx.take());
        if rooms
            .get(&self.doc_id)
            .is_some_and(|sender| sender.receiver_count() == 0)
        {
            rooms.remove(&self.doc_id);
        }
    }
}

impl NoteCollabHub {
    fn new() -> Self {
        Self {
            rooms: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn subscribe(&self, doc_id: i32) -> NoteCollabSubscription {
        let rx = {
            let mut rooms = self
                .rooms
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            rooms
                .entry(doc_id)
                .or_insert_with(|| broadcast::channel(ROOM_CAP).0)
                .subscribe()
        };
        NoteCollabSubscription {
            rooms: self.rooms.clone(),
            doc_id,
            rx: Some(rx),
        }
    }

    pub fn publish(&self, doc_id: i32, event: NoteCollabEvent) {
        let rooms = self
            .rooms
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        if let Some(sender) = rooms.get(&doc_id) {
            let _ = sender.send(event);
        }
    }

    #[cfg(test)]
    fn room_count(&self) -> usize {
        self.rooms
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .len()
    }
}

static HUB: OnceCell<Arc<NoteCollabHub>> = OnceCell::new();

pub fn note_collab_hub() -> Arc<NoteCollabHub> {
    HUB.get_or_init(|| Arc::new(NoteCollabHub::new())).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn event(kind: &str) -> NoteCollabEvent {
        NoteCollabEvent {
            kind: kind.into(),
            peer_id: String::new(),
            user_id: 1,
            name: None,
            revision: None,
            client_request_id: None,
            published_at: None,
            cursor: None,
            title: None,
            content_md: None,
            topic: None,
            image: None,
        }
    }

    #[test]
    fn client_presence_needs_no_server_assigned_identity() {
        let event: NoteCollabEvent =
            serde_json::from_str(r#"{"type":"presence","cursor":3}"#).unwrap();
        assert_eq!(event.kind, "presence");
        assert_eq!(event.cursor, Some(3));
        assert_eq!(event.peer_id, "");
        assert_eq!(event.user_id, 0);
    }

    #[tokio::test]
    async fn room_broadcasts_to_subscribers() {
        let hub = NoteCollabHub::new();
        let mut rx = hub.subscribe(7);
        let mut join = event("join");
        join.peer_id = "a".into();
        join.name = Some("Ada".into());
        join.cursor = Some(4);
        hub.publish(7, join);
        let event = rx.recv().await.expect("event");
        assert_eq!(event.kind, "join");
        assert_eq!(event.peer_id, "a");
        assert_eq!(event.cursor, Some(4));
    }

    #[test]
    fn publish_without_subscribers_does_not_create_rooms() {
        let hub = NoteCollabHub::new();

        for doc_id in 0..1_000 {
            hub.publish(doc_id, event("doc"));
        }

        assert_eq!(hub.room_count(), 0);
    }

    #[test]
    fn simultaneous_subscribers_own_room_until_the_last_drop() {
        let hub = NoteCollabHub::new();
        let rx_a = hub.subscribe(1);
        let rx_b = hub.subscribe(1);
        assert_eq!(hub.room_count(), 1);

        drop(rx_a);
        assert_eq!(hub.room_count(), 1, "the second subscriber still owns it");

        drop(rx_b);
        assert_eq!(hub.room_count(), 0, "the final drop must remove it");
    }

    #[test]
    fn repeated_subscription_churn_reclaims_every_room() {
        let hub = NoteCollabHub::new();
        for doc_id in 0..1_000 {
            drop(hub.subscribe(doc_id));
        }
        assert_eq!(hub.room_count(), 0);
    }

    #[tokio::test]
    async fn aborting_subscription_owner_reclaims_room() {
        let hub = Arc::new(NoteCollabHub::new());
        let task_hub = hub.clone();
        let task = tokio::spawn(async move {
            let _subscription = task_hub.subscribe(42);
            std::future::pending::<()>().await;
        });
        tokio::task::yield_now().await;
        assert_eq!(hub.room_count(), 1);

        task.abort();
        let _ = task.await;
        assert_eq!(hub.room_count(), 0);
    }
}
