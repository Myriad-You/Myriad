//! Her persona as it crosses: one entity, field by field, each field with
//! its own revision so the latest edit of each wins and the value it
//! replaced is kept.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    SyncError,
    entity::Picture,
    hlc::{Rev, check_node},
};

/// How a field merges.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FieldClass {
    /// One value; the later revision wins and the other is kept in history.
    Single,
    /// A list of strings; both sides' items are kept.
    List,
    /// A key only some side gives a meaning to: the site's `displayName`
    /// and `draftSource`, and whatever else a side keeps that the others do
    /// not read. Every side keeps it and sends it back exactly as it came;
    /// it merges like a single value. A comparison leaves it out: a side
    /// that does not use a key has no view of it to differ on.
    Opaque,
}

/// Fields with a known meaning. Her `visual.*` keys are single values of
/// any shape; every other field is [`FieldClass::Opaque`].
pub const KNOWN_FIELDS: &[(&str, FieldClass)] = &[
    ("drives", FieldClass::List),
    ("language", FieldClass::Single),
    ("likes", FieldClass::List),
    ("name", FieldClass::Single),
    ("personality", FieldClass::Single),
    ("socialStyle", FieldClass::Single),
    ("speechStyle", FieldClass::Single),
    ("stickerAvatar", FieldClass::Single),
    ("summary", FieldClass::Single),
    ("temperament", FieldClass::List),
    ("zone", FieldClass::Single),
];

/// Fields that hold text, or nothing.
const TEXT_FIELDS: &[&str] = &[
    "language",
    "name",
    "personality",
    "socialStyle",
    "speechStyle",
    "summary",
    "zone",
];

/// Her visual profile's keys cross under this prefix (`visual.clothingStyle`).
/// The wardrobe crosses as outfits, and which outfit she wears is each
/// side's own.
pub const VISUAL_PREFIX: &str = "visual.";
/// Longest field name.
pub const MAX_FIELD_CHARS: usize = 64;

pub fn field_class(field: &str) -> FieldClass {
    KNOWN_FIELDS
        .iter()
        .find(|(name, _)| *name == field)
        .map(|(_, class)| *class)
        .unwrap_or(if field.starts_with(VISUAL_PREFIX) {
            FieldClass::Single
        } else {
            FieldClass::Opaque
        })
}

/// One field: its value, and the revision and side of its last edit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonaField {
    /// `null` is a field cleared.
    pub value: Value,
    pub rev: Rev,
    pub origin: String,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PersonaBody {
    pub fields: BTreeMap<String, PersonaField>,
}

impl PersonaBody {
    /// Her name, trimmed; none when unset or empty.
    pub fn name(&self) -> Option<&str> {
        self.fields
            .get("name")
            .and_then(|field| field.value.as_str())
            .map(str::trim)
            .filter(|name| !name.is_empty())
    }
}

impl crate::entity::CheckBody for PersonaBody {
    fn check_body(&mut self) -> Result<(), SyncError> {
        for (name, field) in &mut self.fields {
            if name.is_empty()
                || name.chars().count() > MAX_FIELD_CHARS
                || name.chars().any(char::is_control)
            {
                return Err(SyncError::new(format!("field name {name:?}")));
            }
            check_node(&field.origin)?;
            // Her zone is an IANA name, or unset (each side's own): blank is
            // unset, so two sides that leave it blank agree.
            if name == "zone"
                && let Value::String(zone) = &field.value
            {
                let zone = zone.trim();
                field.value = if zone.is_empty() {
                    Value::Null
                } else {
                    Value::String(zone.to_string())
                };
            }
            let fine = match &field.value {
                Value::Null => true,
                value if field_class(name) == FieldClass::List => value
                    .as_array()
                    .is_some_and(|items| items.iter().all(Value::is_string)),
                value if name == "stickerAvatar" => {
                    let mut picture: Picture = serde_json::from_value(value.clone())
                        .map_err(|error| SyncError::new(format!("stickerAvatar: {error}")))?;
                    picture.check()?;
                    field.value = serde_json::to_value(picture).map_err(SyncError::from_json)?;
                    true
                }
                Value::String(_) => true,
                _ => !TEXT_FIELDS.contains(&name.as_str()),
            };
            if !fine {
                return Err(SyncError::new(format!(
                    "field {name:?} has the wrong shape"
                )));
            }
        }
        Ok(())
    }
}

/// The revision and side of one field's last edit, as a side keeps it beside
/// her persona (the site's `agent_persona.field_revs`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FieldRev {
    pub rev: Rev,
    pub origin: String,
}

/// A value a field held until an edit replaced it: kept, so it can be put
/// back (which is one more edit).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReplacedValue {
    pub field: String,
    pub value: Value,
    /// The revision and side of the value replaced.
    pub rev: Rev,
    pub origin: String,
    /// The revision of the edit that replaced it.
    pub replaced_by_rev: Rev,
    /// `edit` (on this side), `lww` (a later edit from another side won),
    /// `subject` (the other side was chosen as her), `trimmed` (a list past
    /// its limit).
    pub reason: String,
}

/// What one write did to her persona, field by field.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaDiff {
    /// The fields whose value changed, by name.
    pub changed: Vec<String>,
    /// Every field's revision after the write: the changed ones at `rev`,
    /// the rest as they were.
    pub revs: BTreeMap<String, FieldRev>,
    /// The values the write replaced (not those that were unset).
    pub history: Vec<ReplacedValue>,
}

/// Compares her persona before and after one write by `origin` at `rev`:
/// which fields changed (a field gone is a field set to `null`), their new
/// revisions, and the values replaced, each with the revision it had. A field
/// with no revision yet (written before revisions were kept) had `legacy`,
/// from `site`.
pub fn persona_diff(
    before: &BTreeMap<String, Value>,
    after: &BTreeMap<String, Value>,
    revs: &BTreeMap<String, FieldRev>,
    legacy: &Rev,
    rev: &Rev,
    origin: &str,
    reason: &str,
) -> Result<PersonaDiff, SyncError> {
    check_node(origin)?;
    if reason.is_empty() || reason.chars().count() > 16 {
        return Err(SyncError::new("a reason is 1 to 16 characters"));
    }
    let mut out = PersonaDiff {
        changed: Vec::new(),
        revs: revs.clone(),
        history: Vec::new(),
    };
    let names: std::collections::BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    for name in names {
        let old = before.get(name).unwrap_or(&Value::Null);
        let new = after.get(name).unwrap_or(&Value::Null);
        if old == new {
            continue;
        }
        let had = revs.get(name).cloned().unwrap_or_else(|| FieldRev {
            rev: legacy.clone(),
            origin: crate::hlc::SITE.to_string(),
        });
        if !old.is_null() {
            out.history.push(ReplacedValue {
                field: name.clone(),
                value: old.clone(),
                rev: had.rev,
                origin: had.origin,
                replaced_by_rev: rev.clone(),
                reason: reason.to_string(),
            });
        }
        out.changed.push(name.clone());
        out.revs.insert(
            name.clone(),
            FieldRev {
                rev: rev.clone(),
                origin: origin.to_string(),
            },
        );
    }
    Ok(out)
}
