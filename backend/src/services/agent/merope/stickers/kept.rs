//! Stickers people sent in groups that she kept to send herself: one she
//! would want, as she is, when she first looked at it (see
//! `myriad_merope::seeing::sticker_prompt`). One only that group would get
//! is sent only there; the rest anywhere. Kept as a picture of their own,
//! since the app's link to it does not last; let go when she has not sent
//! one for a while.

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, Condition, DatabaseConnection, EntityTrait, PaginatorTrait,
    QueryFilter, Set,
};

use crate::models::entities::merope_stickers::{self, Column, Entity, Model};
use crate::services::image_generation::{self, GeneratedImage, GeneratedImageSource};

/// What a kept sticker is drawn as: not her, so whatever she looks like.
pub(super) const KEPT: &str = "kept";
/// Kept stickers still in use, at most.
const AT_MOST: u64 = 100;
/// One kept and not sent for this long is let go.
const FADES_AFTER_DAYS: i64 = 60;
/// Kept stickers are no larger than this across.
const EDGE: u32 = 512;

/// The kept stickers still in use: kept or sent lately.
pub(super) fn in_use() -> Condition {
    let since = Utc::now().fixed_offset() - chrono::Duration::days(FADES_AFTER_DAYS);
    Condition::all().add(Column::KeptFrom.is_not_null()).add(
        Condition::any()
            .add(Column::CreatedAt.gte(since))
            .add(Column::LastSentAt.gte(since)),
    )
}

/// Keep the sticker someone sent in the group `venue` (as sessions keep it),
/// when she would: what she saw of it, and its bytes. Billed to `owner`'s
/// media.
pub async fn keep(
    db: &DatabaseConnection,
    owner: i32,
    venue: &str,
    seen: &myriad_merope::seeing::Seen,
    bytes: Vec<u8>,
) -> Option<Model> {
    if !seen.keep {
        return None;
    }
    let meaning = seen.says.clone().unwrap_or_else(|| seen.what.clone());
    let already = Entity::find()
        .filter(Column::KeptFrom.is_not_null())
        .filter(Column::Picture.eq(seen.what.as_str()))
        .filter(Column::Meaning.eq(meaning.as_str()))
        .count(db)
        .await
        .unwrap_or(0);
    if already > 0 {
        return None;
    }
    if Entity::find()
        .filter(in_use())
        .count(db)
        .await
        .unwrap_or(AT_MOST)
        >= AT_MOST
    {
        tracing::info!("[Merope] has as many stickers kept as she uses");
        return None;
    }
    let png = tokio::task::spawn_blocking(move || as_png(&bytes))
        .await
        .ok()
        .flatten()?;
    let id = uuid::Uuid::new_v4().simple().to_string();
    let actor = crate::services::media::MediaActor::admin(owner).ok()?;
    let persisted = image_generation::persist_generated_with_status(
        db,
        crate::services::media::MediaContext::site(
            actor,
            crate::services::media::MediaSource::Kept,
        )
        .with_producer_key(format!("merope-kept-sticker:{id}")),
        GeneratedImage {
            source: GeneratedImageSource::Inline(png),
            media_type: "image/png".to_string(),
            width: 0,
            height: 0,
        },
        "sticker",
        crate::services::media::MediaExposure::Public,
    )
    .await
    .ok()?;
    let row = merope_stickers::ActiveModel {
        id: Set(id),
        picture: Set(seen.what.clone()),
        meaning: Set(meaning),
        venue: Set(seen.only_there.then(|| venue.to_string())),
        asset_id: Set(persisted.url),
        identity: Set(KEPT.to_string()),
        sent: Set(0),
        last_sent_at: Set(None),
        created_at: Set(Utc::now().fixed_offset()),
        kept_from: Set(Some(venue.to_string())),
    };
    match row.insert(db).await {
        Ok(model) => {
            tracing::info!(%venue, only_there = seen.only_there, "[Merope] kept a sticker someone sent");
            Some(model)
        }
        Err(error) => {
            tracing::warn!(%error, "[Merope] could not keep a sticker someone sent");
            None
        }
    }
}

/// Its first frame as PNG, no larger than `EDGE` across.
fn as_png(bytes: &[u8]) -> Option<Vec<u8>> {
    let image = image::load_from_memory(bytes).ok()?;
    let image = if image.width().max(image.height()) > EDGE {
        image.resize(EDGE, EDGE, image::imageops::FilterType::Triangle)
    } else {
        image
    };
    let mut png = Vec::new();
    image
        .to_rgba8()
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .ok()?;
    Some(png)
}

#[cfg(test)]
mod tests {
    /// The whole way, on its own schema with the site's model: a sticker
    /// someone sent in a group is looked at as she is, kept with its picture,
    /// and offered to her where it belongs. The picture it keeps is removed
    /// from the media store again at the end.
    /// `MYRIAD_MEDIA_TEST_DATABASE_URL=… cargo test --bin myriad-backend a_sticker_someone_sent -- --ignored --nocapture`
    #[tokio::test]
    #[ignore = "spends on the site's model"]
    async fn a_sticker_someone_sent_is_kept_and_offered() {
        use sea_orm::ConnectionTrait;
        let url = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL").expect("a test database");
        let config_db = super::super::super::super::semantic_eval::load_configured_lite().await;
        config_db.close().await.ok();
        let isolated = crate::db::IsolatedSchema::migrated(&url, "kept_sticker").await;
        let db = isolated.db.clone();
        let owner: i32 = db
            .query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "INSERT INTO users (username, display_name, is_admin, is_owner) \
                 VALUES ('owner', 'owner', true, true) RETURNING id",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "id")
            .unwrap();
        crate::services::process_db::set_process_database(db.clone());
        let bytes = std::fs::read(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("assets/merope/sticker-style-reference.webp"),
        )
        .unwrap();
        let soul = "一个爱吐槽、喜欢可爱东西的女生";
        let venue = "onebot:424242";
        let seen = super::super::super::seeing::look(
            &db,
            owner,
            "qq:test-sticker",
            bytes.clone(),
            None,
            Some(soul),
        )
        .await
        .expect("looked at");
        println!("{seen:?}");
        assert!(seen.keep, "{seen:?}");
        let kept = super::keep(&db, owner, venue, &seen, bytes.clone())
            .await
            .expect("kept");
        assert_eq!(kept.kept_from.as_deref(), Some(venue));
        assert_eq!(kept.identity, super::KEPT);
        let (picture, _) = super::super::picture(&kept)
            .await
            .expect("its picture is kept");
        assert!(image::load_from_memory(&picture).is_ok());
        assert!(
            super::keep(&db, owner, venue, &seen, bytes).await.is_none(),
            "kept once"
        );
        let here = super::super::offered(&db, Some(venue), "哈哈哈").await;
        assert!(
            here.iter().any(|sticker| sticker.id == kept.id),
            "offered where she kept it"
        );
        let elsewhere = super::super::offered(&db, None, "哈哈哈").await;
        assert_eq!(
            elsewhere.iter().any(|sticker| sticker.id == kept.id),
            !seen.only_there,
            "anywhere unless only that group would get it"
        );
        assert_eq!(
            super::super::made_this_month(&db).await,
            0,
            "not one she made"
        );
        let assets: Vec<i32> = db
            .query_all_raw(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT id FROM media_assets",
            ))
            .await
            .unwrap()
            .iter()
            .filter_map(|row| row.try_get("", "id").ok())
            .collect();
        assert_eq!(assets.len(), 1);
        for id in assets {
            crate::services::media::MediaService::from_data_paths(
                crate::services::data_paths::paths(),
            )
            .delete(&db, id)
            .await
            .unwrap();
        }
        isolated.drop().await;
    }

    #[test]
    fn a_kept_sticker_is_its_first_frame_small() {
        let mut gif = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::new(800, 400))
            .write_to(&mut std::io::Cursor::new(&mut gif), image::ImageFormat::Gif)
            .unwrap();
        let png = super::as_png(&gif).unwrap();
        let image = image::load_from_memory(&png).unwrap();
        assert_eq!((image.width(), image.height()), (512, 256));
        assert_eq!(super::as_png(b"not a picture"), None);
    }
}
