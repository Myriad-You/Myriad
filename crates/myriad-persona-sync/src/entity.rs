//! What crosses between the sides: the envelope, each entity's version, and
//! the shape of each kind's body.
//!
//! Field names are camelCase and times are Unix microseconds, except in the
//! bodies that are her own records as `myriad-merope` keeps them (a day of
//! vitals, a serial, a game of soup): those cross exactly as stored.
//! The person a memory is with is always written `"owner"`, the site's
//! owner; nobody else's id ever crosses.

use std::fmt;

use myriad_merope::{serial, soup, vitals};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::Value;

use crate::{
    SyncError,
    affect::AffectImpulse,
    canonical::{canonical_bytes, digest},
    hlc::{Rev, check_node},
    persona::PersonaBody,
};

/// The envelope's fixed `format`.
pub const FORMAT: &str = "myriad.her";
/// Bumped only for a change an older side cannot read; a new optional
/// field does not bump it.
pub const FORMAT_VERSION: u32 = 1;
/// The one person a synced memory can be with.
pub const OWNER: &str = "owner";
/// Longest entity id.
pub const MAX_ID_CHARS: usize = 160;

/// One batch of her, as sent or saved.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Envelope {
    pub format: String,
    pub format_version: u32,
    /// Which birth of her this is; none until the site first gives one.
    #[serde(default)]
    pub lineage: Option<String>,
    /// The side that wrote the batch.
    pub origin: String,
    #[serde(default)]
    pub embedding_model: Option<EmbeddingModel>,
    pub entities: Vec<EntityVersion>,
    #[serde(default)]
    pub embeddings: Vec<Embedding>,
    #[serde(default)]
    pub assets: Vec<AssetRef>,
}

/// The kinds of entity, in the order a snapshot lists them (by name).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    AffectAnchor,
    AffectEvent,
    Diary,
    Memory,
    Outfit,
    Persona,
    SerialFollowing,
    SerialPast,
    SerialShelf,
    SerialWork,
    SoupGame,
    Sticker,
    VitalsDay,
}

impl Kind {
    pub const ALL: [Kind; 13] = [
        Kind::AffectAnchor,
        Kind::AffectEvent,
        Kind::Diary,
        Kind::Memory,
        Kind::Outfit,
        Kind::Persona,
        Kind::SerialFollowing,
        Kind::SerialPast,
        Kind::SerialShelf,
        Kind::SerialWork,
        Kind::SoupGame,
        Kind::Sticker,
        Kind::VitalsDay,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Kind::AffectAnchor => "affect_anchor",
            Kind::AffectEvent => "affect_event",
            Kind::Diary => "diary",
            Kind::Memory => "memory",
            Kind::Outfit => "outfit",
            Kind::Persona => "persona",
            Kind::SerialFollowing => "serial_following",
            Kind::SerialPast => "serial_past",
            Kind::SerialShelf => "serial_shelf",
            Kind::SerialWork => "serial_work",
            Kind::SoupGame => "soup_game",
            Kind::Sticker => "sticker",
            Kind::VitalsDay => "vitals_day",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == raw)
    }

    /// The id of a kind that has exactly one entity.
    pub fn fixed_id(self) -> Option<&'static str> {
        match self {
            Kind::Persona => Some(PERSONA_ID),
            Kind::AffectAnchor => Some(OWNER),
            Kind::SerialPast => Some("past"),
            Kind::SerialShelf => Some("shelf"),
            Kind::SoupGame => Some(SOUP_GAME_ID),
            _ => None,
        }
    }

    /// The states an entity of this kind can be in.
    fn states(self) -> &'static [State] {
        match self {
            Kind::Memory => &[State::Live, State::Retired],
            Kind::Outfit => &[State::Live, State::Deleted],
            _ => &[State::Live],
        }
    }
}

impl fmt::Display for Kind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Her persona's id.
pub const PERSONA_ID: &str = "her";
/// The game of soup she plays with the owner, by the site's record id.
pub const SOUP_GAME_ID: &str = "p:owner";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum State {
    #[default]
    Live,
    /// A memory she no longer holds (`invalidReason` says why). Retiring is
    /// one-way unless the owner restores it with a later revision.
    Retired,
    /// An outfit the owner removed: the only tombstone, body `null`.
    Deleted,
}

/// One entity at one revision.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EntityVersion {
    pub kind: Kind,
    /// Shared by both sides, never rewritten.
    pub id: String,
    pub rev: Rev,
    /// The side that wrote this revision.
    pub origin: String,
    #[serde(default)]
    pub state: State,
    pub body: Value,
    /// Of the canonical body ([`digest`]). Filled in by [`canonicalize`];
    /// a stated digest that does not match is refused.
    #[serde(default)]
    pub digest: String,
}

impl EntityVersion {
    /// Builds a canonical entity from a typed body.
    pub fn new<B: Serialize>(
        kind: Kind,
        id: impl Into<String>,
        rev: Rev,
        origin: impl Into<String>,
        state: State,
        body: &B,
    ) -> Result<Self, SyncError> {
        let body = serde_json::to_value(body).map_err(SyncError::from_json)?;
        canonicalize(Self {
            kind,
            id: id.into(),
            rev,
            origin: origin.into(),
            state,
            body,
            digest: String::new(),
        })
    }

    /// The canonical bytes of the whole entity.
    pub fn canonical_bytes(&self) -> Vec<u8> {
        // An entity always serializes: every key is a string.
        canonical_bytes(&serde_json::to_value(self).expect("an entity serializes"))
    }

    /// The body as its kind's type.
    pub fn body_as<B: DeserializeOwned>(&self) -> Result<B, SyncError> {
        serde_json::from_value(self.body.clone())
            .map_err(|error| SyncError::new(format!("{} {:?}: body: {error}", self.kind, self.id)))
    }
}

/// Checks an entity and puts it in its one form: the body read as its
/// kind's type and written back, the digest computed. Whatever a side keeps
/// locally, this is what it compares, sends and saves.
pub fn canonicalize(mut entity: EntityVersion) -> Result<EntityVersion, SyncError> {
    let what =
        |message: String| SyncError::new(format!("{} {:?}: {message}", entity.kind, entity.id));
    check_id(&entity.id).map_err(|error| what(error.to_string()))?;
    if let Some(fixed) = entity.kind.fixed_id()
        && entity.id != fixed
    {
        return Err(what(format!("the id of a {} is {fixed:?}", entity.kind)));
    }
    check_node(&entity.origin).map_err(|error| what(error.to_string()))?;
    if !entity.kind.states().contains(&entity.state) {
        return Err(what(format!(
            "a {} cannot be {:?}",
            entity.kind, entity.state
        )));
    }
    let body = std::mem::take(&mut entity.body);
    entity.body = normalize_body(entity.kind, entity.state, &entity.id, body)
        .map_err(|error| what(error.to_string()))?;
    let computed = digest(&entity.body);
    if !entity.digest.is_empty() && entity.digest != computed {
        return Err(what(format!(
            "digest {} does not match the body ({computed})",
            entity.digest
        )));
    }
    entity.digest = computed;
    Ok(entity)
}

/// Checks a whole batch and puts it in its one form: entities and assets
/// canonical and sorted, each listed once.
pub fn canonicalize_envelope(mut envelope: Envelope) -> Result<Envelope, SyncError> {
    if envelope.format != FORMAT {
        return Err(SyncError::new(format!("format is {FORMAT:?}")));
    }
    if envelope.format_version != FORMAT_VERSION {
        return Err(SyncError::new(format!(
            "format version {} is not one this side reads ({FORMAT_VERSION})",
            envelope.format_version
        )));
    }
    check_node(&envelope.origin)?;
    if let Some(model) = &mut envelope.embedding_model {
        model.model = model.model.trim().to_string();
    }
    envelope.entities = envelope
        .entities
        .into_iter()
        .map(canonicalize)
        .collect::<Result<_, _>>()?;
    envelope
        .entities
        .sort_by(|a, b| (a.kind, &a.id).cmp(&(b.kind, &b.id)));
    if envelope
        .entities
        .windows(2)
        .any(|pair| (pair[0].kind, &pair[0].id) == (pair[1].kind, &pair[1].id))
    {
        return Err(SyncError::new("an entity is listed twice"));
    }
    for asset in &mut envelope.assets {
        asset.check()?;
    }
    envelope.assets.sort_by(|a, b| a.id.cmp(&b.id));
    envelope.assets.dedup();
    if envelope
        .assets
        .windows(2)
        .any(|pair| pair[0].id == pair[1].id)
    {
        return Err(SyncError::new("one asset id is listed as two assets"));
    }
    for embedding in &envelope.embeddings {
        if embedding.digest.len() != 32
            || !embedding
                .digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit())
        {
            return Err(SyncError::new("an embedding's digest is 32 hex characters"));
        }
    }
    envelope
        .embeddings
        .sort_by(|a, b| a.memory_id.cmp(&b.memory_id));
    Ok(envelope)
}

fn check_id(id: &str) -> Result<(), SyncError> {
    if id.is_empty() || id.chars().count() > MAX_ID_CHARS {
        return Err(SyncError::new(format!(
            "id must be 1 to {MAX_ID_CHARS} characters"
        )));
    }
    if id.chars().any(char::is_control) {
        return Err(SyncError::new("id has a control character"));
    }
    Ok(())
}

fn typed<B: DeserializeOwned + Serialize>(body: Value) -> Result<Value, SyncError> {
    let typed: B = serde_json::from_value(body).map_err(SyncError::from_json)?;
    serde_json::to_value(typed).map_err(SyncError::from_json)
}

fn checked<B: DeserializeOwned + Serialize + CheckBody>(body: Value) -> Result<Value, SyncError> {
    let mut typed: B = serde_json::from_value(body).map_err(SyncError::from_json)?;
    typed.check_body()?;
    serde_json::to_value(typed).map_err(SyncError::from_json)
}

fn normalize_body(kind: Kind, state: State, id: &str, body: Value) -> Result<Value, SyncError> {
    if state == State::Deleted {
        return match body {
            Value::Null => Ok(Value::Null),
            _ => Err(SyncError::new("a deleted entity's body is null")),
        };
    }
    let body = match kind {
        Kind::AffectAnchor => checked::<AffectAnchorBody>(body)?,
        Kind::AffectEvent => checked::<AffectEventBody>(body)?,
        Kind::Diary => checked::<DiaryBody>(body)?,
        Kind::Memory => {
            let mut memory: MemoryBody =
                serde_json::from_value(body).map_err(SyncError::from_json)?;
            if memory.invalid_at.is_some() != (state == State::Retired) {
                return Err(SyncError::new(
                    "a memory is retired exactly when it has invalidAt",
                ));
            }
            memory.check_body()?;
            serde_json::to_value(memory).map_err(SyncError::from_json)?
        }
        Kind::Outfit => checked::<OutfitBody>(body)?,
        Kind::Persona => checked::<PersonaBody>(body)?,
        Kind::SerialFollowing => {
            let following: serial::Following =
                serde_json::from_value(body).map_err(SyncError::from_json)?;
            if following.id != id {
                return Err(SyncError::new("a serial followed is under its work's id"));
            }
            serde_json::to_value(following).map_err(SyncError::from_json)?
        }
        Kind::SerialPast => typed::<serial::Past>(body)?,
        Kind::SerialShelf => typed::<serial::Shelf>(body)?,
        Kind::SerialWork => {
            let work: serial::Work = serde_json::from_value(body).map_err(SyncError::from_json)?;
            if work.id != id {
                return Err(SyncError::new("a work is under its own id"));
            }
            serde_json::to_value(work).map_err(SyncError::from_json)?
        }
        Kind::SoupGame => typed::<soup::Game>(body)?,
        Kind::Sticker => checked::<StickerBody>(body)?,
        Kind::VitalsDay => {
            let mut day: vitals::Day =
                serde_json::from_value(body).map_err(SyncError::from_json)?;
            strip_groups(&mut day);
            match id.split_once('@') {
                Some((date, origin)) if date == day.day && check_node(origin).is_ok() => {}
                _ => {
                    return Err(SyncError::new(
                        "a day of vitals is under <day>@<the side that counted it>",
                    ));
                }
            }
            serde_json::to_value(day).map_err(SyncError::from_json)?
        }
    };
    Ok(body)
}

/// Which groups talked never leaves the side that counted them: a day
/// crosses with each group's counts (what merging two sides' days needs),
/// not the group's id, in an order that says nothing about which it was.
fn strip_groups(day: &mut vitals::Day) {
    for group in &mut day.groups {
        group.venue.clear();
    }
    day.groups
        .sort_by_key(|group| std::cmp::Reverse((group.lines, group.hers, group.others)));
}

/// A body's own rules, past its shape.
pub(crate) trait CheckBody {
    fn check_body(&mut self) -> Result<(), SyncError>;
}

/// Where a memory was made: her own life, or alone with the owner. Group
/// memories and anyone else's private ones never cross.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryVenue {
    Own,
    Private,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryAudience {
    pub venue: MemoryVenue,
    /// `["owner"]` in private, `[]` in her own.
    pub members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Concept {
    pub name: String,
    #[serde(default)]
    pub aliases: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MemoryBody {
    pub kind: String,
    pub content: String,
    /// Opaque: carried as written.
    #[serde(default)]
    pub evidence: Option<String>,
    pub speaker: String,
    pub source: String,
    pub audience: MemoryAudience,
    #[serde(default)]
    pub concepts: Vec<Concept>,
    pub importance: f64,
    pub valid_from: i64,
    pub created_at: i64,
    #[serde(default)]
    pub invalid_at: Option<i64>,
    #[serde(default)]
    pub invalid_reason: Option<String>,
    /// The memory that took this one's place, when one did.
    #[serde(default)]
    pub superseded_by: Option<String>,
}

impl CheckBody for MemoryBody {
    fn check_body(&mut self) -> Result<(), SyncError> {
        if self.content.trim().is_empty() {
            return Err(SyncError::new("a memory has content"));
        }
        if !self.importance.is_finite() {
            return Err(SyncError::new("importance is a number"));
        }
        let members = &self.audience.members;
        let ok = match self.audience.venue {
            MemoryVenue::Own => members.is_empty(),
            MemoryVenue::Private => members.len() == 1 && members[0] == OWNER,
        };
        if !ok {
            return Err(SyncError::new(
                "a memory is with nobody (her own) or with the owner alone",
            ));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiaryBody {
    pub content: String,
    pub source: String,
    pub created_at: i64,
    /// Always `"owner"`.
    pub subject: String,
}

impl CheckBody for DiaryBody {
    fn check_body(&mut self) -> Result<(), SyncError> {
        if self.subject != OWNER {
            return Err(SyncError::new("a diary entry crosses only for the owner"));
        }
        Ok(())
    }
}

/// Her mood and emotion.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Affect {
    pub mood: f64,
    pub arousal: f64,
    pub emotion: f64,
    pub emotion_arousal: f64,
}

impl Affect {
    fn check(&self) -> Result<(), SyncError> {
        if [self.mood, self.arousal, self.emotion, self.emotion_arousal]
            .iter()
            .all(|value| value.is_finite())
        {
            Ok(())
        } else {
            Err(SyncError::new("affect values are numbers"))
        }
    }
}

/// Her affect toward the owner at `at`, with every event up to
/// `throughEvent` folded in. Only the site writes it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AffectAnchorBody {
    pub affect: Affect,
    pub at: i64,
    #[serde(default)]
    pub through_event: Option<String>,
}

impl CheckBody for AffectAnchorBody {
    fn check_body(&mut self) -> Result<(), SyncError> {
        self.affect.check()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Baseline {
    pub mood: f64,
    pub arousal: f64,
}

/// One change of her affect toward the owner. Appended, never edited.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AffectEventBody {
    /// Always `"owner"`.
    pub subject: String,
    pub at: i64,
    pub cause: String,
    pub impulse: AffectImpulse,
    /// The baseline used then.
    pub baseline: Baseline,
    /// For audit and display; replay reads neither.
    pub before: Affect,
    pub after: Affect,
    /// Applied at the anchor because it arrived after its time was folded.
    #[serde(default)]
    pub late: bool,
}

impl CheckBody for AffectEventBody {
    fn check_body(&mut self) -> Result<(), SyncError> {
        if self.subject != OWNER {
            return Err(SyncError::new("an affect event crosses only for the owner"));
        }
        self.before.check()?;
        self.after.check()
    }
}

/// The kind of file an asset is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetType {
    /// A compiled rig: `manifest.json` and its atlas. The id is
    /// [`crate::rig_package_id`].
    Rig,
    /// A picture: portrait, sticker, sticker avatar. The id is sha256 of
    /// its bytes.
    Image,
}

/// A file an entity refers to. Fetched on demand by id, never inlined.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AssetRef {
    /// 64 lowercase hex.
    pub id: String,
    #[serde(rename = "type")]
    pub kind: AssetType,
    pub bytes: u64,
    pub media_type: String,
}

impl AssetRef {
    fn check(&mut self) -> Result<(), SyncError> {
        if self.id.len() != 64 || !self.id.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(SyncError::new("an asset id is 64 hex characters"));
        }
        self.id.make_ascii_lowercase();
        if self.media_type.trim().is_empty() {
            return Err(SyncError::new("an asset has a media type"));
        }
        Ok(())
    }
}

/// A picture an entity refers to: the image itself, or, when the side has
/// a picture there but cannot read it (an outside address that no longer
/// answers, a file gone missing), a mark saying so. A side shows the mark
/// ("a picture on the site cannot be read") instead of fetching anything.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Picture {
    Image(AssetRef),
    Unreadable(UnreadablePicture),
}

/// `{"unreadable": true}`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UnreadablePicture {
    pub unreadable: bool,
}

impl Picture {
    /// The mark for a picture that cannot be read.
    pub fn unreadable() -> Self {
        Self::Unreadable(UnreadablePicture { unreadable: true })
    }

    /// The image, when there is one to fetch.
    pub fn asset(&self) -> Option<&AssetRef> {
        match self {
            Self::Image(asset) => Some(asset),
            Self::Unreadable(_) => None,
        }
    }

    pub fn is_unreadable(&self) -> bool {
        matches!(self, Self::Unreadable(_))
    }

    pub(crate) fn check(&mut self) -> Result<(), SyncError> {
        match self {
            Self::Image(asset) => {
                asset.check()?;
                if asset.kind != AssetType::Image {
                    return Err(SyncError::new("a picture is an image"));
                }
                Ok(())
            }
            Self::Unreadable(mark) if mark.unreadable => Ok(()),
            Self::Unreadable(_) => Err(SyncError::new(
                "a picture is an image or {\"unreadable\": true}",
            )),
        }
    }
}

impl From<AssetRef> for Picture {
    fn from(asset: AssetRef) -> Self {
        Self::Image(asset)
    }
}

/// A saved outfit.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OutfitBody {
    #[serde(default)]
    pub name: Option<String>,
    pub clothing_style: String,
    /// What she wears, as the wardrobe keeps it.
    pub outfit: Value,
    #[serde(default)]
    pub visual_identity: Option<Value>,
    /// `bust` or `fullBody`; absent is a bust.
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub generation_fingerprint: Option<String>,
    /// The bust set a full-body set was drawn from.
    #[serde(default)]
    pub reference_outfit_id: Option<String>,
    /// None when the set has no portrait of its own.
    #[serde(default)]
    pub portrait: Option<Picture>,
    #[serde(default)]
    pub rig: Option<AssetRef>,
}

impl CheckBody for OutfitBody {
    fn check_body(&mut self) -> Result<(), SyncError> {
        if let Some(profile) = &self.profile
            && profile != "bust"
            && profile != "fullBody"
        {
            return Err(SyncError::new("an outfit's profile is bust or fullBody"));
        }
        if !self.outfit.is_object() {
            return Err(SyncError::new("an outfit's outfit is an object"));
        }
        if let Some(portrait) = &mut self.portrait {
            portrait.check()?;
        }
        if let Some(rig) = &mut self.rig {
            rig.check()?;
            if rig.kind != AssetType::Rig {
                return Err(SyncError::new("a rig is a rig"));
            }
        }
        Ok(())
    }
}

/// A sticker of her.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StickerBody {
    /// What it shows.
    pub picture: String,
    /// What she sends it for.
    pub meaning: String,
    /// The look it was drawn of (`myriad_merope::stickers::identity_key`):
    /// a sticker of how she no longer looks is not offered.
    pub identity: String,
    pub created_at: i64,
    pub image: Picture,
}

impl CheckBody for StickerBody {
    fn check_body(&mut self) -> Result<(), SyncError> {
        self.image.check()
    }
}

/// Which model a memory's vector came from. Two sides share vectors only
/// when this is equal; where the model is served from and with whose key
/// is no part of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmbeddingModel {
    /// The model name, trimmed.
    pub model: String,
    pub dims: u32,
}

/// A memory's vector.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Embedding {
    pub memory_id: String,
    /// Of the text embedded: the first 16 bytes of sha256 of the trimmed
    /// content, in hex.
    pub digest: String,
    /// f32 little-endian, base64.
    pub vector: String,
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn entity(kind: Kind, id: &str, state: State, body: Value) -> EntityVersion {
        EntityVersion {
            kind,
            id: id.into(),
            rev: "001760000000123-000000-site".parse().unwrap(),
            origin: "site".into(),
            state,
            body,
            digest: String::new(),
        }
    }

    fn memory(venue: &str, members: Value) -> Value {
        json!({
            "kind": "fact", "content": "她喜欢雨天", "speaker": "user", "source": "chat",
            "audience": {"venue": venue, "members": members},
            "importance": 0.5, "validFrom": 1, "createdAt": 1,
        })
    }

    #[test]
    fn a_memory_is_with_the_owner_or_nobody() {
        assert!(
            canonicalize(entity(
                Kind::Memory,
                "m1",
                State::Live,
                memory("private", json!(["owner"]))
            ))
            .is_ok()
        );
        assert!(
            canonicalize(entity(
                Kind::Memory,
                "m1",
                State::Live,
                memory("own", json!([]))
            ))
            .is_ok()
        );
        for (venue, members) in [
            ("private", json!(["7"])),
            ("private", json!([])),
            ("own", json!(["owner"])),
            ("group", json!(["owner"])),
        ] {
            assert!(
                canonicalize(entity(
                    Kind::Memory,
                    "m1",
                    State::Live,
                    memory(venue, members.clone())
                ))
                .is_err(),
                "{venue} {members}"
            );
        }
    }

    #[test]
    fn retired_is_exactly_having_invalid_at() {
        let mut body = memory("own", json!([]));
        assert!(canonicalize(entity(Kind::Memory, "m", State::Retired, body.clone())).is_err());
        body["invalidAt"] = json!(5);
        body["invalidReason"] = json!("superseded");
        assert!(canonicalize(entity(Kind::Memory, "m", State::Live, body.clone())).is_err());
        let retired = canonicalize(entity(Kind::Memory, "m", State::Retired, body)).unwrap();
        assert_eq!(retired.body["invalidReason"], "superseded");
    }

    /// A side that kept a memory as retired without saying when (native
    /// rows from before retirement times) gives its last write time as
    /// `invalidAt` (R9), with or without a reason, even the moment it was
    /// written; the format takes it like any other retirement.
    #[test]
    fn a_retirement_time_taken_from_the_last_write_is_a_retirement_time() {
        let mut body = memory("own", json!([]));
        body["createdAt"] = json!(1_760_000_000_000_000_i64);
        body["validFrom"] = json!(1_760_000_000_000_000_i64);
        body["invalidAt"] = json!(1_760_000_000_000_000_i64);
        let retired = canonicalize(entity(Kind::Memory, "m", State::Retired, body)).unwrap();
        assert_eq!(retired.body["invalidAt"], 1_760_000_000_000_000_i64);
        assert_eq!(retired.body["invalidReason"], Value::Null);
    }

    #[test]
    fn missing_optionals_and_key_order_do_not_change_the_digest() {
        let terse = canonicalize(entity(
            Kind::Memory,
            "m",
            State::Live,
            memory("own", json!([])),
        ))
        .unwrap();
        let mut spelled = memory("own", json!([]));
        spelled["evidence"] = Value::Null;
        spelled["concepts"] = json!([]);
        spelled["supersededBy"] = Value::Null;
        let spelled = canonicalize(entity(Kind::Memory, "m", State::Live, spelled)).unwrap();
        assert_eq!(terse.digest, spelled.digest);
        assert_eq!(terse.canonical_bytes(), spelled.canonical_bytes());
    }

    #[test]
    fn unknown_fields_wrong_ids_and_wrong_digests_are_refused() {
        let mut body = memory("own", json!([]));
        body["strength"] = json!(3);
        assert!(canonicalize(entity(Kind::Memory, "m", State::Live, body)).is_err());
        let persona = json!({"fields": {}});
        assert!(canonicalize(entity(Kind::Persona, "site", State::Live, persona.clone())).is_err());
        let mut stated = entity(Kind::Persona, "her", State::Live, persona);
        stated.digest = "00".repeat(16);
        assert!(canonicalize(stated).is_err());
        assert!(
            canonicalize(entity(
                Kind::Memory,
                "",
                State::Live,
                memory("own", json!([]))
            ))
            .is_err()
        );
    }

    #[test]
    fn only_an_outfit_is_deleted_and_then_its_body_is_null() {
        assert!(canonicalize(entity(Kind::Outfit, "coat", State::Deleted, Value::Null)).is_ok());
        assert!(canonicalize(entity(Kind::Outfit, "coat", State::Deleted, json!({}))).is_err());
        assert!(canonicalize(entity(Kind::Diary, "d", State::Deleted, Value::Null)).is_err());
    }

    #[test]
    fn a_day_of_vitals_is_under_its_day_and_side() {
        let day = json!({
            "day": "2026-10-08", "calls": 3, "failedCalls": 0, "inputTokens": 10,
            "busiest": [], "kept": {}, "unreadable": 0, "things": 1, "ownMinutes": 12.5,
            "lazedMinutes": 0.0, "landed": {}, "replies": 0, "replyP50": null, "replyP90": null,
            "notesLeanOn": [], "repliesLeanOn": [], "alerts": [],
        });
        assert!(
            canonicalize(entity(
                Kind::VitalsDay,
                "2026-10-08@site",
                State::Live,
                day.clone()
            ))
            .is_ok()
        );
        assert!(
            canonicalize(entity(
                Kind::VitalsDay,
                "2026-10-09@site",
                State::Live,
                day.clone()
            ))
            .is_err()
        );
        assert!(canonicalize(entity(Kind::VitalsDay, "2026-10-08", State::Live, day)).is_err());
    }

    #[test]
    fn a_day_crosses_with_each_groups_counts_and_not_which_group() {
        let mut day = json!({
            "day": "2026-10-08", "calls": 0, "failedCalls": 0, "inputTokens": 0,
            "busiest": [], "kept": {}, "unreadable": 0, "things": 0, "ownMinutes": 0.0,
            "lazedMinutes": 0.0, "landed": {}, "replies": 0, "replyP50": null, "replyP90": null,
            "notesLeanOn": [], "repliesLeanOn": [], "alerts": [],
        });
        day["groups"] = json!([
            {"venue": "telegram:-100123", "lines": 4, "hers": 1, "others": 2},
            {"venue": "qq:998877", "lines": 30, "hers": 6, "others": 5},
        ]);
        let crossed =
            canonicalize(entity(Kind::VitalsDay, "2026-10-08@site", State::Live, day)).unwrap();
        assert_eq!(
            crossed.body["groups"],
            json!([
                {"lines": 30, "hers": 6, "others": 5},
                {"lines": 4, "hers": 1, "others": 2},
            ])
        );
        let text = String::from_utf8(crossed.canonical_bytes()).unwrap();
        assert!(
            !text.contains("venue") && !text.contains("998877"),
            "{text}"
        );
        // Crossing again changes nothing.
        assert_eq!(canonicalize(crossed.clone()).unwrap(), crossed);
    }

    #[test]
    fn a_picture_is_an_image_or_marked_unreadable() {
        let sticker = |image: Value| {
            json!({
                "picture": "waving", "meaning": "hi", "identity": "abc", "createdAt": 1,
                "image": image,
            })
        };
        let gone = canonicalize(entity(
            Kind::Sticker,
            "s",
            State::Live,
            sticker(json!({"unreadable": true})),
        ))
        .unwrap();
        assert_eq!(gone.body["image"], json!({"unreadable": true}));
        for bad in [
            json!({"unreadable": false}),
            json!(null),
            json!({"unreadable": true, "id": "ab"}),
            json!({"id": "ab".repeat(32), "type": "rig", "bytes": 1, "mediaType": "x"}),
        ] {
            assert!(
                canonicalize(entity(
                    Kind::Sticker,
                    "s",
                    State::Live,
                    sticker(bad.clone())
                ))
                .is_err(),
                "{bad}"
            );
        }
        let outfit = json!({
            "clothingStyle": "everyday", "outfit": {}, "portrait": {"unreadable": true},
        });
        let outfit = canonicalize(entity(Kind::Outfit, "coat", State::Live, outfit)).unwrap();
        let body: OutfitBody = outfit.body_as().unwrap();
        assert!(body.portrait.unwrap().is_unreadable());
        let persona = json!({"fields": {"stickerAvatar": {
            "value": {"unreadable": true}, "rev": "001760000000123-000000-site", "origin": "site",
        }}});
        assert!(canonicalize(entity(Kind::Persona, "her", State::Live, persona)).is_ok());
    }

    #[test]
    fn asset_ids_are_hex_and_lowercased() {
        let sticker = |id: &str| {
            json!({
                "picture": "waving", "meaning": "hi", "identity": "abc", "createdAt": 1,
                "image": {"id": id, "type": "image", "bytes": 10, "mediaType": "image/png"},
            })
        };
        let upper = canonicalize(entity(
            Kind::Sticker,
            "s",
            State::Live,
            sticker(&"AB".repeat(32)),
        ))
        .unwrap();
        assert_eq!(upper.body["image"]["id"], "ab".repeat(32));
        assert!(canonicalize(entity(Kind::Sticker, "s", State::Live, sticker("abc"))).is_err());
    }
}
