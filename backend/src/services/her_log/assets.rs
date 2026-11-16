//! Her pictures and rigs as her log's rows name them (b1 of protocol 2): a
//! row carries only an asset's content id (an outfit's portrait and rig, a
//! sticker's picture, her sticker avatar); the bytes go by the sync assets
//! endpoints. Another end gets the site's with `GET /sync/assets/{id}`, and
//! puts the ones the site lacks with `PUT` ([`put`]). A record is taken into
//! the site's tables only once every asset it names is here ([`here`]);
//! until then it waits, and is taken in when the last of them comes.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use myriad_persona_sync::{
    AssetType, EntityVersion,
    archive::{RIG_ATLAS_FILE, RIG_MANIFEST_FILE},
    canonical::sha256_hex,
};
use sea_orm::{ConnectionTrait, DatabaseConnection, Statement};

use super::Error;
use crate::services::persona_sync::{self, assets};

/// What of the assets some records name the site has: each picture's url by
/// its id, and the rigs it holds. `missing` when one is not here yet.
#[derive(Debug, Default)]
pub(super) struct Here {
    pub urls: BTreeMap<String, String>,
    pub rigs: BTreeSet<String>,
    pub missing: bool,
}

/// What the site has of the assets `entities` name.
pub(super) async fn here(
    db: &DatabaseConnection,
    owner: i32,
    entities: &[EntityVersion],
) -> Result<Here, Error> {
    let mut here = Here::default();
    for asset in assets::referenced(entities) {
        match asset.kind {
            AssetType::Image => match picture_url(db, owner, &asset.id).await? {
                Some(url) => {
                    here.urls.insert(asset.id, url);
                }
                None => here.missing = true,
            },
            AssetType::Rig => {
                if assets::rig_ref(&asset.id).await.is_some() {
                    here.rigs.insert(asset.id);
                } else {
                    here.missing = true;
                }
            }
        }
    }
    Ok(here)
}

/// The url of the owner's picture whose bytes are `id`, when the site keeps
/// one: one put here, or any the owner's media hold.
async fn picture_url(
    db: &DatabaseConnection,
    owner: i32,
    id: &str,
) -> Result<Option<String>, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            db.get_database_backend(),
            "SELECT url FROM media_assets WHERE checksum_sha256 = $1 AND state = 'ready' \
             AND (owner_user_id = $2 OR owner_user_id IS NULL) ORDER BY id DESC LIMIT 1",
            vec![id.to_ascii_lowercase().into(), owner.into()],
        ))
        .await?;
    if let Some(row) = row {
        return Ok(Some(row.try_get::<String>("", "url")?));
    }
    // One her persona refers to now (its url not one the media table knows).
    let pictures = assets::Pictures::new(db, owner);
    Ok(match assets::find(&pictures, id).await? {
        Some(assets::Source::Image { url, .. }) => Some(url),
        _ => None,
    })
}

/// Another end puts an asset its rows name: a picture as itself (`file`
/// none), a rig as its two files, one put each (`manifest.json`,
/// `atlas.png`). Each is checked against its id; a rig is kept once both
/// files are here. Whether the asset is whole here now.
pub async fn put(
    db: &DatabaseConnection,
    owner: i32,
    id: &str,
    file: Option<&str>,
    bytes: Vec<u8>,
) -> Result<bool, Error> {
    let refused = |why: &str| Error::Refused(why.to_string());
    if bytes.len() as u64 > persona_sync::ASSET_MAX_BYTES {
        return Err(refused("an asset is past the size the site takes"));
    }
    let id = crate::services::merope_rig::normalize_asset_id(id)
        .ok_or_else(|| refused("an asset id is 64 hex digits"))?;
    let mut files: BTreeMap<String, Vec<u8>> = BTreeMap::new();
    match file {
        None => {
            if sha256_hex(&bytes) != id {
                return Err(refused("a picture is not the bytes its id names"));
            }
            if picture_url(db, owner, &id).await?.is_some() {
                return Ok(true);
            }
            files.insert(String::new(), bytes);
        }
        Some(name @ (RIG_MANIFEST_FILE | RIG_ATLAS_FILE)) => {
            if assets::rig_ref(&id).await.is_some() {
                return Ok(true);
            }
            let held = holding(&id);
            let failed = |error: std::io::Error| Error::Refused(error.to_string());
            tokio::fs::create_dir_all(&held).await.map_err(failed)?;
            let temp = held.join(format!(".{}.tmp", uuid::Uuid::new_v4().simple()));
            tokio::fs::write(&temp, &bytes).await.map_err(failed)?;
            tokio::fs::rename(&temp, held.join(name))
                .await
                .map_err(failed)?;
            for part in [RIG_MANIFEST_FILE, RIG_ATLAS_FILE] {
                match tokio::fs::read(held.join(part)).await {
                    Ok(bytes) => {
                        files.insert(part.to_string(), bytes);
                    }
                    Err(_) => return Ok(false),
                }
            }
            let manifest: myriad_merope::RigManifest =
                serde_json::from_slice(&files[RIG_MANIFEST_FILE])
                    .map_err(|error| Error::Refused(error.to_string()))?;
            if myriad_persona_sync::rig_package_id(&files[RIG_ATLAS_FILE], &manifest)
                .ok()
                .as_deref()
                != Some(id.as_str())
            {
                let _ = tokio::fs::remove_dir_all(&held).await;
                return Err(refused("a rig is not the package its id names"));
            }
        }
        Some(_) => return Err(refused("a rig's files are manifest.json and atlas.png")),
    }
    persona_sync::import::keep_files(db, owner, &BTreeMap::from([(id.clone(), files)]))
        .await
        .map_err(|error| Error::Refused(format!("{error:?}")))?;
    let _ = tokio::fs::remove_dir_all(holding(&id)).await;
    Ok(true)
}

/// Where a rig's files wait for each other.
fn holding(id: &str) -> PathBuf {
    crate::services::data_paths::paths()
        .root
        .join("her_log/assets")
        .join(id)
}
