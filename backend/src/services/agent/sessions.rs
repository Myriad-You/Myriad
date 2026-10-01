//! Reading her conversations back: which sessions are a person's own (not a
//! group's), and a session's recent messages as a conversation. The HTTP
//! side of sessions is in `api::agent`.

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ActiveValue::Set, ColumnTrait, DatabaseConnection, EntityTrait,
    PaginatorTrait, QueryFilter, QueryOrder,
};
use serde_json::{Value, json};

use crate::models::entities::{agent_messages, agent_sessions};

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

/// A person's last `max_messages` with her in private chat, oldest first,
/// whichever of their private conversations they were in (the site's panel,
/// a chat app): one talk going on between them, not one per window. Groups
/// and Work are not part of it, nor a conversation they deleted.
pub(crate) async fn load_private_chat_history(
    db: &DatabaseConnection,
    user_id: i32,
    max_messages: u64,
) -> Result<Vec<crate::services::agent::ConversationMessage>, sea_orm::DbErr> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.role, m.content, m.metadata, m.created_at FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL \
               AND NOT s.archived \
             ORDER BY m.created_at DESC LIMIT $2",
            [user_id.into(), (max_messages as i64).into()],
        ))
        .await?;
    let mut history = Vec::with_capacity(rows.len());
    for row in rows.iter().rev() {
        let role: String = row.try_get("", "role")?;
        let content: String = row.try_get("", "content")?;
        let metadata: Option<Value> = row.try_get("", "metadata")?;
        let created_at: chrono::DateTime<chrono::FixedOffset> = row.try_get("", "created_at")?;
        history.push(
            crate::services::agent::chat_prompt::reconstruct_conversation_message(
                role,
                content,
                Some(created_at.to_rfc3339()),
                metadata.as_ref(),
                true,
            ),
        );
    }
    Ok(history)
}

pub(crate) fn session_store_failed(context: &'static str, error: impl std::fmt::Display) -> String {
    tracing::error!(%error, context, "agent session store failed");
    format!("Failed to {context}")
}

pub(crate) fn session_mode(
    context: Option<&Value>,
) -> crate::services::agent::AgentInteractionMode {
    match context
        .and_then(|value| value.get("mode"))
        .and_then(Value::as_str)
    {
        Some("chat") => crate::services::agent::AgentInteractionMode::Chat,
        _ => crate::services::agent::AgentInteractionMode::Work,
    }
}

pub(crate) fn session_context(mode: crate::services::agent::AgentInteractionMode) -> Value {
    session_context_in(mode, None)
}

pub(crate) fn session_context_in(
    mode: crate::services::agent::AgentInteractionMode,
    venue: Option<&str>,
) -> Value {
    match venue {
        Some(venue) => json!({ "mode": mode.as_str(), "venue": venue }),
        None => json!({ "mode": mode.as_str() }),
    }
}

/// The group a session belongs to, if it is a group's.
pub(crate) fn session_venue(context: Option<&Value>) -> Option<String> {
    context
        .and_then(|value| value.get("venue"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

// 会话管理 API

/// 创建会话
/// POST /api/agent/sessions

pub(crate) fn require_user_message_persisted<T, E>(result: Result<T, E>) -> Result<T, E> {
    result
}

/// 标题降级：截取第一条用户消息
pub(crate) fn fallback_title(messages: &[agent_messages::Model]) -> String {
    messages
        .iter()
        .find(|m| m.role == "user")
        .map(|m| {
            let s: String = m.content.chars().take(47).collect();
            if m.content.chars().count() > 50 {
                format!("{}...", s)
            } else {
                s
            }
        })
        .unwrap_or_else(|| "New conversation".to_string())
}

/// 会话消息持久化辅助函数
pub(crate) async fn persist_user_message(
    db: &DatabaseConnection,
    session_id: &str,
    content: &str,
) -> Result<(), String> {
    let now = Utc::now().fixed_offset();
    let msg = agent_messages::ActiveModel {
        id: sea_orm::ActiveValue::NotSet,
        session_id: Set(session_id.to_string()),
        task_id: Set(None),
        role: Set("user".to_string()),
        content: Set(content.to_string()),
        metadata: Set(None),
        created_at: Set(now),
    };
    let inserted = agent_messages::Entity::insert(msg)
        .exec(db)
        .await
        .map_err(|error| session_store_failed("save user message", error))?;
    bind_message_media(db, session_id, inserted.last_insert_id, content, None).await;
    Ok(())
}

/// Protect media a stored message shows. History is read long after the run,
/// so its images must not look unreferenced in the media library. Bound as
/// the session owner: a message cannot pin someone else's media, and guests
/// bind nothing. Never fails the message write: dead links are skipped and
/// a binding error is only logged.
pub(crate) async fn bind_message_media(
    db: &DatabaseConnection,
    session_id: &str,
    message_id: i32,
    content: &str,
    metadata: Option<&Value>,
) {
    use crate::services::media::{Authority, Citations, Consumer, MediaActor, Unresolved, bind};
    use sea_orm::TransactionTrait;
    let owner = agent_sessions::Entity::find_by_id(session_id)
        .one(db)
        .await
        .ok()
        .flatten()
        .map(|session| session.user_id);
    let actor = owner.and_then(|id| MediaActor::user(id).ok());
    let authority = actor
        .as_ref()
        .map_or(Authority::Anonymous, Authority::Actor);
    let origins = crate::services::media::upgrade::configured_origins().await;
    let mut citations = Citations::fields(&origins, None, content);
    if let Some(metadata) = metadata {
        for citation in Citations::strings(&origins, metadata, |i| format!("meta:{i}")).iter() {
            citations.push_path(citation.slot.clone(), citation.path.clone());
        }
    }
    let result = async {
        let txn = db.begin().await?;
        bind(
            &txn,
            &Consumer::agent_message(message_id),
            &citations,
            authority,
            Unresolved::Skip,
        )
        .await?;
        txn.commit().await?;
        Ok::<_, crate::services::media::MediaError>(())
    }
    .await;
    if let Err(error) = result {
        tracing::warn!(%error, message_id, "agent message media references not recorded");
    }
}

pub(crate) async fn persist_assistant_message(
    db: &DatabaseConnection,
    session_id: &str,
    task_id: Option<&str>,
    content: &str,
    metadata: Option<Value>,
) -> Result<(), String> {
    let now = Utc::now().fixed_offset();
    let cited_metadata = metadata.clone();
    let msg = agent_messages::ActiveModel {
        id: sea_orm::ActiveValue::NotSet,
        session_id: Set(session_id.to_string()),
        task_id: Set(task_id.map(|s| s.to_string())),
        role: Set("assistant".to_string()),
        content: Set(content.to_string()),
        metadata: Set(metadata),
        created_at: Set(now),
    };
    let inserted = agent_messages::Entity::insert(msg)
        .exec(db)
        .await
        .map_err(|error| session_store_failed("save assistant message", error))?;
    bind_message_media(
        db,
        session_id,
        inserted.last_insert_id,
        content,
        cited_metadata.as_ref(),
    )
    .await;

    // 更新会话消息计数和最后活跃时间
    if let Ok(Some(session)) = agent_sessions::Entity::find_by_id(session_id).one(db).await {
        let mut active: agent_sessions::ActiveModel = session.into();
        active.last_active_at = Set(now);
        if let Ok(count) = agent_messages::Entity::find()
            .filter(agent_messages::Column::SessionId.eq(session_id))
            .count(db)
            .await
        {
            active.message_count = Set(count as i32);
        }
        let _ = active.update(db).await;
    }

    Ok(())
}

/// 加载会话历史消息作为对话上下文。
///
/// Chat (`for_chat`) 只保留对白；Work 仍可把任务元数据、确认卡和前端动作
/// 追加进 planner 可见内容。
/// Her spoken reply was cut off after it had been saved whole: the voice runs
/// behind the text, so mark it as possibly not heard to the end.
pub(crate) async fn mark_spoken_reply_cut_off(
    db: &DatabaseConnection,
    session_id: &str,
    run_id: &str,
) -> Result<bool, sea_orm::DbErr> {
    let recent = agent_messages::Entity::find()
        .filter(agent_messages::Column::SessionId.eq(session_id))
        .filter(agent_messages::Column::Role.eq("assistant"))
        .order_by_desc(agent_messages::Column::CreatedAt)
        .paginate(db, 4)
        .fetch_page(0)
        .await?;
    let Some(reply) = recent.into_iter().find(|message| {
        message
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get("runId"))
            .and_then(Value::as_str)
            == Some(run_id)
    }) else {
        return Ok(false);
    };
    let mut metadata = reply.metadata.clone().unwrap_or_else(|| json!({}));
    if let Some(object) = metadata.as_object_mut() {
        object.insert(
            crate::services::agent::chat_prompt::CUT_OFF_KEY.into(),
            json!(crate::services::agent::chat_prompt::cut_off_kind(true)),
        );
    }
    let mut active: agent_messages::ActiveModel = reply.into();
    active.metadata = Set(Some(metadata));
    active.update(db).await?;
    Ok(true)
}

/// A private session: the one asked for if it belongs to the user, has this
/// mode and is not a group's; otherwise a new private one.
pub(crate) async fn ensure_session(
    db: &DatabaseConnection,
    session_id: Option<&str>,
    user_id: i32,
    mode: crate::services::agent::AgentInteractionMode,
) -> Result<String, String> {
    ensure_session_in(db, session_id, user_id, mode, None).await
}

/// The session asked for if it belongs to the user and has this mode and
/// venue (`None` private, or a group such as `telegram:-100123`); otherwise
/// a new one, carrying its venue from the moment it exists. A group's
/// conversation is never read back as a private one, and a private request
/// never continues a group's session.
pub(crate) async fn ensure_session_in(
    db: &DatabaseConnection,
    session_id: Option<&str>,
    user_id: i32,
    mode: crate::services::agent::AgentInteractionMode,
    venue: Option<&str>,
) -> Result<String, String> {
    if let Some(sid) = session_id {
        if let Some(session) = agent_sessions::Entity::find_by_id(sid)
            .filter(agent_sessions::Column::UserId.eq(user_id))
            .one(db)
            .await
            .map_err(|error| session_store_failed("find session", error))?
        {
            let stored_venue = session_venue(session.context.as_ref());
            if session_mode(session.context.as_ref()) == mode && stored_venue.as_deref() == venue {
                return Ok(sid.to_string());
            }
            tracing::info!(
                session_id = sid,
                requested_mode = mode.as_str(),
                stored_mode = session_mode(session.context.as_ref()).as_str(),
                group_session = stored_venue.is_some(),
                group_request = venue.is_some(),
                "[Agent API] Session mode or venue mismatch; creating an isolated session"
            );
        }
    }

    // 自动创建新会话
    let new_id = format!("ses_{}", uuid::Uuid::new_v4().simple());
    let now = Utc::now().fixed_offset();
    let session = agent_sessions::ActiveModel {
        id: Set(new_id.clone()),
        user_id: Set(user_id),
        title: Set(None),
        context: Set(Some(session_context_in(mode, venue))),
        message_count: Set(0),
        archived: Set(false),
        created_at: Set(now),
        last_active_at: Set(now),
    };
    agent_sessions::Entity::insert(session)
        .exec(db)
        .await
        .map_err(|error| session_store_failed("create session", error))?;

    Ok(new_id)
}
