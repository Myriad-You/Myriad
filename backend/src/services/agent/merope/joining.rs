//! Whether she joins in a group's talk when nobody called her by name (see
//! `myriad_merope::joining`): what she has that touches the talk, and the
//! judgment, as her. The group itself decides nothing here: when to look and
//! how to answer are `channel_group`'s.

use std::time::Duration;

use chrono::Utc;
use myriad_merope::joining::{Material, SCHEMA_NAME, material_view, parse, reason, schema, system};
use sea_orm::DatabaseConnection;
use serde_json::json;

use super::call::{self, Voice};

const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// Things of hers per kind that the talk may touch.
const PER_KIND: usize = 3;

/// What she has that the talk touches: her views on it, what she did that it
/// brings up, what she heard about it elsewhere, and what of hers she would
/// want to tell someone.
pub async fn material(db: &DatabaseConnection, talk: &str) -> Vec<Material> {
    let mut material: Vec<Material> = super::views::touched(db, talk, PER_KIND)
        .await
        .into_iter()
        .map(|(about, view)| Material {
            kind: "your_view",
            text: format!("{about}: {view}"),
        })
        .collect();
    for (what, stayed) in super::doing::recalled(db, Some(talk), 0, PER_KIND).await {
        material.push(Material {
            kind: "you_did",
            text: format!("{what}: {stayed}"),
        });
    }
    for heard in super::heard::touched(db, talk, PER_KIND).await {
        material.push(Material {
            kind: "you_heard",
            text: heard,
        });
    }
    for told in super::doing::would_tell(db, None).await {
        if !material.iter().any(|item| told.contains(&item.text)) {
            material.push(Material {
                kind: "you_would_tell",
                text: told,
            });
        }
    }
    material
}

fn input(conversation: &[String], last_spoke: Option<&str>, material: &[Material]) -> String {
    json!({
        "conversation": conversation,
        "youLastSpokeHere": last_spoke,
        "yourOwnTime": super::doing::current().map(|doing| super::doing::now_line(&doing, Utc::now())),
        "whatYouHave": material_view(material),
    })
    .to_string()
}

/// Whether she speaks up, and why, as the turn that speaks is told it; the
/// judgment is billed to `owner`. `conversation` is the group's recent lines,
/// hers as `you：…`.
pub async fn decide(
    db: &DatabaseConnection,
    owner: i32,
    conversation: &[String],
    last_spoke: Option<&str>,
) -> Option<String> {
    let material = material(db, &conversation.join("\n")).await;
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let raw = call::Ask::new(Voice::Judge, owner, "group_chime")
        .within(CALL_TIMEOUT)
        .json_raw(
            &system(&soul),
            &input(conversation, last_spoke, &material),
            SCHEMA_NAME,
            &schema(),
        )
        .await
        .ok()?;
    let decision = parse(&raw, material.len()).flatten()?;
    Some(reason(&decision, &material))
}

/// The judgment as production asks it, for the semantic suite.
#[cfg(test)]
pub(crate) fn probe(
    soul: &str,
    conversation: &[String],
    material: &[Material],
) -> (String, serde_json::Value, String) {
    (system(soul), schema(), input(conversation, None, material))
}

#[cfg(test)]
pub(crate) fn verdict(
    raw: &str,
    material: usize,
) -> Option<Option<myriad_merope::joining::Decision>> {
    parse(raw, material)
}
