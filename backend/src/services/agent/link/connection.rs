//! One link, from the upgrade to the close: the handshake, what the child
//! sends, the runs it follows, her mood, and the heartbeat that also checks
//! the device is still one.

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use axum::extract::ws::{CloseFrame, Message, WebSocket};
use futures::stream::SplitSink;
use futures::{SinkExt, StreamExt};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::{mpsc, oneshot};
use tokio::time::Instant;

use crate::services::agent::merope::{self, MoodTransition};
use crate::services::agent::notifications::{NotificationEvent, get_notification_manager};
use crate::services::agent::run::{
    ProcessContext, ProcessRequest, agent_run_envelopes, agent_run_event_is_terminal,
    start_for_user,
};
use crate::services::agent::run_hub::{AgentRun, get_live_run_for_user, get_run_for_user};
use crate::services::agent::{AgentInteractionMode, AgentProgressEvent};
use crate::services::persona_sync::devices::{self, Device, Refused};

use super::contract::{CONTRACT, Close, Timing, valid_id};
use super::turns::{self, Turn};

/// Frames a run follower or the mood follower may have waiting.
const QUEUED_FRAMES: usize = 256;
/// A child kept current (`conversations.live`) is told of a change to her
/// conversations as the database announces it (D45); this slow check is only
/// behind it, for an announcement missed.
const CONVERSATIONS_EVERY: std::time::Duration = std::time::Duration::from_secs(60);

#[cfg(test)]
thread_local! {
    /// A test's own slow check, for the links served on its thread.
    pub(super) static CONVERSATIONS_EVERY_IN_TEST: std::cell::Cell<Option<std::time::Duration>> =
        const { std::cell::Cell::new(None) };
}

fn conversations_every() -> std::time::Duration {
    #[cfg(test)]
    if let Some(every) = CONVERSATIONS_EVERY_IN_TEST.with(std::cell::Cell::get) {
        return every;
    }
    CONVERSATIONS_EVERY
}

/// A frame on its way to the child; the link numbers it as it goes out.
pub(super) struct Outgoing {
    pub(super) t: &'static str,
    id: Option<String>,
    pub(super) body: Value,
}

fn frame(t: &'static str, body: Value) -> Outgoing {
    Outgoing { t, id: None, body }
}

impl Outgoing {
    /// A frame that is not kept: an answer, or a latest value.
    pub(super) fn latest(t: &'static str, body: Value) -> Self {
        frame(t, body)
    }

    /// A reliable frame: kept until acked, by its id.
    pub(super) fn reliable(t: &'static str, id: String, body: Value) -> Self {
        Self {
            t,
            id: Some(id),
            body,
        }
    }
}

/// A frame as a child sends it. Every field but `id` is required.
#[derive(Deserialize)]
pub(super) struct Inbound {
    pub(super) t: String,
    pub(super) seq: u64,
    pub(super) ack: u64,
    #[serde(default)]
    pub(super) id: Option<String>,
    #[allow(dead_code)]
    at: i64,
    pub(super) body: Value,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Hello {
    pub(super) protocol: u32,
    pub(super) role: String,
    pub(super) surface: String,
    #[serde(default)]
    pub(super) capabilities: Vec<String>,
    /// The epoch the end last heard; a host must hold the lease at it.
    #[serde(default)]
    pub(super) host_epoch: Option<i64>,
}

/// What `host.freeze` tells the device she lives on: the move she is frozen
/// for there, and what it is (a move back to the site, or a reclaim from a
/// backup by another device).
pub(super) fn freeze_body(moving: &crate::services::persona_sync::lease::Move) -> Value {
    json!({
        "moveId": moving.id,
        "kind": moving.kind,
        "to": moving.to,
    })
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Input {
    pub(super) surface: String,
    pub(super) text: String,
    #[serde(default)]
    pub(super) session_id: Option<String>,
    #[serde(default)]
    pub(super) rig_state: Option<Value>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Resume {
    pub(super) turn_id: String,
    pub(super) after_seq: u64,
    #[serde(default)]
    pub(super) run_id: Option<String>,
}

/// Serves her link to `device` until either end closes it.
pub(crate) async fn serve(socket: WebSocket, db: DatabaseConnection, device: Device) {
    serve_with(socket, db, device, Timing::from_contract()).await;
}

pub(super) async fn serve_with(
    socket: WebSocket,
    db: DatabaseConnection,
    device: Device,
    timing: Timing,
) {
    let (registered, closed) = super::open(&device.id, device.user_id);
    let (sink, stream) = socket.split();
    let (tx, rx) = mpsc::channel(QUEUED_FRAMES);
    let mut link = Link {
        db,
        owner: device.user_id,
        device,
        timing,
        tx,
        queue: Vec::new(),
        sent: 0,
        heard: 0,
        hello: false,
        last_heard: Instant::now(),
        follows: HashMap::new(),
        moods: None,
        registered,
        takes_say: false,
        playback: false,
        touch: false,
        worn: None,
        directions: HashMap::new(),
        taken: HashSet::new(),
        unacked: Vec::new(),
        said: HashSet::new(),
        senses: Default::default(),
        experience: false,
        moments: false,
        nodes: false,
        proposals: None,
        acts: false,
        wardrobe: None,
        live: None,
        host: false,
        core_refused: HashSet::new(),
    };
    tracing::info!(device = %link.device.id, "[link] opened");
    let why = link.run(sink, stream, rx, closed).await;
    // A device that is not one any more fails what it was asked to do.
    if matches!(why, Some(Close::Revoked | Close::OwnerChanged)) {
        super::act::forget_device(&link.device.id);
    }
    // A newer link of the device keeps the presence it reports.
    link.finish(why != Some(Close::Replaced));
    super::forget(&link.device.id, registered);
    tracing::info!(
        device = %link.device.id,
        why = why.map_or("closed by the child", Close::name),
        "[link] closed"
    );
}

struct Link {
    db: DatabaseConnection,
    owner: i32,
    device: Device,
    timing: Timing,
    /// For the run and mood followers.
    tx: mpsc::Sender<Outgoing>,
    /// What the frame just taken answers with, sent before anything else.
    queue: Vec<Outgoing>,
    /// The last seq this end sent, and the highest the child sent.
    sent: u64,
    heard: u64,
    hello: bool,
    last_heard: Instant,
    /// The run each followed turn is being sent from.
    follows: HashMap<String, tokio::task::AbortHandle>,
    moods: Option<tokio::task::AbortHandle>,
    /// This link in the registry.
    registered: u64,
    /// The child said it takes `say`.
    takes_say: bool,
    /// The child said it takes `playback`: her director's refinements of
    /// what it is saying, and reports of how far it got.
    playback: bool,
    /// The child said it sends `touch`.
    touch: bool,
    /// The set this app wears, as last told, once the child said it takes
    /// `wardrobe` (`Some(None)`: none it can wear).
    worn: Option<Option<super::outfit::Worn>>,
    /// The playback direction each followed run is being sent from.
    directions: HashMap<String, tokio::task::AbortHandle>,
    /// Ids of the child's reliable body frames already taken on this
    /// connection (`speech.delivered`, `touch`).
    taken: HashSet<String>,
    /// Reliable frames sent on this connection and not yet acked: (seq, id).
    unacked: Vec<(u64, String)>,
    /// Reliable ids sent on this connection.
    said: HashSet<String>,
    /// What the child said it senses for her.
    senses: super::senses::Senses,
    /// The child said it sends `experience` (and asks `work.recall`).
    experience: bool,
    /// The child said it sends `moment`: what its senses saw for her.
    moments: bool,
    /// The child said it takes `nodes`: it is kept current with her nodes.
    nodes: bool,
    /// Her open proposals as the child was last told them, once it said it
    /// takes `proposals`.
    proposals: Option<Vec<(String, &'static str, i64)>>,
    /// The child said it takes `act`: the owner lets the site ask it to act.
    acts: bool,
    /// Her wardrobe as the child was last told it, once it said it takes
    /// `wardrobe.list`.
    wardrobe: Option<Vec<Value>>,
    /// Her conversations as the child was last told them, once it said it
    /// takes `conversations.live`.
    live: Option<Live>,
    /// The end dialled in as her host (the device she lives on, H3e-7): it
    /// only hears what becomes of her there.
    host: bool,
    /// What the shared core refused of the child's frames on this link and
    /// was logged: `t code`, a frame type outside the contract as `?`.
    core_refused: HashSet<String>,
}

/// What a child kept current was last told of her conversations: the
/// change digest, and each conversation as sent.
#[derive(Default)]
struct Live {
    changes: String,
    told: HashMap<String, String>,
}

type Sink = SplitSink<WebSocket, Message>;

impl Link {
    async fn run(
        &mut self,
        mut sink: Sink,
        mut stream: futures::stream::SplitStream<WebSocket>,
        mut rx: mpsc::Receiver<Outgoing>,
        mut closed: oneshot::Receiver<Close>,
    ) -> Option<Close> {
        let hello_by = tokio::time::sleep(self.timing.hello);
        tokio::pin!(hello_by);
        let mut beat = tokio::time::interval_at(
            Instant::now() + self.timing.heartbeat,
            self.timing.heartbeat,
        );
        beat.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let every = conversations_every();
        let mut keep = tokio::time::interval_at(Instant::now() + every, every);
        keep.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        let mut changed = super::conversations::subscribe_changes();
        let mut nodes_changed = crate::services::agent::merope::nodes::subscribe();
        loop {
            let why = tokio::select! {
                why = &mut closed => {
                    // What was sent to it just before (her host told she moved).
                    while let Ok(outgoing) = rx.try_recv() {
                        self.queue.push(outgoing);
                    }
                    Some(why.unwrap_or(Close::Unavailable))
                }
                incoming = stream.next() => match incoming {
                    Some(Ok(Message::Text(text))) => {
                        self.last_heard = Instant::now();
                        self.take(text.as_str()).await
                    }
                    Some(Ok(Message::Binary(_))) => {
                        self.last_heard = Instant::now();
                        self.error("binary_unsupported", None);
                        None
                    }
                    Some(Ok(Message::Ping(_) | Message::Pong(_))) => {
                        self.last_heard = Instant::now();
                        None
                    }
                    Some(Ok(Message::Close(_)) | Err(_)) | None => return None,
                },
                Some(outgoing) = rx.recv() => {
                    self.queue.push(outgoing);
                    None
                }
                () = &mut hello_by, if !self.hello => Some(Close::Timeout),
                _ = beat.tick() => self.beat().await,
                _ = keep.tick(), if self.live.is_some() => {
                    self.keep_conversations().await;
                    None
                }
                heard = changed.recv(), if self.live.is_some() => {
                    if self.changed_for_me(heard, &mut changed) {
                        self.keep_conversations().await;
                    }
                    None
                }
                heard = nodes_changed.recv(), if self.nodes => {
                    if self.nodes_changed(heard, &mut nodes_changed) {
                        self.tell_nodes().await;
                    }
                    None
                }
            };
            let queued = std::mem::take(&mut self.queue);
            for outgoing in queued {
                if !self.send(&mut sink, outgoing).await {
                    return None;
                }
            }
            if let Some(why) = why {
                let _ = sink
                    .send(Message::Close(Some(CloseFrame {
                        code: why.code(),
                        reason: why.name().into(),
                    })))
                    .await;
                return Some(why);
            }
        }
    }

    async fn send(&mut self, sink: &mut Sink, outgoing: Outgoing) -> bool {
        self.sent += 1;
        let mut envelope = json!({
            "t": outgoing.t,
            "seq": self.sent,
            "ack": self.heard,
            "at": crate::clock::now().timestamp_millis(),
            "body": outgoing.body,
        });
        if let Some(id) = outgoing.id {
            self.unacked.retain(|(_, sent)| *sent != id);
            self.unacked.push((self.sent, id.clone()));
            self.said.insert(id.clone());
            envelope["id"] = id.into();
        }
        // What the host builds is the contract's; one the core refuses is the
        // host's mistake, and goes out as it did before the core read frames.
        if let Err(refusal) = super::contract::core_check(&envelope) {
            tracing::warn!(t = outgoing.t, code = %refusal.code, detail = %refusal.detail, "[link] the shared core refuses a frame the host sends");
        }
        sink.send(Message::Text(envelope.to_string().into()))
            .await
            .is_ok()
    }

    fn reply(&mut self, outgoing: Outgoing) {
        self.queue.push(outgoing);
    }

    fn error(&mut self, code: &str, frame_seq: Option<u64>) {
        let mut body = json!({ "code": code });
        if let Some(seq) = frame_seq {
            body["ref"] = seq.into();
        }
        self.reply(frame("error", body));
    }

    fn reject(&mut self, turn_id: &str, code: &str) {
        self.reply(frame(
            "run.rejected",
            json!({ "turnId": turn_id, "code": code }),
        ));
    }

    /// What the shared core's rules say of a frame from the child. The host
    /// answers it as before either way (see `contract::core_check`); the
    /// first refusal of each kind on a link is logged, so it shows whether a
    /// child sends one. Only the frame type and the code: the core's detail
    /// quotes what the child sent (an unknown type, a field name).
    fn core_reads(&mut self, text: &str) {
        let Ok(frame) = serde_json::from_str::<Value>(text) else {
            return;
        };
        let Err(refusal) = super::contract::core_check(&frame) else {
            return;
        };
        let t = frame["t"]
            .as_str()
            .filter(|t| CONTRACT.frames.contains_key(*t))
            .unwrap_or("?");
        if self.core_refused.insert(format!("{t} {}", refusal.code)) {
            tracing::info!(device = %self.device.id, t, code = %refusal.code, "[link] the shared core refuses a frame from the child");
        }
    }

    /// A text frame from the child; `Some` closes the link.
    async fn take(&mut self, text: &str) -> Option<Close> {
        self.core_reads(text);
        let Ok(inbound) = serde_json::from_str::<Inbound>(text) else {
            if !self.hello {
                return Some(Close::BadFrame);
            }
            self.error("bad_frame", None);
            return None;
        };
        self.heard = self.heard.max(inbound.seq);
        self.acked(inbound.ack);
        if !self.hello {
            if inbound.t != "hello" {
                self.error("not_ready", Some(inbound.seq));
                return None;
            }
            return self.hello(inbound.body).await;
        }
        // Her host's link carries nothing but the heartbeat and acks.
        if self.host {
            match inbound.t.as_str() {
                "ping" => self.reply(frame("pong", json!({}))),
                "ack" | "pong" => {}
                _ => self.error("bad_frame", Some(inbound.seq)),
            }
            return None;
        }
        match inbound.t.as_str() {
            "input" => self.input(inbound).await,
            "presence" => self.presence(inbound).await,
            "run.resume" => self.resume(inbound).await,
            "playback.observe" if self.playback => self.observe(inbound).await,
            "playback.end" if self.playback => self.playback_end(inbound).await,
            "speech.delivered" if self.playback => self.delivered(inbound).await,
            "touch.appraise" if self.touch => self.appraise(inbound).await,
            "touch" if self.touch => self.touched(inbound).await,
            "outfit.wear" if self.worn.is_some() => self.wear(inbound).await,
            "work.recall" if self.experience => self.recall(inbound),
            "experience" if self.experience => self.experienced(inbound).await,
            "moment" if self.moments => self.moment(inbound).await,
            "node.forget" if self.nodes => self.forget_node(inbound).await,
            "proposal.answer" if self.proposals.is_some() => self.answer_proposal(inbound).await,
            // Taken whatever this link said: a run's end may come after its
            // owner turned the switch off (the request must be this device's).
            "act.result" => self.act_result(inbound).await,
            "ping" => self.reply(frame("pong", json!({}))),
            "ack" | "pong" => {}
            t if CONTRACT.frames.contains_key(t) => self.error("bad_frame", Some(inbound.seq)),
            _ => self.error("unknown_frame", Some(inbound.seq)),
        }
        None
    }

    async fn hello(&mut self, body: Value) -> Option<Close> {
        let Ok(hello) = serde_json::from_value::<Hello>(body) else {
            return Some(Close::BadFrame);
        };
        if hello.protocol != CONTRACT.protocol {
            return Some(Close::UnsupportedProtocol);
        }
        match hello.role.as_str() {
            "child" => {}
            // The device she lives on, as her host (H3e-7).
            "host" => return self.host_hello(hello).await,
            _ => return Some(Close::BadFrame),
        }
        if !CONTRACT.surfaces.contains(&hello.surface) {
            return Some(Close::BadFrame);
        }
        // Where she lives, as it stands now: a child of a her who is moving
        // or lives elsewhere is not welcomed (the upgrade is refused the same).
        let lease = match crate::services::persona_sync::lease::read(&self.db).await {
            Ok(lease) => lease,
            Err(error) => {
                tracing::warn!(%error, device = %self.device.id, "[link] could not read her host lease");
                return Some(Close::Unavailable);
            }
        };
        match lease.as_ref().map(|lease| lease.place()) {
            None | Some(crate::services::persona_sync::lease::Where::Here) => {}
            Some(crate::services::persona_sync::lease::Where::Moving) => {
                return Some(Close::HostFrozen);
            }
            Some(_) => return Some(Close::HostMoved),
        }
        let mut capabilities: Vec<String> = Vec::new();
        for capability in hello.capabilities {
            if CONTRACT.capabilities.contains(&capability) && !capabilities.contains(&capability) {
                capabilities.push(capability);
            }
        }
        self.hello = true;
        let mut told = Vec::new();
        for turn in turns::recent(&self.device.id, turns::TOLD_IN_WELCOME) {
            let Some(run) = get_live_run_for_user(&turn.run_id, self.owner).await else {
                continue;
            };
            let (_, last_sequence, completed) = run.snapshot().await;
            let mut told_turn = json!({
                "turnId": turn.turn_id,
                "runId": turn.run_id,
                "lastSequence": last_sequence,
                "completed": completed,
            });
            if let Some(session_id) = turn.session_id {
                told_turn["sessionId"] = session_id.into();
            }
            told.push(told_turn);
        }
        let mut welcome = json!({
            "protocol": CONTRACT.protocol,
            "role": "host",
            "host": "site",
            "deviceId": self.device.id,
            "deviceName": self.device.name,
            "lineage": lease.as_ref().map(|lease| lease.lineage.clone()),
            "hostEpoch": lease.as_ref().map(|lease| lease.epoch),
            "heartbeatMs": self.timing.heartbeat.as_millis() as u64,
            "capabilities": &capabilities,
            "turns": told,
            "serverTime": crate::clock::now().timestamp_millis(),
        });
        // The account's one partner id, minted here the first time: only to
        // an end that knows the field. Every side of her log for this
        // account goes by it; a device still on another id moves to it in
        // its own core before it syncs.
        if capabilities
            .iter()
            .any(|capability| capability == "partner")
        {
            match crate::services::her_log::partner_id(&self.db, self.owner).await {
                Ok(partner) => welcome["partner"] = json!(partner),
                Err(error) => {
                    tracing::warn!(%error, device = %self.device.id, "[link] the account's partner id could not be read; welcomed without it");
                }
            }
        }
        self.reply(frame("welcome", welcome));
        self.playback = capabilities
            .iter()
            .any(|capability| capability == "playback");
        self.touch = capabilities.iter().any(|capability| capability == "touch");
        self.senses = super::senses::Senses::of(&capabilities);
        // What it does for the owner comes up; what is written is told on its link.
        if capabilities
            .iter()
            .any(|capability| capability == "experience")
        {
            self.experience = true;
            super::takes_experience(&self.device.id, self.registered, self.tx.clone());
        }
        // What its senses see for her comes up, and her nodes made of it go down.
        self.moments = capabilities
            .iter()
            .any(|capability| capability == "moments");
        if capabilities.iter().any(|capability| capability == "nodes") {
            self.nodes = true;
            self.tell_nodes().await;
        }
        // Her lines for the app: what is still waiting goes again, after welcome.
        if capabilities.iter().any(|capability| capability == "say") {
            self.takes_say = true;
            super::takes_say(
                &self.device.id,
                self.registered,
                self.tx.clone(),
                self.playback,
            );
            for (id, body) in super::say::pending(&self.device.id) {
                self.reply(Outgoing::reliable("say", id, body));
            }
        }
        if let Some(mood) = current_mood(&self.db, self.owner).await {
            self.reply(mood);
        }
        // The set it wears: its own, or, the first time, the one it starts with.
        if capabilities
            .iter()
            .any(|capability| capability == "wardrobe")
        {
            let worn = super::outfit::put(&self.db, self.owner, &self.device.id, None)
                .await
                .ok()
                .flatten();
            if let Some(worn) = &worn {
                self.reply(super::outfit::frame(worn));
            }
            self.worn = Some(worn);
            // Her wardrobe, read only, to choose from there.
            if capabilities
                .iter()
                .any(|capability| capability == "wardrobe.list")
            {
                let listed = super::outfit::current_list(&self.db).await;
                self.reply(super::outfit::list_frame(listed.clone()));
                self.wardrobe = Some(listed);
            }
        }
        // Her recent conversations, for the app to show (they stay here).
        if capabilities
            .iter()
            .any(|capability| capability == "conversations")
        {
            // Kept current, and with her lines said outside a chat (D37).
            let live = capabilities
                .iter()
                .any(|capability| capability == "conversations.live");
            let changes = if live {
                super::conversations::changes(&self.db, self.owner)
                    .await
                    .unwrap_or_default()
            } else {
                String::new()
            };
            match super::conversations::recent_with(&self.db, self.owner, live).await {
                Ok(sessions) => {
                    if live {
                        self.live = Some(Live {
                            changes,
                            told: told_conversations(&sessions),
                        });
                    }
                    self.reply(super::conversations::frame(
                        sessions,
                        CONTRACT.limits.max_frame_bytes,
                    ))
                }
                Err(error) => {
                    tracing::warn!(%error, device = %self.device.id, "[link] could not read recent conversations");
                    if live {
                        self.live = Some(Live::default());
                    }
                }
            }
        }
        // The owner lets the site ask it to act: what it was asked and has
        // not acked goes again.
        if capabilities.iter().any(|capability| capability == "act") {
            self.acts = true;
            super::act::linked(&self.device.id);
            super::takes_acts(
                &self.device.id,
                self.registered,
                self.tx.clone(),
                self.device.name.clone(),
            );
            for (id, body) in super::act::pending(&self.device.id) {
                self.reply(Outgoing::reliable("act.request", id, body));
            }
        }
        // Her proposals as cards there.
        if capabilities
            .iter()
            .any(|capability| capability == "proposals")
        {
            self.proposals = Some(Vec::new());
            self.tell_proposals(true).await;
        }
        // Her mood as it changes, and the site's notifications about her
        // for a child that shows notifications.
        if let Some(manager) = get_notification_manager() {
            let events = manager.subscribe();
            let notify = capabilities.iter().any(|capability| capability == "notify");
            // Reminders due on the site, while the owner is using her here.
            let reminders = (notify
                && capabilities
                    .iter()
                    .any(|capability| capability == "reminders"))
            .then(|| self.device.id.clone());
            let task = tokio::spawn(follow_events(
                self.tx.clone(),
                self.owner,
                events,
                notify,
                reminders,
            ));
            self.moods = Some(task.abort_handle());
        }
        tracing::info!(device = %self.device.id, "[link] hello");
        None
    }

    /// The device she lives on dials in as her host: welcomed only while it
    /// holds her lease at the epoch it heard; otherwise she moved (it lets
    /// its copy go). It is told at once if she is frozen there for a move.
    async fn host_hello(&mut self, hello: Hello) -> Option<Close> {
        use crate::services::persona_sync::lease::{self, State};
        if !CONTRACT.surfaces.contains(&hello.surface) {
            return Some(Close::BadFrame);
        }
        let lease = match lease::read(&self.db).await {
            Ok(lease) => lease,
            Err(error) => {
                tracing::warn!(%error, device = %self.device.id, "[link] could not read her host lease");
                return Some(Close::Unavailable);
            }
        };
        let Some(lease) = lease else {
            return Some(Close::HostIsSite);
        };
        let holds = lease.holder == self.device.id
            && matches!(lease.state, State::Live | State::Frozen)
            && hello.host_epoch.is_none_or(|epoch| epoch == lease.epoch);
        if !holds {
            tracing::info!(device = %self.device.id, epoch = lease.epoch, heard = ?hello.host_epoch, "[link] a host that does not hold her lease: she moved");
            return Some(Close::HostMoved);
        }
        self.hello = true;
        self.host = true;
        let frozen = lease
            .moving
            .as_ref()
            .filter(|moving| lease.state == State::Frozen && moving.from == self.device.id);
        super::takes_host(
            &self.device.id,
            self.registered,
            self.tx.clone(),
            frozen.map(|moving| moving.id.clone()),
        );
        self.reply(frame(
            "welcome",
            json!({
                "protocol": CONTRACT.protocol,
                "role": "site",
                "host": "app",
                "deviceId": self.device.id,
                "deviceName": self.device.name,
                "lineage": lease.lineage,
                "hostEpoch": lease.epoch,
                "heartbeatMs": self.timing.heartbeat.as_millis() as u64,
                // What the site itself can do: its core reads her log's
                // merge, split and retract rows (ABI 2).
                "capabilities": ["log2"],
                "turns": [],
                "serverTime": crate::clock::now().timestamp_millis(),
            }),
        ));
        if let Some(moving) = frozen {
            self.reply(frame("host.freeze", freeze_body(moving)));
        }
        tracing::info!(device = %self.device.id, epoch = lease.epoch, "[link] her host is linked");
        None
    }

    async fn input(&mut self, inbound: Inbound) {
        let seq = inbound.seq;
        let Some(turn_id) = inbound.id else {
            self.error("bad_frame", Some(seq));
            return;
        };
        if !valid_id(&turn_id) {
            self.error("bad_id", Some(seq));
            return;
        }
        let Ok(input) = serde_json::from_value::<Input>(inbound.body) else {
            self.error("bad_frame", Some(seq));
            return;
        };
        // A resend: the turn already has its run.
        if let Some(turn) = turns::find(&self.db, &self.device.id, &turn_id).await {
            self.reply(accepted(&turn, true));
            return;
        }
        if !CONTRACT.surfaces.contains(&input.surface) {
            self.reject(&turn_id, "surface_unsupported");
            return;
        }
        if input.text.trim().is_empty() {
            self.reject(&turn_id, "empty_input");
            return;
        }
        if input.text.chars().count() > CONTRACT.limits.max_text_chars {
            self.reject(&turn_id, "input_too_long");
            return;
        }
        // Saying something on the app is using her there.
        crate::services::agent::consciousness::native_used(self.owner, &self.device.id);
        let request = ProcessRequest {
            input: input.text,
            context: Some(ProcessContext {
                mode: Some(AgentInteractionMode::Chat),
                session_id: input.session_id,
                rig_state: input.rig_state,
                // Answered with what she senses there: the app's own.
                custom_data: crate::services::agent::consciousness::native_senses(
                    self.owner,
                    &self.device.id,
                ),
                on_device: Some(self.device.id.clone()),
                ..Default::default()
            }),
        };
        let run = match start_for_user(self.db.clone(), self.owner, request).await {
            Ok(run) => run,
            Err(error) => {
                let code = match error.code() {
                    Some(code) if code != "unmapped" => code.to_string(),
                    _ if error.status().is_server_error() => "internal_error".to_string(),
                    _ => "bad_frame".to_string(),
                };
                tracing::info!(device = %self.device.id, %code, "[link] input refused");
                self.reject(&turn_id, &code);
                return;
            }
        };
        // Answered on the device: the site does not notify about it (the
        // run is so from its start, `on_device`).
        let turn = Turn {
            turn_id,
            run_id: run.run_id().to_string(),
            session_id: run.session_id().map(str::to_string),
        };
        turns::remember(&self.db, &self.device.id, self.owner, turn.clone()).await;
        self.reply(accepted(&turn, false));
        self.follow(turn, run, 0);
    }

    async fn presence(&mut self, inbound: Inbound) {
        if !inbound.body.get("presence").is_some_and(Value::is_object) {
            self.error("bad_frame", Some(inbound.seq));
            return;
        }
        // As on the site: with her persona off nothing is remembered.
        if merope::is_enabled().await {
            // Only what the child said it senses.
            let arrived = crate::services::agent::consciousness::remember_native_presence(
                self.owner,
                &self.device.id,
                &self.senses.admit(&inbound.body),
            );
            // Arriving on the app is arriving: the same greeting as a page.
            if arrived {
                merope::spawn_presence(self.owner);
            }
        }
    }

    async fn resume(&mut self, inbound: Inbound) {
        let Ok(resume) = serde_json::from_value::<Resume>(inbound.body) else {
            self.error("bad_frame", Some(inbound.seq));
            return;
        };
        if !valid_id(&resume.turn_id) {
            self.error("bad_id", Some(inbound.seq));
            return;
        }
        // Only this device's own turns, whatever run id it names.
        let turn = turns::find(&self.db, &self.device.id, &resume.turn_id)
            .await
            .filter(|turn| resume.run_id.as_ref().is_none_or(|id| *id == turn.run_id));
        let Some(turn) = turn else {
            self.reject(&resume.turn_id, "turn_unknown");
            return;
        };
        match get_run_for_user(&turn.run_id, self.owner).await {
            Ok(Some(run)) => self.follow(turn, run, resume.after_seq),
            Ok(None) => self.reject(&resume.turn_id, "turn_unknown"),
            Err(error) => {
                tracing::warn!(%error, device = %self.device.id, "[link] could not load a run");
                self.reject(&resume.turn_id, "internal_error");
            }
        }
    }

    /// Sends the turn's run from after `after_seq`, instead of anything that
    /// was already sending it.
    fn follow(&mut self, turn: Turn, run: Arc<AgentRun>, after_seq: u64) {
        let key = turn.turn_id.clone();
        if let Some(previous) = self.follows.remove(&key) {
            previous.abort();
        }
        self.follows.retain(|_, task| !task.is_finished());
        self.directions.retain(|_, task| !task.is_finished());
        // Her director's refinements of the reply, once per run, while it is said.
        if self.playback && !self.directions.contains_key(&turn.run_id) {
            let task = tokio::spawn(super::body::follow_direction(
                self.tx.clone(),
                turn.clone(),
                run.clone(),
            ));
            self.directions
                .insert(turn.run_id.clone(), task.abort_handle());
        }
        let task = tokio::spawn(follow_run(self.tx.clone(), turn, run, after_seq));
        self.follows.insert(key, task.abort_handle());
    }

    /// The live run one of this device's turns started, for a body frame
    /// that names it; `None` (and the error sent) when it names another.
    async fn own_run(&mut self, run_id: &str, frame_seq: u64) -> Option<(Turn, Arc<AgentRun>)> {
        let Some(turn) = turns::by_run(&self.device.id, run_id) else {
            self.error("not_this_device", Some(frame_seq));
            return None;
        };
        // A run already gone has nothing left to direct.
        let run = get_live_run_for_user(run_id, self.owner).await?;
        Some((turn, run))
    }

    /// `playback.observe`: where saying the reply has got to, for her director.
    async fn observe(&mut self, inbound: Inbound) {
        let Ok(observed) = serde_json::from_value::<super::body::Observe>(inbound.body) else {
            self.error("bad_frame", Some(inbound.seq));
            return;
        };
        if observed.upcoming_text.len() > super::body::UPCOMING_BYTES
            || observed.rig.to_string().len() > super::body::RIG_BYTES
        {
            self.error("too_large", Some(inbound.seq));
            return;
        }
        if let Some((_, run)) = self.own_run(&observed.run_id, inbound.seq).await {
            run.playback_direction.observe(
                crate::services::agent::playback_direction::PlaybackObservation {
                    upcoming_text: observed.upcoming_text,
                    rig: observed.rig,
                },
            );
        }
    }

    /// `playback.end`: the reply is said, or stopped being said.
    async fn playback_end(&mut self, inbound: Inbound) {
        let Ok(ended) = serde_json::from_value::<super::body::End>(inbound.body) else {
            self.error("bad_frame", Some(inbound.seq));
            return;
        };
        if let Some((_, run)) = self.own_run(&ended.run_id, inbound.seq).await {
            run.playback_direction.close();
        }
    }

    /// `speech.delivered`: how much of her reply or line was said aloud. One
    /// cut off is marked in her history, so her next turn knows where she
    /// stopped.
    async fn delivered(&mut self, inbound: Inbound) {
        let seq = inbound.seq;
        let Some(id) = inbound.id.filter(|id| valid_id(id)) else {
            self.error("bad_id", Some(seq));
            return;
        };
        let Ok(delivered) = serde_json::from_value::<super::body::Delivered>(inbound.body) else {
            self.error("bad_frame", Some(seq));
            return;
        };
        if !self.taken.insert(id) {
            return;
        }
        let by = delivered.by().unwrap_or("unknown");
        if let Some(run_id) = &delivered.run_id {
            let Some(turn) = turns::by_run(&self.device.id, run_id) else {
                self.error("not_this_device", Some(seq));
                return;
            };
            if delivered.interrupted {
                let session = match turn.session_id.clone() {
                    Some(session) => Some(session),
                    None => get_run_for_user(run_id, self.owner)
                        .await
                        .ok()
                        .flatten()
                        .and_then(|run| run.session_id().map(str::to_string)),
                };
                if let Some(session) = session {
                    let heard = delivered.spoken_chars.min(delivered.total_chars);
                    let (db, run_id) = (self.db.clone(), run_id.clone());
                    mark_cut_off(self.device.id.clone(), by, "reply", move || {
                        let (db, session, run_id) = (db.clone(), session.clone(), run_id.clone());
                        async move {
                            crate::services::agent::sessions::mark_spoken_reply_cut_off_at(
                                &db,
                                &session,
                                &run_id,
                                Some(heard),
                            )
                            .await
                        }
                    })
                    .await;
                }
            }
        } else if let Some(say_id) = delivered.say_id.clone() {
            if !valid_id(&say_id) {
                self.error("bad_id", Some(seq));
                return;
            }
            // A line of hers cut off reads as her replies do: maybe not
            // heard to the end, and how much was.
            if delivered.interrupted {
                let heard = delivered.spoken_chars.min(delivered.total_chars);
                let (db, owner) = (self.db.clone(), self.owner);
                mark_cut_off(self.device.id.clone(), by, "line", move || {
                    let (db, say_id) = (db.clone(), say_id.clone());
                    async move {
                        merope::store::mark_line_cut_off(&db, owner, &say_id, Some(heard)).await
                    }
                })
                .await;
            }
        }
    }

    /// `touch.appraise`: contact going on with her figure on the app; she
    /// answers with how she takes it, within the touch time.
    async fn appraise(&mut self, inbound: Inbound) {
        let Ok(appraise) = serde_json::from_value::<super::body::Appraise>(inbound.body) else {
            self.error("bad_frame", Some(inbound.seq));
            return;
        };
        if !valid_id(&appraise.touch_id) {
            self.error("bad_id", Some(inbound.seq));
            return;
        }
        // Only a window on screen can be touched.
        let shown =
            crate::services::agent::consciousness::native_presence(self.owner, &self.device.id)
                .is_some_and(|live| live.page_visible);
        if !shown {
            self.reply(super::body::reaction(&appraise.touch_id, None));
            return;
        }
        let (db, owner, tx, seq) = (self.db.clone(), self.owner, self.tx.clone(), inbound.seq);
        tokio::spawn(async move {
            let answer = match crate::services::agent::merope::api::touch::appraise_ongoing(
                &db,
                owner,
                &appraise.touch,
            )
            .await
            {
                Ok(reaction) => super::body::reaction(&appraise.touch_id, reaction),
                Err(_) => frame("error", json!({ "code": "bad_frame", "ref": seq })),
            };
            let _ = tx.send(answer).await;
        });
    }

    /// `work.recall`: the Work memories a run on the app starts from, as the
    /// site's own Work recalls them.
    fn recall(&mut self, inbound: Inbound) {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Recall {
            request_id: String,
            query: String,
            #[serde(default)]
            stuck: bool,
            #[serde(default)]
            shown: Vec<String>,
        }
        let Ok(recall) = serde_json::from_value::<Recall>(inbound.body) else {
            self.error("bad_frame", Some(inbound.seq));
            return;
        };
        if !valid_id(&recall.request_id) {
            self.error("bad_id", Some(inbound.seq));
            return;
        }
        if recall.query.chars().count() > CONTRACT.limits.max_text_chars {
            self.error("too_large", Some(inbound.seq));
            return;
        }
        let (db, owner, tx) = (self.db.clone(), self.owner, self.tx.clone());
        tokio::spawn(async move {
            let shown: Vec<String> = recall.shown.into_iter().take(16).collect();
            let memories =
                super::experience::recall(&db, owner, &recall.query, recall.stuck, &shown).await;
            let _ = tx
                .send(super::experience::recalled_frame(
                    &recall.request_id,
                    &memories,
                ))
                .await;
        });
    }

    /// `experience`: something the app did for the owner, to become part of
    /// her. Taken once it is written; a resend of one already written is
    /// told so again.
    async fn experienced(&mut self, inbound: Inbound) {
        let seq = inbound.seq;
        let Some(id) = inbound.id.filter(|id| valid_id(id)) else {
            self.error("bad_id", Some(seq));
            return;
        };
        let Ok(experience) = serde_json::from_value::<super::experience::Experience>(inbound.body)
        else {
            self.error("bad_frame", Some(seq));
            return;
        };
        if !experience.readable() {
            self.error("bad_frame", Some(seq));
            return;
        }
        if super::experience::was_taken(&self.db, &self.device.id, &id).await {
            self.reply(super::experience::taken_frame(&id));
            return;
        }
        // Another copy is being written; it is told when that is done.
        if !super::experience::begin(&self.device.id, &id) {
            return;
        }
        tokio::spawn(super::experience::write(
            self.db.clone(),
            self.owner,
            self.device.id.clone(),
            id,
            experience,
        ));
    }

    /// `moment`: what the app saw for her, to reach her as her own event.
    /// Taken once it is written; a resend of one already written is told so
    /// again and reaches her no second time.
    async fn moment(&mut self, inbound: Inbound) {
        let seq = inbound.seq;
        let Some(id) = inbound.id.filter(|id| valid_id(id)) else {
            self.error("bad_id", Some(seq));
            return;
        };
        let Ok(moment) = serde_json::from_value::<super::moment::Moment>(inbound.body) else {
            self.error("bad_frame", Some(seq));
            return;
        };
        if !moment.readable() {
            self.error("bad_frame", Some(seq));
            return;
        }
        if super::moment::was_taken(&self.db, &self.device.id, &id).await {
            self.reply(super::moment::taken_frame(&id));
            return;
        }
        if !super::moment::begin(&self.device.id, &id) {
            return;
        }
        let written = super::moment::write(&self.db, self.owner, &self.device.id, &id).await;
        super::moment::end(&self.device.id, &id);
        match written {
            Ok(()) => {
                tracing::info!(device = %self.device.id, kind = %moment.kind, "[link] moment taken");
                super::moment::hand_over(self.owner, &self.device.id, &id, moment);
                self.reply(super::moment::taken_frame(&id));
            }
            // Not taken: the child keeps it and sends it again on its next link.
            Err(error) => {
                tracing::warn!(%error, device = %self.device.id, "[link] could not write a moment");
            }
        }
    }

    /// Her nodes as they are now, to a child that takes them.
    async fn tell_nodes(&mut self) {
        if let Some(frame) = super::nodes::current(&self.db, self.owner).await {
            self.reply(frame);
        }
    }

    /// Whether the owner's nodes changed, from what was heard and what is
    /// queued behind it.
    fn nodes_changed(
        &self,
        heard: Result<i32, tokio::sync::broadcast::error::RecvError>,
        changed: &mut tokio::sync::broadcast::Receiver<i32>,
    ) -> bool {
        use tokio::sync::broadcast::error::{RecvError, TryRecvError};
        let mut read = match heard {
            Ok(owner) => owner == self.owner,
            Err(RecvError::Lagged(_)) => true,
            Err(RecvError::Closed) => false,
        };
        loop {
            match changed.try_recv() {
                Ok(owner) => read |= owner == self.owner,
                Err(TryRecvError::Lagged(_)) => read = true,
                Err(TryRecvError::Empty | TryRecvError::Closed) => return read,
            }
        }
    }

    /// `node.forget`: the owner makes her forget one of her nodes. Deleted,
    /// then the nodes as they are now (also when it was gone already).
    async fn forget_node(&mut self, inbound: Inbound) {
        let seq = inbound.seq;
        if !inbound.id.as_deref().is_some_and(valid_id) {
            self.error("bad_id", Some(seq));
            return;
        }
        let Some(node) = super::nodes::forgotten(&inbound.body) else {
            self.error("bad_frame", Some(seq));
            return;
        };
        match crate::services::agent::merope::nodes::forget(&self.db, self.owner, node).await {
            // Gone: every link of the owner's hears the change, this one too.
            Ok(true) => {
                tracing::info!(device = %self.device.id, "[link] she forgot a node");
            }
            Ok(false) => self.tell_nodes().await,
            Err(error) => {
                tracing::warn!(%error, device = %self.device.id, "[link] could not forget a node");
                self.error("internal_error", Some(seq));
            }
        }
    }

    /// Her open proposals, sent when they changed since the child was told
    /// (or always, with `always`).
    async fn tell_proposals(&mut self, always: bool) {
        let Some(told) = self.proposals.clone() else {
            return;
        };
        let open = match super::proposals::open(&self.db, self.owner).await {
            Ok(open) => open,
            Err(error) => {
                tracing::warn!(%error, device = %self.device.id, "[link] could not read proposals");
                return;
            }
        };
        let digest = super::proposals::digest(&open);
        if always || told != digest {
            self.reply(super::proposals::frame(&open, &self.device.id));
            self.proposals = Some(digest);
        }
    }

    /// `proposal.answer`: the owner accepted or declined one of her
    /// proposals on the app.
    async fn answer_proposal(&mut self, inbound: Inbound) {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Answer {
            proposal_id: String,
            accept: bool,
            #[serde(default)]
            place: Option<String>,
        }
        let Ok(answer) = serde_json::from_value::<Answer>(inbound.body) else {
            self.error("bad_frame", Some(inbound.seq));
            return;
        };
        if !valid_id(&answer.proposal_id) {
            self.error("bad_id", Some(inbound.seq));
            return;
        }
        let answered = if answer.accept {
            match super::proposals::Place::of(answer.place.as_deref()) {
                Some(place) => {
                    super::proposals::accept(
                        &self.db,
                        self.owner,
                        &self.device.id,
                        &answer.proposal_id,
                        place,
                    )
                    .await
                }
                None => super::proposals::refused(&answer.proposal_id, "place_unknown"),
            }
        } else {
            super::proposals::decline(&self.db, self.owner, &answer.proposal_id).await
        };
        self.reply(answered);
        self.tell_proposals(false).await;
    }

    /// `act.result`: what became of a request to act.
    async fn act_result(&mut self, inbound: Inbound) {
        let seq = inbound.seq;
        let Some(id) = inbound.id.filter(|id| valid_id(id)) else {
            self.error("bad_id", Some(seq));
            return;
        };
        let Ok(result) = serde_json::from_value::<super::act::ActResult>(inbound.body) else {
            self.error("bad_frame", Some(seq));
            return;
        };
        if !self.taken.insert(id) {
            return;
        }
        if let Err(code) = super::act::result(&self.db, self.owner, &self.device.id, result).await {
            self.error(code, Some(seq));
        }
        self.tell_proposals(false).await;
    }

    /// `outfit.wear`: the owner put a set of her wardrobe on her on the app.
    async fn wear(&mut self, inbound: Inbound) {
        let Some(outfit_id) = inbound
            .body
            .get("outfitId")
            .and_then(Value::as_str)
            .filter(|id| !id.is_empty())
        else {
            self.error("bad_frame", Some(inbound.seq));
            return;
        };
        match super::outfit::put(&self.db, self.owner, &self.device.id, Some(outfit_id)).await {
            Ok(Some(worn)) => {
                self.reply(super::outfit::frame(&worn));
                self.worn = Some(Some(worn));
            }
            Ok(None) => self.error("internal_error", Some(inbound.seq)),
            Err(_) => self.error("outfit_unknown", Some(inbound.seq)),
        }
    }

    /// `touch`: contact with her figure on the app has ended; she is told
    /// of it, and answers on the app.
    async fn touched(&mut self, inbound: Inbound) {
        let seq = inbound.seq;
        let Some(id) = inbound.id.filter(|id| valid_id(id)) else {
            self.error("bad_id", Some(seq));
            return;
        };
        let Ok(touched) = serde_json::from_value::<super::body::Touched>(inbound.body) else {
            self.error("bad_frame", Some(seq));
            return;
        };
        if !self.taken.insert(id) {
            return;
        }
        let live =
            crate::services::agent::consciousness::native_presence(self.owner, &self.device.id)
                .unwrap_or_default();
        if crate::services::agent::merope::api::touch::ended(
            self.owner,
            &touched.touch,
            &live,
            Some(&self.device.id),
        )
        .await
        .is_err()
        {
            self.error("bad_frame", Some(seq));
        }
    }

    /// Whether an announcement (and any queued behind it) is about this
    /// link's owner. One read covers them all; a lost one may be.
    fn changed_for_me(
        &self,
        heard: Result<i32, tokio::sync::broadcast::error::RecvError>,
        changed: &mut tokio::sync::broadcast::Receiver<i32>,
    ) -> bool {
        use tokio::sync::broadcast::error::{RecvError, TryRecvError};
        let mine = |owner: i32| owner == self.owner || owner == super::conversations::EVERYONE;
        let mut read = match heard {
            Ok(owner) => mine(owner),
            Err(RecvError::Lagged(_)) => true,
            Err(RecvError::Closed) => false,
        };
        loop {
            match changed.try_recv() {
                Ok(owner) => read |= mine(owner),
                Err(TryRecvError::Lagged(_)) => read = true,
                Err(TryRecvError::Empty | TryRecvError::Closed) => return read,
            }
        }
    }

    /// A child kept current: each of her conversations that changed since
    /// it was told goes as `conversation` (D37).
    async fn keep_conversations(&mut self) {
        let Some(live) = self.live.as_ref() else {
            return;
        };
        let changes = match super::conversations::changes(&self.db, self.owner).await {
            Ok(changes) => changes,
            Err(error) => {
                tracing::debug!(%error, device = %self.device.id, "[link] could not check her conversations");
                return;
            }
        };
        if changes == live.changes {
            return;
        }
        let sessions = match super::conversations::recent_with(&self.db, self.owner, true).await {
            Ok(sessions) => sessions,
            Err(error) => {
                tracing::warn!(%error, device = %self.device.id, "[link] could not read recent conversations");
                return;
            }
        };
        let told = told_conversations(&sessions);
        let previous = self.live.take().map(|live| live.told).unwrap_or_default();
        // Oldest first, so the newest change is the last the child takes.
        for session in sessions.into_iter().rev() {
            let id = session["sessionId"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            if previous.get(&id) == told.get(&id) {
                continue;
            }
            if let Some(frame) =
                super::conversations::one_frame(session, CONTRACT.limits.max_frame_bytes)
            {
                self.reply(frame);
            }
        }
        self.live = Some(Live { changes, told });
    }

    /// Every heartbeat: the child is still there, and the device still one.
    async fn beat(&mut self) -> Option<Close> {
        if self.last_heard.elapsed() > self.timing.silence() {
            return Some(Close::Timeout);
        }
        let owner = match crate::services::principal::site_owner_id(&self.db).await {
            Ok(owner) => owner,
            Err(error) => {
                tracing::warn!(%error, "[link] could not resolve the site owner");
                return None;
            }
        };
        match devices::recheck(&self.db, &self.device.id, owner).await {
            Ok(_) => {}
            Err(Refused::Revoked | Refused::Unknown) => return Some(Close::Revoked),
            Err(Refused::OwnerChanged | Refused::NoOwner) => return Some(Close::OwnerChanged),
            Err(Refused::Db(error)) => {
                tracing::warn!(%error, device = %self.device.id, "[link] could not recheck a device");
            }
        }
        if self.hello {
            self.reply(frame("ping", json!({})));
        }
        // What it wears changed on the host (its set's rig was made again,
        // or the set left her wardrobe).
        if let Some(told) = self.worn.clone() {
            let now = super::outfit::current(&self.db, &self.device.id).await;
            if now != told {
                if let Some(worn) = &now {
                    self.reply(super::outfit::frame(worn));
                }
                self.worn = Some(now);
            }
        }
        // Her wardrobe changed on the host.
        if let Some(told) = &self.wardrobe {
            let listed = super::outfit::current_list(&self.db).await;
            if *told != listed {
                self.reply(super::outfit::list_frame(listed.clone()));
                self.wardrobe = Some(listed);
            }
        }
        // Her proposals changed (a new one, or one answered elsewhere).
        self.tell_proposals(false).await;
        if self.takes_say {
            // Lines that could not be queued when said, and none that expired.
            let pending = super::say::pending(&self.device.id);
            let asked = super::act::pending(&self.device.id);
            self.unacked.retain(|(_, id)| {
                pending.iter().any(|(waiting, _)| waiting == id)
                    || asked.iter().any(|(waiting, _)| waiting == id)
            });
            for (id, body) in pending {
                if !self.said.contains(&id) {
                    self.reply(Outgoing::reliable("say", id, body));
                }
            }
        }
        // Requests to act that could not be queued when asked.
        if self.acts {
            for (id, body) in super::act::pending(&self.device.id) {
                if !self.said.contains(&id) {
                    self.reply(Outgoing::reliable("act.request", id, body));
                }
            }
        }
        None
    }

    /// The child's ack covers everything up to `ack` on this connection.
    fn acked(&mut self, ack: u64) {
        let device = &self.device.id;
        self.unacked.retain(|(seq, id)| {
            if *seq <= ack {
                super::say::acked(device, id);
                super::act::acked(device, id);
                false
            } else {
                true
            }
        });
    }

    /// The link is closed: stop sending, and the person is not there now
    /// (unless `gone` is false: another link of the device took over).
    fn finish(&mut self, gone: bool) {
        for (_, task) in self.follows.drain().chain(self.directions.drain()) {
            task.abort();
        }
        if let Some(task) = self.moods.take() {
            task.abort();
        }
        if gone {
            crate::services::agent::consciousness::forget_native_presence(
                self.owner,
                &self.device.id,
            );
        }
    }
}

/// Marks what the app cut off in her history once it is kept there: a reply
/// is saved whole when its run ends and a line of hers once it was
/// delivered, and either can be stopped aloud before that. Tries now, then
/// every half second for a minute.
async fn mark_cut_off<F, Fut>(device: String, by: &str, what: &'static str, mark: F)
where
    F: Fn() -> Fut + Send + 'static,
    Fut: std::future::Future<Output = Result<bool, sea_orm::DbErr>> + Send,
{
    match mark().await {
        Ok(true) => tracing::info!(device = %device, by, what, "[link] cut off on the app"),
        Ok(false) => {
            let by = by.to_string();
            tokio::spawn(async move {
                for _ in 0..120 {
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                    match mark().await {
                        Ok(false) => continue,
                        Ok(true) => {
                            tracing::info!(device = %device, by, what, "[link] cut off on the app")
                        }
                        Err(error) => {
                            tracing::warn!(%error, device = %device, what, "[link] could not mark a cut-off")
                        }
                    }
                    return;
                }
            });
        }
        Err(error) => {
            tracing::warn!(%error, device = %device, what, "[link] could not mark a cut-off")
        }
    }
}

/// Each conversation as sent, by session id.
fn told_conversations(sessions: &[Value]) -> HashMap<String, String> {
    sessions
        .iter()
        .filter_map(|session| {
            Some((
                session["sessionId"].as_str()?.to_string(),
                session.to_string(),
            ))
        })
        .collect()
}

pub(super) fn accepted(turn: &Turn, duplicate: bool) -> Outgoing {
    let mut body = json!({
        "turnId": turn.turn_id,
        "runId": turn.run_id,
        "duplicate": duplicate,
    });
    if let Some(session_id) = &turn.session_id {
        body["sessionId"] = session_id.clone().into();
    }
    frame("run.accepted", body)
}

pub(super) fn mood_frame(mood: &MoodTransition, activity: &str) -> Outgoing {
    frame("mood", json!({ "mood": mood, "activity": activity }))
}

/// Her mood as it stands, while her persona is on.
async fn current_mood(db: &DatabaseConnection, owner: i32) -> Option<Outgoing> {
    if !merope::is_enabled().await {
        return None;
    }
    let state = merope::get_or_create_state(db, owner).await.ok()?;
    let affect = merope::store::affect_from_state(&state);
    let mood = MoodTransition::from_affect(
        &affect,
        &affect,
        "current",
        state.updated_at.timestamp_millis(),
    );
    Some(mood_frame(&mood, merope::current_activity(&state)))
}

/// Her mood as it changes outside a run (the appraisal of what they said
/// lands after the reply, a Work outcome moves her), for the site owner; and
/// with `notify` the site's notifications about her, once each.
pub(super) async fn follow_events(
    tx: mpsc::Sender<Outgoing>,
    owner: i32,
    mut events: tokio::sync::broadcast::Receiver<NotificationEvent>,
    notify: bool,
    reminders: Option<String>,
) {
    use tokio::sync::broadcast::error::RecvError;
    let mut sent = super::notices::Sent::default();
    loop {
        let outgoing = match events.recv().await {
            Ok(NotificationEvent::MeropeStateChanged {
                user_id,
                mood,
                activity,
            }) if user_id == owner => Some(mood_frame(&mood, &activity)),
            Ok(event) if notify => sent.first(&event, owner, || {
                reminders.as_deref().is_some_and(|device| {
                    crate::services::agent::consciousness::native_presence(owner, device).is_some()
                })
            }),
            Ok(_) | Err(RecvError::Lagged(_)) => None,
            Err(RecvError::Closed) => return,
        };
        if let Some(outgoing) = outgoing {
            if tx.send(outgoing).await.is_err() {
                return;
            }
        }
    }
}

/// How she acts on this event, when she still should: a directive while the
/// run is live or being caught up on, and a mood only as it happens.
pub(super) fn acting(
    turn: &Turn,
    sequence: u64,
    event: &AgentProgressEvent,
    live: bool,
) -> Option<Outgoing> {
    let directive = match event {
        AgentProgressEvent::PerformancePlan { performance } => {
            serde_json::to_value(performance).ok()
        }
        AgentProgressEvent::TaskCompleted { response, .. } => response
            .get("performance")
            .filter(|performance| !performance.is_null())
            .cloned(),
        AgentProgressEvent::MeropeStateChanged { mood, activity } if live => {
            return Some(mood_frame(mood, activity));
        }
        _ => None,
    }?;
    Some(frame(
        "perform",
        json!({
            "turnId": turn.turn_id,
            "runId": turn.run_id,
            "sequence": sequence,
            "directive": directive,
        }),
    ))
}

/// The turn's run events after `after_seq`, then live until it ends, then
/// `run.end`. A run already over when caught up on sends its record only:
/// she does not act it again.
async fn follow_run(tx: mpsc::Sender<Outgoing>, turn: Turn, run: Arc<AgentRun>, after_seq: u64) {
    let (_, sent_before, over_before) = run.snapshot().await;
    let envelopes = agent_run_envelopes(run);
    tokio::pin!(envelopes);
    let mut last = after_seq;
    while let Some(envelope) = envelopes.next().await {
        // Sequence 0 is a run_started made up for a run that had none.
        if (envelope.sequence == 0 && after_seq > 0)
            || (envelope.sequence > 0 && envelope.sequence <= after_seq)
        {
            continue;
        }
        let live = envelope.sequence > sent_before;
        let terminal = agent_run_event_is_terminal(&envelope.event);
        let acted = if live || !over_before {
            acting(&turn, envelope.sequence, &envelope.event, live)
        } else {
            None
        };
        let event = serde_json::to_value(&envelope.event).unwrap_or(Value::Null);
        let record = frame(
            "run.event",
            json!({
                "turnId": turn.turn_id,
                "runId": turn.run_id,
                "sequence": envelope.sequence,
                "event": event,
            }),
        );
        last = last.max(envelope.sequence);
        for outgoing in std::iter::once(record).chain(acted) {
            if tx.send(outgoing).await.is_err() {
                return;
            }
        }
        if terminal {
            break;
        }
    }
    let _ = tx
        .send(frame(
            "run.end",
            json!({
                "turnId": turn.turn_id,
                "runId": turn.run_id,
                "lastSequence": last,
            }),
        ))
        .await;
}
