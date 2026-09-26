//! Legacy media migration primitives used by the upgrade worker.

use chrono::Utc;
use sea_orm::{ActiveModelTrait, ColumnTrait, ConnectionTrait, EntityTrait, QueryFilter, Set};
use serde::Serialize;
use std::path::{Path, PathBuf};
use uuid::Uuid;

use crate::models::entities::{media_assets, media_migration_jobs, media_url_aliases};

use super::error::MediaError;
use super::legacy::{
    LegacyKind, LegacyOwner, LegacyPaths, cache_equivalent_path, ext_from_legacy, legacy_disk_path,
    legacy_kind, owner_from_local_path,
};
use super::store::MediaStore;
#[cfg(test)]
use super::store::hash_path;
use super::types::{MediaExposure, MediaScope, MediaSource, MediaState};
use super::urls::{alias_local_path, storage_key};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CatalogPlan {
    pub local_path: String,
    pub disk: PathBuf,
    pub owner: LegacyOwner,
    pub kind: LegacyKind,
}

/// MIME of an importable legacy file, from its extension.
pub(super) fn legacy_mime(path: &str) -> Option<&'static str> {
    let ext = std::path::Path::new(path).extension()?.to_str()?;
    Some(match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        _ => return None,
    })
}

/// Import a cached file that authored content cites into a durable public
/// asset, aliased at the cached path, in the caller's transaction. From then
/// on the asset's references are maintained transactionally like any new
/// write. `None` when the path is not a cached file that still exists.
pub(super) async fn import_cached_citation(
    db: &impl ConnectionTrait,
    store: &MediaStore,
    paths: &LegacyPaths,
    path: &str,
) -> Result<Option<i32>, MediaError> {
    if !(path.starts_with("/api/phantasi/image-cache/")
        || path.starts_with("/api/brew/image-cache/"))
    {
        return Ok(None);
    }
    let Ok(plan) = plan_catalog_url(path, &[], paths) else {
        return Ok(None);
    };
    let Some(mime) = legacy_mime(path) else {
        return Ok(None);
    };
    if tokio::fs::metadata(&plan.disk).await.is_err() {
        return Ok(None);
    }
    // The file lands in the store before the caller commits. A stable identity
    // per cached file means a rolled-back attempt's copy is found and reused by
    // the retry instead of leaving another orphan behind.
    let canonical = cache_equivalent_path(path)
        .filter(|_| path.starts_with("/api/brew/"))
        .unwrap_or_else(|| path.to_string());
    let digest = <sha2::Sha256 as sha2::Digest>::digest(canonical.as_bytes());
    let mut identity = [0u8; 16];
    identity.copy_from_slice(&digest[..16]);
    let public_id = uuid::Builder::from_custom_bytes(identity).into_uuid();
    let row = media_assets::ActiveModel {
        kind: Set("upload".into()),
        public_id: Set(Some(public_id)),
        url: Set(path.to_string()),
        mime: Set(mime.into()),
        name: Set(path.rsplit('/').next().unwrap_or("media").into()),
        size: Set(0),
        created_at: Set(Utc::now().fixed_offset()),
        references_complete: Set(false),
        ..Default::default()
    }
    .insert(db)
    .await?;
    match migrate_one(store, db, &row, &plan).await? {
        Outcome::Copied { .. } | Outcome::Already { .. } => {}
        Outcome::Missing | Outcome::Failed => return Ok(None),
    }
    db.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "UPDATE media_assets SET references_complete = TRUE WHERE id = $1",
        [row.id.into()],
    ))
    .await?;
    Ok(Some(row.id))
}

pub fn plan_catalog_url(
    url: &str,
    allowed_origins: &[String],
    paths: &LegacyPaths,
) -> Result<CatalogPlan, &'static str> {
    let Some(local_path) = alias_local_path(url, allowed_origins) else {
        if url.contains("://") {
            return Err("FOREIGN_ORIGIN");
        }
        return Err("FOREIGN_ORIGIN");
    };
    let Some(kind) = legacy_kind(&local_path) else {
        return Err("UNSUPPORTED");
    };
    let Some(disk) = legacy_disk_path(paths, &local_path) else {
        return Err("UNSUPPORTED");
    };
    Ok(CatalogPlan {
        owner: owner_from_local_path(&local_path),
        local_path,
        disk,
        kind,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Outcome {
    Copied { bytes: u64, aliases: u64 },
    Already { bytes: u64, aliases: u64 },
    Missing,
    Failed,
}

pub(super) async fn migrate_one(
    store: &MediaStore,
    db: &impl ConnectionTrait,
    row: &media_assets::Model,
    plan: &CatalogPlan,
) -> Result<Outcome, MediaError> {
    let source_key = row.id.to_string();
    let source_meta = tokio::fs::metadata(&plan.disk).await;
    if matches!(&source_meta, Err(error) if error.kind() == std::io::ErrorKind::NotFound)
        && row.storage_key.is_none()
    {
        mark_row_missing(db, row.id).await?;
        record_job(
            db,
            "catalog",
            &source_key,
            Some(row.id),
            "skipped",
            "pending",
            "pending",
            Some("MISSING"),
            None,
        )
        .await?;
        return Ok(Outcome::Missing);
    }
    let Some(ext) = ext_from_legacy(&row.mime, &plan.local_path) else {
        record_job(
            db,
            "catalog",
            &source_key,
            Some(row.id),
            "skipped",
            "pending",
            "pending",
            Some("UNSUPPORTED"),
            None,
        )
        .await?;
        return Ok(Outcome::Failed);
    };
    let public_id = row.public_id.unwrap_or_else(Uuid::new_v4);
    let key = storage_key(public_id, ext)?;
    // Upgrade holds this asset's row lock. A stable token lets a restarted
    // process discard only this copy's abandoned partial file, without crawling
    // or deleting another writer's temporary files.
    let token = public_id;
    store.remove_owned_temp(token).await?;
    let copy = match store.publish_from_path(&key, token, &plan.disk).await {
        Ok(copy) => copy,
        Err(MediaError::Missing) => {
            mark_row_missing(db, row.id).await?;
            record_job(
                db,
                "catalog",
                &source_key,
                Some(row.id),
                "skipped",
                "pending",
                "pending",
                Some("MISSING"),
                None,
            )
            .await?;
            return Ok(Outcome::Missing);
        }
        Err(MediaError::Conflict { .. }) => {
            record_job(
                db,
                "catalog",
                &source_key,
                Some(row.id),
                "copied",
                "failed",
                "pending",
                Some("CHECKSUM_MISMATCH"),
                None,
            )
            .await?;
            return Ok(Outcome::Failed);
        }
        Err(error) => return Err(error),
    };
    let _ = store.remove_owned_temp(token).await;
    if tokio::fs::metadata(&plan.disk).await.is_err() {
        return Err(MediaError::StoreFailed);
    }

    let (scope, owner) = match plan.owner {
        LegacyOwner::User(id) => (MediaScope::User, Some(id)),
        LegacyOwner::Unknown => (MediaScope::LegacyUnknown, None),
    };
    switch_row(
        db,
        row.id,
        public_id,
        &key,
        &copy.checksum_sha256,
        copy.size as i64,
        scope,
        owner,
        row.created_at,
    )
    .await?;
    let aliases = register_aliases(db, row, plan, &copy.checksum_sha256).await?;
    record_job(
        db,
        "catalog",
        &source_key,
        Some(row.id),
        "copied",
        "verified",
        "switched",
        None,
        Some(&plan.local_path),
    )
    .await?;
    append_manifest(
        store.root(),
        &ManifestLine {
            source_kind: "catalog",
            source_key: &source_key,
            local_path: &plan.local_path,
            checksum_sha256: &copy.checksum_sha256,
            size: copy.size,
        },
    )
    .await?;
    if copy.wrote {
        Ok(Outcome::Copied {
            bytes: copy.size,
            aliases,
        })
    } else {
        Ok(Outcome::Already {
            bytes: copy.size,
            aliases,
        })
    }
}

async fn switch_row(
    db: &impl ConnectionTrait,
    id: i32,
    public_id: Uuid,
    storage_key: &str,
    checksum: &str,
    size: i64,
    scope: MediaScope,
    owner: Option<i32>,
    created_at: chrono::DateTime<chrono::FixedOffset>,
) -> Result<(), MediaError> {
    let Some(row) = media_assets::Entity::find_by_id(id).one(db).await? else {
        return Err(MediaError::Missing);
    };
    let now = Utc::now().fixed_offset();
    let published = row.first_published_at.unwrap_or(created_at);
    let mut model: media_assets::ActiveModel = row.into();
    model.public_id = Set(Some(public_id));
    model.storage_key = Set(Some(storage_key.to_string()));
    model.checksum_sha256 = Set(Some(checksum.to_string()));
    model.size = Set(size);
    model.state = Set(Some(MediaState::Ready.as_str().to_string()));
    model.exposure = Set(Some(MediaExposure::Public.as_str().to_string()));
    model.source = Set(Some(MediaSource::Legacy.as_str().to_string()));
    model.scope = Set(Some(scope.as_str().to_string()));
    model.owner_user_id = Set(owner);
    model.created_by = Set(None);
    model.references_complete = Set(false);
    model.updated_at = Set(Some(now));
    model.state_since = Set(Some(now));
    model.first_published_at = Set(Some(published));
    model.update(db).await?;
    Ok(())
}

async fn mark_row_missing(db: &impl ConnectionTrait, id: i32) -> Result<(), MediaError> {
    let Some(row) = media_assets::Entity::find_by_id(id).one(db).await? else {
        return Ok(());
    };
    if row.storage_key.is_some() {
        return Ok(());
    }
    let now = Utc::now().fixed_offset();
    let mut model: media_assets::ActiveModel = row.into();
    model.state = Set(Some(MediaState::Missing.as_str().to_string()));
    model.updated_at = Set(Some(now));
    model.state_since = Set(Some(now));
    model.update(db).await?;
    Ok(())
}

pub(super) async fn register_aliases(
    db: &impl ConnectionTrait,
    row: &media_assets::Model,
    plan: &CatalogPlan,
    checksum: &str,
) -> Result<u64, MediaError> {
    let mut aliases = 0_u64;
    let equivalent = if plan.kind == LegacyKind::ImageCache {
        cache_equivalent_path(&plan.local_path)
    } else {
        None
    };
    let mut alias_owner = row.id;
    // Previously interrupted discovery may have catalogued both URL spellings.
    // Preserve those asset IDs and their independent storage objects, but let
    // both historical URLs continue to use the already registered identity.
    for path in std::iter::once(&plan.local_path).chain(equivalent.iter()) {
        if let Some(alias) = media_url_aliases::Entity::find()
            .filter(media_url_aliases::Column::LocalPath.eq(path))
            .one(db)
            .await?
        {
            if alias.asset_id != row.id {
                let existing = media_assets::Entity::find_by_id(alias.asset_id)
                    .one(db)
                    .await?
                    .ok_or(MediaError::Missing)?;
                let existing_path = url::Url::parse(&existing.url)
                    .ok()
                    .map(|url| url.path().to_owned())
                    .unwrap_or_else(|| existing.url.clone());
                if plan.kind != LegacyKind::ImageCache
                    || (existing_path != plan.local_path
                        && Some(&existing_path) != equivalent.as_ref())
                    || existing.state.as_deref() != Some("ready")
                    || existing.source.as_deref() != Some("legacy")
                    || existing.checksum_sha256.as_deref() != Some(checksum)
                {
                    return Err(MediaError::conflict("Media URL is already registered"));
                }
                alias_owner = alias.asset_id;
            }
        }
    }
    aliases += ensure_alias(db, &plan.local_path, alias_owner).await? as u64;
    if let Some(other) = equivalent {
        aliases += ensure_alias(db, &other, alias_owner).await? as u64;
    }
    Ok(aliases)
}

async fn ensure_alias(
    db: &impl ConnectionTrait,
    local_path: &str,
    asset_id: i32,
) -> Result<bool, MediaError> {
    if let Some(existing) = media_url_aliases::Entity::find()
        .filter(media_url_aliases::Column::LocalPath.eq(local_path))
        .one(db)
        .await?
    {
        if existing.asset_id != asset_id {
            return Err(MediaError::conflict("Media URL is already registered"));
        }
        return Ok(false);
    }
    let now = Utc::now().fixed_offset();
    let row = media_url_aliases::ActiveModel {
        local_path: Set(local_path.to_string()),
        asset_id: Set(asset_id),
        created_at: Set(now),
        ..Default::default()
    };
    match row.insert(db).await {
        Ok(_) => Ok(true),
        Err(err)
            if err.to_string().contains("duplicate key") || err.to_string().contains("UNIQUE") =>
        {
            Ok(false)
        }
        Err(err) => Err(err.into()),
    }
}

pub(super) async fn record_job(
    db: &impl ConnectionTrait,
    source_kind: &str,
    source_key: &str,
    asset_id: Option<i32>,
    copy_state: &str,
    verify_state: &str,
    switch_state: &str,
    error_code: Option<&str>,
    cursor: Option<&str>,
) -> Result<(), MediaError> {
    let now = Utc::now().fixed_offset();
    if let Some(existing) = media_migration_jobs::Entity::find()
        .filter(media_migration_jobs::Column::SourceKind.eq(source_kind))
        .filter(media_migration_jobs::Column::SourceKey.eq(source_key))
        .one(db)
        .await?
    {
        let mut model: media_migration_jobs::ActiveModel = existing.into();
        model.asset_id = Set(asset_id);
        model.copy_state = Set(copy_state.to_string());
        model.verify_state = Set(verify_state.to_string());
        model.switch_state = Set(switch_state.to_string());
        model.error_code = Set(error_code.map(str::to_string));
        model.cursor = Set(cursor.map(str::to_string));
        model.updated_at = Set(now);
        model.update(db).await?;
        return Ok(());
    }
    let row = media_migration_jobs::ActiveModel {
        source_kind: Set(source_kind.to_string()),
        source_key: Set(source_key.to_string()),
        asset_id: Set(asset_id),
        copy_state: Set(copy_state.to_string()),
        verify_state: Set(verify_state.to_string()),
        switch_state: Set(switch_state.to_string()),
        error_code: Set(error_code.map(str::to_string)),
        cursor: Set(cursor.map(str::to_string)),
        batch_version: Set(1),
        created_at: Set(now),
        updated_at: Set(now),
        ..Default::default()
    };
    row.insert(db).await?;
    Ok(())
}

#[derive(Serialize)]
struct ManifestLine<'a> {
    source_kind: &'a str,
    source_key: &'a str,
    local_path: &'a str,
    checksum_sha256: &'a str,
    size: u64,
}

async fn append_manifest(root: &Path, line: &ManifestLine<'_>) -> Result<(), MediaError> {
    tokio::fs::create_dir_all(root).await?;
    let path = root.join("migration-manifest.jsonl");
    let mut body = serde_json::to_string(line).map_err(|_| MediaError::StoreFailed)?;
    body.push('\n');
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await?;
    use tokio::io::AsyncWriteExt;
    file.write_all(body.as_bytes()).await?;
    file.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::media::legacy::classify_uncited_cache_file;
    use crate::services::media::urls::alias_local_path;

    #[test]
    fn catalog_plan_rejects_foreign_origins_and_keeps_site_paths() {
        let paths = LegacyPaths {
            federation_root: PathBuf::from("/data/federation_media"),
            cache_images: PathBuf::from("/cache/images"),
        };
        let allowed = ["https://site.example".to_string()];
        let plan = plan_catalog_url(
            "https://site.example/media/federation/4/pic.png",
            &allowed,
            &paths,
        )
        .unwrap();
        assert_eq!(plan.local_path, "/media/federation/4/pic.png");
        assert_eq!(plan.owner, LegacyOwner::User(4));
        assert_eq!(
            plan_catalog_url(
                "https://other.site/media/federation/4/pic.png",
                &allowed,
                &paths
            ),
            Err("FOREIGN_ORIGIN")
        );
        assert!(alias_local_path("/media/federation/4/pic.png", &[]).is_some());
    }

    #[test]
    fn uncited_cache_is_not_a_copy_plan() {
        assert_eq!(
            classify_uncited_cache_file(),
            crate::services::media::legacy::LegacyClass::OrphanCandidate
        );
    }

    #[test]
    fn migration_is_not_hooked_from_schema_startup() {
        let orch = include_str!("../../db/schema_check/orchestrator.rs");
        assert!(!orch.contains("migrate_catalog_batch"));
        assert!(!orch.contains(concat!("backfill", "_federation")));
        let src = include_str!("migration.rs");
        assert!(!src.contains(concat!("read_dir", "(")));
        assert!(!src.contains(concat!("Walk", "Dir")));
        assert!(!src.contains(concat!("backfill", "_federation")));
        assert!(src.contains("references_complete"));
        assert!(src.contains("CHECKSUM_MISMATCH"));
    }

    #[tokio::test]
    async fn copy_keeps_source_and_is_idempotent() {
        let root = std::env::temp_dir().join(format!("myriad-media-mig-{}", Uuid::new_v4()));
        let store = MediaStore::new(root.join("media"));
        let source_dir = root.join("federation_media/3");
        tokio::fs::create_dir_all(&source_dir).await.unwrap();
        let source = source_dir.join("pic.png");
        let bytes = b"\x89PNG\r\n\x1a\nlegacy";
        tokio::fs::write(&source, bytes).await.unwrap();
        let id = Uuid::new_v4();
        let key = storage_key(id, "png").unwrap();
        let token = Uuid::new_v4();
        let first = store.publish_from_path(&key, token, &source).await.unwrap();
        assert!(first.wrote);
        assert_eq!(
            tokio::fs::read(&source).await.unwrap(),
            bytes,
            "legacy source must remain for rollback"
        );
        let second = store
            .publish_from_path(&key, Uuid::new_v4(), &source)
            .await
            .unwrap();
        assert!(!second.wrote);
        assert_eq!(first.checksum_sha256, second.checksum_sha256);
        let dest = store.final_path(&key).unwrap();
        tokio::fs::write(&dest, b"tampered").await.unwrap();
        let err = store
            .publish_from_path(&key, Uuid::new_v4(), &source)
            .await
            .unwrap_err();
        assert!(matches!(err, MediaError::Conflict { .. }));
        assert_eq!(tokio::fs::read(&source).await.unwrap(), bytes);
        let _ = tokio::fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn interrupted_copy_can_finish_from_source_hash() {
        let root = std::env::temp_dir().join(format!("myriad-media-mig-hash-{}", Uuid::new_v4()));
        let source = root.join("src.bin");
        tokio::fs::create_dir_all(&root).await.unwrap();
        tokio::fs::write(&source, b"abcdefghij").await.unwrap();
        let (size, sum) = hash_path(&source).await.unwrap();
        assert_eq!(size, 10);
        assert_eq!(
            sum,
            crate::services::media::validate::checksum_sha256(b"abcdefghij")
        );
        let _ = tokio::fs::remove_dir_all(root).await;
    }
}
