//! `core/shared/her_link_contract.json`, read once: the protocol number, timings,
//! limits and the codes the host closes and refuses with all come from it,
//! so the host cannot drift from what children build against.
//!
//! What a frame must look like is the shared core's (`myriad_her_link`),
//! which reads the same contract: [`core_check`].

use std::collections::BTreeMap;
use std::sync::LazyLock;
use std::time::Duration;

use serde::Deserialize;
use serde_json::Value;

pub(super) use myriad_her_link::valid_id;

pub(super) const CONTRACT_JSON: &str =
    include_str!("../../../../../core/shared/her_link_contract.json");

/// The host reads some of these at run time; the rest are checked against
/// the code by the contract tests.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct Contract {
    pub protocol: u32,
    pub path: String,
    pub timing: TimingSpec,
    pub limits: Limits,
    pub hosts: Vec<String>,
    pub surfaces: Vec<String>,
    pub capabilities: Vec<String>,
    pub say_kinds: Vec<String>,
    pub notice_kinds: Vec<String>,
    pub work_places: Vec<String>,
    pub act_statuses: Vec<String>,
    pub moments: MomentRules,
    pub cap_names: CapNames,
    pub frames: BTreeMap<String, FrameSpec>,
    pub error_codes: BTreeMap<String, String>,
    pub close_codes: BTreeMap<String, CloseSpec>,
    pub next: Next,
}

/// `next`: protocol 2, which the shared core's frame rules are written for.
#[derive(Debug, Deserialize)]
pub(super) struct Next {
    pub encoding: Encoding,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Encoding {
    pub frame_version: u64,
}

/// `moments`: what a child's `moment` may be.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct MomentRules {
    pub kinds: Vec<String>,
    pub keeps_memory: Vec<String>,
    pub urgencies: Vec<String>,
    pub headline_chars: usize,
    pub facts_at_most: usize,
    pub fact_chars: usize,
    pub fact_key: FactKeyRules,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FactKeyRules {
    pub max_chars: usize,
    pub refused_parts: Vec<String>,
}

/// `capNames`: the site's permission names a cap may carry, and the Mac's
/// grants each stands for.
#[derive(Debug, Deserialize)]
pub(super) struct CapNames {
    pub mac: BTreeMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TimingSpec {
    pub heartbeat_ms: u64,
    pub say_ttl_ms: u64,
    #[cfg_attr(not(test), allow(dead_code))]
    pub touch_appraise_ms: u64,
    pub act_pickup_ms: u64,
    pub act_max_run_ms: u64,
    pub missed_heartbeats: u32,
    pub hello_timeout_ms: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Limits {
    pub max_frame_bytes: usize,
    pub max_text_chars: usize,
    #[cfg_attr(not(test), allow(dead_code))]
    pub id_pattern: String,
}

/// Who sends a frame type and how it is delivered; its fields are the
/// shared core's to check.
#[derive(Debug, Deserialize)]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct FrameSpec {
    pub from: String,
    pub delivery: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct CloseSpec {
    pub code: u16,
    pub forget_credentials: bool,
    pub reconnect: bool,
}

pub(super) static CONTRACT: LazyLock<Contract> = LazyLock::new(|| {
    serde_json::from_str(CONTRACT_JSON).expect("core/shared/her_link_contract.json")
});

/// How often the link is checked, and how long it may stay quiet.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Timing {
    pub heartbeat: Duration,
    pub missed: u32,
    pub hello: Duration,
}

impl Timing {
    pub(crate) fn from_contract() -> Self {
        let timing = &CONTRACT.timing;
        Self {
            heartbeat: Duration::from_millis(timing.heartbeat_ms),
            missed: timing.missed_heartbeats,
            hello: Duration::from_millis(timing.hello_timeout_ms),
        }
    }

    /// Nothing heard for this long: the link is gone.
    pub(crate) fn silence(&self) -> Duration {
        self.heartbeat * self.missed
    }
}

/// Why the host closes a link. The numbers are the contract's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Close {
    Revoked,
    OwnerChanged,
    Replaced,
    /// She lives on another end now (her host lease names it).
    HostMoved,
    /// She is moving: while the move is under way no end has her.
    HostFrozen,
    HostIsSite,
    BadFrame,
    UnsupportedProtocol,
    Timeout,
    Unavailable,
}

impl Close {
    #[cfg(test)]
    pub(crate) const ALL: [Close; 10] = [
        Close::Revoked,
        Close::OwnerChanged,
        Close::Replaced,
        Close::HostMoved,
        Close::HostFrozen,
        Close::HostIsSite,
        Close::BadFrame,
        Close::UnsupportedProtocol,
        Close::Timeout,
        Close::Unavailable,
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Close::Revoked => "revoked",
            Close::OwnerChanged => "owner_changed",
            Close::Replaced => "replaced",
            Close::HostMoved => "host_moved",
            Close::HostFrozen => "host_frozen",
            Close::HostIsSite => "host_is_site",
            Close::BadFrame => "bad_frame",
            Close::UnsupportedProtocol => "unsupported_protocol",
            Close::Timeout => "timeout",
            Close::Unavailable => "unavailable",
        }
    }

    pub(crate) fn code(self) -> u16 {
        CONTRACT
            .close_codes
            .get(self.name())
            .map(|spec| spec.code)
            .expect("every close the host uses is in the contract")
    }
}

/// Frame types the host sends.
#[cfg(test)]
pub(super) const HOST_FRAMES: [&str; 28] = [
    "welcome",
    "host.freeze",
    "host.thaw",
    "host.moved",
    "run.accepted",
    "run.rejected",
    "run.event",
    "run.end",
    "perform",
    "mood",
    "ping",
    "pong",
    "error",
    "say",
    "touch.reaction",
    "outfit",
    "work.recalled",
    "experience.taken",
    "conversations",
    "notice",
    "proposals",
    "proposal.answered",
    "act.request",
    "act.cancel",
    "wardrobe",
    "conversation",
    "moment.taken",
    "nodes",
];

/// Frame types the host takes from a child.
#[cfg(test)]
pub(super) const CHILD_FRAMES: [&str; 19] = [
    "hello",
    "input",
    "presence",
    "run.resume",
    "ack",
    "ping",
    "pong",
    "playback.observe",
    "playback.end",
    "speech.delivered",
    "touch.appraise",
    "touch",
    "outfit.wear",
    "work.recall",
    "experience",
    "proposal.answer",
    "act.result",
    "moment",
    "node.forget",
];

/// Error codes of the link: what the host sends (run.rejected may also carry
/// an AI quota code), and the act codes `act.result` and `act.cancel` carry.
#[cfg(test)]
pub(super) const HOST_ERROR_CODES: [&str; 24] = [
    "unknown_frame",
    "bad_frame",
    "not_ready",
    "binary_unsupported",
    "empty_input",
    "input_too_long",
    "bad_id",
    "surface_unsupported",
    "turn_unknown",
    "agent_access_denied",
    "host_frozen",
    "host_moved",
    "internal_error",
    "not_this_device",
    "too_large",
    "outfit_unknown",
    "proposal_unknown",
    "place_unknown",
    "act_off",
    "act_busy",
    "act_unavailable",
    "act_timeout",
    "act_cancelled",
    "act_revoked",
];

/// The names of `cap` a Mac knows (`capNames`), in the order given: the
/// rest stand for nothing there, so they are not sent.
pub(super) fn cap_for_mac(cap: Vec<String>) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for name in cap {
        if CONTRACT.cap_names.mac.contains_key(&name) && !kept.contains(&name) {
            kept.push(name);
        }
    }
    kept
}

/// What the shared core's rules say of a frame of the link: its envelope,
/// `seq`, `ack`, `at`, a reliable frame's `id`, its body's fields and its
/// size (`myriad_her_link::check`).
///
/// The core reads protocol-1 frames inside the protocol-2 envelope, which is
/// protocol 1's with `v`. A protocol-1 frame has no `v`, so it is read as the
/// contract's frame version; one that carries its own is read as it says.
/// Senders' roles are the core's for protocol 2 only, so none are given.
///
/// The host does not refuse a frame on this alone. The core is stricter than
/// the host has been in a few places, and the host keeps its own answer
/// there until those are settled in the core; each is in the tests
/// (`where_the_core_and_the_host_differ`).
pub(super) fn core_check(frame: &Value) -> Result<(), myriad_her_link::Refusal> {
    let mut frame = frame.clone();
    if let Some(envelope) = frame.as_object_mut() {
        envelope
            .entry("v")
            .or_insert(CONTRACT.next.encoding.frame_version.into());
    }
    myriad_her_link::check(&frame, &[]).map(|_| ())
}
