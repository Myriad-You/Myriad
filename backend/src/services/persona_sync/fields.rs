//! Her persona as fields, each with the revision of its last edit, and the
//! values edits replaced (`agent_persona_field_history`).
//!
//! Every write to her persona row through `upsert_persona_on` is diffed
//! field by field (`myriad_persona_sync::persona_diff`): a changed field gets
//! the write's revision in `agent_persona.field_revs`, and the value it held
//! is kept in the history, where the owner can look back and put one back
//! (which is one more edit). Her sticker avatar is a picture and counts by
//! its bytes, not its address; it keeps the row's revision and no history.

use std::collections::BTreeMap;

use chrono::{DateTime, FixedOffset, Utc};
use myriad_persona_sync::{FieldRev, Rev, hlc, persona::VISUAL_PREFIX, persona_diff};
use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement};
use serde::Serialize;
use serde_json::Value;

type PersonaRow = crate::models::entities::agent_persona::Model;

/// Keys of her visual profile that are not persona fields: the wardrobe
/// crosses as outfits, and which outfit she wears, bust or full body, is
/// each side's own (Q12, R6).
pub const VISUAL_NOT_FIELDS: [&str; 3] = ["wardrobe", "activeOutfitId", "activeFullBodyOutfitId"];

/// The longest history a field keeps.
pub const HISTORY_KEPT: usize = 50;

/// Her persona's fields and their values, as they cross (her sticker avatar
/// aside, which is read as a picture).
pub fn values(row: &PersonaRow) -> BTreeMap<String, Value> {
    let mut fields = BTreeMap::new();
    fields.insert("name".to_string(), Value::String(row.name.clone()));
    fields.insert(
        "personality".to_string(),
        Value::String(row.personality.clone()),
    );
    if let Some(Value::Object(persona)) = &row.persona_json {
        for (key, value) in persona {
            fields.insert(key.clone(), value.clone());
        }
    }
    // Her zone: empty is the site's own, which crosses as unset.
    let zone = row.zone.trim();
    fields.insert(
        "zone".to_string(),
        if zone.is_empty() {
            Value::Null
        } else {
            Value::String(zone.to_string())
        },
    );
    if let Some(Value::Object(visual)) = &row.visual_profile {
        for (key, value) in visual {
            if !VISUAL_NOT_FIELDS.contains(&key.as_str()) {
                fields.insert(format!("{VISUAL_PREFIX}{key}"), value.clone());
            }
        }
    }
    fields
}

/// The revision of each field's last edit, as kept on the row. An entry that
/// does not read is as if the field had none.
pub fn revs(row: &PersonaRow) -> BTreeMap<String, FieldRev> {
    row.field_revs
        .as_object()
        .map(|revs| {
            revs.iter()
                .filter_map(|(field, rev)| {
                    Some((field.clone(), serde_json::from_value(rev.clone()).ok()?))
                })
                .collect()
        })
        .unwrap_or_default()
}

/// The revision a row written before field revisions had: its last write.
pub fn legacy_rev(at: DateTime<FixedOffset>) -> Rev {
    // Times before 1970 are clamped; never fails for a node of "site".
    hlc::legacy(at.timestamp_micros(), None).expect("a site revision")
}

/// Records one write of her persona by the site: `before` is the row as it
/// was (none for a new persona), `after` as written. Changed fields get a new
/// revision, after every revision the row already holds; the values they
/// replaced go to the history (at most [`HISTORY_KEPT`] per field). Returns
/// `after` with its revisions. Call it in the transaction of the write.
pub async fn record_write_on<C: ConnectionTrait>(
    db: &C,
    before: Option<&PersonaRow>,
    mut after: PersonaRow,
    reason: &str,
) -> Result<PersonaRow, DbErr> {
    let old = before.map(values).unwrap_or_default();
    let new = values(&after);
    let held = revs(&after);
    let legacy = legacy_rev(before.unwrap_or(&after).updated_at);
    let last = held
        .values()
        .map(|field| field.rev.clone())
        .chain([legacy.clone()])
        .max();
    let rev = hlc::next(last.as_ref(), Utc::now().timestamp_micros(), hlc::SITE)
        .map_err(|error| DbErr::Custom(format!("persona revision: {error}")))?;
    let diff = persona_diff(&old, &new, &held, &legacy, &rev, hlc::SITE, reason)
        .map_err(|error| DbErr::Custom(format!("persona diff: {error}")))?;
    if diff.changed.is_empty() {
        return Ok(after);
    }
    let revs = serde_json::to_value(&diff.revs)
        .map_err(|error| DbErr::Custom(format!("persona revisions: {error}")))?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_persona SET field_revs = $1 WHERE id = $2",
        vec![
            revs.clone().into(),
            crate::services::agent::merope::api::store::PERSONA_ROW_ID.into(),
        ],
    ))
    .await?;
    for replaced in &diff.history {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_persona_field_history \
             (field, value, rev, origin, replaced_at, replaced_by_rev, reason) \
             VALUES ($1, $2, $3, $4, NOW(), $5, $6)",
            vec![
                replaced.field.clone().into(),
                replaced.value.clone().into(),
                replaced.rev.to_string().into(),
                replaced.origin.clone().into(),
                replaced.replaced_by_rev.to_string().into(),
                replaced.reason.clone().into(),
            ],
        ))
        .await?;
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM agent_persona_field_history WHERE field = $1 AND id NOT IN \
             (SELECT id FROM agent_persona_field_history WHERE field = $1 \
              ORDER BY replaced_at DESC, id DESC LIMIT $2)",
            vec![replaced.field.clone().into(), (HISTORY_KEPT as i64).into()],
        ))
        .await?;
    }
    after.field_revs = revs;
    Ok(after)
}

/// One value a field held, as the owner looks back on it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoryEntry {
    pub id: i64,
    pub field: String,
    pub value: Value,
    pub rev: String,
    pub origin: String,
    pub replaced_at: DateTime<FixedOffset>,
    pub reason: String,
}

/// The values `field` held, newest first, at most `limit`.
pub async fn history<C: ConnectionTrait>(
    db: &C,
    field: &str,
    limit: u64,
) -> Result<Vec<HistoryEntry>, DbErr> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT id, field, value, rev, origin, replaced_at, reason \
         FROM agent_persona_field_history WHERE field = $1 \
         ORDER BY replaced_at DESC, id DESC LIMIT $2",
        vec![field.into(), (limit as i64).into()],
    ))
    .await?
    .iter()
    .map(|row| {
        Ok(HistoryEntry {
            id: row.try_get("", "id")?,
            field: row.try_get("", "field")?,
            value: row
                .try_get::<Option<Value>>("", "value")?
                .unwrap_or(Value::Null),
            rev: row.try_get("", "rev")?,
            origin: row.try_get("", "origin")?,
            replaced_at: row.try_get("", "replaced_at")?,
            reason: row.try_get("", "reason")?,
        })
    })
    .collect()
}
