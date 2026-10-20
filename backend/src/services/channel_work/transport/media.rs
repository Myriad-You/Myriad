//! Images to and from a platform: read for sending, and what people send
//! kept as their own media.
use super::*;

pub(in crate::services::channel_work) async fn load_channel_image_bytes(
    url: &str,
) -> Result<ChannelImageBytes, String> {
    let cache = crate::services::image_cache::ImageCacheService::new();
    if cache.local_path_for_public_url(url).is_some() {
        let (bytes, mime) = cache.read_local_public_url(url).await?;
        return Ok(ChannelImageBytes { bytes, mime });
    }
    if url.starts_with("/media/assets/") {
        let (bytes, mime) = crate::services::image_generation::read_public_local_media(url)
            .await
            .ok_or("imageUrl is not readable")?;
        return Ok(ChannelImageBytes { bytes, mime });
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("imageUrl is not a sendable path".to_string());
    }
    let cached = cache.cache_image(url).await?;
    let (bytes, mime) = cache.read_local_public_url(&cached).await?;
    Ok(ChannelImageBytes { bytes, mime })
}

pub(in crate::services::channel_work) struct ChannelImageBytes {
    pub(in crate::services::channel_work) bytes: Vec<u8>,
    pub(in crate::services::channel_work) mime: String,
}

pub(in crate::services::channel_work) async fn cache_inbound_images(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    sink: &ChannelTransport,
    images: &[ChannelImageRef],
) -> Result<Option<Value>, String> {
    if images.is_empty() || !sink.capabilities().inbound_media {
        return Ok(None);
    }
    let cache = crate::services::image_cache::ImageCacheService::new();
    let mut attachments = Vec::new();
    for image in images.iter().take(CHANNEL_IMAGE_LIMIT) {
        match resolve_inbound_image(db, user_id, sink, image, &cache).await {
            Ok((url, mime, size, name)) => attachments.push(serde_json::json!({
                "name": name,
                "mime": mime,
                "size": size,
                "url": url,
            })),
            Err(error) => {
                warn!(%error, "channel inbound image cache failed");
                return Err("图片暂时无法读取，请重新发送；本次没有开始办事。".into());
            }
        }
    }
    Ok((!attachments.is_empty()).then(|| serde_json::json!({ "attachments": attachments })))
}

async fn resolve_inbound_image(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    sink: &ChannelTransport,
    image: &ChannelImageRef,
    cache: &crate::services::image_cache::ImageCacheService,
) -> Result<(String, String, usize, String), String> {
    if cache.local_path_for_public_url(&image.url).is_some() {
        return persist_cached(db, user_id, image, &image.url).await;
    }
    if image.url.starts_with("/media/assets/") {
        return Ok((image.url.clone(), image.mime.clone(), 0, image.name.clone()));
    }
    if let ChannelTransport::Telegram { token, .. } = sink {
        if let Some(file_id) = image.url.strip_prefix("tg:") {
            let (bytes, mime) =
                crate::services::telegram_bot::download_file_bytes(token, file_id).await?;
            let stored_mime = if image.mime.starts_with("image/") {
                image.mime.clone()
            } else {
                mime
            };
            return persist_channel_asset(
                db,
                user_id,
                sink.platform(),
                file_id,
                bytes,
                stored_mime,
                image,
            )
            .await;
        }
    }
    if let ChannelTransport::Feishu { .. } = sink {
        if let Some(resource) = image.url.strip_prefix("feishu:") {
            let (message_id, image_key) = resource
                .split_once('/')
                .ok_or("missing Feishu message resource id")?;
            let (bytes, mime) =
                crate::services::feishu_bot_api::download_image_bytes(message_id, image_key)
                    .await?;
            let stored_mime = if image.mime.starts_with("image/") {
                image.mime.clone()
            } else {
                mime
            };
            return persist_channel_asset(
                db,
                user_id,
                sink.platform(),
                resource,
                bytes,
                stored_mime,
                image,
            )
            .await;
        }
    }
    let cached = cache.cache_image(&image.url).await?;
    persist_cached(db, user_id, image, &cached).await
}

async fn persist_channel_asset(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    platform: ChannelPlatform,
    producer: &str,
    bytes: Vec<u8>,
    mime: String,
    image: &ChannelImageRef,
) -> Result<(String, String, usize, String), String> {
    let actor =
        crate::services::media::MediaActor::user(user_id).map_err(|error| error.to_string())?;
    let ctx = crate::services::media::MediaContext::user(
        actor,
        crate::services::media::MediaSource::Channel,
    )
    .map_err(|error| error.to_string())?
    .with_producer_key(format!("channel:{platform}:{producer}"));
    let size = bytes.len();
    let (asset, _) =
        crate::services::media::MediaService::from_data_paths(crate::services::data_paths::paths())
            .persist_ready_bytes(
                db,
                ctx,
                crate::services::media::NewMediaBytes {
                    bytes: bytes.into(),
                    claimed_mime: mime.clone(),
                    filename: image.name.clone(),
                    max_bytes: crate::services::memory_profile::note_image_limit(),
                    derived_from_id: None,
                    exposure: crate::services::media::MediaExposure::Private,
                },
            )
            .await
            .map_err(|error| error.to_string())?;
    Ok((asset.catalog_url(), mime, size, image.name.clone()))
}

/// Image-cache paths are not citable media; promote the cached bytes to an
/// asset owned by the sender.
async fn persist_cached(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    image: &ChannelImageRef,
    cached: &str,
) -> Result<(String, String, usize, String), String> {
    let actor =
        crate::services::media::MediaActor::user(user_id).map_err(|error| error.to_string())?;
    let ctx = crate::services::media::MediaContext::user(
        actor,
        crate::services::media::MediaSource::Channel,
    )
    .map_err(|error| error.to_string())?;
    let asset =
        crate::services::media::MediaService::from_data_paths(crate::services::data_paths::paths())
            .persist_cached(
                db,
                ctx,
                cached,
                Some(&image.mime),
                &image.name,
                crate::services::media::MediaExposure::Private,
            )
            .await
            .map_err(|error| error.to_string())?;
    Ok((
        asset.url,
        asset.mime,
        asset.size as usize,
        image.name.clone(),
    ))
}
