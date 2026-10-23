//! Her link's frames (`myriad_her_link`): whether a frame may be sent or
//! taken under protocol 2, and its one byte form. The rules read the
//! contract the core was built with.

use myriad_her_link::check;
use serde::Deserialize;
use serde_json::{Value, json};

use super::input;
use crate::Failure;

/// `{frame, roles?}` → `{accepted: {t, family, delivery, canonical}}` or
/// `{refused: {code, detail}}`. A refusal is an answer, not bad input: the
/// frame came from the other end.
pub(super) fn check_frame(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        frame: Value,
        #[serde(default)]
        roles: Vec<String>,
    }
    let In { frame, roles } = input(raw)?;
    Ok(match check(&frame, &roles) {
        Ok(checked) => json!({ "accepted": checked }),
        Err(refusal) => json!({ "refused": refusal }),
    })
}

/// `{frame}` → `{text}`: the frame this end is about to send, checked, in
/// its one byte form. A frame that does not pass is bad input.
pub(super) fn encode(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        frame: Value,
    }
    let In { frame } = input(raw)?;
    let checked = check(&frame, &[])
        .map_err(|refusal| Failure::BadInput(format!("{}: {}", refusal.code, refusal.detail)))?;
    Ok(json!({ "text": checked.canonical }))
}
