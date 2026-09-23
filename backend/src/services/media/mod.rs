//! Platform media asset service.
//!
//! Business modules depend on this crate path. This module must not depend on
//! `crate::federation`.
//!
#![allow(dead_code)]

mod access;
mod assets;
mod cite;
mod error;
#[cfg(test)]
mod integration_tests;
pub(crate) mod legacy;
mod maintenance;
mod migration;
mod recovery;
#[cfg(test)]
mod reference_tests;
mod references;
mod scan;
pub(crate) mod serve;
mod store;
#[cfg(test)]
mod test_support;
mod types;
pub(crate) mod upgrade;
mod urls;
mod validate;

pub use access::{can_manage, can_read};
pub(crate) use cite::sync_note_history_refs;
pub use cite::{
    bind_ai_task, bind_and_publish_dashboard_layout, bind_and_publish_wallpaper,
    bind_channel_message, bind_consumer, bind_note_draft, bind_note_published, bind_persona,
    bind_rss_item, bind_stickers, clear_note_doc, clear_rss_source, extract_registered_paths,
    publish_asset_ids, publish_cited_media, publish_local_url, references_from_fields,
    references_from_urls, resolve_asset_id,
};
pub use error::MediaError;
pub use legacy::{LegacyClass, LegacyPaths};
pub use maintenance::{maintain, start_upgrade_worker};
pub use migration::{
    MigrationBatch, MigrationJobInput, MigrationStats, migrate_catalog_batch, upsert_job,
};
pub use recovery::{RecoverPlan, plan_recovery};
pub use references::{NewReference, active_count, parse_consumer_type, replace_for_consumer};
pub use scan::catalog_labels_for_assets;
pub use serve::{
    FileServe, NO_STORE, ServeOutcome, resolve_alias_or_legacy, resolve_authenticated_content,
    resolve_public_asset,
};
pub use store::MediaStore;
pub use types::{
    DeleteOutcome, MediaActor, MediaAsset, MediaContext, MediaExposure, MediaScope, MediaSource,
    MediaState, NewMediaBytes, RecoveryReport, task_media_context,
};
pub use urls::{cite_local_path, content_path, public_path, registered_local_path, storage_key};
pub use validate::{ValidatedPayload, allowed_media_mimes, validate_bytes};

use sea_orm::{DatabaseConnection, TransactionTrait};
use uuid::Uuid;

use crate::models::entities::media_assets;
use crate::services::data_paths::DataPaths;

use self::urls::{compatible_url, filename_for_mime};

pub const WRITE_LEASE_SECS: i64 = 600;

pub struct MediaService {
    store: MediaStore,
}

impl MediaService {
    pub fn new(root: std::path::PathBuf) -> Self {
        Self {
            store: MediaStore::new(root),
        }
    }

    pub fn from_data_paths(paths: &DataPaths) -> Self {
        Self::new(paths.media.clone())
    }

    pub fn store(&self) -> &MediaStore {
        &self.store
    }

    pub async fn create_from_bytes(
        &self,
        db: &DatabaseConnection,
        ctx: MediaContext,
        input: NewMediaBytes,
    ) -> Result<MediaAsset, MediaError> {
        ctx.validate()?;
        let payload = validate_bytes(&input.bytes, &input.claimed_mime, input.max_bytes)?;
        if let Some(existing) = assets::find_by_producer(db, &ctx).await? {
            return existing_producer_result(existing);
        }
        let write_token = Uuid::new_v4();
        let row = match assets::insert_staging(
            db,
            &ctx,
            &payload,
            &input.filename,
            input.derived_from_id,
            input.exposure,
            write_token,
            WRITE_LEASE_SECS,
        )
        .await
        {
            Ok(row) => row,
            Err(MediaError::StoreFailed) => {
                if let Some(existing) = assets::find_by_producer(db, &ctx).await? {
                    return existing_producer_result(existing);
                }
                return Err(MediaError::StoreFailed);
            }
            Err(error) => return Err(error),
        };
        let public_id = row.public_id.ok_or(MediaError::StoreFailed)?;
        let key = row.storage_key.clone().ok_or(MediaError::StoreFailed)?;
        let write = async {
            let mut tick = tokio::time::interval(std::time::Duration::from_secs(
                (WRITE_LEASE_SECS / 3) as u64,
            ));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let staged = self.store.stage_bytes(write_token, &input.bytes);
            tokio::pin!(staged);
            loop {
                tokio::select! {
                    result = &mut staged => { result?; break; }
                    _ = tick.tick() => { self.renew_write_lease(db, row.id, write_token).await?; }
                }
            }
            let filename = filename_for_mime(&row.name, &payload.mime, public_id)?;
            let catalog_url = match input.exposure {
                MediaExposure::Public => compatible_url(public_id, &filename),
                MediaExposure::Private => content_path(row.id),
            };
            self.commit_staged(db, row.id, write_token, &key, &catalog_url)
                .await
        }
        .await;
        if let Err(error) = write {
            let _ = self.store.remove_owned_temp(write_token).await;
            return Err(error);
        }
        let saved = assets::find_by_id(db, row.id)
            .await?
            .ok_or(MediaError::Missing)?;
        assets::to_domain(saved, 0)
    }

    async fn commit_staged(
        &self,
        db: &DatabaseConnection,
        id: i32,
        token: Uuid,
        key: &str,
        url: &str,
    ) -> Result<(), MediaError> {
        let txn = db.begin().await?;
        let row = assets::lock_by_id(&txn, id)
            .await?
            .ok_or(MediaError::Missing)?;
        if row.state.as_deref() != Some("staging")
            || row.write_token != Some(token)
            || row
                .write_lease_until
                .is_none_or(|until| until <= chrono::Utc::now())
        {
            return Err(MediaError::NotReady);
        }
        // The recovery claim uses this same row lock. An expired writer may
        // never rename after a recovery worker has taken ownership.
        self.store.publish_staged(key, token).await?;
        if !assets::commit_ready(&txn, id, token, url).await? {
            return Err(MediaError::NotReady);
        }
        txn.commit().await?;
        Ok(())
    }

    /// Persist bytes as a ready asset. `created` is false when `producer_key` hits.
    pub async fn persist_ready_bytes(
        &self,
        db: &DatabaseConnection,
        ctx: MediaContext,
        input: NewMediaBytes,
    ) -> Result<(MediaAsset, bool), MediaError> {
        if let Some(existing) = assets::find_by_producer(db, &ctx).await? {
            if MediaState::parse(existing.state.as_deref().unwrap_or("")).ok()
                == Some(MediaState::Ready)
            {
                return Ok((assets::to_domain(existing, 0)?, false));
            }
        }
        Ok((self.create_from_bytes(db, ctx, input).await?, true))
    }

    pub async fn renew_write_lease(
        &self,
        db: &DatabaseConnection,
        id: i32,
        write_token: Uuid,
    ) -> Result<(), MediaError> {
        if assets::renew_write_lease(db, id, write_token, WRITE_LEASE_SECS).await? {
            Ok(())
        } else {
            Err(MediaError::NotReady)
        }
    }

    pub async fn recover_expired(
        &self,
        db: &DatabaseConnection,
        limit: u32,
    ) -> Result<RecoveryReport, MediaError> {
        recovery::recover_expired(&self.store, db, limit, WRITE_LEASE_SECS).await
    }

    pub async fn migrate_legacy_catalog_batch(
        &self,
        db: &DatabaseConnection,
        paths: &LegacyPaths,
        allowed_origins: &[String],
        after_id: i32,
        limit: u32,
    ) -> Result<MigrationBatch, MediaError> {
        migrate_catalog_batch(&self.store, db, paths, allowed_origins, after_id, limit).await
    }

    pub async fn delete(
        &self,
        db: &DatabaseConnection,
        id: i32,
    ) -> Result<DeleteOutcome, MediaError> {
        let plan = db
            .transaction(|txn| {
                Box::pin(async move {
                    let Some(row) = assets::lock_by_id(txn, id).await? else {
                        return Err(MediaError::Missing);
                    };
                    let state = row
                        .state
                        .as_deref()
                        .ok_or_else(|| MediaError::invalid("Asset is not migrated"))?;
                    match MediaState::parse(state)? {
                        MediaState::Deleted => Ok(DeletePlan::AlreadyGone),
                        MediaState::Deleting => Ok(DeletePlan::Unlink(row.storage_key)),
                        MediaState::Ready => {
                            if !row.references_complete {
                                return Err(MediaError::InUse);
                            }
                            if references::has_active(txn, id, false).await? {
                                return Err(MediaError::InUse);
                            }
                            if !assets::mark_deleting(txn, id).await? {
                                return Err(MediaError::NotReady);
                            }
                            Ok(DeletePlan::Unlink(row.storage_key))
                        }
                        MediaState::Staging | MediaState::Missing => Err(MediaError::NotReady),
                    }
                })
            })
            .await
            .map_err(txn_error)?;
        match plan {
            DeletePlan::AlreadyGone => Ok(DeleteOutcome::Deleted),
            DeletePlan::Unlink(key) => {
                if let Some(key) = key {
                    if let Err(error) = self.store.remove_final(&key).await {
                        tracing::error!(error = ?error, "media delete unlink failed");
                        return Ok(DeleteOutcome::PendingRetry);
                    }
                }
                if assets::mark_deleted(db, id).await? {
                    Ok(DeleteOutcome::Deleted)
                } else {
                    Ok(DeleteOutcome::PendingRetry)
                }
            }
        }
    }

    pub async fn publish(
        &self,
        db: &DatabaseConnection,
        id: i32,
    ) -> Result<MediaAsset, MediaError> {
        db.transaction(|txn| Box::pin(async move { publish_locked(txn, id).await }))
            .await
            .map_err(txn_error)
    }

    pub async fn unpublish(
        &self,
        db: &DatabaseConnection,
        id: i32,
    ) -> Result<MediaAsset, MediaError> {
        db.transaction(|txn| {
            Box::pin(async move {
                let Some(row) = assets::lock_by_id(txn, id).await? else {
                    return Err(MediaError::Missing);
                };
                if MediaState::parse(row.state.as_deref().unwrap_or("")).ok()
                    != Some(MediaState::Ready)
                {
                    return Err(MediaError::NotReady);
                }
                if references::has_active(txn, id, true).await? {
                    return Err(MediaError::PublicInUse);
                }
                assets::mark_private(txn, id, &content_path(id)).await?;
                let saved = assets::find_by_id(txn, id)
                    .await?
                    .ok_or(MediaError::Missing)?;
                assets::to_domain(saved, references::active_count(txn, id).await?)
            })
        })
        .await
        .map_err(txn_error)
    }
}

enum DeletePlan {
    AlreadyGone,
    Unlink(Option<String>),
}

async fn publish_locked(
    txn: &impl sea_orm::ConnectionTrait,
    id: i32,
) -> Result<MediaAsset, MediaError> {
    let Some(row) = assets::lock_by_id(txn, id).await? else {
        return Err(MediaError::Missing);
    };
    if MediaState::parse(row.state.as_deref().unwrap_or("")).ok() != Some(MediaState::Ready) {
        return Err(MediaError::NotReady);
    }
    let public_id = row.public_id.ok_or(MediaError::NotReady)?;
    let filename = filename_for_mime(&row.name, &row.mime, public_id)?;
    assets::mark_public(txn, id, &compatible_url(public_id, &filename)).await?;
    let saved = assets::find_by_id(txn, id)
        .await?
        .ok_or(MediaError::Missing)?;
    assets::to_domain(saved, references::active_count(txn, id).await?)
}

fn existing_producer_result(row: media_assets::Model) -> Result<MediaAsset, MediaError> {
    let state = row.state.as_deref().unwrap_or("");
    match MediaState::parse(state) {
        Ok(MediaState::Ready) => assets::to_domain(row, 0),
        Ok(MediaState::Staging) => Err(MediaError::NotReady),
        _ => Err(MediaError::conflict("Producer key already used")),
    }
}

fn txn_error(err: sea_orm::TransactionError<MediaError>) -> MediaError {
    match err {
        sea_orm::TransactionError::Connection(err) => MediaError::from(err),
        sea_orm::TransactionError::Transaction(err) => err,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_sources_do_not_import_federation() {
        for src in [
            include_str!("mod.rs"),
            include_str!("access.rs"),
            include_str!("assets.rs"),
            include_str!("error.rs"),
            include_str!("legacy.rs"),
            include_str!("migration.rs"),
            include_str!("recovery.rs"),
            include_str!("references.rs"),
            include_str!("cite.rs"),
            include_str!("scan.rs"),
            include_str!("serve.rs"),
            include_str!("store.rs"),
            include_str!("types.rs"),
            include_str!("urls.rs"),
            include_str!("validate.rs"),
        ] {
            for line in src.lines() {
                let trimmed = line.trim();
                if trimmed.starts_with("//") || trimmed.starts_with("//!") {
                    continue;
                }
                assert!(
                    !trimmed.contains(concat!("use crate", "::", "federation")),
                    "media service must not import federation: {trimmed}"
                );
            }
        }
    }

    #[test]
    fn error_codes_are_stable_and_redact_paths() {
        let err = MediaError::StoreFailed.into_app_error();
        assert_eq!(err.code(), Some("MEDIA_STORE_FAILED"));
        let json = err.to_json().to_string();
        assert!(!json.contains("/data"));
        assert!(!json.contains("storage_key"));
    }

    #[test]
    fn allowed_mimes_keep_current_upload_surface() {
        let mimes: Vec<_> = allowed_media_mimes().collect();
        assert_eq!(
            mimes,
            vec![
                "image/jpeg",
                "image/png",
                "image/gif",
                "image/webp",
                "video/mp4",
                "video/webm",
                "video/quicktime",
            ]
        );
    }

    #[test]
    fn user_context_rejects_owner_zero() {
        assert!(MediaActor::user(0).is_err());
        let actor = MediaActor {
            user_id: Some(0),
            is_admin: false,
        };
        assert!(MediaContext::user(actor, MediaSource::Upload).is_err());
    }

    #[test]
    fn task_media_context_uses_subject_not_worker() {
        let ctx = task_media_context(7, 1);
        assert_eq!(ctx.scope, MediaScope::User);
        assert_eq!(ctx.actor.user_id, Some(7));
        assert!(!ctx.actor.is_admin);
        assert_eq!(ctx.source, MediaSource::Generated);
        let site = task_media_context(0, 3);
        assert_eq!(site.scope, MediaScope::Site);
        assert_eq!(site.actor.user_id, Some(3));
        assert!(site.actor.is_admin);
    }

    #[tokio::test]
    async fn postgres_create_recover_delete_when_configured() {
        let Some(fixture) = super::test_support::Fixture::new().await else {
            return;
        };
        let db = fixture.db.clone();

        let root = std::env::temp_dir().join(format!("myriad-media-pg-{}", Uuid::new_v4()));
        let service = MediaService::new(root.clone());
        let png = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode("iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAACXBIWXMAAAPoAAAD6AG1e1JrAAAADklEQVQImWNw6fj/H4QBFnsFlbfmtiMAAAAASUVORK5CYII=")
                .unwrap()
        };
        let ctx = MediaContext::site(MediaActor::admin(1).unwrap(), MediaSource::Upload);
        let created = service
            .create_from_bytes(
                &db,
                ctx,
                NewMediaBytes {
                    bytes: png.clone().into(),
                    claimed_mime: "image/png".into(),
                    filename: "shot.png".into(),
                    max_bytes: 1024 * 1024,
                    derived_from_id: None,
                    exposure: MediaExposure::Private,
                },
            )
            .await
            .expect("create asset");
        assert_eq!(created.state, MediaState::Ready);
        assert_eq!(created.exposure, MediaExposure::Private);
        assert!(created.references_complete);
        assert!(created.checksum_sha256.is_some());

        let asset_id = created.id;
        db.transaction(|txn| {
            Box::pin(async move {
                replace_for_consumer(
                    txn,
                    "note_draft",
                    "1",
                    &[NewReference {
                        asset_id,
                        slot: "cover".into(),
                        requires_public: false,
                        expires_at: None,
                    }],
                )
                .await
            })
        })
        .await
        .expect("bind reference");
        let blocked = service.delete(&db, created.id).await.expect_err("in use");
        assert_eq!(blocked, MediaError::InUse);
        db.transaction(|txn| {
            Box::pin(async move { replace_for_consumer(txn, "note_draft", "1", &[]).await })
        })
        .await
        .expect("clear reference");
        let store = service.store();
        let owner = MediaActor::admin(1).unwrap();
        let stranger = MediaActor::user(8).unwrap();
        match resolve_authenticated_content(&db, store, created.id, &owner)
            .await
            .expect("owner read")
        {
            ServeOutcome::File(file) => {
                assert_eq!(file.mime, "image/png");
                assert_eq!(file.cache_control, NO_STORE);
                let bytes = tokio::fs::read(&file.path)
                    .await
                    .expect("read private bytes");
                assert_eq!(bytes, png);
            }
            other => panic!("expected private file, got {other:?}"),
        }
        assert!(matches!(
            resolve_authenticated_content(&db, store, created.id, &stranger)
                .await
                .expect("stranger read"),
            ServeOutcome::NotFound { no_store: true }
        ));
        assert!(created.public_path.is_none());
        assert!(matches!(
            resolve_public_asset(&db, store, created.public_id, "shot.png")
                .await
                .expect("public while private"),
            ServeOutcome::NotFound { no_store: true }
        ));

        let published = service.publish(&db, created.id).await.expect("publish");
        assert_eq!(published.exposure, MediaExposure::Public);
        assert!(published.public_path.is_some());
        let public_name = published
            .public_path
            .as_deref()
            .and_then(|path| path.rsplit('/').next())
            .expect("public filename");
        match resolve_public_asset(&db, store, published.public_id, public_name)
            .await
            .expect("public read")
        {
            ServeOutcome::File(file) => {
                assert_eq!(file.cache_control, super::serve::PUBLIC_CACHE_CONTROL)
            }
            other => panic!("expected public file, got {other:?}"),
        }
        assert!(matches!(
            resolve_public_asset(&db, store, published.public_id, "../secret.png")
                .await
                .expect("traversal"),
            ServeOutcome::NotFound { no_store: true }
        ));

        db.transaction(|txn| {
            Box::pin(async move {
                replace_for_consumer(
                    txn,
                    "note_published",
                    "9",
                    &[NewReference {
                        asset_id,
                        slot: "body:0".into(),
                        requires_public: true,
                        expires_at: None,
                    }],
                )
                .await
            })
        })
        .await
        .expect("bind public reference");
        assert_eq!(
            service
                .unpublish(&db, created.id)
                .await
                .expect_err("public in use"),
            MediaError::PublicInUse
        );
        db.transaction(|txn| {
            Box::pin(async move { replace_for_consumer(txn, "note_published", "9", &[]).await })
        })
        .await
        .expect("clear public reference");
        let unpublished = service.unpublish(&db, created.id).await.expect("unpublish");
        assert_eq!(unpublished.exposure, MediaExposure::Private);
        assert!(unpublished.first_published_at.is_some());
        assert!(unpublished.public_path.is_none());

        let outcome = service.delete(&db, created.id).await.expect("delete");
        assert_eq!(outcome, DeleteOutcome::Deleted);
        assert!(matches!(
            resolve_public_asset(&db, store, created.public_id, public_name)
                .await
                .expect("deleted public"),
            ServeOutcome::NotFound { no_store: true }
        ));
        let _ = tokio::fs::remove_dir_all(root).await;
        fixture.close().await;
    }

    #[test]
    fn federation_upload_no_longer_best_effort_registers() {
        let src = include_str!("../../api/federation/social.rs");
        let prod = src.split("mod tests").next().unwrap_or(src);
        assert!(prod.contains("persist_federation_upload"));
        assert!(!prod.contains("media_catalog::register"));
        assert!(prod.contains("MediaService"));
    }

    #[test]
    fn legacy_write_paths_are_retired() {
        let federation_media = include_str!("../../federation/content/media.rs");
        assert!(!federation_media.contains(concat!("store_federation", "_media")));
        let orch = include_str!("../../db/schema_check/orchestrator.rs");
        assert!(!orch.contains(concat!("backfill", "_federation")));
        let catalog = include_str!("../media_catalog.rs");
        assert!(!catalog.contains(concat!("pub async fn ", "register(")));
        let cache = include_str!("../image_cache.rs");
        assert!(!cache.contains(concat!("store_bytes", "_with_status")));
        assert!(cache.contains("store_cache_bytes"));
    }
}
