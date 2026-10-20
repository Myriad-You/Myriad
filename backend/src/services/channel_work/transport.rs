//! Platform senders and media I/O. No Work or session state lives here.
use super::*;

mod media;
pub(super) use media::{ChannelImageBytes, cache_inbound_images, load_channel_image_bytes};

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
    OneBot {
        user_id: String,
        /// Set when they wrote from a group's temp session (临时会话):
        /// replies go back through it, and there is no typing mark.
        temp_group: Option<String>,
    },
}

impl ChannelTransport {
    pub(super) fn platform(&self) -> ChannelPlatform {
        match self {
            Self::Telegram { .. } => ChannelPlatform::Telegram,
            Self::Discord { .. } => ChannelPlatform::Discord,
            Self::Qq { .. } => ChannelPlatform::Qq,
            Self::Feishu { .. } => ChannelPlatform::Feishu,
            Self::OneBot { .. } => ChannelPlatform::OneBot,
        }
    }

    pub(super) fn capabilities(&self) -> myriad_agent_rules::channel::ChannelCapabilities {
        match self {
            Self::Telegram { .. } => telegram_dm_capabilities(),
            Self::Discord { .. } => discord_dm_capabilities(),
            Self::Qq { .. } => qq_c2c_capabilities(),
            Self::Feishu { .. } => feishu_dm_capabilities(),
            Self::OneBot { .. } => myriad_agent_rules::onebot::rules::onebot_private_capabilities(),
        }
    }

    pub(super) fn text_limit(&self) -> usize {
        match self {
            Self::Telegram { .. } => TELEGRAM_TEXT_LIMIT,
            Self::Discord { .. } => DISCORD_TEXT_LIMIT,
            Self::Qq { .. } => QQ_TEXT_LIMIT,
            Self::Feishu { .. } => FEISHU_TEXT_LIMIT,
            Self::OneBot { .. } => QQ_TEXT_LIMIT,
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
            Self::OneBot { temp_group, .. } => DeliveryContext {
                inbound_msg_id: None,
                passive_window_open: false,
                remaining_passive_replies: 0,
                // SnowLuma's typing mark is for friend chats only.
                typing: temp_group.is_none()
                    && myriad_agent_rules::onebot::rules::onebot_worker_supports_typing(),
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
            Self::OneBot {
                temp_group: Some(_),
                ..
            } => {}
            Self::OneBot {
                user_id,
                temp_group: None,
            } => {
                let Some(action) = myriad_agent_rules::onebot::encode::encode_typing(user_id, true)
                else {
                    return;
                };
                if let Err(error) = crate::services::onebot_send::send_action(action).await {
                    warn!(%error, "onebot typing failed");
                }
            }
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
            .map_err(|error| SendError::from(error).to_string()),
            Self::Discord { .. } | Self::Qq { .. } | Self::Feishu { .. } | Self::OneBot { .. } => {
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
                    Self::Qq { .. } | Self::OneBot { .. } => None,
                });
            self.send_text_chunk(chunk, markup)
                .await
                .map_err(|error| error.to_string())?;
        }
        for (index, url) in images.iter().enumerate() {
            let markup =
                prompt
                    .filter(|_| index + 1 == images.len())
                    .and_then(|prompt| match self {
                        Self::Telegram { .. } => telegram_reply_markup(prompt),
                        Self::Discord { .. } => discord_reply_markup(prompt),
                        Self::Feishu { .. } => feishu_reply_markup(prompt),
                        Self::Qq { .. } | Self::OneBot { .. } => None,
                    });
            self.send_image_chunk(url, markup)
                .await
                .map_err(|error| error.to_string())?;
        }
        Ok(())
    }

    pub(super) async fn send_text_chunk(
        &self,
        chunk: &str,
        markup: Option<Value>,
    ) -> Result<(), SendError> {
        match self {
            Self::Telegram { token, chat_id } => {
                crate::services::telegram_bot::send_outbound(token, chat_id, chunk, markup)
                    .await
                    .map_err(SendError::from)
            }
            Self::Discord { token, channel_id } => {
                crate::services::discord_bot::send_outbound(token, channel_id, chunk, markup)
                    .await
                    .map_err(SendError::from)
            }
            Self::Qq {
                db,
                openid,
                inbound_msg_id,
            } => crate::services::qq_work::send_c2c(
                db,
                &crate::services::qq_bot::outbound_auth_header()
                    .await
                    .map_err(|_| SendError::Transient)?,
                openid,
                chunk,
                inbound_msg_id.as_deref(),
            )
            .await
            .map_err(SendError::from),
            Self::Feishu { chat_id } => {
                crate::services::feishu_bot_api::send_outbound(chat_id, chunk, markup)
                    .await
                    .map_err(SendError::from)
            }
            Self::OneBot {
                user_id,
                temp_group,
            } => {
                // SnowLuma sends markdown only as a flash-message card, not as
                // a message segment. Tables stay readable as plain text.
                let Some(action) =
                    myriad_agent_rules::onebot::encode::plan_private_delivery(user_id, chunk, &[])
                else {
                    return Ok(());
                };
                let action = myriad_agent_rules::onebot::encode::via_temp_session(
                    action,
                    temp_group.as_deref(),
                );
                map_onebot_send(crate::services::onebot_send::send_action(action).await)
            }
        }
    }

    pub(super) async fn send_image_chunk(
        &self,
        url: &str,
        markup: Option<Value>,
    ) -> Result<(), SendError> {
        // Unreadable media (a private asset, an expired link) is skipped so the
        // rest of the reply still goes out; delivery failures still propagate.
        let image = match load_channel_image_bytes(url).await {
            Ok(image) => image,
            Err(error) => {
                tracing::warn!(%error, "channel reply image skipped");
                return Ok(());
            }
        };
        self.send_image_bytes(&image, markup).await
    }

    /// A sticker of hers, from its drawn PNG: a real sticker where the app
    /// has them, a small picture elsewhere.
    pub(crate) async fn send_sticker(&self, png: Vec<u8>) -> Result<(), SendError> {
        let Some(prepared) = crate::services::sticker_send::prepare(png, self.platform()).await
        else {
            return Ok(());
        };
        if let Self::Telegram { token, chat_id } = self {
            return crate::services::telegram_bot::send_sticker(
                token,
                chat_id,
                &prepared.bytes,
                None,
            )
            .await
            .map_err(SendError::from);
        }
        let image = ChannelImageBytes {
            bytes: prepared.bytes,
            mime: prepared.mime.to_string(),
        };
        self.send_image_bytes(&image, None).await
    }

    async fn send_image_bytes(
        &self,
        image: &ChannelImageBytes,
        markup: Option<Value>,
    ) -> Result<(), SendError> {
        match self {
            Self::Telegram { token, chat_id } => crate::services::telegram_bot::send_photo(
                token,
                chat_id,
                &image.bytes,
                &image.mime,
                markup,
            )
            .await
            .map_err(SendError::from),
            Self::Discord { token, channel_id } => crate::services::discord_bot::send_photo(
                token,
                channel_id,
                &image.bytes,
                &image.mime,
                markup,
            )
            .await
            .map_err(SendError::from),
            Self::Qq {
                db,
                openid,
                inbound_msg_id,
            } => crate::services::qq_work::send_c2c_image(
                db,
                &crate::services::qq_bot::outbound_auth_header()
                    .await
                    .map_err(|_| SendError::Transient)?,
                openid,
                &image.bytes,
                inbound_msg_id.as_deref(),
            )
            .await
            .map_err(SendError::from),
            Self::Feishu { chat_id } => crate::services::feishu_bot_api::send_photo(
                chat_id,
                &image.bytes,
                &image.mime,
                markup,
            )
            .await
            .map_err(SendError::from),
            Self::OneBot {
                user_id,
                temp_group,
            } => {
                // SnowLuma fetches a `file` URL itself and cannot reach a site-relative
                // image-cache path. Send the bytes already read, as other platforms do.
                use base64::Engine as _;
                let inline = format!(
                    "base64://{}",
                    base64::engine::general_purpose::STANDARD.encode(&image.bytes)
                );
                let Some(action) = myriad_agent_rules::onebot::encode::plan_private_delivery(
                    user_id,
                    "",
                    &[inline],
                ) else {
                    return Ok(());
                };
                let action = myriad_agent_rules::onebot::encode::via_temp_session(
                    action,
                    temp_group.as_deref(),
                );
                map_onebot_send(crate::services::onebot_send::send_action(action).await)
            }
        }
    }
}

fn map_onebot_send(
    result: Result<Option<myriad_agent_rules::channel::ConnectFailureKind>, String>,
) -> Result<(), SendError> {
    match result {
        Ok(None) => Ok(()),
        Ok(Some(kind)) => Err(SendError::from(kind)),
        Err(_) => Err(SendError::Transient),
    }
}

/// Why a platform did not accept an outbound item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum SendError {
    /// Network, 5xx, rate limit: the same item may go through later.
    Transient,
    /// The platform refused this item (bad request, forbidden, revoked
    /// token); resending it unchanged cannot succeed.
    Permanent,
}

impl From<myriad_agent_rules::channel::ConnectFailureKind> for SendError {
    fn from(kind: myriad_agent_rules::channel::ConnectFailureKind) -> Self {
        match kind {
            myriad_agent_rules::channel::ConnectFailureKind::Transient => Self::Transient,
            myriad_agent_rules::channel::ConnectFailureKind::Permanent => Self::Permanent,
        }
    }
}

impl std::fmt::Display for SendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Transient => "channel send failed; retryable",
            Self::Permanent => "channel send refused by the platform",
        })
    }
}

/// Persistable routing information. No token or credential can enter the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) enum ChannelAddress {
    Telegram {
        chat_id: String,
    },
    Discord {
        channel_id: String,
    },
    Qq {
        openid: String,
    },
    Feishu {
        chat_id: String,
    },
    OneBot {
        user_id: String,
        /// Absent in addresses stored before temp sessions were answered.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        temp_group: Option<String>,
    },
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
            Self::OneBot {
                user_id,
                temp_group,
            } => ChannelAddress::OneBot {
                user_id: user_id.clone(),
                temp_group: temp_group.clone(),
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
            Self::OneBot {
                user_id,
                temp_group,
            } => Some(ChannelTransport::OneBot {
                user_id: user_id.clone(),
                temp_group: temp_group.clone(),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn refused_items_are_not_retried_like_outages() {
        use myriad_agent_rules::channel::{ConnectFailure, classify_connect_failure};
        let status = |status: u16| {
            super::SendError::from(classify_connect_failure(&ConnectFailure::HttpStatus {
                status,
                body: "",
            }))
        };
        assert_eq!(status(400), super::SendError::Permanent);
        assert_eq!(status(403), super::SendError::Permanent);
        assert_eq!(status(429), super::SendError::Transient);
        assert_eq!(status(503), super::SendError::Transient);
        let delivery = include_str!("delivery.rs");
        let permanent = delivery
            .split("Err(super::transport::SendError::Permanent)")
            .nth(1)
            .expect("delivery handles refusals")
            .split("Err(error @")
            .next()
            .unwrap();
        assert!(
            permanent.contains("acknowledge_item"),
            "a refusal skips the item"
        );
    }

    #[test]
    fn inbound_platform_files_persist_as_user_channel_assets() {
        let prod = include_str!("transport/media.rs");
        assert!(prod.contains("MediaSource::Channel"));
        assert!(prod.contains("persist_channel_asset"));
        assert!(prod.contains("MediaContext::user"));
        assert!(!prod.contains("register_if_created"));
    }
}
