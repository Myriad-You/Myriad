//! Platform senders and media I/O. No Work or session state lives here.
use super::*;

#[derive(Clone)]
pub enum ChannelTransport {
    Telegram {
        token: String,
        chat_id: String,
    },
    Discord {
        token: String,
        channel_id: String,
    },
    Qq {
        db: DatabaseConnection,
        openid: String,
        inbound_msg_id: Option<String>,
    },
    Feishu {
        chat_id: String,
    },
}

impl ChannelTransport {
    pub(super) fn platform(&self) -> &'static str {
        match self {
            Self::Telegram { .. } => "telegram",
            Self::Discord { .. } => "discord",
            Self::Qq { .. } => "qq",
            Self::Feishu { .. } => "feishu",
        }
    }

    pub(super) fn capabilities(&self) -> myriad_agent_rules::channel::ChannelCapabilities {
        match self {
            Self::Telegram { .. } => telegram_dm_capabilities(),
            Self::Discord { .. } => discord_dm_capabilities(),
            Self::Qq { .. } => qq_c2c_capabilities(),
            Self::Feishu { .. } => feishu_dm_capabilities(),
        }
    }

    pub(super) fn text_limit(&self) -> usize {
        match self {
            Self::Telegram { .. } => TELEGRAM_TEXT_LIMIT,
            Self::Discord { .. } => DISCORD_TEXT_LIMIT,
            Self::Qq { .. } => QQ_TEXT_LIMIT,
            Self::Feishu { .. } => FEISHU_TEXT_LIMIT,
        }
    }

    pub(super) fn delivery_context(&self) -> DeliveryContext {
        match self {
            Self::Telegram { .. } | Self::Discord { .. } => DeliveryContext {
                inbound_msg_id: None,
                passive_window_open: false,
                remaining_passive_replies: 0,
                typing: true,
            },
            Self::Feishu { .. } => DeliveryContext {
                inbound_msg_id: None,
                passive_window_open: false,
                remaining_passive_replies: 0,
                typing: false,
            },
            Self::Qq { inbound_msg_id, .. } => DeliveryContext {
                inbound_msg_id: inbound_msg_id.clone(),
                passive_window_open: inbound_msg_id.as_ref().is_some_and(|id| !id.is_empty()),
                remaining_passive_replies: 4,
                typing: false,
            },
        }
    }

    pub(super) async fn send_typing(&self) {
        match self {
            Self::Telegram { token, chat_id } => {
                if let Err(error) = crate::services::telegram_bot::send_typing(token, chat_id).await
                {
                    warn!(?error, "channel typing failed");
                }
            }
            Self::Discord { token, channel_id } => {
                if let Err(error) =
                    crate::services::discord_bot::send_typing(token, channel_id).await
                {
                    warn!(?error, "channel typing failed");
                }
            }
            Self::Qq { .. } | Self::Feishu { .. } => {}
        }
    }

    pub(super) async fn send_force_reply(&self, placeholder: &str) -> Result<(), String> {
        match self {
            Self::Telegram { token, chat_id } => crate::services::telegram_bot::send_outbound(
                token,
                chat_id,
                "请在这里输入。",
                Some(telegram_force_reply_markup(placeholder)),
            )
            .await
            .map_err(|error| format!("{error:?}")),
            Self::Discord { .. } | Self::Qq { .. } | Self::Feishu { .. } => {
                self.send_text("请直接回复这一问。").await
            }
        }
    }

    pub(super) async fn send_text(&self, content: &str) -> Result<(), String> {
        self.send_chunks(&[content.to_string()], &[], None).await
    }

    pub(super) async fn send_prompt(
        &self,
        content: &str,
        prompt: &PendingPrompt,
    ) -> Result<(), String> {
        self.send_chunks(&[content.to_string()], &[], Some(prompt))
            .await
    }

    async fn send_chunks(
        &self,
        chunks: &[String],
        image_urls: &[String],
        prompt: Option<&PendingPrompt>,
    ) -> Result<(), String> {
        let images = if self.capabilities().outbound_image {
            image_urls
        } else {
            &[]
        };
        if chunks.is_empty() && images.is_empty() {
            return Ok(());
        }
        let last_text = chunks.len().saturating_sub(1);
        for (index, chunk) in chunks.iter().enumerate() {
            if chunk.trim().is_empty() {
                continue;
            }
            let markup = prompt
                .filter(|_| images.is_empty() && index == last_text)
                .and_then(|prompt| match self {
                    Self::Telegram { .. } => telegram_reply_markup(prompt),
                    Self::Discord { .. } => discord_reply_markup(prompt),
                    Self::Feishu { .. } => feishu_reply_markup(prompt),
                    Self::Qq { .. } => None,
                });
            self.send_text_chunk(chunk, markup).await?;
        }
        for (index, url) in images.iter().enumerate() {
            let markup =
                prompt
                    .filter(|_| index + 1 == images.len())
                    .and_then(|prompt| match self {
                        Self::Telegram { .. } => telegram_reply_markup(prompt),
                        Self::Discord { .. } => discord_reply_markup(prompt),
                        Self::Feishu { .. } => feishu_reply_markup(prompt),
                        Self::Qq { .. } => None,
                    });
            self.send_image_chunk(url, markup).await?;
        }
        Ok(())
    }

    pub(super) async fn send_text_chunk(
        &self,
        chunk: &str,
        markup: Option<Value>,
    ) -> Result<(), String> {
        match self {
            Self::Telegram { token, chat_id } => {
                crate::services::telegram_bot::send_outbound(token, chat_id, chunk, markup)
                    .await
                    .map_err(|error| format!("{error:?}"))
            }
            Self::Discord { token, channel_id } => {
                crate::services::discord_bot::send_outbound(token, channel_id, chunk, markup)
                    .await
                    .map_err(|error| format!("{error:?}"))
            }
            Self::Qq {
                db,
                openid,
                inbound_msg_id,
            } => {
                crate::services::qq_work::send_c2c(
                    db,
                    &crate::services::qq_bot::outbound_auth_header()
                        .await
                        .map_err(|_| "QQ credentials unavailable")?,
                    openid,
                    chunk,
                    inbound_msg_id.as_deref(),
                )
                .await
            }
            Self::Feishu { chat_id } => {
                crate::services::feishu_bot_api::send_outbound(chat_id, chunk, markup)
                    .await
                    .map_err(|error| format!("{error:?}"))
            }
        }
    }

    pub(super) async fn send_image_chunk(
        &self,
        url: &str,
        markup: Option<Value>,
    ) -> Result<(), String> {
        let image = load_channel_image_bytes(url).await?;
        match self {
            Self::Telegram { token, chat_id } => crate::services::telegram_bot::send_photo(
                token,
                chat_id,
                &image.bytes,
                &image.mime,
                markup,
            )
            .await
            .map_err(|error| format!("{error:?}")),
            Self::Discord { token, channel_id } => crate::services::discord_bot::send_photo(
                token,
                channel_id,
                &image.bytes,
                &image.mime,
                markup,
            )
            .await
            .map_err(|error| format!("{error:?}")),
            Self::Qq {
                db,
                openid,
                inbound_msg_id,
            } => {
                crate::services::qq_work::send_c2c_image(
                    db,
                    &crate::services::qq_bot::outbound_auth_header()
                        .await
                        .map_err(|_| "QQ credentials unavailable")?,
                    openid,
                    &image.bytes,
                    inbound_msg_id.as_deref(),
                )
                .await
            }
            Self::Feishu { chat_id } => crate::services::feishu_bot_api::send_photo(
                chat_id,
                &image.bytes,
                &image.mime,
                markup,
            )
            .await
            .map_err(|error| format!("{error:?}")),
        }
    }
}

async fn load_channel_image_bytes(url: &str) -> Result<ChannelImageBytes, String> {
    let cache = crate::services::image_cache::ImageCacheService::new();
    if cache.local_path_for_public_url(url).is_some() {
        let (bytes, mime) = cache.read_local_public_url(url).await?;
        return Ok(ChannelImageBytes { bytes, mime });
    }
    if !(url.starts_with("http://") || url.starts_with("https://")) {
        return Err("imageUrl is not a sendable path".to_string());
    }
    let cached = cache.cache_image(url).await?;
    let (bytes, mime) = cache.read_local_public_url(&cached).await?;
    Ok(ChannelImageBytes { bytes, mime })
}

struct ChannelImageBytes {
    bytes: Vec<u8>,
    mime: String,
}

pub(super) async fn cache_inbound_images(
    sink: &ChannelTransport,
    images: &[ChannelImageRef],
) -> Result<Option<Value>, String> {
    if images.is_empty() || !sink.capabilities().inbound_media {
        return Ok(None);
    }
    let cache = crate::services::image_cache::ImageCacheService::new();
    let mut attachments = Vec::new();
    for image in images.iter().take(CHANNEL_IMAGE_LIMIT) {
        match resolve_inbound_image(sink, image, &cache).await {
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
    sink: &ChannelTransport,
    image: &ChannelImageRef,
    cache: &crate::services::image_cache::ImageCacheService,
) -> Result<(String, String, usize, String), String> {
    if cache.local_path_for_public_url(&image.url).is_some() {
        let (bytes, mime) = cache.read_local_public_url(&image.url).await?;
        return Ok((image.url.clone(), mime, bytes.len(), image.name.clone()));
    }
    if let ChannelTransport::Telegram { token, .. } = sink {
        if let Some(file_id) = image.url.strip_prefix("tg:") {
            let (bytes, mime) =
                crate::services::telegram_bot::download_file_bytes(token, file_id).await?;
            let stored = cache.store_bytes_with_status(&bytes, &mime).await?;
            return Ok((
                stored.url,
                if image.mime.starts_with("image/") {
                    image.mime.clone()
                } else {
                    mime
                },
                bytes.len(),
                image.name.clone(),
            ));
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
            let stored = cache.store_bytes_with_status(&bytes, &mime).await?;
            return Ok((
                stored.url,
                if image.mime.starts_with("image/") {
                    image.mime.clone()
                } else {
                    mime
                },
                bytes.len(),
                image.name.clone(),
            ));
        }
    }
    let cached = cache.cache_image(&image.url).await?;
    finish_cached(cache, image, cached).await
}

async fn finish_cached(
    cache: &crate::services::image_cache::ImageCacheService,
    image: &ChannelImageRef,
    cached: String,
) -> Result<(String, String, usize, String), String> {
    let (bytes, mime) = cache.read_local_public_url(&cached).await?;
    Ok((
        cached,
        if image.mime.starts_with("image/") {
            image.mime.clone()
        } else {
            mime
        },
        bytes.len(),
        image.name.clone(),
    ))
}

/// Persistable routing information. No token or credential can enter the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum ChannelAddress {
    Telegram { chat_id: String },
    Discord { channel_id: String },
    Qq { openid: String },
    Feishu { chat_id: String },
}

impl ChannelTransport {
    pub(super) fn address(&self) -> ChannelAddress {
        match self {
            Self::Telegram { chat_id, .. } => ChannelAddress::Telegram {
                chat_id: chat_id.clone(),
            },
            Self::Discord { channel_id, .. } => ChannelAddress::Discord {
                channel_id: channel_id.clone(),
            },
            Self::Qq { openid, .. } => ChannelAddress::Qq {
                openid: openid.clone(),
            },
            Self::Feishu { chat_id } => ChannelAddress::Feishu {
                chat_id: chat_id.clone(),
            },
        }
    }
}

impl ChannelAddress {
    pub(super) async fn connect(&self, db: &DatabaseConnection) -> Option<ChannelTransport> {
        match self {
            Self::Telegram { chat_id } => Some(ChannelTransport::Telegram {
                token: crate::GLOBAL_DYNAMIC_CONFIG
                    .read()
                    .await
                    .telegram_bot_token
                    .clone()?,
                chat_id: chat_id.clone(),
            }),
            Self::Discord { channel_id } => Some(ChannelTransport::Discord {
                token: crate::GLOBAL_DYNAMIC_CONFIG
                    .read()
                    .await
                    .discord_bot_token
                    .clone()?,
                channel_id: channel_id.clone(),
            }),
            Self::Qq { openid } => Some(ChannelTransport::Qq {
                db: db.clone(),
                openid: openid.clone(),
                inbound_msg_id: None,
            }),
            Self::Feishu { chat_id } => Some(ChannelTransport::Feishu {
                chat_id: chat_id.clone(),
            }),
        }
    }
}
