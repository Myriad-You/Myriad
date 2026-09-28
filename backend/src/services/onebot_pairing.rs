//! OneBot private-chat pairing. The QQ number is the identity key.

use myriad_agent_rules::channel::InboundC2cText;
use sea_orm::DatabaseConnection;

use crate::services::channel_pairing::{self, PrivateText};
use crate::services::channel_platform::ChannelPlatform;

struct OneBotText {
    inbound: InboundC2cText,
}

impl PrivateText for OneBotText {
    const PLATFORM: ChannelPlatform = ChannelPlatform::OneBot;

    fn inbound(&self) -> &InboundC2cText {
        &self.inbound
    }

    fn session_chat_id(&self) -> String {
        self.inbound.user_openid.clone()
    }

    async fn reply(&self, _db: &DatabaseConnection, text: &str) {
        let Some(action) = myriad_agent_rules::onebot::encode::plan_private_delivery(
            &self.inbound.user_openid,
            text,
            &[],
        ) else {
            return;
        };
        match crate::services::onebot_send::send_action(action).await {
            Ok(None) => {}
            Ok(Some(_)) => tracing::warn!("OneBot pairing reply refused"),
            Err(error) => tracing::warn!(error = %error, "OneBot pairing reply failed"),
        }
    }

    async fn start_work(
        &self,
        db: &DatabaseConnection,
        user_id: i32,
        input: &str,
        session_key: &str,
    ) {
        // Voice, file, and other segments are dropped by the decoder. A paired
        // sender would otherwise start Work with an empty input and no reply.
        if input.trim().is_empty() && self.inbound.images.is_empty() {
            self.reply(
                db,
                "这条消息里没有文字或图片，语音、文件和其他类型暂不支持。",
            )
            .await;
            return;
        }
        crate::services::onebot_work::start_paired_work_with_images(
            db,
            user_id,
            &self.inbound.user_openid,
            &self.inbound.user_openid,
            input,
            &self.inbound.images,
            session_key,
        )
        .await;
    }
}

/// Worker entry for one decoded private message.
pub async fn handle_inbound(inbound: InboundC2cText) {
    channel_pairing::handle_private_text(OneBotText { inbound }).await;
}
