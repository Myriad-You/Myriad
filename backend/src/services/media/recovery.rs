//! Recover expired staging rows. Claim with SKIP LOCKED, inspect files off-lock.

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use uuid::Uuid;

use crate::models::entities::media_assets;

use super::assets;
use super::error::MediaError;
use super::store::MediaStore;
use super::types::RecoveryReport;
use super::urls::filename_for_mime;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoverPlan {
    CompleteReady,
    CleanupMissing,
}

pub fn plan_recovery(final_exists: bool, checksum_matches: bool) -> RecoverPlan {
    if final_exists && checksum_matches {
        RecoverPlan::CompleteReady
    } else {
        RecoverPlan::CleanupMissing
    }
}

struct ClaimedStaging {
    row: media_assets::Model,
    prev_token: Option<Uuid>,
}

async fn claim_one(
    db: &impl ConnectionTrait,
    new_token: Uuid,
    lease_secs: i64,
) -> Result<Option<ClaimedStaging>, MediaError> {
    let Some(result) = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
WITH selected AS MATERIALIZED (
    SELECT id, write_token AS prev_token
    FROM media_assets
    WHERE state = 'staging'
      AND write_lease_until IS NOT NULL
      AND write_lease_until < $3::timestamptz
    ORDER BY id
    LIMIT 1
    FOR UPDATE SKIP LOCKED
)
UPDATE media_assets AS asset
SET write_token = $1,
    write_lease_until = $3::timestamptz + make_interval(secs => $2::double precision),
    updated_at = $3::timestamptz
FROM selected
WHERE asset.id = selected.id
RETURNING asset.id, selected.prev_token
"#,
            [
                new_token.into(),
                lease_secs.into(),
                crate::clock::now().into(),
            ],
        ))
        .await?
    else {
        return Ok(None);
    };
    let id = result
        .try_get::<i32>("", "id")
        .map_err(|_| MediaError::StoreFailed)?;
    let prev_token = result
        .try_get::<Option<Uuid>>("", "prev_token")
        .ok()
        .flatten();
    let Some(row) = assets::find_by_id(db, id).await? else {
        return Ok(None);
    };
    Ok(Some(ClaimedStaging { row, prev_token }))
}

pub async fn recover_expired(
    store: &MediaStore,
    db: &impl ConnectionTrait,
    limit: u32,
    lease_secs: i64,
) -> Result<RecoveryReport, MediaError> {
    let mut report = RecoveryReport {
        claimed: 0,
        completed: 0,
        cleaned: 0,
    };
    for _ in 0..limit.max(1).min(32) {
        let new_token = Uuid::new_v4();
        let Some(claimed) = claim_one(db, new_token, lease_secs).await? else {
            break;
        };
        report.claimed += 1;
        if let Some(prev) = claimed.prev_token {
            store.remove_owned_temp(prev).await?;
        }
        let Some(key) = claimed.row.storage_key.clone() else {
            assets::mark_missing(db, claimed.row.id, new_token).await?;
            report.cleaned += 1;
            continue;
        };
        let actual = store.final_checksum(&key).await?;
        let checksum_matches = match (&claimed.row.checksum_sha256, &actual) {
            (Some(expected), Some(actual_sum)) => expected == actual_sum,
            _ => false,
        };
        match plan_recovery(actual.is_some(), checksum_matches) {
            RecoverPlan::CompleteReady => {
                let public_id = claimed.row.public_id.ok_or(MediaError::StoreFailed)?;
                let filename = filename_for_mime(&claimed.row.name, &claimed.row.mime, public_id)?;
                let catalog = super::urls::compatible_url(public_id, &filename);
                if assets::commit_ready(db, claimed.row.id, new_token, &catalog).await? {
                    report.completed += 1;
                } else {
                    report.cleaned += 1;
                }
            }
            RecoverPlan::CleanupMissing => {
                if actual.is_some() && !checksum_matches {
                    store.remove_final(&key).await?;
                }
                store.remove_owned_temp(new_token).await?;
                assets::mark_missing(db, claimed.row.id, new_token).await?;
                report.cleaned += 1;
            }
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::super::assets;
    use super::*;

    #[test]
    fn complete_only_when_checksum_matches() {
        assert_eq!(plan_recovery(true, true), RecoverPlan::CompleteReady);
        assert_eq!(plan_recovery(true, false), RecoverPlan::CleanupMissing);
        assert_eq!(plan_recovery(false, false), RecoverPlan::CleanupMissing);
        assert_eq!(plan_recovery(false, true), RecoverPlan::CleanupMissing);
    }

    #[test]
    fn recovery_sql_uses_skip_locked_and_does_not_copy() {
        let src = include_str!("recovery.rs");
        assert!(src.contains("FOR UPDATE SKIP LOCKED"));
        assert!(!src.contains(concat!("fs::copy", "(")));
    }

    #[tokio::test]
    async fn postgres_skips_live_lease_and_finishes_expired() {
        let Some(fixture) = super::super::test_support::Fixture::new().await else {
            return;
        };
        let db = fixture.db.clone();

        let png = {
            use base64::Engine;
            base64::engine::general_purpose::STANDARD
                .decode("iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAACXBIWXMAAAPoAAAD6AG1e1JrAAAADklEQVQImWNw6fj/H4QBFnsFlbfmtiMAAAAASUVORK5CYII=")
                .unwrap()
        };
        let payload =
            crate::services::media::validate_bytes(&png, "image/png", 1024 * 1024).expect("png");
        let ctx = crate::services::media::MediaContext::site(
            crate::services::media::MediaActor::admin(1).unwrap(),
            crate::services::media::MediaSource::Upload,
        );
        let root = std::env::temp_dir().join(format!("myriad-media-rec-{}", Uuid::new_v4()));
        let store = MediaStore::new(root.clone());
        let cache =
            std::env::temp_dir().join(format!("myriad-media-cache-missing-{}", Uuid::new_v4()));
        assert!(!cache.exists(), "T18: cache dir must stay absent");

        let live_token = Uuid::new_v4();
        let live = assets::insert_staging(
            &db,
            &ctx,
            &payload,
            "live.png",
            None,
            crate::services::media::MediaExposure::Private,
            live_token,
            600,
        )
        .await
        .expect("live staging");
        let live_key = live.storage_key.clone().expect("live key");
        store
            .publish_bytes(&live_key, live_token, &png)
            .await
            .expect("live bytes");
        let skipped = recover_expired(&store, &db, 8, 600)
            .await
            .expect("skip live");
        assert_eq!(skipped.claimed, 0);
        let still = assets::find_by_id(&db, live.id)
            .await
            .expect("reload live")
            .expect("live row");
        assert_eq!(still.state.as_deref(), Some("staging"));
        assert_eq!(still.write_token, Some(live_token));

        let expired_token = Uuid::new_v4();
        let expired = assets::insert_staging(
            &db,
            &ctx,
            &payload,
            "expired.png",
            None,
            crate::services::media::MediaExposure::Private,
            expired_token,
            600,
        )
        .await
        .expect("expired staging");
        let expired_key = expired.storage_key.clone().expect("expired key");
        store
            .publish_bytes(&expired_key, expired_token, &png)
            .await
            .expect("expired bytes");
        sea_orm::ConnectionTrait::execute_unprepared(
            &db,
            &format!(
                "UPDATE media_assets SET write_lease_until = NOW() - INTERVAL '1 second' WHERE id = {}",
                expired.id
            ),
        )
        .await
        .expect("expire lease");
        let recovered = recover_expired(&store, &db, 8, 600)
            .await
            .expect("recover expired");
        assert_eq!(recovered.claimed, 1);
        assert_eq!(recovered.completed, 1);
        let ready = assets::find_by_id(&db, expired.id)
            .await
            .expect("reload expired")
            .expect("expired row");
        assert_eq!(ready.state.as_deref(), Some("ready"));
        assert!(!cache.exists(), "recovery must not create a cache volume");
        let _ = tokio::fs::remove_dir_all(root).await;
        fixture.close().await;
    }
}
