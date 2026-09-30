//! Bringing something of hers up in a group first (see
//! `myriad_merope::sharing`): the judgment, as her, of whether one of the
//! groups she is in is where she would say it. Where and how it is said are
//! `channel_group`'s.

use std::time::Duration;

use serde_json::{Value, json};

use super::call::{self, Voice};
use myriad_merope::sharing::{SCHEMA_NAME, parse, schema, system};

const CALL_TIMEOUT: Duration = Duration::from_secs(45);

/// A group as she weighs it: its id, its latest lines, how long it has been
/// quiet, what she and it share, and her speaking up there unasked lately.
pub struct Offered {
    pub id: String,
    pub lines: Vec<String>,
    pub quiet_for: String,
    pub bits: Vec<String>,
    pub spoke_up_lately: Vec<Value>,
}

/// The group she would say `what` in, and why; billed to `owner`.
pub async fn choose(owner: i32, what: &str, groups: &[Offered]) -> Option<(String, String)> {
    if groups.is_empty() || !super::is_enabled().await {
        return None;
    }
    let soul: String = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let input = json!({
        "what": what,
        "groups": groups.iter().map(|group| json!({
            "id": group.id,
            "latestLines": group.lines,
            "quietFor": group.quiet_for,
            "sharedWithYou": group.bits,
            "spokeUpLately": group.spoke_up_lately,
        })).collect::<Vec<_>>(),
    })
    .to_string();
    let raw = call::Ask::new(Voice::Judge, owner, "share_first")
        .within(CALL_TIMEOUT)
        .json_raw(&system(&soul), &input, SCHEMA_NAME, &schema())
        .await
        .ok()?;
    let offered: Vec<String> = groups.iter().map(|group| group.id.clone()).collect();
    parse(&raw, &offered)
}
