//! OneBot private chat: pairing already resolved → shared private-chat Work.

use sea_orm::DatabaseConnection;

use crate::services::channel_work::{self, ChannelTransport};

pub async fn start_paired_work_with_images(
    db: &DatabaseConnection,
    user_id: i32,
    sender_id: &str,
    user_qq: &str,
    input: &str,
    images: &[myriad_agent_rules::channel::ChannelImageRef],
    session_key: &str,
) {
    channel_work::handle_text_with_images(
        db,
        user_id,
        sender_id,
        user_qq,
        input,
        images,
        session_key,
        ChannelTransport::OneBot {
            user_id: user_qq.to_string(),
        },
    )
    .await;
}
