//! Reading her conversations back: which sessions are a person's own (not a
//! group's), and a session's recent messages as a conversation. The HTTP
//! side of sessions is in `api::agent`.

use sea_orm::{
    ColumnTrait, DatabaseConnection, EntityTrait, PaginatorTrait, QueryFilter, QueryOrder,
};

use crate::models::entities::agent_messages;

/// Sessions that are the user's own conversations, not a group's.
pub(crate) fn private_sessions() -> sea_orm::sea_query::SimpleExpr {
    sea_orm::sea_query::Expr::cust("(agent_sessions.context::jsonb ->> 'venue') IS NULL")
}

/// A session's last `max_messages`, oldest first, as a conversation.
pub(crate) async fn load_session_history(
    db: &DatabaseConnection,
    session_id: &str,
    max_messages: u64,
    for_chat: bool,
) -> Result<Vec<crate::services::agent::ConversationMessage>, sea_orm::DbErr> {
    let messages = agent_messages::Entity::find()
        .filter(agent_messages::Column::SessionId.eq(session_id))
        .order_by_desc(agent_messages::Column::CreatedAt)
        .paginate(db, max_messages)
        .fetch_page(0)
        .await?;

    Ok(messages
        .into_iter()
        .rev()
        .map(|message| {
            crate::services::agent::chat_prompt::reconstruct_conversation_message(
                message.role,
                message.content,
                Some(message.created_at.to_rfc3339()),
                message.metadata.as_ref(),
                for_chat,
            )
        })
        .collect())
}
