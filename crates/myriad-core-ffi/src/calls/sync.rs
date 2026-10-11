//! Her data across the site and this platform (`myriad_persona_sync`): the
//! one byte form of an entity, the comparison of two of her, the clock and
//! a rig's id. The format's types are the crate's; these calls only carry
//! them across.

use base64::Engine as _;
use myriad_merope::RigManifest;
use myriad_persona_sync::{
    EntityVersion, FieldRev, Holdings, Rev,
    affect::{self, Change},
    canonicalize as canonical_entity, compare as compare_sides,
    entity::{AffectAnchorBody, Baseline},
    hlc, is_empty as holds_nothing, rig_package_id as package_id,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::input;
use crate::Failure;

fn refused(error: impl std::fmt::Display) -> Failure {
    Failure::BadInput(error.to_string())
}

fn rev(raw: &str) -> Result<Rev, Failure> {
    raw.parse().map_err(refused)
}

/// Rough entities in, each in its one form out: the entity as JSON and its
/// canonical bytes as a string, which is what a platform stores and sends.
pub(super) fn canonicalize(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        entities: Vec<EntityVersion>,
    }
    let req: In = input(raw)?;
    let entities = req
        .entities
        .into_iter()
        .map(|entity| {
            let entity = canonical_entity(entity).map_err(refused)?;
            let canonical = String::from_utf8(entity.canonical_bytes())
                .map_err(|_| refused("canonical bytes are UTF-8"))?;
            Ok(json!({ "entity": entity, "canonical": canonical }))
        })
        .collect::<Result<Vec<_>, Failure>>()?;
    Ok(json!({ "entities": entities }))
}

pub(super) fn compare(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        local: Vec<EntityVersion>,
        remote: Vec<EntityVersion>,
    }
    let req: In = input(raw)?;
    let comparison = compare_sides(req.local, req.remote).map_err(refused)?;
    serde_json::to_value(comparison).map_err(refused)
}

pub(super) fn is_empty(raw: &[u8]) -> Result<Value, Failure> {
    let holdings: Holdings = input(raw)?;
    Ok(json!({ "empty": holds_nothing(&holdings) }))
}

pub(super) fn hlc_next(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        #[serde(default)]
        last: Option<String>,
        now: i64,
        node: String,
    }
    let req: In = input(raw)?;
    let last = req.last.as_deref().map(rev).transpose()?;
    let next = hlc::next(last.as_ref(), req.now, &req.node).map_err(refused)?;
    Ok(json!({ "rev": next.to_string() }))
}

pub(super) fn hlc_observe(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        #[serde(default)]
        last: Option<String>,
        seen: String,
        now: i64,
        node: String,
    }
    let req: In = input(raw)?;
    let last = req.last.as_deref().map(rev).transpose()?;
    let seen = rev(&req.seen)?;
    let after = hlc::observe(last.as_ref(), &seen, req.now, &req.node).map_err(refused)?;
    Ok(json!({ "last": after.to_string() }))
}

/// Whether the site takes `rev` when its clock reads `now`: at most
/// `maxAheadMs` ahead. A device checks itself with it against the site's
/// time before pushing; the site checks every revision a device sends.
pub(super) fn check_rev(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        rev: String,
        now: i64,
    }
    let req: In = input(raw)?;
    let rev = rev(&req.rev)?;
    Ok(json!({
        "accepted": hlc::check_ahead(&rev, req.now).is_ok(),
        "aheadMs": hlc::ahead_ms(&rev, req.now),
        "maxAheadMs": hlc::MAX_AHEAD_MS,
    }))
}

pub(super) fn legacy_rev(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        updated_at: i64,
        #[serde(default)]
        origin: Option<String>,
    }
    let req: In = input(raw)?;
    let legacy = hlc::legacy(req.updated_at, req.origin.as_deref()).map_err(refused)?;
    Ok(json!({ "rev": legacy.to_string() }))
}

pub(super) fn rig_package_id(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// The atlas file's bytes, base64.
        atlas: String,
        manifest: RigManifest,
    }
    let req: In = input(raw)?;
    let atlas = base64::engine::general_purpose::STANDARD
        .decode(req.atlas.as_bytes())
        .map_err(|_| refused("atlas is not base64"))?;
    let id = package_id(&atlas, &req.manifest).map_err(refused)?;
    Ok(json!({ "id": id }))
}

/// Her affect toward the owner now: the anchor with every change after it
/// replayed in `(at, id)` order, settled to `now` toward `baselineNow`.
/// A change is `{id, at, baseline, impulse}`; an affect event's body with its
/// id alongside will do (the rest of it is not read).
pub(super) fn affect_replay(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        anchor: AffectAnchorBody,
        #[serde(default)]
        events: Vec<Change>,
        now: i64,
        baseline_now: Baseline,
    }
    let req: In = input(raw)?;
    let affect =
        affect::replay(&req.anchor, &req.events, req.now, req.baseline_now).map_err(refused)?;
    Ok(json!({ "affect": affect }))
}

/// The anchor with every change up to `upto` (inclusive) folded in, standing
/// right after the last of them. Only the site writes the anchor; this is
/// the rule it folds by.
pub(super) fn affect_fold(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        anchor: AffectAnchorBody,
        #[serde(default)]
        events: Vec<Change>,
        upto: i64,
    }
    let req: In = input(raw)?;
    let anchor = affect::fold(&req.anchor, &req.events, req.upto).map_err(refused)?;
    Ok(json!({ "anchor": anchor }))
}

/// One write to her persona, field by field: `before` and `after` as
/// `{field: value}` (a field gone is `null`), `revs` the fields' revisions
/// before it (`{field: {rev, origin}}`; one missing had `legacyRev`), the
/// write's `rev` and `origin`. Gives the changed fields, every field's
/// revision after it, and the values it replaced, to keep as her history.
pub(super) fn persona_diff(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        before: std::collections::BTreeMap<String, Value>,
        after: std::collections::BTreeMap<String, Value>,
        #[serde(default)]
        revs: std::collections::BTreeMap<String, FieldRev>,
        legacy_rev: String,
        rev: String,
        origin: String,
        #[serde(default = "edit")]
        reason: String,
    }
    fn edit() -> String {
        "edit".into()
    }
    let req: In = input(raw)?;
    let diff = myriad_persona_sync::persona_diff(
        &req.before,
        &req.after,
        &req.revs,
        &rev(&req.legacy_rev)?,
        &rev(&req.rev)?,
        &req.origin,
        &req.reason,
    )
    .map_err(refused)?;
    serde_json::to_value(diff).map_err(refused)
}
