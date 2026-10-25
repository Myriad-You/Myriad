//! Where one thing she sees (a window, a message) goes, from what each step
//! of the pipeline found. Steps earlier in the pipeline win, and a host rule
//! that hits is never overruled by the model.
//!
//! Closed by default: a missing input, a value this core does not know, an
//! unavailable model, an unsupported language, an error or an unsure answer
//! never lets anything through and never falls back to a cloud model.
//!
//! A sensitive site or app and an account page leave nothing at all, not even
//! the "busy with something private" marker: that the owner was on such a
//! page is itself what must not leave. A page no rule places (`unknown`) is
//! not held for that alone: its text goes on to the rules and the model.

use serde::{Deserialize, Deserializer, Serialize};

use crate::sources::SourceKind;

/// From the strictest to the widest. The first three carry nothing off the
/// device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Disposition {
    /// Not read at all: no pixels, no title, no text.
    Exclude,
    /// She knows only the textless facts (which app, playing or not, how
    /// long idle) and the owner is told she cannot see.
    Blind,
    /// Seen, shapes her present moment on this device, never remembered or
    /// sent anywhere.
    PresentOnly,
    /// Only who is talking and a detail-free topic may leave the device.
    WhoWhat,
    /// The scrubbed text may leave (remembering is still a later, separate gate).
    Carry,
}

/// Which step decided.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Capture,
    Window,
    /// A password field is showing.
    PasswordField,
    Identifier,
    Source,
    /// The page: an account page, or a sensitive site.
    Page,
    Private,
    Text,
    ModelUnavailable,
    Language,
    Relay,
    Classify,
    /// An input the step needs was not given.
    Missing,
}

/// What a `present_only` window is about. Used on this device only, to
/// keep her from interrupting or bringing it up, unless the policy says
/// otherwise.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrivateKind {
    Identity,
    Finance,
    Health,
    PrivateBrowsing,
    /// Account, security, password or payment settings.
    Account,
    Unknown,
}

/// What may leave the device.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Leaves {
    Nothing,
    /// One "the owner is busy with something private" mark.
    PrivateMarker,
    /// The sender's display name, the topic and the category.
    WhoWhat,
    /// The text after the secret and identifier scans.
    ScrubbedText,
}

/// One on-device model call for sensitivity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Ask {
    General,
    Tagging,
}

/// The two choices still open to the owner, each a one-line change here.
#[derive(Clone, Copy, Debug)]
pub struct Policy {
    /// Every listed call must say `not_sensitive` for the owner's own
    /// content to be carried. Asking only `General` carries about twice as
    /// much (mostly terminals and config files) and missed one more page in
    /// the synthetic corpus.
    pub classify_asks: &'static [Ask],
    /// Whether the off-device marker of a `present_only` window names its
    /// kind. "Looking at a health page" is itself health information.
    pub present_only_leaves_kind: bool,
}

/// Until the owner decides: ask twice, carry no kind. The more closed of each.
pub const CLASSIFY_ASKS: &[Ask] = &[Ask::General, Ask::Tagging];
pub const PRESENT_ONLY_LEAVES_KIND: bool = false;

impl Policy {
    pub const CURRENT: Self = Self {
        classify_asks: CLASSIFY_ASKS,
        present_only_leaves_kind: PRESENT_ONLY_LEAVES_KIND,
    };
}

/// What each step found. Enumerations are strings: a value this core does
/// not know (from a newer client) is read as the more closed one.
#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Decision {
    /// `excluded` | `seen`.
    pub capture: Option<String>,
    /// `seen` | `withheld`.
    pub window: Option<String>,
    /// A password field is showing in the window. Not given: none known.
    pub password_field: Option<bool>,
    /// Identifier kinds the scan wiped (`card`…); empty when none.
    pub identifiers: Option<Vec<String>>,
    /// The source kind, `null` when the table has none.
    #[serde(default, deserialize_with = "given")]
    pub source: Option<Option<String>>,
    /// A web page's kind from `filter.page`: `ordinary` | `messaging` |
    /// `sign_in` | `sensitive` | `account` | `unknown`. Not given: not a web
    /// page.
    pub page: Option<String>,
    /// `true` | `false` | `"unknown"`.
    pub private: Option<Private>,
    /// Text rule kinds that hit (`finance`, `health`); empty when none.
    pub text_hits: Option<Vec<String>>,
    /// `available` | `unavailable`.
    pub model: Option<String>,
    /// `supported` | `unsupported`.
    pub language: Option<String>,
    /// The relay of other people's words; `null` when there is none.
    #[serde(default, deserialize_with = "given")]
    pub relay: Option<Option<Relay>>,
    /// The sensitivity calls on the owner's own content; `null` when none.
    #[serde(default, deserialize_with = "given")]
    pub classify: Option<Option<Classify>>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(untagged)]
pub enum Private {
    Known(bool),
    Other(String),
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Relay {
    /// `ok` (the model answered and `filter.check_relay` passed) |
    /// `rejected` | `error`.
    pub result: String,
    /// The model's category (`plans`, `work`, `money`, `health`…).
    pub category: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Classify {
    /// `not_sensitive` | `sensitive` | `unsure` | `error` (a refusal, an
    /// unparsable answer or a timeout is an error).
    pub general: Option<String>,
    pub tagging: Option<String>,
    /// The kind a call gave (`health`, `finance`, `private_browsing`…).
    pub kind: Option<String>,
}

/// A field that is present, even as `null`, is `Some`.
fn given<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Decided {
    pub disposition: Disposition,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<PrivateKind>,
    pub reason: Reason,
    /// The input that was missing, when `reason` is `missing`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub missing: Option<&'static str>,
    pub leaves: Leaves,
    /// The only category that may leave with it.
    pub leaves_kind: Option<String>,
}

/// Relay categories a `who_what` may carry; anything else leaves as `other`.
const RELAY_CATEGORIES: &[&str] = &[
    "plans",
    "work",
    "family",
    "romance",
    "help",
    "share",
    "logistics",
    "other",
];

pub fn decide(decision: &Decision) -> Decided {
    decide_with(decision, &Policy::CURRENT)
}

pub fn decide_with(d: &Decision, policy: &Policy) -> Decided {
    // 1. Not read at all.
    if d.capture
        .as_deref()
        .is_some_and(|capture| capture != "seen")
    {
        return closed(Disposition::Exclude, Reason::Capture);
    }
    if d.window.as_deref().is_some_and(|window| window != "seen") {
        return closed(Disposition::Exclude, Reason::Window);
    }
    if d.password_field == Some(true) || d.page.as_deref() == Some("sign_in") {
        return closed(Disposition::Exclude, Reason::PasswordField);
    }
    if d.capture.is_none() {
        return missing("capture");
    }
    if d.window.is_none() {
        return missing("window");
    }

    // 2. Host rules: the owner's own private matters.
    let Some(identifiers) = &d.identifiers else {
        return missing("identifiers");
    };
    let Some(source) = &d.source else {
        return missing("source");
    };
    let Some(private) = &d.private else {
        return missing("private");
    };
    let Some(text_hits) = &d.text_hits else {
        return missing("textHits");
    };
    let source = source.as_deref().map(source_kind);
    // Whatever puts it in `present_only`, a sensitive source or page means
    // not even the marker leaves.
    let sensitive_source =
        matches!(source, Some(None)) || matches!(source, Some(Some(kind)) if kind.sensitive());
    let page = d.page.as_deref().map(|page| match page {
        "ordinary" | "messaging" | "unknown" => None,
        "account" => Some(PrivateKind::Account),
        _ => Some(PrivateKind::Unknown),
    });
    let says_nothing = sensitive_source || matches!(page, Some(Some(_)));
    let held = |kind, reason| {
        let mut decided = present(kind, reason, policy);
        if says_nothing {
            decided.leaves = Leaves::Nothing;
            decided.leaves_kind = None;
        }
        decided
    };
    if !identifiers.is_empty() {
        return held(PrivateKind::Identity, Reason::Identifier);
    }
    if let Some(Some(kind)) = source.filter(|kind| *kind != Some(SourceKind::Messaging)) {
        let private = match kind {
            SourceKind::Finance => PrivateKind::Finance,
            SourceKind::Health => PrivateKind::Health,
            SourceKind::Account => PrivateKind::Account,
            SourceKind::GovernmentId => PrivateKind::Identity,
            _ => PrivateKind::PrivateBrowsing,
        };
        return held(private, Reason::Source);
    }
    if source == Some(None) {
        // A kind this core does not know.
        return held(PrivateKind::Unknown, Reason::Source);
    }
    if let Some(Some(kind)) = page {
        return held(kind, Reason::Page);
    }
    // `unknown` is a known gap (some browsers cannot tell), not a pass:
    // the rules after this still apply.
    let private = match private {
        Private::Known(private) => *private,
        Private::Other(other) => other != "unknown",
    };
    if private {
        return held(PrivateKind::PrivateBrowsing, Reason::Private);
    }
    if let Some(first) = text_hits.first() {
        let kind = match first.as_str() {
            "finance" => PrivateKind::Finance,
            "health" => PrivateKind::Health,
            _ => PrivateKind::Unknown,
        };
        return held(kind, Reason::Text);
    }

    // 3. The on-device model and the language it reads.
    match d.model.as_deref() {
        None => return missing("model"),
        Some("available") => {}
        Some(_) => return closed(Disposition::Blind, Reason::ModelUnavailable),
    }
    match d.language.as_deref() {
        None => return missing("language"),
        Some("supported") => {}
        Some(_) => return closed(Disposition::Blind, Reason::Language),
    }

    // 4. Other people's words.
    if source == Some(Some(SourceKind::Messaging)) {
        let Some(Some(relay)) = &d.relay else {
            return closed(Disposition::Blind, Reason::Relay);
        };
        if relay.result != "ok" {
            return closed(Disposition::Blind, Reason::Relay);
        }
        let category = relay.category.as_deref().unwrap_or("other");
        return match category {
            "health" => present(PrivateKind::Health, Reason::Relay, policy),
            "money" => present(PrivateKind::Finance, Reason::Relay, policy),
            _ => {
                let category = if RELAY_CATEGORIES.contains(&category) {
                    category
                } else {
                    "other"
                };
                Decided {
                    disposition: Disposition::WhoWhat,
                    kind: None,
                    reason: Reason::Relay,
                    missing: None,
                    leaves: Leaves::WhoWhat,
                    leaves_kind: Some(category.to_string()),
                }
            }
        };
    }

    // 5. The owner's own content: carried only when clearly not sensitive.
    let Some(Some(classify)) = &d.classify else {
        return missing("classify");
    };
    let clear = policy.classify_asks.iter().all(|ask| {
        let label = match ask {
            Ask::General => &classify.general,
            Ask::Tagging => &classify.tagging,
        };
        label.as_deref() == Some("not_sensitive")
    });
    if clear {
        return Decided {
            disposition: Disposition::Carry,
            kind: None,
            reason: Reason::Classify,
            missing: None,
            leaves: Leaves::ScrubbedText,
            leaves_kind: None,
        };
    }
    let kind = match classify.kind.as_deref() {
        Some("health") => PrivateKind::Health,
        Some("finance") => PrivateKind::Finance,
        Some("private_browsing") => PrivateKind::PrivateBrowsing,
        _ => PrivateKind::Unknown,
    };
    present(kind, Reason::Classify, policy)
}

/// A source kind by name; `None` when this core does not know it.
fn source_kind(name: &str) -> Option<SourceKind> {
    serde_json::from_value(serde_json::Value::String(name.to_string())).ok()
}

fn closed(disposition: Disposition, reason: Reason) -> Decided {
    Decided {
        disposition,
        kind: None,
        reason,
        missing: None,
        leaves: Leaves::Nothing,
        leaves_kind: None,
    }
}

fn missing(field: &'static str) -> Decided {
    Decided {
        missing: Some(field),
        ..closed(Disposition::Blind, Reason::Missing)
    }
}

fn present(kind: PrivateKind, reason: Reason, policy: &Policy) -> Decided {
    Decided {
        disposition: Disposition::PresentOnly,
        kind: Some(kind),
        reason,
        missing: None,
        leaves: Leaves::PrivateMarker,
        leaves_kind: policy.present_only_leaves_kind.then(|| {
            serde_json::to_value(kind)
                .ok()
                .and_then(|kind| kind.as_str().map(str::to_string))
                .unwrap_or_default()
        }),
    }
}
