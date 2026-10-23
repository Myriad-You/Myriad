//! Her link's frames: what a frame must look like before it is sent or
//! taken, and its one byte form. The contract is
//! `shared/her_link_contract.json`; protocol 2 (its `next` block, planned:
//! no host speaks it yet) is what these rules check. Protocol 2 keeps every
//! protocol-1 frame, so those check here too, against their protocol-1
//! bodies inside the protocol-2 envelope.
//!
//! A frame is one canonical JSON text: `{v, t, seq, ack, id?, at, body}`.
//! A sealed field (HPKE, RFC 9180) is checked for its shape only: opening
//! and sealing are the platform's, with its own keys.
//!
//! Pure: nothing here reads a clock, a file, the network or a key.

use std::{collections::BTreeMap, sync::LazyLock};

use base64::Engine as _;
use myriad_persona_sync::canonical::canonical_bytes;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The contract this crate was built with.
pub const CONTRACT_JSON: &str = include_str!("../../../shared/her_link_contract.json");

/// One frame type's rules.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrameSpec {
    pub from: String,
    pub delivery: String,
    pub required: Vec<String>,
    #[serde(default)]
    pub optional: Vec<String>,
    /// Pairs of fields of which exactly one is present.
    #[serde(default)]
    pub one_of: Vec<Vec<String>>,
    /// The most items an array field may hold.
    #[serde(default)]
    pub max_items: BTreeMap<String, usize>,
    /// Fields that may come sealed.
    #[serde(default)]
    pub sealed: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Envelope {
    required: Vec<String>,
    optional: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Limits {
    max_frame_bytes: usize,
    /// What [`valid_id`] checks; read by the tests.
    #[cfg_attr(not(test), allow(dead_code))]
    id_pattern: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Encoding {
    frame_version: u64,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sealing {
    pub suite: String,
    pub enc_bytes: usize,
    pub tag_bytes: usize,
    pub info: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Next {
    protocol: u32,
    envelope: Envelope,
    limits: Limits,
    encoding: Encoding,
    sealing: Sealing,
    families: BTreeMap<String, String>,
    frames: BTreeMap<String, FrameSpec>,
    error_codes: BTreeMap<String, String>,
}

#[derive(Debug, Deserialize)]
struct ProtocolOne {
    frames: BTreeMap<String, FrameSpec>,
}

struct Contract {
    next: Next,
    one: ProtocolOne,
}

static CONTRACT: LazyLock<Contract> = LazyLock::new(|| {
    let raw: Value = serde_json::from_str(CONTRACT_JSON).expect("her_link_contract.json");
    Contract {
        next: serde_json::from_value(raw["next"].clone()).expect("her_link_contract.json: next"),
        one: serde_json::from_value(raw.clone()).expect("her_link_contract.json: frames"),
    }
});

/// The protocol these rules check.
pub fn protocol() -> u32 {
    CONTRACT.next.protocol
}

/// The sealing suite every sealed field uses.
pub fn sealing() -> &'static Sealing {
    &CONTRACT.next.sealing
}

/// The HPKE `info` for a sealed field of frame type `t`.
pub fn seal_info(t: &str) -> String {
    format!("{} {t}", CONTRACT.next.sealing.info)
}

/// The additional data a sealed field of frame `t` is bound to: canonical
/// JSON `{from, t, to}`.
pub fn seal_aad(from: &str, t: &str, to: &str) -> Vec<u8> {
    canonical_bytes(&serde_json::json!({ "from": from, "t": t, "to": to }))
}

/// Why a frame was refused: one of the contract's `next.errorCodes`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Refusal {
    pub code: String,
    pub detail: String,
}

fn refuse(code: &str, detail: impl Into<String>) -> Refusal {
    debug_assert!(CONTRACT.next.error_codes.contains_key(code), "{code}");
    Refusal {
        code: code.to_string(),
        detail: detail.into(),
    }
}

/// A frame that passed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Checked {
    pub t: String,
    /// The family (`fact`, `lease`…), or `null` for a protocol-1 frame.
    pub family: Option<String>,
    pub delivery: String,
    /// Its one byte form.
    pub canonical: String,
}

/// `limits.idPattern`: `^[A-Za-z0-9_.:-]{1,64}$`.
pub fn valid_id(id: &str) -> bool {
    (1..=64).contains(&id.len())
        && id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
}

fn spec(t: &str) -> Option<(&'static FrameSpec, Option<String>)> {
    if let Some(spec) = CONTRACT.next.frames.get(t) {
        let family = t.split('.').next().map(str::to_string);
        return Some((spec, family));
    }
    CONTRACT.one.frames.get(t).map(|spec| (spec, None))
}

fn decoded(what: &str, raw: &Value) -> Result<Vec<u8>, Refusal> {
    raw.as_str()
        .and_then(|raw| base64::engine::general_purpose::STANDARD.decode(raw).ok())
        .ok_or_else(|| refuse("bad_sealed", format!("{what} is not base64")))
}

/// Whether `value` is a sealed field of the contract's suite.
fn check_sealed(field: &str, value: &Value) -> Result<(), Refusal> {
    let sealing = &CONTRACT.next.sealing;
    let Some(object) = value.as_object() else {
        return Err(refuse("bad_sealed", format!("{field} is not an object")));
    };
    let keys: Vec<&str> = object.keys().map(String::as_str).collect();
    let mut wanted = ["ct", "enc", "keyId", "suite", "to"];
    wanted.sort_unstable();
    let mut have = keys.clone();
    have.sort_unstable();
    if have != wanted {
        return Err(refuse(
            "bad_sealed",
            format!("{field} has {have:?}, not {wanted:?}"),
        ));
    }
    if object["suite"].as_str() != Some(sealing.suite.as_str()) {
        return Err(refuse("bad_sealed", format!("{field} is of another suite")));
    }
    if !object["to"].as_str().is_some_and(valid_id) {
        return Err(refuse(
            "bad_sealed",
            format!("{field}.to is not a device id"),
        ));
    }
    let key_id = object["keyId"].as_str().unwrap_or_default();
    if key_id.len() != 32
        || !key_id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(refuse("bad_sealed", format!("{field}.keyId is not 32 hex")));
    }
    if decoded("enc", &object["enc"])?.len() != sealing.enc_bytes {
        return Err(refuse(
            "bad_sealed",
            format!("{field}.enc is not {} bytes", sealing.enc_bytes),
        ));
    }
    if decoded("ct", &object["ct"])?.len() < sealing.tag_bytes {
        return Err(refuse(
            "bad_sealed",
            format!("{field}.ct is shorter than a tag"),
        ));
    }
    Ok(())
}

fn fields(value: &Value, what: &str) -> Result<Vec<String>, Refusal> {
    value
        .as_object()
        .map(|object| object.keys().cloned().collect())
        .ok_or_else(|| refuse("bad_frame", format!("{what} is not an object")))
}

fn check_fields(
    what: &str,
    have: &[String],
    required: &[String],
    optional: &[String],
) -> Result<(), Refusal> {
    if let Some(missing) = required.iter().find(|field| !have.contains(field)) {
        return Err(refuse("bad_frame", format!("{what} has no {missing}")));
    }
    if let Some(extra) = have
        .iter()
        .find(|field| !required.contains(field) && !optional.contains(field))
    {
        return Err(refuse("bad_frame", format!("{what} takes no {extra}")));
    }
    Ok(())
}

/// Whether `frame` is one that may be sent or taken, from a sender with
/// `roles` (`device`, `host`, `mind`, `server`; empty: not checked).
pub fn check(frame: &Value, roles: &[String]) -> Result<Checked, Refusal> {
    let next = &CONTRACT.next;
    let bytes = canonical_bytes(frame);
    if bytes.len() > next.limits.max_frame_bytes {
        return Err(refuse(
            "too_large",
            format!(
                "{} bytes, at most {}",
                bytes.len(),
                next.limits.max_frame_bytes
            ),
        ));
    }
    let envelope = fields(frame, "the frame")?;
    check_fields(
        "the envelope",
        &envelope,
        &next.envelope.required,
        &next.envelope.optional,
    )?;
    if frame["v"].as_u64() != Some(next.encoding.frame_version) {
        return Err(refuse(
            "version_unsupported",
            format!(
                "v is {}, this end reads {}",
                frame["v"], next.encoding.frame_version
            ),
        ));
    }
    let Some(t) = frame["t"].as_str() else {
        return Err(refuse("bad_frame", "t is not text"));
    };
    let Some((spec, family)) = spec(t) else {
        return Err(refuse("unknown_frame", format!("no frame {t:?}")));
    };
    if !frame["seq"].as_u64().is_some_and(|seq| seq >= 1) {
        return Err(refuse("bad_frame", "seq counts from 1"));
    }
    if frame["ack"].as_u64().is_none() || frame["at"].as_u64().is_none() {
        return Err(refuse("bad_frame", "ack and at are whole numbers from 0"));
    }
    let reliable = spec.delivery == "reliable";
    match frame.get("id") {
        Some(id) if !reliable => {
            return Err(refuse(
                "bad_frame",
                format!("{t} is not reliable and takes no id: {id}"),
            ));
        }
        Some(id) if !id.as_str().is_some_and(valid_id) => {
            return Err(refuse(
                "bad_id",
                format!("id {id} does not match the pattern"),
            ));
        }
        None if reliable => {
            return Err(refuse(
                "bad_frame",
                format!("{t} is reliable and needs an id"),
            ));
        }
        _ => {}
    }
    if family.is_some() && !roles.is_empty() && spec.from != "either" && !roles.contains(&spec.from)
    {
        return Err(refuse(
            "bad_frame",
            format!("{t} comes from a {}, not from {roles:?}", spec.from),
        ));
    }
    let body = &frame["body"];
    let have = fields(body, "the body")?;
    check_fields(
        &format!("{t}'s body"),
        &have,
        &spec.required,
        &spec.optional,
    )?;
    for pair in &spec.one_of {
        let present = pair.iter().filter(|field| have.contains(field)).count();
        if present != 1 {
            return Err(refuse(
                "bad_frame",
                format!("{t} takes exactly one of {pair:?}"),
            ));
        }
    }
    for (field, most) in &spec.max_items {
        if let Some(items) = body.get(field) {
            let Some(items) = items.as_array() else {
                return Err(refuse("bad_frame", format!("{t}.{field} is not a list")));
            };
            if items.len() > *most {
                return Err(refuse(
                    "too_many",
                    format!("{t}.{field} has {}, at most {most}", items.len()),
                ));
            }
        }
    }
    for field in &spec.sealed {
        if let Some(value) = body.get(field) {
            check_sealed(field, value)?;
        }
    }
    Ok(Checked {
        t: t.to_string(),
        family,
        delivery: spec.delivery.clone(),
        canonical: String::from_utf8(bytes).expect("canonical JSON is UTF-8"),
    })
}

/// [`check`] for the bytes off the wire.
pub fn check_bytes(bytes: &[u8], roles: &[String]) -> Result<Checked, Refusal> {
    if bytes.len() > CONTRACT.next.limits.max_frame_bytes {
        return Err(refuse("too_large", format!("{} bytes", bytes.len())));
    }
    let frame: Value =
        serde_json::from_slice(bytes).map_err(|error| refuse("bad_frame", error.to_string()))?;
    check(&frame, roles)
}

/// Every family and what it carries.
pub fn families() -> &'static BTreeMap<String, String> {
    &CONTRACT.next.families
}

#[cfg(test)]
mod tests;
