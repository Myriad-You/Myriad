//! How someone types: her lines set beside theirs (`contrast`), and how far
//! someone's last lines are from their usual way (`style`).

use myriad_merope::{contrast, style};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Nothing, input, text};
use crate::Failure;

pub(super) fn pieces(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        message: String,
    }
    let req: In = input(raw)?;
    Ok(json!({ "pieces": contrast::pieces(&req.message) }))
}

/// A piece she uses far more than they do.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Overused {
    piece: String,
    hers: u32,
    her_messages: u32,
    theirs: u32,
    their_messages: u32,
    z: f64,
}

impl From<contrast::Overused> for Overused {
    fn from(value: contrast::Overused) -> Self {
        Self {
            piece: value.piece,
            hers: value.hers,
            her_messages: value.her_messages,
            theirs: value.theirs,
            their_messages: value.their_messages,
            z: value.z,
        }
    }
}

impl From<Overused> for contrast::Overused {
    fn from(value: Overused) -> Self {
        Self {
            piece: value.piece,
            hers: value.hers,
            her_messages: value.her_messages,
            theirs: value.theirs,
            their_messages: value.their_messages,
            z: value.z,
        }
    }
}

/// How many messages, and how many had each piece: `[piece, count]` pairs,
/// so that pieces a platform's own strings would take for equal (another
/// normalization of the same characters) stay apart, as they do here.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Counts {
    messages: u32,
    pieces: Vec<(String, u32)>,
}

impl From<Counts> for contrast::Counts {
    fn from(value: Counts) -> Self {
        Self {
            messages: value.messages,
            pieces: value.pieces.into_iter().collect(),
        }
    }
}

pub(super) fn overused(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        hers: Counts,
        theirs: Counts,
    }
    let req: In = input(raw)?;
    let found: Vec<Overused> = contrast::overused(&req.hers.into(), &req.theirs.into())
        .into_iter()
        .map(Overused::from)
        .collect();
    Ok(json!({ "overused": found }))
}

pub(super) fn describe(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        overused: Vec<Overused>,
    }
    let req: In = input(raw)?;
    let found: Vec<contrast::Overused> = req.overused.into_iter().map(Into::into).collect();
    Ok(text(contrast::describe(&found)))
}

pub(super) fn unlike(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// Their lines before, and their last few.
        history: Vec<String>,
        recent: Vec<String>,
    }
    let req: In = input(raw)?;
    Ok(json!({ "z": style::unlike(&req.history, &req.recent) }))
}

pub(super) fn distance(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// Two sets of lines, each read as one way of typing.
        left: Vec<String>,
        right: Vec<String>,
    }
    let req: In = input(raw)?;
    Ok(json!({
        "distance": style::distance(&style::profile(&req.left), &style::profile(&req.right)),
    }))
}

pub(super) fn noticed(raw: &[u8]) -> Result<Value, Failure> {
    let Nothing {} = input(raw)?;
    Ok(json!({
        "knownAfter": style::KNOWN_AFTER,
        "tellsAfter": style::TELLS_AFTER,
        "noticedAt": style::NOTICED_AT,
        "text": style::NOTICED,
    }))
}
