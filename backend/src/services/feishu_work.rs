//! Feishu p2p adapter: pairing already resolved → shared private-chat Work.

use sea_orm::DatabaseConnection;

use crate::services::channel_work::{self, ChannelTransport};

pub async fn start_paired_work_with_images(
    db: &DatabaseConnection,
    user_id: i32,
    sender_id: &str,
    chat_id: &str,
    input: &str,
    images: &[myriad_agent_rules::channel::ChannelImageRef],
    session_key: &str,
    message_id: &str,
) {
    channel_work::handle_text_with_images(
        db,
        user_id,
        sender_id,
        chat_id,
        input,
        images,
        session_key,
        message_id,
        ChannelTransport::Feishu {
            chat_id: chat_id.to_string(),
        },
    )
    .await;
}

pub async fn start_paired_callback(
    db: &DatabaseConnection,
    user_id: i32,
    sender_id: &str,
    chat_id: &str,
    data: &str,
    session_key: &str,
    inbound_id: &str,
) {
    channel_work::handle_callback(
        db,
        user_id,
        sender_id,
        chat_id,
        data,
        session_key,
        inbound_id,
        ChannelTransport::Feishu {
            chat_id: chat_id.to_string(),
        },
    )
    .await;
}
