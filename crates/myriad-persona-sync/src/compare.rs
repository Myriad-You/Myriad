//! Two of her side by side, before the first merge: what is the same, what
//! only one side has, what both have but differently, and whether she has
//! the same name on both (if not, the owner says which one she is).
//!
//! Nothing here decides a merge. The result is what the owner is shown.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::{
    SyncError,
    canonical::digest,
    entity::{
        Affect, AffectAnchorBody, AssetRef, EntityVersion, Kind, MemoryBody, MemoryVenue,
        OutfitBody, Picture, StickerBody, canonicalize,
    },
    persona::{FieldClass, PersonaBody, PersonaField, field_class},
};

/// The same thing on each side.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sides<T> {
    pub local: T,
    pub remote: T,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Status {
    Same,
    OnlyLocal,
    OnlyRemote,
    /// On both, not the same.
    Differ,
}

/// The rows of the comparison, besides her persona and her affect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Category {
    /// What she knows of the owner, alone with them.
    Memories,
    /// Her own life: what she did, thought and kept.
    Experiences,
    Diary,
    AffectEvents,
    Vitals,
    Serial,
    Soup,
    Wardrobe,
    Stickers,
}

impl Category {
    pub const ALL: [Category; 9] = [
        Category::Memories,
        Category::Experiences,
        Category::Diary,
        Category::AffectEvents,
        Category::Vitals,
        Category::Serial,
        Category::Soup,
        Category::Wardrobe,
        Category::Stickers,
    ];
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EntityKey {
    pub kind: Kind,
    pub id: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Counts {
    pub same: usize,
    pub only_local: usize,
    pub only_remote: usize,
    pub differ: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entries {
    pub same: Vec<EntityKey>,
    pub only_local: Vec<EntityKey>,
    pub only_remote: Vec<EntityKey>,
    pub differ: Vec<EntityKey>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryComparison {
    pub counts: Counts,
    pub entries: Entries,
}

impl CategoryComparison {
    fn add(&mut self, status: Status, key: EntityKey) {
        let (count, entries) = match status {
            Status::Same => (&mut self.counts.same, &mut self.entries.same),
            Status::OnlyLocal => (&mut self.counts.only_local, &mut self.entries.only_local),
            Status::OnlyRemote => (&mut self.counts.only_remote, &mut self.entries.only_remote),
            Status::Differ => (&mut self.counts.differ, &mut self.entries.differ),
        };
        *count += 1;
        entries.push(key);
    }
}

/// One persona field on both sides.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FieldComparison {
    pub field: String,
    pub class: FieldClass,
    pub status: Status,
    pub local: Option<PersonaField>,
    pub remote: Option<PersonaField>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersonaComparison {
    pub present: Sides<bool>,
    /// By field name. Opaque fields ([`FieldClass::Opaque`]) are not listed:
    /// they cross as they are and are not anyone's to compare.
    pub fields: Vec<FieldComparison>,
}

/// Where a picture is that its side cannot read.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PictureKey {
    pub kind: Kind,
    pub id: String,
    /// `portrait` of an outfit, `image` of a sticker, `stickerAvatar` of
    /// her persona.
    pub field: String,
}

/// Her affect toward the owner as one side holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AnchorView {
    pub affect: Affect,
    pub at: i64,
    /// `myriad_merope::affect::mood_band` of it, unsettled.
    pub band: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AffectComparison {
    pub anchor: Sides<Option<AnchorView>>,
    pub events: Sides<usize>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    /// Her name, trimmed, on each side.
    pub names: Sides<Option<String>>,
    pub same_name: bool,
    /// Both sides have her and her names differ: the owner picks which side
    /// she is (the subject) before anything merges.
    pub needs_subject: bool,
    pub persona: PersonaComparison,
    pub affect: AffectComparison,
    pub categories: BTreeMap<Category, CategoryComparison>,
    /// Memories each side has under a different id with the same words, in
    /// the same place. Both are kept; this only says so.
    pub memory_twins: Vec<Sides<String>>,
    /// Pictures each side refers to but cannot read (marked
    /// `{"unreadable": true}`): "a picture on the site cannot be read".
    #[serde(default)]
    pub unreadable_pictures: Sides<Vec<PictureKey>>,
}

type Side = BTreeMap<(Kind, String), EntityVersion>;

fn side(entities: Vec<EntityVersion>, which: &str) -> Result<Side, SyncError> {
    let mut out = BTreeMap::new();
    for entity in entities {
        let entity =
            canonicalize(entity).map_err(|error| SyncError::new(format!("{which}: {error}")))?;
        let key = (entity.kind, entity.id.clone());
        if out.insert(key, entity).is_some() {
            return Err(SyncError::new(format!(
                "{which}: an entity is listed twice"
            )));
        }
    }
    Ok(out)
}

fn category_of(entity: &EntityVersion) -> Result<Option<Category>, SyncError> {
    Ok(Some(match entity.kind {
        Kind::Persona | Kind::AffectAnchor => return Ok(None),
        Kind::AffectEvent => Category::AffectEvents,
        Kind::Diary => Category::Diary,
        Kind::Memory => match entity.body_as::<MemoryBody>()?.audience.venue {
            MemoryVenue::Own => Category::Experiences,
            MemoryVenue::Private => Category::Memories,
        },
        Kind::Outfit => Category::Wardrobe,
        Kind::SerialFollowing | Kind::SerialPast | Kind::SerialShelf | Kind::SerialWork => {
            Category::Serial
        }
        Kind::SoupGame => Category::Soup,
        Kind::Sticker => Category::Stickers,
        Kind::VitalsDay => Category::Vitals,
    }))
}

fn persona_of(side: &Side) -> Result<Option<PersonaBody>, SyncError> {
    side.get(&(Kind::Persona, crate::entity::PERSONA_ID.to_string()))
        .map(EntityVersion::body_as::<PersonaBody>)
        .transpose()
}

fn anchor_of(side: &Side) -> Result<Option<AnchorView>, SyncError> {
    side.get(&(Kind::AffectAnchor, crate::entity::OWNER.to_string()))
        .map(|entity| {
            let anchor: AffectAnchorBody = entity.body_as()?;
            Ok(AnchorView {
                band: myriad_merope::affect::mood_band(anchor.affect.mood, anchor.affect.arousal)
                    .to_string(),
                affect: anchor.affect,
                at: anchor.at,
            })
        })
        .transpose()
}

/// The pictures a side refers to and cannot read, in key order.
fn unreadable_pictures(side: &Side) -> Result<Vec<PictureKey>, SyncError> {
    let mut out = Vec::new();
    let mut mark = |entity: &EntityVersion, field: &str, picture: Option<&Picture>| {
        if picture.is_some_and(Picture::is_unreadable) {
            out.push(PictureKey {
                kind: entity.kind,
                id: entity.id.clone(),
                field: field.to_string(),
            });
        }
    };
    for entity in side.values() {
        if entity.state == crate::State::Deleted {
            continue;
        }
        match entity.kind {
            Kind::Outfit => {
                let outfit: OutfitBody = entity.body_as()?;
                mark(entity, "portrait", outfit.portrait.as_ref());
            }
            Kind::Sticker => {
                let sticker: StickerBody = entity.body_as()?;
                mark(entity, "image", Some(&sticker.image));
            }
            Kind::Persona => {
                let persona: PersonaBody = entity.body_as()?;
                let avatar = persona
                    .fields
                    .get("stickerAvatar")
                    .and_then(|field| serde_json::from_value::<Picture>(field.value.clone()).ok());
                mark(entity, "stickerAvatar", avatar.as_ref());
            }
            _ => {}
        }
    }
    Ok(out)
}

/// An asset as a comparison sees it: its content id and type. The id is
/// the hash of the content (a rig's package id, an image's sha256), so two
/// sides with the same id have the same file; the size and media type each
/// side states are not what differs (R8).
fn by_content(asset: &mut AssetRef) {
    asset.bytes = 0;
    asset.media_type.clear();
}

fn picture_by_content(picture: &mut Picture) {
    if let Picture::Image(asset) = picture {
        by_content(asset);
    }
}

/// The digest a comparison goes by: the entity's own, except that the
/// assets an outfit or a sticker refers to count by content id alone.
fn compared_digest(entity: &EntityVersion) -> Result<String, SyncError> {
    if entity.state == crate::State::Deleted {
        return Ok(entity.digest.clone());
    }
    let body = match entity.kind {
        Kind::Outfit => {
            let mut outfit: OutfitBody = entity.body_as()?;
            if let Some(portrait) = &mut outfit.portrait {
                picture_by_content(portrait);
            }
            if let Some(rig) = &mut outfit.rig {
                by_content(rig);
            }
            serde_json::to_value(outfit)
        }
        Kind::Sticker => {
            let mut sticker: StickerBody = entity.body_as()?;
            picture_by_content(&mut sticker.image);
            serde_json::to_value(sticker)
        }
        _ => return Ok(entity.digest.clone()),
    }
    .map_err(SyncError::from_json)?;
    Ok(digest(&body))
}

/// A persona field's value as a comparison sees it: her sticker avatar by
/// content id alone, everything else as it is.
fn compared_value(field: &str, value: &serde_json::Value) -> serde_json::Value {
    if field == "stickerAvatar"
        && let Ok(mut picture) = serde_json::from_value::<Picture>(value.clone())
    {
        picture_by_content(&mut picture);
        return serde_json::to_value(picture).unwrap_or_else(|_| value.clone());
    }
    value.clone()
}

fn words(content: &str) -> String {
    content.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Compares this side (`local`) with the other (`remote`). Each list is a
/// whole snapshot; every entity is checked and canonicalized first. An
/// asset (a rig, a portrait, a sticker, her sticker avatar) counts by its
/// content id alone: two sides that state its size or media type
/// differently still have the same file.
pub fn compare(
    local: Vec<EntityVersion>,
    remote: Vec<EntityVersion>,
) -> Result<Comparison, SyncError> {
    let local = side(local, "local")?;
    let remote = side(remote, "remote")?;

    let personas = Sides {
        local: persona_of(&local)?,
        remote: persona_of(&remote)?,
    };
    let names = Sides {
        local: personas
            .local
            .as_ref()
            .and_then(PersonaBody::name)
            .map(str::to_string),
        remote: personas
            .remote
            .as_ref()
            .and_then(PersonaBody::name)
            .map(str::to_string),
    };
    let same_name = names.local == names.remote;
    let present = Sides {
        local: personas.local.is_some(),
        remote: personas.remote.is_some(),
    };
    let empty = PersonaBody::default();
    let local_fields = &personas.local.as_ref().unwrap_or(&empty).fields;
    let remote_fields = &personas.remote.as_ref().unwrap_or(&empty).fields;
    let fields = local_fields
        .keys()
        .chain(remote_fields.keys())
        .filter(|field| field_class(field) != FieldClass::Opaque)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|field| {
            let (local, remote) = (local_fields.get(field), remote_fields.get(field));
            let status = match (local, remote) {
                (Some(local), Some(remote))
                    if compared_value(field, &local.value)
                        == compared_value(field, &remote.value) =>
                {
                    Status::Same
                }
                (Some(_), Some(_)) => Status::Differ,
                (Some(_), None) => Status::OnlyLocal,
                _ => Status::OnlyRemote,
            };
            FieldComparison {
                field: field.clone(),
                class: field_class(field),
                status,
                local: local.cloned(),
                remote: remote.cloned(),
            }
        })
        .collect();

    let mut categories: BTreeMap<Category, CategoryComparison> = Category::ALL
        .into_iter()
        .map(|category| (category, CategoryComparison::default()))
        .collect();
    let keys = local.keys().chain(remote.keys()).collect::<BTreeSet<_>>();
    let mut events = Sides {
        local: 0,
        remote: 0,
    };
    for key in keys {
        let (here, there) = (local.get(key), remote.get(key));
        let Some(either) = here.or(there) else {
            continue;
        };
        if key.0 == Kind::AffectEvent {
            events.local += usize::from(here.is_some());
            events.remote += usize::from(there.is_some());
        }
        let Some(category) = category_of(either)? else {
            continue;
        };
        let status = match (here, there) {
            (Some(here), Some(there))
                if here.state == there.state
                    && compared_digest(here)? == compared_digest(there)? =>
            {
                Status::Same
            }
            // A memory both have is placed where this side keeps it.
            (Some(_), Some(_)) => Status::Differ,
            (Some(_), None) => Status::OnlyLocal,
            _ => Status::OnlyRemote,
        };
        categories
            .get_mut(&category)
            .expect("every category is listed")
            .add(
                status,
                EntityKey {
                    kind: key.0,
                    id: key.1.clone(),
                },
            );
    }

    // The same words in the same place, under two ids.
    let unmatched =
        |side: &Side, ids: &[EntityKey]| -> Result<Vec<(String, MemoryVenue, String)>, SyncError> {
            ids.iter()
                .filter(|key| key.kind == Kind::Memory)
                .map(|key| {
                    let body: MemoryBody = side[&(key.kind, key.id.clone())].body_as()?;
                    Ok((words(&body.content), body.audience.venue, key.id.clone()))
                })
                .collect()
        };
    let mut memory_twins = Vec::new();
    for category in [Category::Memories, Category::Experiences] {
        let entries = &categories[&category].entries;
        let mut theirs = unmatched(&remote, &entries.only_remote)?;
        for (content, venue, id) in unmatched(&local, &entries.only_local)? {
            if let Some(at) = theirs
                .iter()
                .position(|(other, place, _)| *other == content && *place == venue)
            {
                let (_, _, other) = theirs.remove(at);
                memory_twins.push(Sides {
                    local: id,
                    remote: other,
                });
            }
        }
    }

    Ok(Comparison {
        needs_subject: present.local && present.remote && !same_name,
        same_name,
        names,
        persona: PersonaComparison { present, fields },
        affect: AffectComparison {
            anchor: Sides {
                local: anchor_of(&local)?,
                remote: anchor_of(&remote)?,
            },
            events,
        },
        categories,
        memory_twins,
        unreadable_pictures: Sides {
            local: unreadable_pictures(&local)?,
            remote: unreadable_pictures(&remote)?,
        },
    })
}

/// What a side holds that makes it not empty.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Holdings {
    /// She has a persona there.
    pub persona: bool,
    pub memories: u64,
    pub diary: u64,
    pub affect_events: u64,
}

impl Holdings {
    pub fn of(entities: &[EntityVersion]) -> Self {
        let mut holdings = Self::default();
        for entity in entities {
            match entity.kind {
                Kind::Persona => holdings.persona = true,
                Kind::Memory => holdings.memories += 1,
                Kind::Diary => holdings.diary += 1,
                Kind::AffectEvent => holdings.affect_events += 1,
                _ => {}
            }
        }
        holdings
    }
}

/// Whether a side has no her yet: no persona and nothing she remembers,
/// wrote or felt. Moving her onto an empty side is an import, not a merge.
pub fn is_empty(holdings: &Holdings) -> bool {
    !holdings.persona
        && holdings.memories == 0
        && holdings.diary == 0
        && holdings.affect_events == 0
}
