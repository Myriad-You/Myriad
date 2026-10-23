//! One row of her log: its envelope and its one byte form.
//!
//! A row never holds content, only a reference to its payload, so a
//! forgotten row can lose what it said and keep its place in the log (a
//! stub). Once written a row is never changed; becoming a stub is the only
//! thing that happens to it.

use std::cmp::Ordering;

use myriad_persona_sync::{
    Rev,
    canonical::{canonical_bytes, digest},
    hlc,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    LogError,
    kinds::{self, Rule},
};

/// The row format this core writes and the newest it reads.
pub const LOG_VERSION: u32 = 1;

/// Longest id, object, subject, audience or payload reference, in bytes.
pub const MAX_TOKEN_BYTES: usize = 512;

/// Who wrote a row. Her rows come only from the holder of the lease; the
/// owner's (what they said, set, approved, revoked, had her forget) from
/// any side, offline too.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Author {
    Owner,
    Her,
}

/// Whether a row may leave the side that wrote it. A local row (what the
/// screen or another agent's record gave rise to, unless the owner opens it
/// up) is never encoded for another side and takes no `dseq`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Replicate {
    Shared,
    Local,
}

/// Where a row's content is kept: a reference the writing side resolves,
/// and the id of the key it is sealed with when it is. Never the content.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Payload {
    #[serde(rename = "ref")]
    pub reference: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key: Option<String>,
}

/// One row of the log.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Row {
    /// The format it was written in ([`LOG_VERSION`] or older).
    pub version: u32,
    /// Unique across every side: dedupe is by id.
    pub id: String,
    /// The side that wrote it: `site`, `dev_…`, `loc_…` (the clock's node).
    pub origin: String,
    pub author: Author,
    /// Her rows only: the lease epoch the writer held when it wrote it;
    /// [`crate::lease::ALONE`] (0) before the writer ever held or saw one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<u64>,
    /// Shared rows only: the how-manyth shared row of `origin` in
    /// `audience`, from 1, without gaps, so a version vector of that
    /// audience says what is missing. Counted per audience because sides
    /// keep different audiences (a group's log is the server's alone): one
    /// count across them would leave holes no side can fill.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dseq: Option<u64>,
    /// The writer's clock when it wrote it; its node is `origin`.
    pub hlc: Rev,
    /// `personal:<owner>`, `own:` or `group:<id>`.
    pub audience: String,
    /// Who it is about, when it is about someone.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// What it is about: `<type>:<id>` (`node:…`, `memory:…`, `persona:<field>`).
    pub object: String,
    /// One of [`kinds::all`].
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub payload: Option<Payload>,
    pub replicate: Replicate,
    /// A stub: what it said is gone, its place in the log stays.
    #[serde(default, skip_serializing_if = "is_false")]
    pub forgotten: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// Whose audience a row is in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Audience<'a> {
    /// The owner's private talk; the members joined by `,`.
    Personal(&'a str),
    /// Her own.
    Own,
    /// One group.
    Group(&'a str),
}

impl<'a> Audience<'a> {
    pub fn parse(raw: &'a str) -> Result<Self, LogError> {
        let bad = || LogError::new(format!("audience {raw:?} is not personal:, own: or group:"));
        let (venue, rest) = raw.split_once(':').ok_or_else(bad)?;
        match venue {
            "personal" if !rest.is_empty() && rest.split(',').all(|one| !one.is_empty()) => {
                Ok(Self::Personal(rest))
            }
            "own" if rest.is_empty() => Ok(Self::Own),
            "group" if !rest.is_empty() => Ok(Self::Group(rest)),
            _ => Err(bad()),
        }
    }

    /// Whether `subject` may be the subject of a row here. Who is in a
    /// group is the caller's to say ([`crate::admit`]).
    pub fn admits(&self, subject: Option<&str>) -> bool {
        match (self, subject) {
            (_, None) => true,
            (Self::Personal(members), Some(subject)) => members.split(',').any(|m| m == subject),
            (Self::Own, Some(_)) => false,
            (Self::Group(_), Some(_)) => true,
        }
    }
}

fn check_token(what: &str, value: &str) -> Result<(), LogError> {
    if value.is_empty() || value.len() > MAX_TOKEN_BYTES {
        return Err(LogError::new(format!(
            "{what} must be 1 to {MAX_TOKEN_BYTES} bytes"
        )));
    }
    if value.chars().any(char::is_control) {
        return Err(LogError::new(format!("{what} holds a control character")));
    }
    Ok(())
}

/// The type of an object: the part before `:`.
pub fn object_type(object: &str) -> Option<&str> {
    let (kind, id) = object.split_once(':')?;
    let named = !kind.is_empty()
        && kind
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte == b'_');
    (named && !id.is_empty()).then_some(kind)
}

impl Row {
    /// How it folds. Only for a row that passed [`Row::check`].
    pub fn rule(&self) -> Rule {
        kinds::rule(&self.kind).unwrap_or(Rule::Event)
    }

    /// Whether the envelope is whole and consistent. Says nothing about
    /// the log it goes into: that is [`crate::admit`].
    pub fn check(&self) -> Result<(), LogError> {
        if self.version == 0 || self.version > LOG_VERSION {
            return Err(LogError::new(format!(
                "row version {} is not one this core reads (1 to {LOG_VERSION})",
                self.version
            )));
        }
        check_token("id", &self.id)?;
        hlc::check_node(&self.origin).map_err(|error| LogError::new(error.to_string()))?;
        if self.hlc.node != self.origin {
            return Err(LogError::new(format!(
                "hlc node {:?} is not the origin {:?}",
                self.hlc.node, self.origin
            )));
        }
        match (self.author, self.epoch) {
            (Author::Her, None) => {
                return Err(LogError::new(
                    "her row carries no lease epoch (0 while there is none yet)",
                ));
            }
            (Author::Owner, Some(_)) => {
                return Err(LogError::new("the owner's row carries no lease epoch"));
            }
            _ => {}
        }
        match (self.replicate, self.dseq) {
            (Replicate::Shared, None | Some(0)) => {
                return Err(LogError::new("a shared row needs a dseq from 1"));
            }
            (Replicate::Local, Some(_)) => {
                return Err(LogError::new("a local row takes no dseq"));
            }
            _ => {}
        }
        check_token("audience", &self.audience)?;
        let audience = Audience::parse(&self.audience)?;
        if let Some(subject) = &self.subject {
            check_token("subject", subject)?;
        }
        if !audience.admits(self.subject.as_deref()) {
            return Err(LogError::new(format!(
                "subject is not one of audience {:?}",
                self.audience
            )));
        }
        check_token("object", &self.object)?;
        let object = object_type(&self.object)
            .ok_or_else(|| LogError::new(format!("object {:?} is not <type>:<id>", self.object)))?;
        let rule = kinds::rule(&self.kind).ok_or_else(|| {
            LogError::new(format!("kind {:?} is not one the log holds", self.kind))
        })?;
        match kinds::object_type(&self.kind) {
            Some(wanted) if wanted != object => {
                return Err(LogError::new(format!(
                    "a {} row is about a {wanted}, not {object:?}",
                    self.kind
                )));
            }
            _ => {}
        }
        if object == kinds::SUBJECT && rule != Rule::Forget {
            return Err(LogError::new("only a forget is about a whole subject"));
        }
        if let Some(payload) = &self.payload {
            check_token("payload ref", &payload.reference)?;
            if let Some(key) = &payload.key {
                check_token("payload key", key)?;
            }
        }
        if rule == Rule::Forget && self.payload.is_some() {
            return Err(LogError::new("a forget holds no payload"));
        }
        if rule == Rule::Forget && self.forgotten {
            return Err(LogError::new("a forget is never forgotten"));
        }
        if self.forgotten && self.payload.is_some() {
            return Err(LogError::new("a stub keeps no payload"));
        }
        Ok(())
    }

    /// Its JSON, as the format writes it.
    pub fn to_value(&self) -> Value {
        // Every field serializes: strings, numbers and a `Rev`.
        serde_json::to_value(self).expect("a row serializes")
    }

    /// Its one byte form: canonical JSON.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        canonical_bytes(&self.to_value())
    }

    /// The first 16 bytes of sha256 of its canonical bytes, in hex.
    pub fn digest(&self) -> String {
        digest(&self.to_value())
    }

    /// The row once forgotten: no payload, marked.
    pub fn stub(&self) -> Self {
        Self {
            payload: None,
            forgotten: true,
            ..self.clone()
        }
    }

    /// Whether this is `other` forgotten.
    pub fn is_stub_of(&self, other: &Self) -> bool {
        self.forgotten && !other.forgotten && *self == other.stub()
    }

    /// Log order: the clock, then the side, then its count. The clock
    /// follows causality (a side observes what it receives before writing),
    /// so a row written after seeing another sorts after it.
    pub fn log_order(&self, other: &Self) -> Ordering {
        (&self.hlc, &self.origin, self.dseq, &self.id).cmp(&(
            &other.hlc,
            &other.origin,
            other.dseq,
            &other.id,
        ))
    }
}

/// Sorts rows into log order.
pub fn sort(rows: &mut [Row]) {
    rows.sort_by(Row::log_order);
}
