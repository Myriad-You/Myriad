//! What the site has of her, as entities, a page at a time.
//!
//! A pull from the start is a whole snapshot: every entity in `(kind, id)`
//! order, `kind` in [`Kind::ALL`] order and `id` in byte order (SQL compares
//! `COLLATE "C"` to agree with Rust). A pull for the first comparison lists
//! only [`Kind::COMPARED`]; a move or an export lists every kind
//! ([`whole`]). The cursor is the last `(kind, id)`
//! sent; the next page starts after it, so an interrupted pull resumes and
//! a page fetched twice is the same page. Read only: nothing here writes.
//!
//! Rows that predate revisions get one computed from when they last changed
//! (`hlc::legacy`), all from the site.

use std::collections::BTreeMap;

use base64::Engine as _;
use chrono::{DateTime, FixedOffset};
use myriad_persona_sync::{
    AssetRef, EntityVersion, Kind, OWNER, Rev, State,
    entity::{
        Affect, AffectAnchorBody, AffectEventBody, Baseline, Concept, ConversationBody,
        ConversationLine, DiaryBody, Embedding, EmbeddingModel, FieldHistoryBody, IntentionBody,
        LinePicture, MemoryAudience, MemoryBody, MemoryVenue, OutfitBody, OwnerStateBody, PACE_ID,
        PERSONA_ID, PRESENT_ID, PaceBody, PresentBody, PresentDoing, PresentLazing,
        ProactiveLineBody, SOUP_GAME_ID, StickerBody, WearingBody, Worn,
    },
    hlc,
    persona::{PersonaBody, PersonaField},
};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, EntityTrait, QueryResult,
    Statement,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::{SYNCED_MEMORY, assets};
use crate::services::agent::merope::api::sync as her;

/// Where the next page starts: after this entity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    #[serde(rename = "k")]
    pub kind: Kind,
    #[serde(rename = "i")]
    pub id: String,
}

/// Opaque to the client: base64url of `{"k","i","v"}`.
#[derive(Serialize, Deserialize)]
struct Wire {
    v: u8,
    #[serde(flatten)]
    cursor: Cursor,
}

impl Cursor {
    pub fn encode(&self) -> String {
        let wire = Wire {
            v: 1,
            cursor: self.clone(),
        };
        base64::engine::general_purpose::URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&wire).expect("a cursor serializes"))
    }

    pub fn decode(raw: &str) -> Option<Self> {
        let bytes = base64::engine::general_purpose::URL_SAFE_NO_PAD
            .decode(raw.trim())
            .ok()?;
        let wire: Wire = serde_json::from_slice(&bytes).ok()?;
        (wire.v == 1).then_some(wire.cursor)
    }
}

/// One page of a pull.
#[derive(Debug, Default)]
pub struct Page {
    pub entities: Vec<EntityVersion>,
    pub embeddings: Vec<Embedding>,
    /// Every asset the page's entities refer to, once.
    pub assets: Vec<AssetRef>,
    /// The cursor after this page; the request's when the page is empty.
    pub next: Option<Cursor>,
    /// There may be more after `next`.
    pub more: bool,
}

#[derive(Debug)]
pub struct Limits {
    pub entities: usize,
    pub bytes: usize,
}

fn site_rev(at: DateTime<FixedOffset>) -> Rev {
    // Times before 1970 are clamped; never fails for a node of "site".
    hlc::legacy(at.timestamp_micros(), None).expect("a site revision")
}

fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values)
}

fn entity<B: Serialize>(
    kind: Kind,
    id: &str,
    rev: Rev,
    state: State,
    body: &B,
) -> Option<EntityVersion> {
    entity_from(kind, id, rev, hlc::SITE, state, body)
}

/// [`entity`], written by `origin`.
fn entity_from<B: Serialize>(
    kind: Kind,
    id: &str,
    rev: Rev,
    origin: &str,
    state: State,
    body: &B,
) -> Option<EntityVersion> {
    match EntityVersion::new(kind, id, rev, origin, state, body) {
        Ok(entity) => Some(entity),
        Err(error) => {
            // A row her own code cannot read is left out, not sent broken.
            tracing::warn!(%error, %kind, id, "[sync] a row does not fit the format; left out");
            None
        }
    }
}

/// One page of the snapshot for the owner `owner` of `kinds` (in
/// [`Kind::ALL`] order), after `after`.
pub async fn page(
    db: &DatabaseConnection,
    owner: i32,
    kinds: &[Kind],
    after: Option<&Cursor>,
    limits: &Limits,
    embedding_model: Option<&EmbeddingModel>,
) -> Result<Page, DbErr> {
    let pictures = assets::Pictures::new(db, owner);
    let mut page = Page {
        next: after.cloned(),
        ..Page::default()
    };
    let mut bytes = 0usize;
    'kinds: for kind in kinds.iter().copied() {
        let after_id = match after {
            Some(cursor) if kind < cursor.kind => continue,
            Some(cursor) if kind == cursor.kind => Some(cursor.id.as_str()),
            _ => None,
        };
        let room = limits.entities - page.entities.len();
        let found = of_kind(db, &pictures, owner, kind, after_id, room + 1).await?;
        for entity in found {
            let size = entity.canonical_bytes().len();
            let full = page.entities.len() >= limits.entities
                || (!page.entities.is_empty() && bytes + size > limits.bytes);
            if full {
                page.more = true;
                break 'kinds;
            }
            bytes += size;
            page.next = Some(Cursor {
                kind: entity.kind,
                id: entity.id.clone(),
            });
            page.entities.push(entity);
        }
    }
    page.assets = assets::referenced(&page.entities);
    if let Some(model) = embedding_model {
        page.embeddings = embeddings(db, &page.entities, model).await?;
    }
    Ok(page)
}

/// Up to `limit` entities of `kind` after `after`, in id order.
async fn of_kind<C: ConnectionTrait>(
    db: &C,
    pictures: &assets::Pictures,
    owner: i32,
    kind: Kind,
    after: Option<&str>,
    limit: usize,
) -> Result<Vec<EntityVersion>, DbErr> {
    let mut found = match kind {
        Kind::AffectAnchor => anchor(db, owner).await?.into_iter().collect(),
        Kind::AffectEvent => return affect_events(db, owner, after, limit).await,
        Kind::Diary => return diary(db, owner, after, limit).await,
        Kind::Memory => return memories(db, owner, after, limit).await,
        Kind::Outfit => outfits(db, pictures).await?,
        Kind::Persona => persona(db, pictures).await?.into_iter().collect(),
        Kind::SerialFollowing | Kind::SerialPast | Kind::SerialShelf | Kind::SerialWork => {
            serials(db, kind).await?
        }
        Kind::SoupGame => soup(db, owner).await?.into_iter().collect(),
        Kind::Sticker => return stickers(db, pictures, after, limit).await,
        Kind::VitalsDay => vitals(db).await?,
        Kind::Conversation => return conversations(db, pictures, owner, after, limit).await,
        Kind::FieldHistory => field_history(db).await?,
        Kind::Intention => return intentions(db, owner, after, limit).await,
        Kind::OwnerState => owner_state(db, owner).await?.into_iter().collect(),
        Kind::Pace => pace(db).await?.into_iter().collect(),
        Kind::Present => present(db).await?.into_iter().collect(),
        Kind::ProactiveLine => proactive_lines(db, owner).await?,
        Kind::Wearing => wearing(db).await?.into_iter().collect(),
    };
    found.sort_by(|a, b| a.id.cmp(&b.id));
    found.retain(|entity| after.is_none_or(|after| entity.id.as_str() > after));
    found.truncate(limit);
    Ok(found)
}

/// `AND id > $n` in byte order, when resuming.
fn after_clause(after: Option<&str>, column: &str, at: usize) -> String {
    match after {
        Some(_) => format!(" AND {column} COLLATE \"C\" > ${at}"),
        None => String::new(),
    }
}

fn concepts(raw: &Value) -> Vec<Concept> {
    raw.as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| match item {
                    Value::String(name) => Some(Concept {
                        name: name.clone(),
                        aliases: Vec::new(),
                    }),
                    Value::Object(_) => serde_json::from_value(item.clone()).ok(),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default()
}

async fn memories<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    after: Option<&str>,
    limit: usize,
) -> Result<Vec<EntityVersion>, DbErr> {
    let mut values: Vec<sea_orm::Value> = vec![owner.into(), (limit as i64).into()];
    if let Some(after) = after {
        values.push(after.into());
    }
    let rows = db
        .query_all_raw(statement(
            &format!(
                "SELECT id, kind, content, evidence, speaker, source, venue, concepts, importance, \
                 valid_from, invalid_at, invalid_reason, superseded_by, origin, created_at, \
                 updated_at \
                 FROM agent_memories WHERE {SYNCED_MEMORY}{} \
                 ORDER BY id COLLATE \"C\" LIMIT $2",
                after_clause(after, "id", 3)
            ),
            values,
        ))
        .await?;
    rows.iter()
        .filter_map(|row| memory(row).transpose())
        .collect()
}

fn memory(row: &QueryResult) -> Result<Option<EntityVersion>, DbErr> {
    let id: String = row.try_get("", "id")?;
    let venue: String = row.try_get("", "venue")?;
    let audience = match venue.as_str() {
        "own" => MemoryAudience {
            venue: MemoryVenue::Own,
            members: Vec::new(),
        },
        _ => MemoryAudience {
            venue: MemoryVenue::Private,
            members: vec![OWNER.to_string()],
        },
    };
    let invalid_at: Option<DateTime<FixedOffset>> = row.try_get("", "invalid_at")?;
    let valid_from: DateTime<FixedOffset> = row.try_get("", "valid_from")?;
    let created_at: DateTime<FixedOffset> = row.try_get("", "created_at")?;
    let updated_at: DateTime<FixedOffset> = row.try_get("", "updated_at")?;
    let body = MemoryBody {
        kind: row.try_get("", "kind")?,
        content: row.try_get("", "content")?,
        evidence: row.try_get("", "evidence")?,
        speaker: row.try_get("", "speaker")?,
        source: row.try_get("", "source")?,
        audience,
        concepts: concepts(&row.try_get::<Value>("", "concepts")?),
        importance: row.try_get("", "importance")?,
        valid_from: valid_from.timestamp_micros(),
        created_at: created_at.timestamp_micros(),
        invalid_at: invalid_at.map(|at| at.timestamp_micros()),
        invalid_reason: row.try_get("", "invalid_reason")?,
        superseded_by: row.try_get("", "superseded_by")?,
    };
    let state = if invalid_at.is_some() {
        State::Retired
    } else {
        State::Live
    };
    // Written here or by a device: the revision is that side's.
    let origin: String = row.try_get("", "origin")?;
    let rev = match hlc::legacy(updated_at.timestamp_micros(), Some(&origin)) {
        Ok(rev) => rev,
        Err(error) => {
            tracing::warn!(%error, id, "[sync] a memory's origin is not a side; left out");
            return Ok(None);
        }
    };
    Ok(entity_from(Kind::Memory, &id, rev, &origin, state, &body))
}

async fn diary<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    after: Option<&str>,
    limit: usize,
) -> Result<Vec<EntityVersion>, DbErr> {
    let mut values: Vec<sea_orm::Value> = vec![owner.into(), (limit as i64).into()];
    if let Some(after) = after {
        values.push(after.into());
    }
    let rows = db
        .query_all_raw(statement(
            &format!(
                "SELECT id, content, source, created_at FROM agent_diary WHERE user_id = $1{} \
                 ORDER BY id COLLATE \"C\" LIMIT $2",
                after_clause(after, "id", 3)
            ),
            values,
        ))
        .await?;
    let mut out = Vec::new();
    for row in &rows {
        let id: String = row.try_get("", "id")?;
        let created_at: DateTime<FixedOffset> = row.try_get("", "created_at")?;
        let body = DiaryBody {
            content: row.try_get("", "content")?,
            source: row.try_get("", "source")?,
            created_at: created_at.timestamp_micros(),
            subject: OWNER.to_string(),
        };
        out.extend(entity(
            Kind::Diary,
            &id,
            site_rev(created_at),
            State::Live,
            &body,
        ));
    }
    Ok(out)
}

async fn stickers<C: ConnectionTrait>(
    db: &C,
    pictures: &assets::Pictures,
    after: Option<&str>,
    limit: usize,
) -> Result<Vec<EntityVersion>, DbErr> {
    let mut values: Vec<sea_orm::Value> = vec![(limit as i64).into()];
    if let Some(after) = after {
        values.push(after.into());
    }
    // A group's sticker belongs to that group and stays here.
    let rows = db
        .query_all_raw(statement(
            &format!(
                "SELECT id, picture, meaning, asset_id, identity, sent, last_sent_at, created_at \
                 FROM merope_stickers \
                 WHERE venue IS NULL{} ORDER BY id COLLATE \"C\" LIMIT $1",
                after_clause(after, "id", 2)
            ),
            values,
        ))
        .await?;
    let mut out = Vec::new();
    for row in &rows {
        let id: String = row.try_get("", "id")?;
        let url: String = row.try_get("", "asset_id")?;
        let created_at: DateTime<FixedOffset> = row.try_get("", "created_at")?;
        // A picture the site cannot read still crosses, marked so: the
        // other side can say so instead of losing the sticker silently.
        let image = pictures.picture_at(&url).await;
        if image.is_unreadable() {
            tracing::warn!(sticker = %id, "[sync] a sticker's picture cannot be read");
        }
        let body = StickerBody {
            picture: row.try_get("", "picture")?,
            meaning: row.try_get("", "meaning")?,
            identity: row.try_get("", "identity")?,
            created_at: created_at.timestamp_micros(),
            image,
            sent: u32::try_from(row.try_get::<i32>("", "sent")?).ok(),
            last_sent_at: row
                .try_get::<Option<DateTime<FixedOffset>>>("", "last_sent_at")?
                .map(|at| at.timestamp_micros()),
        };
        out.extend(entity(
            Kind::Sticker,
            &id,
            site_rev(created_at),
            State::Live,
            &body,
        ));
    }
    Ok(out)
}

type PersonaRow = crate::models::entities::agent_persona::Model;

async fn persona_row<C: ConnectionTrait>(db: &C) -> Result<Option<PersonaRow>, DbErr> {
    crate::models::entities::agent_persona::Entity::find_by_id(
        crate::services::agent::merope::api::store::PERSONA_ROW_ID,
    )
    .one(db)
    .await
}

async fn persona<C: ConnectionTrait>(
    db: &C,
    pictures: &assets::Pictures,
) -> Result<Option<EntityVersion>, DbErr> {
    let Some(row) = persona_row(db).await? else {
        return Ok(None);
    };
    // Each field at its last edit; one edited before revisions were kept, at
    // the row's last write. A field cleared since still crosses, as null.
    let legacy = site_rev(row.updated_at);
    let revs = super::fields::revs(&row);
    let mut values = super::fields::values(&row);
    for cleared in revs.keys() {
        values.entry(cleared.clone()).or_insert(Value::Null);
    }
    let mut fields = BTreeMap::new();
    for (name, value) in values {
        let (rev, origin) = match revs.get(&name) {
            Some(edit) => (edit.rev.clone(), edit.origin.clone()),
            None => (legacy.clone(), hlc::SITE.to_string()),
        };
        fields.insert(name, PersonaField { value, rev, origin });
    }
    let field = |value: Value| PersonaField {
        value,
        rev: legacy.clone(),
        origin: hlc::SITE.to_string(),
    };
    if let Some(avatar) = row
        .avatar_asset_id
        .as_deref()
        .map(str::trim)
        .filter(|url| !url.is_empty())
    {
        let picture = pictures.picture_at(avatar).await;
        if picture.is_unreadable() {
            tracing::warn!("[sync] her sticker avatar cannot be read");
        }
        fields.insert(
            "stickerAvatar".to_string(),
            field(serde_json::to_value(picture).unwrap_or(Value::Null)),
        );
    }
    // The persona as a whole is as new as its newest field.
    let rev = fields
        .values()
        .map(|field| field.rev.clone())
        .chain([legacy])
        .max()
        .expect("at least the row's revision");
    Ok(entity(
        Kind::Persona,
        PERSONA_ID,
        rev,
        State::Live,
        &PersonaBody { fields },
    ))
}

fn text(item: &Value, key: &str) -> Option<String> {
    item.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

async fn outfits<C: ConnectionTrait>(
    db: &C,
    pictures: &assets::Pictures,
) -> Result<Vec<EntityVersion>, DbErr> {
    let Some(row) = persona_row(db).await? else {
        return Ok(Vec::new());
    };
    let rev = site_rev(row.updated_at);
    let items = row
        .visual_profile
        .as_ref()
        .and_then(|profile| profile.get("wardrobe"))
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut out = Vec::new();
    for item in &items {
        let Some(id) = text(item, "id") else {
            continue;
        };
        // The master portrait is the default set's when it has none of its own.
        let portrait_url = text(item, "portraitAssetId").or_else(|| {
            (id == myriad_merope::DEFAULT_WARDROBE_ID)
                .then(|| row.portrait_asset_id.clone())
                .flatten()
                .filter(|url| !url.trim().is_empty())
        });
        let portrait = match portrait_url {
            Some(url) => {
                let picture = pictures.picture_at(&url).await;
                if picture.is_unreadable() {
                    tracing::warn!(outfit = %id, "[sync] an outfit's portrait cannot be read");
                }
                Some(picture)
            }
            None => None,
        };
        let rig = match text(item, "rigAssetId") {
            Some(rig) => assets::rig_ref(&rig).await,
            None => None,
        };
        let body = OutfitBody {
            name: text(item, "name"),
            clothing_style: text(item, "clothingStyle").unwrap_or_default(),
            outfit: item
                .get("outfit")
                .filter(|outfit| outfit.is_object())
                .cloned()
                .unwrap_or_else(|| Value::Object(Default::default())),
            visual_identity: item.get("visualIdentity").filter(|v| !v.is_null()).cloned(),
            profile: text(item, "profile"),
            generation_fingerprint: text(item, "generationFingerprint"),
            reference_outfit_id: text(item, "referenceOutfitId"),
            portrait,
            rig,
        };
        out.extend(entity(Kind::Outfit, &id, rev.clone(), State::Live, &body));
    }
    Ok(out)
}

/// Her own records kept in the runtime registry, unexpired.
async fn registry<C: ConnectionTrait>(
    db: &C,
    namespace: &str,
) -> Result<Vec<(String, Value, DateTime<FixedOffset>)>, DbErr> {
    db.query_all_raw(statement(
        "SELECT record_id, payload, updated_at FROM runtime_registry \
         WHERE namespace = $1 AND expires_at > EXTRACT(EPOCH FROM $2::timestamptz)::BIGINT",
        vec![namespace.into(), crate::clock::now().into()],
    ))
    .await?
    .iter()
    .map(|row| {
        Ok((
            row.try_get("", "record_id")?,
            row.try_get("", "payload")?,
            row.try_get("", "updated_at")?,
        ))
    })
    .collect()
}

async fn serials<C: ConnectionTrait>(db: &C, kind: Kind) -> Result<Vec<EntityVersion>, DbErr> {
    let records = registry(db, her::SERIAL_NAMESPACE).await?;
    let mut out = Vec::new();
    let has_reading = records.iter().any(|(record, _, _)| record == "reading");
    for (record, payload, updated_at) in records {
        let rev = site_rev(updated_at);
        match (kind, record.as_str()) {
            (Kind::SerialFollowing, "reading") => {
                for following in payload.as_array().into_iter().flatten() {
                    if let Some(id) = following.get("id").and_then(Value::as_str) {
                        out.extend(entity(kind, id, rev.clone(), State::Live, following));
                    }
                }
            }
            // The single book followed before the list.
            (Kind::SerialFollowing, "following") if !has_reading => {
                if let Some(id) = payload.get("id").and_then(Value::as_str) {
                    out.extend(entity(kind, id, rev, State::Live, &payload));
                }
            }
            (Kind::SerialPast, "past") => {
                out.extend(entity(kind, "past", rev, State::Live, &payload))
            }
            (Kind::SerialShelf, "shelf") => {
                out.extend(entity(kind, "shelf", rev, State::Live, &payload));
            }
            (Kind::SerialWork, record) if record.starts_with("work:") => {
                out.extend(entity(
                    kind,
                    &record["work:".len()..],
                    rev,
                    State::Live,
                    &payload,
                ));
            }
            _ => {}
        }
    }
    Ok(out)
}

async fn soup<C: ConnectionTrait>(db: &C, owner: i32) -> Result<Option<EntityVersion>, DbErr> {
    let theirs = format!("p:{owner}");
    Ok(registry(db, her::SOUP_NAMESPACE)
        .await?
        .into_iter()
        .find(|(record, _, _)| *record == theirs)
        .and_then(|(_, payload, updated_at)| {
            entity(
                Kind::SoupGame,
                SOUP_GAME_ID,
                site_rev(updated_at),
                State::Live,
                &payload,
            )
        }))
}

async fn vitals<C: ConnectionTrait>(db: &C) -> Result<Vec<EntityVersion>, DbErr> {
    Ok(registry(db, her::VITALS_NAMESPACE)
        .await?
        .into_iter()
        .filter_map(|(record, payload, updated_at)| {
            let day = record.strip_prefix("day:")?;
            entity(
                Kind::VitalsDay,
                &format!("{day}@{}", hlc::SITE),
                site_rev(updated_at),
                State::Live,
                &payload,
            )
        })
        .collect())
}

/// The owner's private conversations with her (D14, H3-1), each with all of
/// its lines: not a group's, not Work, not one they deleted. Pictures in a
/// line cross by the address written in it; only the site's own media is
/// read, never an outside address in what someone wrote.
async fn conversations<C: ConnectionTrait>(
    db: &C,
    pictures: &assets::Pictures,
    owner: i32,
    after: Option<&str>,
    limit: usize,
) -> Result<Vec<EntityVersion>, DbErr> {
    let mut values: Vec<sea_orm::Value> = vec![owner.into(), (limit as i64).into()];
    if let Some(after) = after {
        values.push(after.into());
    }
    let sessions = db
        .query_all_raw(statement(
            &format!(
                "SELECT id, title, created_at, last_active_at FROM agent_sessions \
                 WHERE user_id = $1 AND context->>'mode' = 'chat' AND context->>'venue' IS NULL \
                   AND NOT archived AND message_count > 0{} \
                 ORDER BY id COLLATE \"C\" LIMIT $2",
                after_clause(after, "id", 3)
            ),
            values,
        ))
        .await?;
    let mut out = Vec::new();
    for session in &sessions {
        let id: String = session.try_get("", "id")?;
        let created_at: DateTime<FixedOffset> = session.try_get("", "created_at")?;
        let active: DateTime<FixedOffset> = session.try_get("", "last_active_at")?;
        let rows = db
            .query_all_raw(statement(
                "SELECT id, role, content, metadata, created_at FROM agent_messages \
                 WHERE session_id = $1 AND role IN ('user', 'assistant') \
                 ORDER BY created_at, id",
                vec![id.clone().into()],
            ))
            .await?;
        let mut lines = Vec::with_capacity(rows.len());
        let mut newest = active;
        for row in &rows {
            let content: String = row.try_get("", "content")?;
            if content.trim().is_empty() {
                continue;
            }
            let at: DateTime<FixedOffset> = row.try_get("", "created_at")?;
            newest = newest.max(at);
            let metadata: Option<Value> = row.try_get("", "metadata")?;
            let mut line_pictures = Vec::new();
            for url in pictures_in(&content) {
                line_pictures.push(LinePicture {
                    image: pictures.local_picture_at(&url).await,
                    url,
                });
            }
            lines.push(ConversationLine {
                id: format!("msg_{}", row.try_get::<i32>("", "id")?),
                role: row.try_get("", "role")?,
                content,
                at: at.timestamp_micros(),
                cut_off: metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("cutOff"))
                    .and_then(Value::as_str)
                    .map(str::to_string),
                heard_chars: metadata
                    .as_ref()
                    .and_then(|metadata| metadata.get("heardChars"))
                    .and_then(Value::as_u64),
                pictures: line_pictures,
            });
        }
        if lines.is_empty() {
            continue;
        }
        let body = ConversationBody {
            title: session
                .try_get::<Option<String>>("", "title")?
                .filter(|title| !title.trim().is_empty()),
            created_at: created_at.timestamp_micros(),
            last_active_at: active.timestamp_micros(),
            lines,
        };
        out.extend(entity(
            Kind::Conversation,
            &id,
            site_rev(newest),
            State::Live,
            &body,
        ));
    }
    Ok(out)
}

/// The site media a line shows, by the address written in it: a path under
/// `/media/` (or `/api/media/`), bare or as a markdown picture. Outside
/// addresses are not pictures of the site and are left as written.
pub(crate) fn pictures_in(content: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for token in content.split(|c: char| {
        c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '"' | '\'' | '<' | '>')
    }) {
        let url = token.trim_end_matches(['.', ',', ';', '!', '?', '。', '，']);
        let local = url.starts_with("/media/") || url.starts_with("/api/media/");
        if local && url.len() > "/media/".len() && !out.iter().any(|seen| seen == url) {
            out.push(url.to_string());
        }
    }
    out
}

/// The values her persona's fields had before a write replaced them.
async fn field_history<C: ConnectionTrait>(db: &C) -> Result<Vec<EntityVersion>, DbErr> {
    let rows = db
        .query_all_raw(statement(
            "SELECT field, value, rev, origin, replaced_at, replaced_by_rev, reason \
             FROM agent_persona_field_history ORDER BY replaced_at, id",
            Vec::new(),
        ))
        .await?;
    let mut by_id: BTreeMap<String, EntityVersion> = BTreeMap::new();
    for row in &rows {
        let replaced_at: DateTime<FixedOffset> = row.try_get("", "replaced_at")?;
        let (Ok(rev), Ok(replaced_by)) = (
            row.try_get::<String>("", "rev")?.parse::<Rev>(),
            row.try_get::<String>("", "replaced_by_rev")?.parse::<Rev>(),
        ) else {
            tracing::warn!("[sync] a past field value has no readable revision; left out");
            continue;
        };
        let body = FieldHistoryBody {
            field: row.try_get("", "field")?,
            value: row
                .try_get::<Option<Value>>("", "value")?
                .unwrap_or(Value::Null),
            rev,
            origin: row.try_get("", "origin")?,
            replaced_at: replaced_at.timestamp_micros(),
            replaced_by_rev: replaced_by.clone(),
            reason: row.try_get("", "reason")?,
        };
        let id = body.id();
        let origin = replaced_by.node.clone();
        // The same value replaced twice is one past value, at its last.
        if let Some(found) = entity_from(
            Kind::FieldHistory,
            &id,
            replaced_by,
            &origin,
            State::Live,
            &body,
        ) {
            by_id.insert(id, found);
        }
    }
    Ok(by_id.into_values().collect())
}

/// What she proposed to do for the owner, open or settled. Which Work run
/// it led to is the platform's and stays here.
async fn intentions<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    after: Option<&str>,
    limit: usize,
) -> Result<Vec<EntityVersion>, DbErr> {
    let mut values: Vec<sea_orm::Value> = vec![owner.into(), (limit as i64).into()];
    if let Some(after) = after {
        values.push(after.into());
    }
    let rows = db
        .query_all_raw(statement(
            &format!(
                "SELECT id, source_event_id, summary, reason_code, status, proposal, \
                 result_summary, expires_at, created_at, updated_at, accept_source \
                 FROM agent_intentions WHERE user_id = $1{} ORDER BY id COLLATE \"C\" LIMIT $2",
                after_clause(after, "id", 3)
            ),
            values,
        ))
        .await?;
    let mut out = Vec::new();
    for row in &rows {
        let id: String = row.try_get("", "id")?;
        let created_at: DateTime<FixedOffset> = row.try_get("", "created_at")?;
        let updated_at: DateTime<FixedOffset> = row.try_get("", "updated_at")?;
        let expires_at: Option<DateTime<FixedOffset>> = row.try_get("", "expires_at")?;
        let body = IntentionBody {
            summary: row.try_get("", "summary")?,
            reason_code: row.try_get("", "reason_code")?,
            status: row.try_get("", "status")?,
            proposal: row
                .try_get::<Option<Value>>("", "proposal")?
                .filter(Value::is_object),
            result_summary: row.try_get("", "result_summary")?,
            source_event_id: row.try_get("", "source_event_id")?,
            accept_source: row.try_get("", "accept_source")?,
            created_at: created_at.timestamp_micros(),
            updated_at: updated_at.timestamp_micros(),
            expires_at: expires_at.map(|at| at.timestamp_micros()),
        };
        out.extend(entity(
            Kind::Intention,
            &id,
            site_rev(updated_at),
            State::Live,
            &body,
        ));
    }
    Ok(out)
}

/// Her state toward the owner past her affect.
async fn owner_state<C: ConnectionTrait>(
    db: &C,
    owner: i32,
) -> Result<Option<EntityVersion>, DbErr> {
    let Some(state) = crate::models::entities::agent_addressee_state::Entity::find_by_id(owner)
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let micros = |at: Option<DateTime<FixedOffset>>| at.map(|at| at.timestamp_micros());
    let body = OwnerStateBody {
        activity: Some(state.activity.clone()).filter(|activity| !activity.trim().is_empty()),
        activity_updated_at: Some(state.activity_updated_at.timestamp_micros()),
        do_not_disturb: state.do_not_disturb,
        dnd_start_minute: state.dnd_start_minute,
        dnd_end_minute: state.dnd_end_minute,
        last_user_message_at: micros(state.last_user_message_at),
        last_proactive_at: micros(state.last_proactive_at),
        music_mood_credited_at: micros(state.music_mood_credited_at),
    };
    Ok(entity(
        Kind::OwnerState,
        OWNER,
        site_rev(state.updated_at),
        State::Live,
        &body,
    ))
}

/// Her pace: her usual as of a day, and each day's lazing kept.
async fn pace<C: ConnectionTrait>(db: &C) -> Result<Option<EntityVersion>, DbErr> {
    let records = registry(db, her::PACE_NAMESPACE).await?;
    let mut body = PaceBody {
        pace: None,
        lazed: BTreeMap::new(),
    };
    let mut newest = None;
    for (record, payload, updated_at) in records {
        if record == "pace" {
            body.pace = serde_json::from_value(payload).ok();
        } else if let Some(day) = record.strip_prefix("lazed:") {
            match payload.as_f64() {
                Some(minutes) => {
                    body.lazed.insert(day.to_string(), minutes);
                }
                None => continue,
            }
        } else {
            continue;
        }
        newest = newest.max(Some(updated_at));
    }
    let Some(newest) = newest else {
        return Ok(None);
    };
    Ok(entity(
        Kind::Pace,
        PACE_ID,
        site_rev(newest),
        State::Live,
        &body,
    ))
}

/// What she is in the middle of now, as the site keeps it across a restart
/// (`merope/doing/kept.rs`).
async fn present<C: ConnectionTrait>(db: &C) -> Result<Option<EntityVersion>, DbErr> {
    #[derive(Deserialize)]
    struct Kept {
        doing: Option<KeptDoing>,
        lazing: Option<KeptLazing>,
    }
    #[derive(Deserialize)]
    struct KeptDoing {
        thing: myriad_merope::sources::Thing,
        started: DateTime<chrono::Utc>,
        ends: DateTime<chrono::Utc>,
        why: String,
        #[serde(default)]
        had_before: u64,
    }
    #[derive(Deserialize)]
    struct KeptLazing {
        kind: String,
        started: DateTime<chrono::Utc>,
        ends: DateTime<chrono::Utc>,
    }
    let Some((_, payload, updated_at)) = registry(db, her::PRESENT_NAMESPACE)
        .await?
        .into_iter()
        .find(|(record, _, _)| record == PRESENT_ID)
    else {
        return Ok(None);
    };
    let Ok(kept) = serde_json::from_value::<Kept>(payload) else {
        tracing::warn!("[sync] what she is doing now does not read; left out");
        return Ok(None);
    };
    let body = PresentBody {
        doing: kept.doing.map(|doing| PresentDoing {
            thing: doing.thing,
            started: doing.started.timestamp_micros(),
            ends: doing.ends.timestamp_micros(),
            why: doing.why,
            had_before: doing.had_before,
        }),
        lazing: kept.lazing.map(|lazing| PresentLazing {
            kind: lazing.kind,
            started: lazing.started.timestamp_micros(),
            ends: lazing.ends.timestamp_micros(),
        }),
    };
    Ok(entity(
        Kind::Present,
        PRESENT_ID,
        site_rev(updated_at),
        State::Live,
        &body,
    ))
}

/// The lines she said to the owner unprompted.
async fn proactive_lines<C: ConnectionTrait>(
    db: &C,
    owner: i32,
) -> Result<Vec<EntityVersion>, DbErr> {
    let rows = db
        .query_all_raw(statement(
            "SELECT id, role, content, event_key, notified, created_at, say_id, metadata \
             FROM agent_proactive_messages WHERE user_id = $1 ORDER BY id",
            vec![owner.into()],
        ))
        .await?;
    let mut out = Vec::new();
    for row in &rows {
        let content: String = row.try_get("", "content")?;
        if content.trim().is_empty() {
            continue;
        }
        let number: i64 = row.try_get("", "id")?;
        let say_id: Option<String> = row.try_get("", "say_id")?;
        let created_at: DateTime<FixedOffset> = row.try_get("", "created_at")?;
        let metadata: Option<Value> = row.try_get("", "metadata")?;
        let id = say_id
            .as_deref()
            .map(|say| format!("say_{say}"))
            .unwrap_or_else(|| format!("pro_{number}"));
        let body = ProactiveLineBody {
            role: row.try_get("", "role")?,
            content,
            at: created_at.timestamp_micros(),
            say_id,
            event_key: row
                .try_get::<Option<String>>("", "event_key")?
                .filter(|key| !key.is_empty()),
            notified: row.try_get("", "notified")?,
            cut_off: metadata
                .as_ref()
                .and_then(|metadata| metadata.get("cutOff"))
                .and_then(Value::as_str)
                .map(str::to_string),
            heard_chars: metadata
                .as_ref()
                .and_then(|metadata| metadata.get("heardChars"))
                .and_then(Value::as_u64),
        };
        out.extend(entity(
            Kind::ProactiveLine,
            &id,
            site_rev(created_at),
            State::Live,
            &body,
        ));
    }
    Ok(out)
}

/// Which set each end wears (D7): the site's bust and full-body sets, and
/// each app's on her link.
async fn wearing<C: ConnectionTrait>(db: &C) -> Result<Option<EntityVersion>, DbErr> {
    let Some(row) = persona_row(db).await? else {
        return Ok(None);
    };
    let Some(profile) = row.visual_profile.as_ref() else {
        return Ok(None);
    };
    let worn = |key: &str| text(profile, key);
    let mut body = WearingBody::default();
    let site = Worn {
        outfit: worn("activeOutfitId"),
        full_body: worn("activeFullBodyOutfitId"),
    };
    if site != Worn::default() {
        body.ends.insert(hlc::SITE.to_string(), site);
    }
    for (end, outfit) in profile
        .get("activeOutfitByEnd")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        if let Some(outfit) = outfit.as_str().filter(|id| !id.trim().is_empty()) {
            body.ends.insert(
                end.clone(),
                Worn {
                    outfit: Some(outfit.to_string()),
                    full_body: None,
                },
            );
        }
    }
    if body.ends.is_empty() {
        return Ok(None);
    }
    Ok(entity(
        Kind::Wearing,
        PERSONA_ID,
        site_rev(row.updated_at),
        State::Live,
        &body,
    ))
}

/// How she looks on the site: her persona, her wardrobe and which set each
/// end wears (what a merge's confirmation shows, H3d).
pub(super) async fn looks<C: ConnectionTrait>(
    db: &C,
    pictures: &assets::Pictures,
) -> Result<Vec<EntityVersion>, DbErr> {
    let mut out: Vec<EntityVersion> = persona(db, pictures).await?.into_iter().collect();
    out.extend(outfits(db, pictures).await?);
    out.extend(wearing(db).await?);
    Ok(out)
}

/// Everything the site has of her for the owner `owner`, every kind, read
/// through `db` (an export reads it in one snapshot transaction), with the
/// vectors of `embedding_model` when given.
pub async fn whole<C: ConnectionTrait>(
    db: &C,
    pictures: &assets::Pictures,
    owner: i32,
    embedding_model: Option<&EmbeddingModel>,
) -> Result<(Vec<EntityVersion>, Vec<Embedding>), DbErr> {
    const ALL: usize = 1 << 30;
    let mut entities = Vec::new();
    for kind in Kind::ALL {
        entities.extend(of_kind(db, pictures, owner, kind, None, ALL).await?);
    }
    let embeddings = match embedding_model {
        Some(model) => embeddings(db, &entities, model).await?,
        None => Vec::new(),
    };
    Ok((entities, embeddings))
}

/// How her affect toward the owner moved, each change after the anchor's
/// last as it was recorded (`store/addressee.rs`), in id order.
async fn affect_events<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    after: Option<&str>,
    limit: usize,
) -> Result<Vec<EntityVersion>, DbErr> {
    let mut values: Vec<sea_orm::Value> = vec![owner.into(), (limit as i64).into()];
    if let Some(after) = after {
        values.push(after.into());
    }
    let rows = db
        .query_all_raw(statement(
            &format!(
                "SELECT id, at, cause, impulse, baseline_mood, baseline_arousal, before, after, \
                 late, origin, rev, created_at FROM agent_affect_events WHERE user_id = $1{} \
                 ORDER BY id COLLATE \"C\" LIMIT $2",
                after_clause(after, "id", 3)
            ),
            values,
        ))
        .await?;
    let mut out = Vec::new();
    for row in &rows {
        let id: String = row.try_get("", "id")?;
        let at: DateTime<FixedOffset> = row.try_get("", "at")?;
        let created_at: DateTime<FixedOffset> = row.try_get("", "created_at")?;
        let origin: String = row.try_get("", "origin")?;
        let affect = |column: &str| -> Result<Option<Affect>, DbErr> {
            Ok(serde_json::from_value(row.try_get::<Value>("", column)?).ok())
        };
        let (Some(before), Some(after), Ok(impulse)) = (
            affect("before")?,
            affect("after")?,
            serde_json::from_value(row.try_get::<Value>("", "impulse")?),
        ) else {
            tracing::warn!(id, "[sync] an affect event does not read; left out");
            continue;
        };
        let body = AffectEventBody {
            subject: OWNER.to_string(),
            at: at.timestamp_micros(),
            cause: row.try_get("", "cause")?,
            impulse,
            baseline: Baseline {
                mood: row.try_get("", "baseline_mood")?,
                arousal: row.try_get("", "baseline_arousal")?,
            },
            before,
            after,
            late: row.try_get("", "late")?,
        };
        let stored: Option<String> = row.try_get("", "rev")?;
        let rev = match stored.as_deref().map(str::parse::<Rev>) {
            Some(Ok(rev)) => Ok(rev),
            _ => hlc::legacy(created_at.timestamp_micros(), Some(&origin)),
        };
        match rev {
            Ok(rev) => out.extend(entity_from(
                Kind::AffectEvent,
                &id,
                rev,
                &origin,
                State::Live,
                &body,
            )),
            Err(error) => {
                tracing::warn!(%error, id, "[sync] an affect event's origin is not a side")
            }
        }
    }
    Ok(out)
}

/// Where replaying her affect toward the owner starts: the anchor kept since
/// her first recorded change, or, before any, her state with both parts
/// settled to the later of their two settle times (the one point it stands
/// at). Read only; an owner she has no state with has no anchor.
async fn anchor<C: ConnectionTrait>(db: &C, owner: i32) -> Result<Option<EntityVersion>, DbErr> {
    if let Some(kept) = kept_anchor(db, owner).await? {
        return Ok(Some(kept));
    }
    let Some(state) = crate::models::entities::agent_addressee_state::Entity::find_by_id(owner)
        .one(db)
        .await?
    else {
        return Ok(None);
    };
    let base = her::load_affect_baseline(db)
        .await
        .map_err(|error| DbErr::Custom(format!("affect baseline: {error}")))?;
    let weighs = her::sore_weighs_on(db, owner).await?;
    let base = myriad_merope::affect::AffectBaseline {
        mood: (base.mood - weighs).max(0.0),
        ..base
    };
    let at = state.mood_settled_at.max(state.emotion_settled_at);
    let hours = |since: DateTime<FixedOffset>| ((at - since).num_seconds().max(0) as f64) / 3600.0;
    let settled = myriad_merope::affect::settle(
        myriad_merope::affect::Affect {
            mood: state.mood,
            arousal: state.arousal,
            emotion: state.emotion,
            emotion_arousal: state.emotion_arousal,
        },
        base,
        hours(state.mood_settled_at),
        hours(state.emotion_settled_at),
    );
    let body = AffectAnchorBody {
        affect: Affect {
            mood: settled.mood,
            arousal: settled.arousal,
            emotion: settled.emotion,
            emotion_arousal: settled.emotion_arousal,
        },
        at: at.timestamp_micros(),
        through_event: None,
    };
    Ok(entity(
        Kind::AffectAnchor,
        OWNER,
        site_rev(state.updated_at),
        State::Live,
        &body,
    ))
}

async fn kept_anchor<C: ConnectionTrait>(
    db: &C,
    owner: i32,
) -> Result<Option<EntityVersion>, DbErr> {
    let Some(row) = db
        .query_one_raw(statement(
            "SELECT mood, arousal, emotion, emotion_arousal, at, through_event, rev \
             FROM agent_affect_anchor WHERE user_id = $1",
            vec![owner.into()],
        ))
        .await?
    else {
        return Ok(None);
    };
    let at: DateTime<FixedOffset> = row.try_get("", "at")?;
    let body = AffectAnchorBody {
        affect: Affect {
            mood: row.try_get("", "mood")?,
            arousal: row.try_get("", "arousal")?,
            emotion: row.try_get("", "emotion")?,
            emotion_arousal: row.try_get("", "emotion_arousal")?,
        },
        at: at.timestamp_micros(),
        through_event: row.try_get("", "through_event")?,
    };
    let rev = row
        .try_get::<String>("", "rev")?
        .parse()
        .unwrap_or_else(|_| site_rev(at));
    Ok(entity(Kind::AffectAnchor, OWNER, rev, State::Live, &body))
}

/// Vectors of the page's live memories, when the side asking uses the same
/// model and the vector is of the memory's current words.
async fn embeddings<C: ConnectionTrait>(
    db: &C,
    entities: &[EntityVersion],
    model: &EmbeddingModel,
) -> Result<Vec<Embedding>, DbErr> {
    let live: BTreeMap<&str, &EntityVersion> = entities
        .iter()
        .filter(|entity| entity.kind == Kind::Memory && entity.state == State::Live)
        .map(|entity| (entity.id.as_str(), entity))
        .collect();
    if live.is_empty() {
        return Ok(Vec::new());
    }
    let ids: Vec<String> = live.keys().map(|id| id.to_string()).collect();
    let rows = db
        .query_all_raw(statement(
            "SELECT memory_id, digest, vector FROM agent_memory_embeddings \
             WHERE model = $1 AND memory_id = ANY($2) ORDER BY memory_id COLLATE \"C\"",
            vec![model.model.clone().into(), ids.into()],
        ))
        .await?;
    let mut out = Vec::new();
    for row in &rows {
        let id: String = row.try_get("", "memory_id")?;
        let digest: String = row.try_get("", "digest")?;
        let vector: Vec<u8> = row.try_get("", "vector")?;
        let Some(memory) = live.get(id.as_str()) else {
            continue;
        };
        let content = memory
            .body
            .get("content")
            .and_then(Value::as_str)
            .unwrap_or("");
        let current = crate::services::agent::memory::meaning::digest(content);
        if digest != current || vector.len() != model.dims as usize * 4 {
            continue;
        }
        out.push(Embedding {
            memory_id: id,
            digest,
            vector: base64::engine::general_purpose::STANDARD.encode(vector),
        });
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A whole snapshot paged two at a time on a real database (an isolated
    /// schema; skipped without a test database): every kind in order, only
    /// what is hers or the owner's alone, resumable from any cursor.
    #[tokio::test]
    async fn a_snapshot_pages_through_only_what_crosses() {
        use sea_orm::ConnectionTrait;
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        let digest = crate::services::agent::memory::meaning::digest("她喜欢雨天");
        let fixture_sql = format!(
            r#"
INSERT INTO users (id, username) VALUES (41, 'owner41'), (42, 'guest42');
INSERT INTO agent_persona (id, name, personality, zone, field_revs, persona_json, visual_profile,
                           updated_at)
VALUES ('site', ' Merope ', '安静', 'Asia/Tokyo',
        '{{"summary":{{"rev":"001791600000000-000002-site","origin":"site"}},
          "speechStyle":{{"rev":"001791600000000-000001-site","origin":"site"}}}}', '{{"summary":"住在站点上","likes":["雨天"]}}'::jsonb,
        '{{"clothingStyle":"everyday","activeOutfitId":"default","activeFullBodyOutfitId":"coat","wardrobe":[
          {{"id":"default","clothingStyle":"everyday","outfit":{{"top":"白衬衫"}}}},
          {{"id":"coat","name":"大衣","clothingStyle":"everyday","outfit":{{"top":"大衣"}},
            "portraitAssetId":"/media/assets/00000000-0000-0000-0000-000000000000/gone.png"}}]}}'::jsonb,
        '2026-10-09T00:00:00Z');
INSERT INTO agent_memories (id, user_id, kind, content, speaker, source, venue, audience, concepts,
                            valid_from, invalid_at, invalid_reason, created_at, updated_at) VALUES
  ('m_own', NULL, 'doing', '去河边走了一圈', 'agent', 'own', 'own', '[]', '[]',
   '2026-10-01Z', NULL, NULL, '2026-10-01Z', '2026-10-01Z'),
  ('m_owner', 41, 'preference', '她喜欢雨天', 'user', 'chat', 'private', '[41]',
   '[{{"name":"雨天","aliases":["rain"]}}]', '2026-10-02Z', NULL, NULL, '2026-10-02Z', '2026-10-02Z'),
  ('m_retired', 41, 'fact', '住在北边', 'user', 'chat', 'private', '[]', '["北边"]',
   '2026-10-03Z', '2026-10-04Z', 'superseded', '2026-10-03Z', '2026-10-04Z'),
  ('m_guest', 42, 'fact', '别人的事', 'user', 'chat', 'private', '[42]', '[]',
   '2026-10-03Z', NULL, NULL, '2026-10-03Z', '2026-10-03Z'),
  ('m_shared', 41, 'fact', '两个人一起说的', 'user', 'chat', 'private', '[41,42]', '[]',
   '2026-10-03Z', NULL, NULL, '2026-10-03Z', '2026-10-03Z'),
  ('m_group', 41, 'fact', '群里的梗', 'user', 'chat', 'group:telegram:-1', '[41]', '[]',
   '2026-10-03Z', NULL, NULL, '2026-10-03Z', '2026-10-03Z');
UPDATE agent_memories SET superseded_by = 'm_owner' WHERE id = 'm_retired';
INSERT INTO agent_memory_embeddings (memory_id, model, digest, vector, created_at) VALUES
  ('m_owner', 'pplx', '{digest}', decode('0000803f000000bf', 'hex'), NOW()),
  ('m_own', 'pplx', 'stale-digest', decode('0000803f000000bf', 'hex'), NOW());
INSERT INTO merope_stickers (id, picture, meaning, venue, asset_id, identity, created_at) VALUES
  ('stk_gone', '她挥手', '你好', NULL, 'gone-picture', 'abc', '2026-10-05Z'),
  ('stk_group', '群里的', '群', 'telegram:-1', 'gone-picture', 'abc', '2026-10-05Z');
INSERT INTO agent_diary (id, user_id, content, source, created_at) VALUES
  ('d1', 41, '今天下雨了', 'chat', '2026-10-05Z'),
  ('d2', 42, '别人的日记', 'chat', '2026-10-05Z');
INSERT INTO agent_affect_events (id, user_id, at, cause, impulse, baseline_mood, baseline_arousal,
                                 before, after, created_at) VALUES
  ('6f1c2b9e-3d4a-4e5f-8a7b-0c1d2e3f4a5b', 41, '2026-10-08T12:00:00Z', 'user_praise',
   '{{"appraisal":{{"emotion":82.0,"arousal":58.0,"scale":0.8}}}}', 70, 48,
   '{{"mood":63.25,"arousal":48.0,"emotion":71.3,"emotionArousal":55.5}}',
   '{{"mood":64.1,"arousal":49.2,"emotion":75.0,"emotionArousal":56.0}}', '2026-10-08T12:00:00Z'),
  ('7a0d3c8f-0000-4000-8000-000000000042', 42, '2026-10-08T12:00:00Z', 'user_turn', '"none"',
   70, 48, '{{"mood":70,"arousal":48,"emotion":50,"emotionArousal":50}}',
   '{{"mood":70,"arousal":48,"emotion":50,"emotionArousal":50}}', '2026-10-08T12:00:00Z');
INSERT INTO agent_addressee_state (user_id, mood, arousal, emotion, emotion_arousal,
       mood_settled_at, emotion_settled_at, updated_at)
VALUES (41, 40, 50, 60, 50, '2026-10-09T00:00:00Z', '2026-10-09T01:00:00Z', '2026-10-09T01:00:00Z');
INSERT INTO runtime_registry (namespace, record_id, payload, expires_at, updated_at) VALUES
  ('merope_vitals', 'day:2026-10-08', '{{"day":"2026-10-08","calls":3,"failedCalls":0,"inputTokens":1,
     "busiest":[],"kept":{{}},"unreadable":0,"things":0,"ownMinutes":0.0,"lazedMinutes":0.0,
     "landed":{{}},"replies":0,"replyP50":null,"replyP90":null,"notesLeanOn":[],"repliesLeanOn":[],
     "groups":[{{"venue":"telegram:-100123","lines":4,"hers":1,"others":2}}],
     "alerts":[]}}', 9999999999, '2026-10-09Z'),
  ('merope_serial', 'past', '{{"finished":["a"],"dropped":[]}}', 9999999999, '2026-10-09Z'),
  ('merope_serial', 'reading', '[{{"id":"w1","next":1,"total":3,"started":"2026-10-01T00:00:00Z","guess":null}}]',
     9999999999, '2026-10-09Z'),
  ('merope_serial', 'work:w1', '{{"id":"w1","title":"T","author":"A","lang":"ja","about":"",
     "source":{{"kind":"aozora","path":"p"}}}}', 9999999999, '2026-10-09Z'),
  ('merope_soup', 'p:41', '{{"surface":"s","truth":"t","keys":[],"asked":[],"found":[],"ending":null,
     "solver":null,"started":"2026-10-09T00:00:00Z"}}', 9999999999, '2026-10-09Z'),
  ('merope_soup', 'g:telegram:-1', '{{"surface":"s","truth":"t","keys":[],"asked":[],"found":[],
     "ending":null,"solver":null,"started":"2026-10-09T00:00:00Z"}}', 9999999999, '2026-10-09Z'),
  ('merope_soup', 'p:42', '{{"surface":"s","truth":"t","keys":[],"asked":[],"found":[],"ending":null,
     "solver":null,"started":"2026-10-09T00:00:00Z"}}', 9999999999, '2026-10-09Z');
"#
        );
        for statement in fixture_sql
            .split(";\n")
            .filter(|sql| !sql.trim().is_empty())
        {
            db.execute_unprepared(statement)
                .await
                .unwrap_or_else(|error| panic!("{error}: {statement}"));
        }

        let model = EmbeddingModel {
            model: "pplx".into(),
            dims: 2,
        };
        let limits = Limits {
            entities: 2,
            bytes: 1 << 20,
        };
        let mut all = Vec::new();
        let mut embeddings = Vec::new();
        let mut after: Option<Cursor> = None;
        let mut pages = 0;
        loop {
            let page = page(
                db,
                41,
                &Kind::COMPARED,
                after.as_ref(),
                &limits,
                Some(&model),
            )
            .await
            .unwrap();
            pages += 1;
            assert!(page.entities.len() <= 2);
            // The same page again is the same page.
            let again = super::page(
                db,
                41,
                &Kind::COMPARED,
                after.as_ref(),
                &limits,
                Some(&model),
            )
            .await
            .unwrap();
            assert_eq!(again.entities, page.entities);
            all.extend(page.entities);
            embeddings.extend(page.embeddings);
            if !page.more {
                break;
            }
            after = page.next;
            assert!(pages < 50);
        }
        let keys: Vec<(Kind, &str)> = all.iter().map(|e| (e.kind, e.id.as_str())).collect();
        assert_eq!(
            keys,
            vec![
                (Kind::AffectAnchor, "owner"),
                (Kind::AffectEvent, "6f1c2b9e-3d4a-4e5f-8a7b-0c1d2e3f4a5b"),
                (Kind::Diary, "d1"),
                (Kind::Memory, "m_own"),
                (Kind::Memory, "m_owner"),
                (Kind::Memory, "m_retired"),
                (Kind::Outfit, "coat"),
                (Kind::Outfit, "default"),
                (Kind::Persona, "her"),
                (Kind::SerialFollowing, "w1"),
                (Kind::SerialPast, "past"),
                (Kind::SerialWork, "w1"),
                (Kind::SoupGame, "p:owner"),
                (Kind::Sticker, "stk_gone"),
                (Kind::VitalsDay, "2026-10-08@site"),
            ]
        );
        // Every entity is canonical, and revisions come from the site.
        for entity in &all {
            assert_eq!(
                myriad_persona_sync::canonicalize(entity.clone()).unwrap(),
                *entity
            );
            assert_eq!(entity.origin, "site");
        }
        let by = |id: &str| all.iter().find(|e| e.id == id).unwrap();
        assert_eq!(
            by("m_owner").body["audience"]["members"],
            serde_json::json!(["owner"])
        );
        assert_eq!(by("m_owner").body["concepts"][0]["aliases"][0], "rain");
        assert_eq!(by("m_retired").state, State::Retired);
        assert_eq!(by("m_retired").body["invalidReason"], "superseded");
        assert_eq!(by("m_retired").body["supersededBy"], "m_owner");
        assert_eq!(by("m_owner").body["supersededBy"], Value::Null);
        assert_eq!(by("m_own").body["audience"]["venue"], "own");
        let persona = by("her");
        assert_eq!(persona.body["fields"]["name"]["value"], " Merope ");
        assert_eq!(persona.body["fields"]["zone"]["value"], "Asia/Tokyo");
        // Each field at its own edit; one never edited since, at the row's
        // last write; a cleared one crosses as null; the persona is as new as
        // its newest field.
        assert_eq!(
            persona.body["fields"]["summary"]["rev"],
            "001791600000000-000002-site"
        );
        assert_eq!(
            persona.body["fields"]["name"]["rev"],
            "001791504000000-000000-site"
        );
        assert_eq!(persona.body["fields"]["speechStyle"]["value"], Value::Null);
        assert_eq!(persona.rev.to_string(), "001791600000000-000002-site");
        assert_eq!(
            persona.body["fields"]["likes"]["value"],
            serde_json::json!(["雨天"])
        );
        assert_eq!(
            persona.body["fields"]["visual.clothingStyle"]["value"],
            "everyday"
        );
        for worn in ["visual.activeOutfitId", "visual.activeFullBodyOutfitId"] {
            assert!(persona.body["fields"].get(worn).is_none(), "{worn}");
        }
        assert!(persona.body["fields"].get("visual.wardrobe").is_none());
        // Mood settled to the later settle time, an hour on for mood.
        let anchor = by("owner");
        assert_eq!(anchor.body["at"], 1_791_507_600_000_000_i64);
        assert!(anchor.body["affect"]["mood"].as_f64().unwrap() > 40.0);
        // Only the owner's changes cross, as recorded.
        let event = by("6f1c2b9e-3d4a-4e5f-8a7b-0c1d2e3f4a5b");
        assert_eq!(event.body["subject"], "owner");
        assert_eq!(event.body["cause"], "user_praise");
        assert_eq!(event.body["impulse"]["appraisal"]["scale"], 0.8);
        assert_eq!(event.body["after"]["emotionArousal"], 56.0);
        // Pictures the site cannot read cross marked, not dropped.
        assert_eq!(
            by("coat").body["portrait"],
            serde_json::json!({ "unreadable": true })
        );
        assert_eq!(by("default").body["portrait"], Value::Null);
        assert_eq!(
            by("stk_gone").body["image"],
            serde_json::json!({ "unreadable": true })
        );
        // A day's groups cross as counts, without which group.
        let day = by("2026-10-08@site");
        assert_eq!(
            day.body["groups"],
            serde_json::json!([{ "lines": 4, "hers": 1, "others": 2 }])
        );
        assert!(!day.canonical_bytes().windows(8).any(|w| w == b"telegram"));
        // Only the vector of the live memory's current words.
        assert_eq!(embeddings.len(), 1);
        assert_eq!(embeddings[0].memory_id, "m_owner");
        assert_eq!(embeddings[0].vector, "AACAPwAAAL8=");

        // Resuming after any entity gives the rest.
        let middle = Cursor {
            kind: Kind::Memory,
            id: "m_owner".into(),
        };
        let rest = page(
            db,
            41,
            &Kind::COMPARED,
            Some(&middle),
            &Limits {
                entities: 100,
                bytes: 1 << 20,
            },
            None,
        )
        .await
        .unwrap();
        assert_eq!(rest.entities[0].id, "m_retired");
        assert_eq!(rest.entities.len(), 10);

        // Once her first change is recorded, the anchor is the one kept.
        db.execute_unprepared(
            "INSERT INTO agent_affect_anchor (user_id, mood, arousal, emotion, emotion_arousal, \
             at, through_event, rev) VALUES (41, 64.1, 49.2, 75, 56, '2026-10-08T12:00:00Z', \
             '6f1c2b9e-3d4a-4e5f-8a7b-0c1d2e3f4a5b', '001791460800000-000000-site')",
        )
        .await
        .unwrap();
        let kept = of_kind(
            db,
            &assets::Pictures::new(db, 41),
            41,
            Kind::AffectAnchor,
            None,
            10,
        )
        .await
        .unwrap();
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].body["affect"]["mood"], 64.1);
        assert_eq!(
            kept[0].body["throughEvent"],
            "6f1c2b9e-3d4a-4e5f-8a7b-0c1d2e3f4a5b"
        );
        assert_eq!(kept[0].rev.to_string(), "001791460800000-000000-site");
        assert!(!rest.more && rest.embeddings.is_empty());

        // The site's holdings count only what crosses.
        let holdings = super::super::holdings(db, 41).await.unwrap();
        assert!(holdings.persona);
        assert_eq!(
            (holdings.memories, holdings.diary, holdings.affect_events),
            (3, 1, 1)
        );

        fixture.close().await;
    }

    #[test]
    fn a_cursor_round_trips_and_nothing_else_reads_as_one() {
        let cursor = Cursor {
            kind: Kind::Memory,
            id: "mem_3b1f".into(),
        };
        assert_eq!(Cursor::decode(&cursor.encode()), Some(cursor));
        assert_eq!(Cursor::decode("not a cursor"), None);
        assert_eq!(Cursor::decode(""), None);
    }
}
