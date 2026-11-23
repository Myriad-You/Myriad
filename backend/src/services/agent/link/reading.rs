//! A link's socket, apart from what is done with what comes on it: a task of
//! its own reads the child's frames and answers its pings at once, while the
//! link's loop works through the frames one at a time, in the order they
//! came. A child that pings while a frame takes long is answered; one that
//! sends other frames meanwhile pings nothing, and is pinged; either stays.
//! Not for ever: a step of the loop's that takes longer than [`LONGEST`] is
//! stuck for all the site knows, so the link is no longer kept up, and the
//! child gives it up and comes back on another. It is logged as it passes
//! that; once the child has left first, the socket is read no more, and a
//! step stuck then is not.
//!
//! What waits to be worked through is bounded (frames and bytes); once it is
//! full nothing more is read until there is room, and nothing is let go.
//! The child's own close waits its turn too: what it sent before it is
//! worked through first, though nothing can be sent to it any more.

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
use super::held_up::frame_type;

/// Frames read and not yet worked through.
const WAITING_FRAMES: usize = 256;
/// Their text, in frames of the largest size.
const WAITING_LARGEST: usize = 16;
/// A frame longer than this is no ping.
const PING_BYTES: usize = 512;
/// How long a link whose socket takes no more waits for the next of what the
/// child sent before.
pub(super) const LAST_WORDS: Duration = Duration::from_millis(500);
/// How long it works through them in all.
pub(super) const LAST_FRAMES: Duration = Duration::from_secs(30);
/// A step of the loop's that takes longer keeps the link up no more.
const LONGEST: Duration = Duration::from_secs(120);

#[cfg(test)]
thread_local! {
    /// A test's own [`LONGEST`], for the links served on its thread.
    pub(super) static LONGEST_IN_TEST: std::cell::Cell<Option<Duration>> =
        const { std::cell::Cell::new(None) };
}

fn longest() -> Duration {
    #[cfg(test)]
    if let Some(longest) = LONGEST_IN_TEST.with(std::cell::Cell::get) {
        return longest;
    }
    LONGEST
}

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
    heard: Heard,
    /// What the loop is at.
    step: std::sync::Mutex<Option<Step>>,
    longest: Duration,
    /// The link in the registry.
    device: String,
    registered: u64,
}

struct Out {
    sink: SplitSink<WebSocket, Message>,
    /// The last seq this end sent, and when.
    sent: u64,
    sent_at: Instant,
}

/// When the child was last heard.
struct Heard {
    last: std::sync::Mutex<Instant>,
    /// What came is not read on from: it is the site that is behind, not the
    /// child that is silent.
    behind: AtomicBool,
}

impl Heard {
    fn new() -> Self {
        Self {
            last: std::sync::Mutex::new(Instant::now()),
            behind: AtomicBool::new(false),
        }
    }

    fn now(&self) {
        *lock(&self.last) = Instant::now();
    }

    /// Something came, and nothing more is read until it has its place.
    fn behind(&self) {
        self.now();
        self.behind.store(true, Ordering::Release);
    }

    /// Reading again: heard up to here, however long there was no room.
    fn reading(&self) {
        self.now();
        self.behind.store(false, Ordering::Release);
    }

    /// Whether nothing was heard for longer than `most`. Whether it is behind
    /// is read first: it is set straight only after the last time is.
    fn silent(&self, most: Duration) -> bool {
        !self.behind.load(Ordering::Acquire) && lock(&self.last).elapsed() > most
    }
}

fn lock<T>(of: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    of.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What the link's loop is at, as the log names it.
pub(super) enum At {
    Frame(Utf8Bytes),
    Own(&'static str),
}

struct Step {
    at: At,
    began: Instant,
    /// It was logged as stuck.
    told: bool,
}

/// The loop is at a step until this is dropped.
pub(super) struct Working(Arc<Wire>);

impl Drop for Working {
    fn drop(&mut self) {
        *lock(&self.0.step) = None;
    }
}

impl Wire {
    pub(super) fn new(
        sink: SplitSink<WebSocket, Message>,
        device: &str,
        registered: u64,
    ) -> Arc<Self> {
        Arc::new(Self {
            out: Mutex::new(Out {
                sink,
                sent: 0,
                sent_at: Instant::now(),
            }),
            settled: AtomicU64::new(0),
            welcomed: AtomicBool::new(false),
            heard: Heard::new(),
            step: std::sync::Mutex::new(None),
            longest: longest(),
            device: device.to_string(),
            registered,
        })
    }

    /// The loop begins a step.
    pub(super) fn working(self: &Arc<Self>, at: At) -> Working {
        *lock(&self.step) = Some(Step {
            at,
            began: Instant::now(),
            told: false,
        });
        Working(self.clone())
    }

    /// Whether the loop's step takes too long to come to an end; logged once
    /// as it does, since a step that never ends logs nothing itself.
    fn stuck(&self) -> bool {
        let mut step = lock(&self.step);
        let Some(step) = step
            .as_mut()
            .filter(|step| step.began.elapsed() > self.longest)
        else {
            return false;
        };
        if !std::mem::replace(&mut step.told, true) {
            let t = match &step.at {
                At::Frame(text) => frame_type(text.as_str()),
                At::Own(name) => name,
            };
            tracing::warn!(device = %self.device, t, took_ms = step.began.elapsed().as_millis(), "[link] a step of the link has not ended; it is kept up no more");
        }
        true
    }

    /// Whether the link is kept up while its loop is at something: welcomed,
    /// not stuck, and still the device's link.
    fn kept_up(&self) -> bool {
        self.welcomed.load(Ordering::Acquire)
            && !self.stuck()
            && super::is_link(&self.device, self.registered)
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
        self.heard.silent(most)
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
        let went = out
            .sink
            .send(Message::Text(envelope.to_string().into()))
            .await
            .is_ok();
        if went {
            out.sent_at = Instant::now();
        }
        went
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
        if text.len() > PING_BYTES || !self.kept_up() {
            return false;
        }
        let ping = serde_json::from_str::<Inbound>(text).is_ok_and(|frame| {
            frame.t == "ping" && frame.v == Some(CONTRACT.encoding.frame_version)
        });
        ping && self.send("pong", None, json!({}), |_| {}).await
    }

    /// Pings the child whenever nothing was sent to it for a little over a
    /// heartbeat: the loop sends a ping every heartbeat itself, unless it is
    /// at something.
    async fn keep_up(&self, heartbeat: Duration) {
        let most = heartbeat + heartbeat / 4;
        loop {
            // A step that is stuck is logged as it is, whatever is sent.
            self.stuck();
            let idle = self.out.lock().await.sent_at.elapsed();
            if idle < most {
                tokio::time::sleep(most - idle).await;
            } else if !self.kept_up() || !self.send("ping", None, json!({}), |_| {}).await {
                tokio::time::sleep(heartbeat).await;
            }
        }
    }
}

/// The task that reads a link's socket; it stops when this is dropped.
pub(super) struct Reading(tokio::task::AbortHandle);

impl Drop for Reading {
    fn drop(&mut self) {
        self.0.abort();
    }
}

/// Reads `stream` from now on: what came, in its order. While it does, the
/// child hears from the site every `heartbeat` or so.
pub(super) fn read(
    stream: SplitStream<WebSocket>,
    wire: Arc<Wire>,
    heartbeat: Duration,
) -> (mpsc::Receiver<Came>, Reading) {
    let (came, frames) = mpsc::channel(WAITING_FRAMES);
    let read = reading(stream, wire.clone(), came);
    let task = tokio::spawn(async move {
        tokio::select! { () = read => {} () = wire.keep_up(heartbeat) => {} }
    });
    (frames, Reading(task.abort_handle()))
}

async fn reading(mut stream: SplitStream<WebSocket>, wire: Arc<Wire>, frames: mpsc::Sender<Came>) {
    let most = CONTRACT.limits.max_frame_bytes;
    let room = Arc::new(Semaphore::new(most * WAITING_LARGEST));
    loop {
        let came = match stream.next().await {
            Some(Ok(Message::Text(text))) => {
                wire.heard.behind();
                let answered = wire.answer_ping(text.as_str()).await;
                let bytes = u32::try_from(text.len().clamp(1, most)).unwrap_or(u32::MAX);
                let room = room.clone().acquire_many_owned(bytes).await.ok();
                Came::Text {
                    text,
                    answered,
                    _room: room,
                }
            }
            Some(Ok(Message::Binary(_))) => {
                wire.heard.behind();
                Came::Binary
            }
            Some(Ok(Message::Ping(_) | Message::Pong(_))) => {
                wire.heard.now();
                continue;
            }
            Some(Ok(Message::Close(frame))) => Came::Ended(Ended::by_child(frame)),
            Some(Err(error)) => Came::Ended(Ended::cut(&error.to_string())),
            None => Came::Ended(Ended::cut("the socket ended")),
        };
        let last = matches!(came, Came::Ended(_));
        wire.heard.behind.store(true, Ordering::Release);
        if frames.send(came).await.is_err() {
            return;
        }
        if last {
            #[cfg(test)]
            held::read_to_the_end(&wire.device);
            return;
        }
        wire.heard.reading();
    }
}

/// A test's hold on a frame being worked through, as one that takes long, and
/// its word that a device's socket was read to its end.
#[cfg(test)]
pub(super) mod held {
    use std::collections::HashMap;
    use std::sync::{LazyLock, Mutex};

    use tokio::sync::oneshot;

    type Hold = (oneshot::Sender<()>, oneshot::Receiver<()>);
    static HELD: LazyLock<Mutex<HashMap<(String, String), Hold>>> = LazyLock::new(Mutex::default);
    static ENDS: LazyLock<Mutex<HashMap<String, oneshot::Sender<()>>>> =
        LazyLock::new(Mutex::default);

    /// Hears when the device's socket is read to its end (its close, or cut
    /// off), what came before it read too.
    pub(crate) fn end(device_id: &str) -> oneshot::Receiver<()> {
        let (read, end) = oneshot::channel();
        ENDS.lock().unwrap().insert(device_id.to_string(), read);
        end
    }

    pub(crate) fn read_to_the_end(device_id: &str) {
        if let Some(read) = ENDS.lock().unwrap().remove(device_id) {
            let _ = read.send(());
        }
    }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn a_child_is_silent_only_while_the_site_reads_on() {
        let most = Duration::from_secs(30);
        let heard = Heard::new();
        let later = || tokio::time::sleep(Duration::from_secs(31));
        later().await;
        assert!(heard.silent(most));
        heard.now();
        assert!(!heard.silent(most));
        // What came waits for room: however long, the child is not silent.
        heard.behind();
        later().await;
        assert!(!heard.silent(most));
        // And is heard as of when it had its room.
        heard.reading();
        assert!(!heard.silent(most));
        later().await;
        assert!(heard.silent(most));
    }
}
