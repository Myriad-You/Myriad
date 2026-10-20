//! The host filter for what a client sees on its owner's screen
//! (`myriad_perception_filter`): every client decides where a window goes by
//! the same rules.
//!
//! Every input may name the `shape` it speaks (default 1) and every output
//! names the shape it answers in. The text these calls read can hold keys,
//! so a refusal never quotes the input: it names only a missing or unknown
//! field, or says the input does not fit.

use myriad_perception_filter::{
    self as filter, CLASSIFY_ASKS, Decision, PRESENT_ONLY_LEAVES_KIND, RULES, RelayCheck, SHAPE,
    Sources, Window, decide, identifiers, secrets, window,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};

use crate::Failure;

/// The input without its `shape`, refused when the shape is not this core's.
fn shaped<T: DeserializeOwned>(raw: &[u8]) -> Result<T, Failure> {
    let unfit = || Failure::BadInput(format!("input does not fit filter shape {SHAPE}"));
    let mut value: Value = serde_json::from_slice(raw).map_err(|_| unfit())?;
    let object = value.as_object_mut().ok_or_else(unfit)?;
    if let Some(shape) = object.remove("shape")
        && shape.as_u64() != Some(u64::from(SHAPE))
    {
        return Err(Failure::BadInput(format!(
            "this core speaks filter shape {SHAPE} only"
        )));
    }
    serde_json::from_value(value).map_err(|error| {
        let message = error.to_string();
        // Field names are the client's own words; values may be secrets.
        if message.starts_with("unknown field") || message.starts_with("missing field") {
            Failure::BadInput(message.split(", expected").next().unwrap_or("").to_string())
        } else {
            unfit()
        }
    })
}

fn answer(mut value: Value) -> Result<Value, Failure> {
    value["shape"] = json!(SHAPE);
    Ok(value)
}

fn kinds<K: Serialize>(wiped: &[K]) -> Value {
    json!(
        wiped
            .iter()
            .map(|kind| json!({ "kind": kind }))
            .collect::<Vec<_>>()
    )
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Text {
    text: String,
}

/// `{bundleId?, title}` → `{seen}` (the scrubbed one-line title) or
/// `{withheld: "password_app" | "key_file"}`: then nothing in the window is read.
pub(super) fn window_title(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        bundle_id: Option<String>,
        title: String,
    }
    let req: In = shaped(raw)?;
    answer(
        match window(req.bundle_id.as_deref(), &req.title, Sources::built_in()) {
            Window::Seen(title) => json!({ "seen": title }),
            Window::Withheld(why) => json!({ "withheld": why }),
        },
    )
}

/// `{text}` → `{text, wiped: [{kind}]}`, kinds `private_key`,
/// `vendor_prefix`, `bearer`, `assignment`, `high_entropy`.
pub(super) fn scan_secrets(raw: &[u8]) -> Result<Value, Failure> {
    let req: Text = shaped(raw)?;
    let scan = secrets::scan(&req.text);
    answer(json!({ "text": scan.text, "wiped": kinds(&scan.wiped) }))
}

/// `{text}` → `{text, wiped: [{kind}]}`, kinds `card`, `sg_nric`,
/// `jp_my_number`. Run after `filter.scan_secrets`.
pub(super) fn scan_identifiers(raw: &[u8]) -> Result<Value, Failure> {
    let req: Text = shaped(raw)?;
    let scan = identifiers::scan(&req.text);
    answer(json!({ "text": scan.text, "wiped": kinds(&scan.wiped) }))
}

/// `{bundleId?, urlHost?, market?}` → `{kind}`: `finance`, `health`,
/// `dating`, `job`, `legal`, `messaging` or null. `table` (the source table's
/// shape) replaces the built-in table, for regression corpora only.
pub(super) fn source(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        bundle_id: Option<String>,
        url_host: Option<String>,
        market: Option<String>,
        table: Option<Sources>,
    }
    let req: In = shaped(raw)?;
    let table = req.table.as_ref().unwrap_or_else(|| Sources::built_in());
    let kind = table.kind(
        req.bundle_id.as_deref(),
        req.url_host.as_deref(),
        req.market.as_deref(),
    );
    answer(json!({ "kind": kind }))
}

/// `{text}` (after both scans) → `{hits: [{kind, rule}]}`.
pub(super) fn text_hits(raw: &[u8]) -> Result<Value, Failure> {
    let req: Text = shaped(raw)?;
    answer(json!({ "hits": filter::text_hits(&req.text) }))
}

/// `{source, who, topic}` (the message the model relayed and its answer) →
/// `{ok: true}` or `{ok: false, reason: "empty" | "digits" | "copied"}`.
pub(super) fn check_relay(raw: &[u8]) -> Result<Value, Failure> {
    #[derive(Deserialize)]
    #[serde(rename_all = "camelCase", deny_unknown_fields)]
    struct In {
        source: String,
        who: String,
        topic: String,
    }
    let req: In = shaped(raw)?;
    answer(
        match filter::check_relay(&req.source, &req.who, &req.topic) {
            RelayCheck::Ok => json!({ "ok": true }),
            rejected => json!({ "ok": false, "reason": rejected }),
        },
    )
}

/// Every step's result → `{rules, disposition, kind?, reason, missing?,
/// leaves, leavesKind}`. A missing input or an unknown value is the closed
/// answer, not a refusal.
pub(super) fn decide_call(raw: &[u8]) -> Result<Value, Failure> {
    let req: Decision = shaped(raw)?;
    let mut out = serde_json::to_value(decide(&req)).unwrap_or_default();
    out["rules"] = json!(RULES);
    answer(out)
}

/// `{}` → what the client needs before any call: the apps whose windows are
/// never captured, which sensitivity calls to make, whether a `present_only`
/// marker names its kind, the title cap and the marks.
pub(super) fn lists(raw: &[u8]) -> Result<Value, Failure> {
    let _: super::Nothing = shaped(raw)?;
    answer(json!({
        "rules": RULES,
        "captureExcluded": Sources::built_in().capture_excluded,
        "classifyAsks": CLASSIFY_ASKS,
        "presentOnlyLeavesKind": PRESENT_ONLY_LEAVES_KIND,
        "titleLimit": filter::window::TITLE_LIMIT,
        "marks": { "secret": secrets::MARK, "identifier": identifiers::MARK },
    }))
}
