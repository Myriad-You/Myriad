//! The files her entities refer to: rig packages by their content id and
//! pictures (portraits, stickers, the sticker avatar) by the sha256 of their
//! bytes. Only a file the site's persona refers to right now can be fetched,
//! so an id cannot be used to look around the site's media.
//!
//! A picture at an outside address is copied into the site's media library
//! the first time a device reads her, and her data is pointed at the copy
//! (see [`super::outside`]): the device never fetches an outside address,
//! and a dead link cannot make the picture unreadable later. One that cannot
//! be fetched is tried again after [`PICTURE_TTL`]; until then, and when it
//! truly cannot be read, the entity carries the mark `{"unreadable": true}`
//! in its place.

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::{Duration, Instant},
};

use myriad_persona_sync::{
    AssetRef, AssetType, EntityVersion, Kind, PersonaBody, Picture,
    canonical::sha256_hex,
    entity::{OutfitBody, StickerBody},
};
use sea_orm::{ConnectionTrait, DatabaseConnection, DbErr, EntityTrait};

use crate::services::{
    image_cache::ImageCacheService,
    media::{MediaService, MediaStore},
    merope_rig,
};

/// What a rig asset is served as: its manifest and atlas, in that order.
pub const RIG_MEDIA_TYPE: &str = "multipart/mixed";

/// Pictures read lately, by address: hashing every sticker for every page
/// would read them all each time.
static PICTURES: LazyLock<Mutex<HashMap<String, (Instant, AssetRef)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
pub const PICTURE_TTL: Duration = Duration::from_secs(600);
pub(super) const PICTURES_KEPT: usize = 4096;

/// Where the site's pictures are read from, and where an outside picture is
/// copied to: the site's database, its owner, the image cache an outside
/// picture is fetched through and the media library it is kept in.
pub struct Pictures {
    pub(super) db: DatabaseConnection,
    pub(super) owner: i32,
    pub(super) cache: ImageCacheService,
    pub(super) media: MediaService,
}

impl Pictures {
    pub fn new(db: &DatabaseConnection, owner: i32) -> Self {
        Self {
            db: db.clone(),
            owner,
            cache: ImageCacheService::new(),
            media: MediaService::from_data_paths(crate::services::data_paths::paths()),
        }
    }

    /// The same, with the cache and the media library kept under `root`.
    #[cfg(test)]
    pub fn under(db: &DatabaseConnection, owner: i32, root: &std::path::Path) -> Self {
        Self {
            db: db.clone(),
            owner,
            cache: ImageCacheService::at(root.join("cache")),
            media: MediaService::new(root.join("media")),
        }
    }

    pub(super) fn store(&self) -> &MediaStore {
        self.media.store()
    }

    /// The picture at `url`, as an asset. None when the site cannot read it.
    pub async fn image_ref(&self, url: &str) -> Option<AssetRef> {
        if let Ok(kept) = PICTURES.lock()
            && let Some((at, asset)) = kept.get(url)
            && at.elapsed() < PICTURE_TTL
        {
            return Some(asset.clone());
        }
        let (bytes, media_type) = self.bytes(url).await?;
        let asset = AssetRef {
            id: sha256_hex(&bytes),
            kind: AssetType::Image,
            bytes: bytes.len() as u64,
            media_type,
        };
        if let Ok(mut kept) = PICTURES.lock() {
            if kept.len() >= PICTURES_KEPT {
                kept.clear();
            }
            kept.insert(url.to_string(), (Instant::now(), asset.clone()));
        }
        Some(asset)
    }

    /// A picture field as it crosses: the image, or the mark that the site
    /// has a picture there and cannot read it.
    pub async fn picture_at(&self, url: &str) -> Picture {
        match self.image_ref(url).await {
            Some(asset) => Picture::Image(asset),
            None => Picture::unreadable(),
        }
    }

    /// The bytes and type of the picture at `url`: a site media file as it
    /// is, an outside one from its copy in the media library (made now
    /// when there is none yet).
    pub async fn bytes(&self, url: &str) -> Option<(Vec<u8>, String)> {
        if let Some(found) = crate::services::image_generation::read_public_local_media_on(
            &self.db,
            self.store(),
            url,
        )
        .await
        {
            return Some(found);
        }
        super::outside::copy_of(self, url).await
    }
}

/// A compiled rig the site keeps, as an asset.
pub async fn rig_ref(id: &str) -> Option<AssetRef> {
    let id = merope_rig::normalize_asset_id(id)?;
    let manifest = tokio::fs::metadata(merope_rig::manifest_path(&id))
        .await
        .ok()?;
    let atlas = tokio::fs::metadata(merope_rig::atlas_path(&id))
        .await
        .ok()?;
    Some(AssetRef {
        id,
        kind: AssetType::Rig,
        bytes: manifest.len() + atlas.len(),
        media_type: RIG_MEDIA_TYPE.to_string(),
    })
}

/// Every asset `entities` refer to, once, by id.
pub fn referenced(entities: &[EntityVersion]) -> Vec<AssetRef> {
    let mut out: Vec<AssetRef> = Vec::new();
    for entity in entities {
        match entity.kind {
            Kind::Outfit => {
                if let Ok(outfit) = entity.body_as::<OutfitBody>() {
                    out.extend(outfit.portrait.as_ref().and_then(Picture::asset).cloned());
                    out.extend(outfit.rig);
                }
            }
            Kind::Sticker => {
                if let Ok(sticker) = entity.body_as::<StickerBody>() {
                    out.extend(sticker.image.asset().cloned());
                }
            }
            Kind::Persona => {
                if let Ok(persona) = entity.body_as::<PersonaBody>()
                    && let Some(field) = persona.fields.get("stickerAvatar")
                    && let Ok(asset) = serde_json::from_value::<AssetRef>(field.value.clone())
                {
                    out.push(asset);
                }
            }
            _ => {}
        }
    }
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out.dedup_by(|a, b| a.id == b.id);
    out
}

/// Where an asset the persona refers to is kept.
pub enum Source {
    Rig { id: String },
    Image { url: String, media_type: String },
}

/// The asset `id`, if the site's persona refers to it now: one of her
/// outfits' portrait or rig, the sticker avatar, or one of her stickers
/// (not a group's).
pub async fn find(pictures: &Pictures, id: &str) -> Result<Option<Source>, DbErr> {
    let db = &pictures.db;
    let Some(id) = merope_rig::normalize_asset_id(id) else {
        return Ok(None);
    };
    let Some(row) = crate::models::entities::agent_persona::Entity::find_by_id(
        crate::services::agent::merope::api::store::PERSONA_ROW_ID,
    )
    .one(db)
    .await?
    else {
        return Ok(None);
    };
    let mut urls: Vec<String> = Vec::new();
    let wardrobe = row
        .visual_profile
        .as_ref()
        .and_then(|profile| profile.get("wardrobe"))
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for item in &wardrobe {
        if let Some(rig) = item.get("rigAssetId").and_then(serde_json::Value::as_str)
            && merope_rig::normalize_asset_id(rig).as_deref() == Some(id.as_str())
            && rig_ref(&id).await.is_some()
        {
            return Ok(Some(Source::Rig { id }));
        }
        if let Some(url) = item
            .get("portraitAssetId")
            .and_then(serde_json::Value::as_str)
        {
            urls.push(url.to_string());
        }
    }
    urls.extend(row.portrait_asset_id.clone());
    urls.extend(row.avatar_asset_id.clone());
    let stickers = db
        .query_all_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT asset_id FROM merope_stickers WHERE venue IS NULL".to_string(),
        ))
        .await?;
    for sticker in &stickers {
        urls.push(sticker.try_get("", "asset_id")?);
    }
    for url in urls {
        let url = url.trim();
        if url.is_empty() {
            continue;
        }
        if let Some(asset) = pictures.image_ref(url).await
            && asset.id == id
        {
            return Ok(Some(Source::Image {
                url: url.to_string(),
                media_type: asset.media_type,
            }));
        }
    }
    Ok(None)
}

/// The bytes of a picture the persona refers to, checked against its id
/// again: the file may have changed since it was hashed.
pub async fn image_bytes(pictures: &Pictures, url: &str, id: &str) -> Option<Vec<u8>> {
    let (bytes, _) = pictures.bytes(url).await?;
    (sha256_hex(&bytes) == id).then_some(bytes)
}
