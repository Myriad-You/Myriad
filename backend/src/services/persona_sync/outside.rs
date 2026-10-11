//! Pictures at outside addresses: copied into the site's media library the
//! first time a device reads her, and her data pointed at the copy.
//!
//! Fetched through the outbound guards of the image cache (public addresses
//! only, DNS pinned, no redirects, at most 10 MiB, `image/*`), the bytes
//! checked to be an image, kept as a public media asset under a producer key
//! of the address (fetched once, whoever asks), and the persona's portrait,
//! sticker avatar, outfit portraits and stickers that held the address are
//! rewritten to the copy's. After that nothing of her depends on the outside
//! host: a dead link cannot make the picture unreadable.
//!
//! Lazily, when a snapshot or an asset is asked for: only then does anyone
//! need the copy, and a site with no devices fetches nothing. One that
//! cannot be fetched is tried again after [`PICTURE_TTL`]. Neither the
//! address nor anything in it is logged: an address can carry a signature.
//!
//! The rewrite is the persona's own media-URL rewrite
//! (`merope::rewrite_persona_media_urls`, as a persona save does after it
//! normalizes its media addresses), under the persona lock, then the
//! persona's media bindings. It is the same picture under a new address,
//! not an edit of her: her name, look, portrait lineage and sticker avatar
//! stay as they are, and nothing that crosses changes (a picture crosses by
//! the sha256 of its bytes). A portrait a rig was built from keeps its
//! address, since the rig names its master by it; it is read from the copy.

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex},
    time::Instant,
};

use myriad_persona_sync::canonical::sha256_hex;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ConnectionTrait, DatabaseBackend, DatabaseConnection,
    Statement, TransactionTrait,
};
use serde_json::Value;

use super::assets::{PICTURE_TTL, PICTURES_KEPT, Pictures};
use crate::services::{
    image_cache::ImageCacheService,
    media::{MediaActor, MediaContext, MediaExposure, MediaSource, NewMediaBytes},
};

/// Outside pictures that could not be fetched lately, by the sha256 of
/// their address: a snapshot of many pages does not wait on a dead host
/// once per page.
static UNFETCHABLE: LazyLock<Mutex<HashMap<String, Instant>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
/// One copy at a time: two pages asking for the same address fetch it once.
static COPYING: LazyLock<tokio::sync::Mutex<()>> = LazyLock::new(|| tokio::sync::Mutex::new(()));

/// The address, when it is an outside one (`http`/`https`).
fn outside(url: &str) -> Option<url::Url> {
    let parsed = url::Url::parse(url.trim()).ok()?;
    matches!(parsed.scheme(), "http" | "https").then_some(parsed)
}

/// Fetches an outside picture through the image cache's guards; its bytes
/// and their image type. The cached file is the caller's to remove.
pub(super) async fn download(
    cache: &ImageCacheService,
    url: &str,
) -> Result<(Vec<u8>, &'static str, String), String> {
    let local = match cache.get_cached_url(url).await {
        Some(local) => local,
        None => cache.cache_image(url).await?,
    };
    let (bytes, _) = cache.read_local_public_url(&local).await?;
    let media_type = crate::services::agent::chat_attachments::sniff(&bytes)
        .ok_or_else(|| "not an image".to_string())?;
    Ok((bytes, media_type, local))
}

fn recently_failed(key: &str) -> bool {
    UNFETCHABLE
        .lock()
        .ok()
        .and_then(|failed| failed.get(key).copied())
        .is_some_and(|at| at.elapsed() < PICTURE_TTL)
}

fn failed(key: String) {
    if let Ok(mut failed) = UNFETCHABLE.lock() {
        if failed.len() >= PICTURES_KEPT {
            failed.clear();
        }
        failed.insert(key, Instant::now());
    }
}

fn extension(media_type: &str) -> &'static str {
    match media_type {
        "image/jpeg" => "jpg",
        "image/gif" => "gif",
        "image/webp" => "webp",
        _ => "png",
    }
}

/// The outside picture at `url` from its copy in the media library, copied
/// there now when there is none; her data is pointed at the copy either
/// way. None when it is no outside address or cannot be fetched.
pub(super) async fn copy_of(pictures: &Pictures, url: &str) -> Option<(Vec<u8>, String)> {
    let parsed = outside(url)?;
    let url = url.trim();
    let key = sha256_hex(url.as_bytes());
    let actor = MediaActor::admin(pictures.owner).ok()?;
    let ctx = MediaContext::site(actor, MediaSource::Import)
        .with_producer_key(format!("persona-sync:{key}"));
    let _one = COPYING.lock().await;
    let copied = match pictures.media.ready_by_producer(&pictures.db, &ctx).await {
        Ok(Some(asset)) => asset,
        Ok(None) => {
            if recently_failed(&key) {
                return None;
            }
            let host = parsed.host_str().unwrap_or("-").to_string();
            let (bytes, media_type, cached) = match download(&pictures.cache, url).await {
                Ok(found) => found,
                Err(error) => {
                    tracing::warn!(%host, %error, "[sync] an outside picture cannot be read");
                    failed(key);
                    return None;
                }
            };
            let kept = pictures
                .media
                .persist_ready_bytes(
                    &pictures.db,
                    ctx,
                    NewMediaBytes {
                        bytes: bytes.into(),
                        claimed_mime: media_type.to_string(),
                        filename: format!("{}.{}", &key[..16], extension(media_type)),
                        max_bytes: crate::services::memory_profile::note_image_limit(),
                        derived_from_id: None,
                        exposure: MediaExposure::Public,
                    },
                )
                .await;
            let _ = pictures.cache.remove_stored_url(&cached).await;
            match kept {
                Ok((asset, _)) => {
                    tracing::info!(%host, "[sync] an outside picture was copied into the media library");
                    asset
                }
                Err(error) => {
                    tracing::warn!(%host, %error, "[sync] an outside picture cannot be kept");
                    failed(key);
                    return None;
                }
            }
        }
        Err(error) => {
            tracing::warn!(%error, "[sync] could not look up an outside picture's copy");
            return None;
        }
    };
    let found = crate::services::image_generation::read_public_local_media_on(
        &pictures.db,
        pictures.store(),
        &copied.url,
    )
    .await?;
    if let Err(error) = point_at_copy(&pictures.db, url, &copied.url).await {
        tracing::warn!(%error, "[sync] could not point her data at a picture's copy");
    }
    Some(found)
}

/// Whether a rig in her wardrobe was built from the master at `url`: the rig
/// names its master by address, so that address stays.
async fn a_rig_descends_from(visual_profile: Option<&Value>, url: &str) -> bool {
    let rigs = visual_profile
        .and_then(|profile| profile.get("wardrobe"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|item| item.get("rigAssetId").and_then(Value::as_str));
    for rig in rigs {
        let Some(id) = crate::services::merope_rig::normalize_asset_id(rig) else {
            continue;
        };
        if let Ok(bytes) = crate::services::merope_rig::read_manifest_bytes(&id).await
            && let Ok(manifest) = serde_json::from_slice::<myriad_merope::RigManifest>(&bytes)
            && manifest.source_master_asset_id.as_deref().map(str::trim) == Some(url)
        {
            return true;
        }
    }
    false
}

/// Points every place in her data that holds the outside address `from` at
/// the copy `to`: the persona's portrait, sticker avatar and outfit
/// portraits, and her stickers. One transaction, under the persona lock.
pub(super) async fn point_at_copy(
    db: &DatabaseConnection,
    from: &str,
    to: &str,
) -> anyhow::Result<()> {
    use crate::services::agent::merope;
    let txn = db.begin().await?;
    merope::api::store::lock_persona_on(&txn).await?;
    if let Some(persona) = merope::get_persona_on(&txn).await? {
        let kept = a_rig_descends_from(persona.visual_profile.as_ref(), from).await;
        let swap = |held: &Option<String>, may: bool| match held {
            Some(url) if may && url.trim() == from => (Some(to.to_string()), true),
            other => (other.clone(), false),
        };
        let (portrait, portrait_moved) = swap(&persona.portrait_asset_id, !kept);
        let (avatar, avatar_moved) = swap(&persona.avatar_asset_id, true);
        let mut visual = persona.visual_profile.clone();
        let mut outfits_moved = false;
        if !kept
            && let Some(items) = visual
                .as_mut()
                .and_then(|profile| profile.get_mut("wardrobe"))
                .and_then(Value::as_array_mut)
        {
            for item in items {
                if item
                    .get("portraitAssetId")
                    .and_then(Value::as_str)
                    .is_some_and(|url| url.trim() == from)
                {
                    item["portraitAssetId"] = Value::String(to.to_string());
                    outfits_moved = true;
                }
            }
        }
        if portrait_moved || avatar_moved || outfits_moved {
            let persona = if outfits_moved {
                let mut active: crate::models::entities::agent_persona::ActiveModel =
                    persona.into();
                active.visual_profile = Set(visual);
                active.update(&txn).await?
            } else {
                persona
            };
            let saved = merope::rewrite_persona_media_urls(&txn, persona, portrait, avatar).await?;
            crate::services::media::bind_persona(
                &txn,
                saved.portrait_asset_id.as_deref(),
                saved.avatar_asset_id.as_deref(),
                saved.visual_profile.as_ref(),
                &crate::services::media::configured_origins().await,
            )
            .await
            .map_err(|error| anyhow::anyhow!("bind the persona's media: {error}"))?;
        }
    }
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE merope_stickers SET asset_id = $2 WHERE btrim(asset_id) = $1",
        vec![from.into(), to.into()],
    ))
    .await?;
    txn.commit().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use super::*;

    /// A real (tiny) PNG: the media library decodes what it keeps.
    fn png() -> Vec<u8> {
        png_of(200)
    }

    fn png_of(red: u8) -> Vec<u8> {
        let mut out = std::io::Cursor::new(Vec::new());
        image::RgbaImage::from_pixel(2, 2, image::Rgba([red, 120, 40, 255]))
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        out.into_inner()
    }

    /// A local stand-in for outside hosts, counting requests by path.
    async fn fake_host() -> (String, Arc<Mutex<HashMap<String, usize>>>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}", listener.local_addr().unwrap());
        let hits: Arc<Mutex<HashMap<String, usize>>> = Arc::default();
        let counted = hits.clone();
        let picture = png();
        let other = png_of(30);
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    return;
                };
                let counted = counted.clone();
                let picture = picture.clone();
                let other = other.clone();
                tokio::spawn(async move {
                    let mut request = Vec::new();
                    let mut chunk = [0u8; 1024];
                    while !request.windows(4).any(|w| w == b"\r\n\r\n") {
                        let Ok(n) = socket.read(&mut chunk).await else {
                            return;
                        };
                        if n == 0 {
                            return;
                        }
                        request.extend_from_slice(&chunk[..n]);
                    }
                    let line = String::from_utf8_lossy(&request);
                    let path = line.split_whitespace().nth(1).unwrap_or("/").to_string();
                    *counted.lock().unwrap().entry(path.clone()).or_default() += 1;
                    let (status, media_type, body): (&str, &str, Vec<u8>) = match path.as_str() {
                        "/ok.png" | "/guarded.png" => ("200 OK", "image/png", picture),
                        "/sticker.png" => ("200 OK", "image/png", other),
                        "/page" => ("200 OK", "text/html", b"<html></html>".to_vec()),
                        "/fake.png" => ("200 OK", "image/png", b"<html></html>".to_vec()),
                        _ => ("404 Not Found", "text/plain", b"gone".to_vec()),
                    };
                    let length = if path == "/big.png" {
                        // Declared past the limit: refused before the body.
                        11 * 1024 * 1024
                    } else {
                        body.len()
                    };
                    let head = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {media_type}\r\n\
                         Content-Length: {length}\r\nConnection: close\r\n\r\n"
                    );
                    let _ = socket.write_all(head.as_bytes()).await;
                    let _ = socket.write_all(&body).await;
                    let _ = socket.shutdown().await;
                });
            }
        });
        (base, hits)
    }

    fn hits(of: &Arc<Mutex<HashMap<String, usize>>>, path: &str) -> usize {
        of.lock().unwrap().get(path).copied().unwrap_or(0)
    }

    /// Holds the lab flag that lets the fake host on loopback stand in for
    /// the outside, and puts the environment back afterwards.
    struct Lab {
        lab: Option<String>,
        env: Option<String>,
    }

    impl Lab {
        fn closed() -> Self {
            let lab = Self {
                lab: std::env::var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND").ok(),
                env: std::env::var("ENVIRONMENT").ok(),
            };
            unsafe { std::env::remove_var("ENVIRONMENT") };
            unsafe { std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND") };
            lab
        }

        fn open(&self) {
            unsafe { std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", "1") };
        }
    }

    impl Drop for Lab {
        fn drop(&mut self) {
            match self.lab.take() {
                Some(value) => unsafe {
                    std::env::set_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND", value)
                },
                None => unsafe { std::env::remove_var("MYRIAD_FEDERATION_LAB_PRIVATE_OUTBOUND") },
            }
            if let Some(value) = self.env.take() {
                unsafe { std::env::set_var("ENVIRONMENT", value) };
            }
        }
    }

    /// The fetch alone, against a local fake host only: the guards hold for
    /// loopback until the lab flag stands it in for the outside; a dead
    /// address, a page, bytes that are no image and an oversized body are
    /// not pictures.
    #[tokio::test]
    async fn an_outside_picture_is_fetched_through_the_guards() {
        let _lock = crate::services::outbound_security::tests_lab_env_lock().await;
        let lab = Lab::closed();
        let dir = std::env::temp_dir().join(format!("sync-outside-{}", uuid::Uuid::new_v4()));
        let cache = ImageCacheService::at(dir.clone());
        let (base, served) = fake_host().await;

        assert!(
            download(&cache, &format!("{base}/guarded.png"))
                .await
                .is_err()
        );
        assert_eq!(hits(&served, "/guarded.png"), 0);

        lab.open();
        let (bytes, media_type, _) = download(&cache, &format!("{base}/ok.png")).await.unwrap();
        assert_eq!((bytes, media_type), (png(), "image/png"));
        for path in ["/gone.png", "/page", "/fake.png", "/big.png"] {
            assert!(
                download(&cache, &format!("{base}{path}")).await.is_err(),
                "{path}"
            );
        }
        assert!(outside("ftp://127.0.0.1/x.png").is_none());
        assert!(outside("/media/assets/x/y.png").is_none());
        drop(lab);
        let _ = std::fs::remove_dir_all(dir);
    }

    /// The whole copy on a real database (an isolated schema; skipped
    /// without a test database) and a local fake host: copied into the
    /// media library once, her portrait, outfit portrait, sticker avatar
    /// and sticker pointed at the copy without touching anything else of
    /// her, readable after the host is gone, and a dead address not asked
    /// again within the back-off.
    #[tokio::test]
    async fn an_outside_picture_is_copied_into_the_media_library_and_her_data_points_at_it() {
        use sea_orm::ConnectionTrait;
        let _lock = crate::services::outbound_security::tests_lab_env_lock().await;
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let lab = Lab::closed();
        lab.open();
        let db = fixture.db.clone();
        let (base, served) = fake_host().await;
        let ok = format!("{base}/ok.png");
        let sticker = format!("{base}/sticker.png");
        db.execute_unprepared(&format!(
            "INSERT INTO users (id, username, is_admin, is_owner) VALUES (41, 'owner41', true, true);
             INSERT INTO agent_persona (id, name, personality, visual_profile, portrait_asset_id,
                                        avatar_asset_id, avatar_generation, updated_at)
             VALUES ('site', 'Merope', '安静', '{{\"wardrobe\":[{{\"id\":\"coat\",\
               \"clothingStyle\":\"everyday\",\"outfit\":{{}},\"portraitAssetId\":\"{ok}\"}}]}}'::jsonb,
               '{ok}', '{sticker}', '{{\"kept\":true}}'::jsonb, '2026-10-01T00:00:00Z');
             INSERT INTO merope_stickers (id, picture, meaning, asset_id, identity)
             VALUES ('s1', '挥手', '你好', '{sticker}', 'abc');"
        ))
        .await
        .unwrap();
        crate::services::principal::invalidate_site_owner_cache();
        let root = std::env::temp_dir().join(format!("sync-outside-{}", uuid::Uuid::new_v4()));
        let pictures = Pictures::under(&db, 41, &root);

        let (bytes, media_type) = pictures.bytes(&ok).await.unwrap();
        assert_eq!((bytes.clone(), media_type.as_str()), (png(), "image/png"));
        let persona = crate::services::agent::merope::get_persona_on(&db)
            .await
            .unwrap()
            .unwrap();
        let copy = persona.portrait_asset_id.clone().unwrap();
        assert!(copy.starts_with("/media/assets/"), "{copy}");
        assert_eq!(
            persona.visual_profile.as_ref().unwrap()["wardrobe"][0]["portraitAssetId"],
            copy.as_str()
        );
        // The same picture under a new address: nothing else of her moved.
        assert_eq!(persona.name, "Merope");
        assert_eq!(persona.avatar_asset_id.as_deref(), Some(sticker.as_str()));
        assert_eq!(
            persona.avatar_generation,
            Some(serde_json::json!({ "kept": true }))
        );
        assert_eq!(
            persona.updated_at,
            chrono::DateTime::parse_from_rfc3339("2026-10-01T00:00:00Z").unwrap()
        );

        // The sticker and the avatar: one address, one copy.
        pictures.bytes(&sticker).await.unwrap();
        let persona = crate::services::agent::merope::get_persona_on(&db)
            .await
            .unwrap()
            .unwrap();
        let avatar = persona.avatar_asset_id.unwrap();
        assert!(avatar.starts_with("/media/assets/") && avatar != copy);
        let row = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT asset_id FROM merope_stickers WHERE id = 's1'".to_string(),
            ))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(row.try_get::<String>("", "asset_id").unwrap(), avatar);

        // Read from the copy from now on, also by the old address.
        assert_eq!(pictures.bytes(&copy).await.unwrap().0, bytes);
        assert_eq!(pictures.bytes(&ok).await.unwrap().0, bytes);
        assert_eq!(hits(&served, "/ok.png"), 1);
        assert_eq!(hits(&served, "/sticker.png"), 1);
        let kept = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT COUNT(*)::BIGINT AS n FROM media_assets".to_string(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i64>("", "n")
            .unwrap();
        assert_eq!(kept, 2);

        // A dead address: unreadable, and not asked again at once.
        let gone = format!("{base}/gone.png");
        assert!(pictures.bytes(&gone).await.is_none());
        assert!(pictures.bytes(&gone).await.is_none());
        assert_eq!(hits(&served, "/gone.png"), 1);

        drop(lab);
        let _ = std::fs::remove_dir_all(root);
        fixture.close().await;
    }
}
