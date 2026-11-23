//! A link's socket, apart from what is done with what comes on it: a task of
//! its own reads the child's frames and answers its pings at once, while the
//! link's loop works through the frames one at a time, in the order they
//! came. A child that pings while a frame takes long is answered, and stays.
//!
//! What waits to be worked through is bounded (frames and bytes); once it is
//! full nothing more is read until there is room, and nothing is let go.
//! The child's own close waits its turn too: what it sent before it is
//! worked through first.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::time::Duration;

use axum::extract::ws::{CloseFrame, Message, Utf8Bytes, WebSocket};
use futures::stream::{SplitSink, SplitStream};
use futures::{SinkExt, StreamExt};
use serde_json::{Value, json};
use tokio::sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc};
use tokio::time::Instant;

use super::connection::Inbound;
use super::contract::CONTRACT;
use super::ended::Ended;

/// Frames read and not yet worked through.
const WAITING_FRAMES: usize = 256;
/// Their text, in frames of the largest size.
const WAITING_LARGEST: usize = 16;
/// A frame longer than this is no ping.
const PING_BYTES: usize = 512;
/// How long a link whose socket takes no more waits for what the child said
/// of it.
const LAST_WORDS: Duration = Duration::from_millis(500);

/// What came on the socket, in its order.
pub(super) enum Came {
    Text {
        text: Utf8Bytes,
        /// A ping already answered as it was read.
        answered: bool,
        /// Its room among what waits, given back once it is worked through.
        _room: Option<OwnedSemaphorePermit>,
    },
    Binary,
    /// Nothing more comes.
    Ended(Ended),
}

/// The socket's way out, and what is known of the link outside its loop.
pub(super) struct Wire {
    out: Mutex<Out>,
    /// The child's highest seq worked through: what every frame acks.
    settled: AtomicU64,
    /// The welcome went out: a ping is answered as it is read.
    welcomed: AtomicBool,
    last_heard: std::sync::Mutex<Instant>,
    /// What waits is full: it is the site that is behind, not the child that
    /// is silent.
    behind: AtomicBool,
}

struct Out {
    sink: SplitSink<WebSocket, Message>,
    /// The last seq this end sent.
    sent: u64,
}

impl Wire {
    pub(super) fn new(sink: SplitSink<WebSocket, Message>) -> Arc<Self> {
        Arc::new(Self {
            out: Mutex::new(Out { sink, sent: 0 }),
            settled: AtomicU64::new(0),
            welcomed: AtomicBool::new(false),
            last_heard: std::sync::Mutex::new(Instant::now()),
            behind: AtomicBool::new(false),
        })
    }

    /// The child's frames up to `seq` are worked through.
    pub(super) fn settled(&self, seq: u64) {
        self.settled.store(seq, Ordering::Release);
    }

    /// The welcome is out.
    pub(super) fn welcomed(&self) {
        self.welcomed.store(true, Ordering::Release);
    }

    /// Whether nothing was heard of the child for longer than `most`.
    pub(super) fn silent(&self, most: Duration) -> bool {
        let last = match self.last_heard.lock() {
            Ok(last) => *last,
            Err(poisoned) => *poisoned.into_inner(),
        };
        !self.behind.load(Ordering::Acquire) && last.elapsed() > most
    }

    fn heard(&self) {
        match self.last_heard.lock() {
            Ok(mut last) => *last = Instant::now(),
            Err(poisoned) => *poisoned.into_inner() = Instant::now(),
        }
    }

    /// Sends a frame as the next one of this end; `numbered` is told its seq
    /// before it goes. Whether it went.
    pub(super) async fn send(
        &self,
        t: &'static str,
        id: Option<String>,
        body: Value,
        numbered: impl FnOnce(u64),
    ) -> bool {
        let mut out = self.out.lock().await;
        out.sent += 1;
        numbered(out.sent);
        let mut envelope = json!({
            "v": CONTRACT.encoding.frame_version,
            "t": t,
            "seq": out.sent,
            "ack": self.settled.load(Ordering::Acquire),
            "at": crate::clock::now().timestamp_millis(),
            "body": body,
        });
        if let Some(id) = id {
            envelope["id"] = id.into();
        }
        // What the host builds is the contract's; one the core refuses is the
        // host's mistake, and goes out as it did before the core read frames.
        if let Err(refusal) = super::contract::core_check(&envelope) {
            tracing::warn!(t, code = %refusal.code, detail = %refusal.detail, "[link] the shared core refuses a frame the host sends");
        }
        out.sink
            .send(Message::Text(envelope.to_string().into()))
            .await
            .is_ok()
    }

    /// Closes the socket.
    pub(super) async fn close(&self, frame: CloseFrame) {
        let _ = self
            .out
            .lock()
            .await
            .sink
            .send(Message::Close(Some(frame)))
            .await;
    }

    /// Answers `text` when it is a ping that may be answered before its turn.
    /// Its turn still comes: the ack it carries is read then, and its own seq
    /// acked.
    async fn answer_ping(&self, text: &str) -> bool {
        if text.len() > PING_BYTES || !self.welcomed.load(Ordering::Acquire) {
            return false;
        }
        let ping = serde_json::from_str::<Inbound>(text).is_ok_and(|frame| {
            frame.t == "ping" && frame.v == Some(CONTRACT.encoding.frame_version)
        });
        ping && self.send("pong", None, json!({}), |_| {}).await
    }
}

/// The task that reads a link's socket; it stops when this is dropped.
pub(super) struct Reading(tokio::task::AbortHandle);

impl Drop for Reading {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Reads `stream` from now on: what came, in its order.
pub(super) fn read(
    stream: SplitStream<WebSocket>,
    wire: Arc<Wire>,
) -> (mpsc::Receiver<Came>, Reading) {
    let (came, frames) = mpsc::channel(WAITING_FRAMES);
    let task = tokio::spawn(reading(stream, wire, came));
    (frames, Reading(task.abort_handle()))
}

async fn reading(mut stream: SplitStream<WebSocket>, wire: Arc<Wire>, frames: mpsc::Sender<Came>) {
    let most = CONTRACT.limits.max_frame_bytes;
    let room = Arc::new(Semaphore::new(most * WAITING_LARGEST));
    loop {
        let came = match stream.next().await {
            Some(Ok(Message::Text(text))) => {
                wire.heard();
                let answered = wire.answer_ping(text.as_str()).await;
                let bytes = u32::try_from(text.len().clamp(1, most)).unwrap_or(u32::MAX);
                wire.behind.store(true, Ordering::Release);
                let room = room.clone().acquire_many_owned(bytes).await.ok();
                Came::Text {
                    text,
                    answered,
                    _room: room,
                }
            }
            Some(Ok(Message::Binary(_))) => {
                wire.heard();
                Came::Binary
            }
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {
                wire.heard();
                continue;
            }
            Some(Ok(Message::Close(frame))) => Came::Ended(Ended::by_child(frame)),
            Some(Err(error)) => Came::Ended(Ended::cut(&error.to_string())),
            None => Came::Ended(Ended::cut("the socket ended")),
        };
        let last = matches!(came, Came::Ended(_));
        wire.behind.store(true, Ordering::Release);
        if frames.send(came).await.is_err() || last {
            return;
        }
        // Heard up to here, however long there was no room.
        wire.heard();
        wire.behind.store(false, Ordering::Release);
    }
}

/// How a link ended whose socket would take no more: as the child said, when
/// that comes soon.
pub(super) async fn unsent(frames: &mut mpsc::Receiver<Came>) -> Ended {
    let said = async {
        while let Some(came) = frames.recv().await {
            if let Came::Ended(ended) = came {
                return ended;
            }
        }
        Ended::cut("a frame could not be sent")
    };
    tokio::time::timeout(LAST_WORDS, said)
        .await
        .unwrap_or_else(|_| Ended::cut("a frame could not be sent"))
}

/// A test's hold on a frame being worked through, as one that takes long.
#[cfg(test)]
pub(super) mod held {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};

    use tokio::sync::oneshot;

    type Hold = (oneshot::Sender<()>, oneshot::Receiver<()>);
    static HELD: LazyLock<Mutex<HashMap<(String, String), Hold>>> = LazyLock::new(Mutex::default);

    /// The device's next frame `t` is worked through only once the sender is
    /// used (or dropped); the receiver hears when its turn came.
    pub(crate) fn next(device_id: &str, t: &str) -> (oneshot::Receiver<()>, oneshot::Sender<()>) {
        let (began, begun) = oneshot::channel();
        let (go, gone) = oneshot::channel();
        HELD.lock()
            .unwrap()
            .insert((device_id.to_string(), t.to_string()), (began, gone));
        (begun, go)
    }

    pub(crate) async fn wait(device_id: &str, t: &str) {
        let hold = HELD
            .lock()
            .unwrap()
            .remove(&(device_id.to_string(), t.to_string()));
        if let Some((began, gone)) = hold {
            let _ = began.send(());
            let _ = gone.await;
        }
    }
}
