//! Changing clothes in a chat, for a while (`myriad_merope`'s outfit
//! overlay): her wardrobe as chat sees it, the `[[wear:…]]` marker, and
//! what a request or a marker comes to.

use myriad_merope::{OverlayDecision, WardrobeLook, WearDirective};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{input, text};
use crate::Failure;

/// A saved set she can change into in chat.
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Look {
    id: String,
    label: String,
    clothing_style: String,
    #[serde(default)]
    portrait_asset_id: Option<String>,
    #[serde(default)]
    rig_asset_id: Option<String>,
    #[serde(default)]
    generation_fingerprint: Option<String>,
    hints: Vec<String>,
}

impl From<Look> for WardrobeLook {
    fn from(value: Look) -> Self {
        Self {
            id: value.id,
            label: value.label,
            clothing_style: value.clothing_style,
            portrait_asset_id: value.portrait_asset_id,
            rig_asset_id: value.rig_asset_id,
            generation_fingerprint: value.generation_fingerprint,
            hints: value.hints,
        }
    }
}

impl From<WardrobeLook> for Look {
    fn from(value: WardrobeLook) -> Self {
        Self {
            id: value.id,
            label: value.label,
            clothing_style: value.clothing_style,
            portrait_asset_id: value.portrait_asset_id,
            rig_asset_id: value.rig_asset_id,
            generation_fingerprint: value.generation_fingerprint,
            hints: value.hints,
        }
    }
}

/// A marker's directive: back to what she wears, or the set it names.
#[derive(Deserialize, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum Directive {
    Revert,
    Label { label: String },
}

impl From<Directive> for WearDirective {
    fn from(value: Directive) -> Self {
        match value {
            Directive::Revert => Self::Revert,
            Directive::Label { label } => Self::Label(label),
        }
    }
}

impl From<WearDirective> for Directive {
    fn from(value: WearDirective) -> Self {
        match value {
            WearDirective::Revert => Self::Revert,
            WearDirective::Label(label) => Self::Label { label },
        }
    }
}

fn decision(decided: OverlayDecision) -> Value {
    match decided {
        OverlayDecision::Unchanged => json!({ "kind": "unchanged" }),
        OverlayDecision::Clear => json!({ "kind": "clear" }),
        OverlayDecision::Wear(id) => json!({ "kind": "wear", "id": id }),
    }
}

fn looks(looks: Vec<Look>) -> Vec<WardrobeLook> {
    looks.into_iter().map(Into::into).collect()
}

pub(super) fn looks_from_profile(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// Her visual profile as stored (`activeOutfitId`, `wardrobe`, …).
        profile: Value,
    }
    let req: In = input(raw)?;
    let found: Vec<Look> = myriad_merope::looks_from_visual_profile(&req.profile)
        .into_iter()
        .map(Look::from)
        .collect();
    Ok(json!({ "looks": found }))
}

pub(super) fn wardrobe_section(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        looks: Vec<Look>,
        /// The set she wears, and the one shown over it in this chat.
        worn: String,
        #[serde(default)]
        overlay: Option<String>,
    }
    let req: In = input(raw)?;
    Ok(text(myriad_merope::format_chat_wardrobe_section(
        &looks(req.looks),
        &req.worn,
        req.overlay.as_deref(),
    )))
}

pub(super) fn split_wear(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        raw: String,
    }
    let req: In = input(raw)?;
    let (spoken, directive) = myriad_merope::split_chat_wear_directive(&req.raw);
    Ok(json!({ "spoken": spoken, "directive": directive.map(Directive::from) }))
}

pub(super) fn hold_wear(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        spoken: String,
    }
    let req: In = input(raw)?;
    Ok(text(Some(
        myriad_merope::hold_incomplete_wear_marker(&req.spoken).to_string(),
    )))
}

pub(super) fn resolve_directive(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        directive: Directive,
        looks: Vec<Look>,
        worn: String,
        #[serde(default)]
        overlay: Option<String>,
    }
    let req: In = input(raw)?;
    let all = looks(req.looks);
    Ok(json!({
        "decision": decision(myriad_merope::resolve_wear_directive(
            &req.directive.into(),
            &all,
            &req.worn,
            req.overlay.as_deref(),
        )),
    }))
}

pub(super) fn after_reply(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        /// What they said, what she said, and her marker if she gave one.
        input: String,
        spoken: String,
        #[serde(default)]
        marker: Option<Directive>,
    }
    let req: In = input(raw)?;
    let directive = myriad_merope::wear_directive_after_reply(
        &req.input,
        &req.spoken,
        req.marker.map(Into::into),
    );
    Ok(json!({ "directive": directive.map(Directive::from) }))
}

pub(super) fn resolve(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct In {
        input: String,
        looks: Vec<Look>,
        worn: String,
        #[serde(default)]
        overlay: Option<String>,
    }
    let req: In = input(raw)?;
    let all = looks(req.looks);
    Ok(json!({
        "decision": decision(myriad_merope::resolve_chat_outfit_overlay(
            &req.input,
            &all,
            &req.worn,
            req.overlay.as_deref(),
        )),
    }))
}
