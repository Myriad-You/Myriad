//! Agent API — sessions
use super::*;
use crate::error::HttpError;
use myriad_error::AppError;

fn session_store_http(context: &'static str, error: impl std::fmt::Display) -> HttpError {
    tracing::error!(%error, context, "agent session store failed");
    HttpError(AppError::internal(format!("Failed to {context}")))
}

fn session_store_failed(context: &'static str, error: impl std::fmt::Display) -> String {
    tracing::error!(%error, context, "agent session store failed");
    format!("Failed to {context}")
}

fn session_mode(context: Option<&Value>) -> crate::services::agent::AgentInteractionMode {
    match context
        .and_then(|value| value.get("mode"))
        .and_then(Value::as_str)
    {
        Some("chat") => crate::services::agent::AgentInteractionMode::Chat,
        _ => crate::services::agent::AgentInteractionMode::Work,
    }
}

fn session_context(mode: crate::services::agent::AgentInteractionMode) -> Value {
    json!({ "mode": mode.as_str() })
}

// 会话管理 API

/// 创建会话
/// POST /api/agent/sessions
pub async fn create_session(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    let session_id = format!("ses_{}", uuid::Uuid::new_v4().simple());
    let now = Utc::now().fixed_offset();

    let session = agent_sessions::ActiveModel {
        id: Set(session_id.clone()),
        user_id: Set(user_id),
        title: Set(None),
        context: Set(Some(session_context(
            crate::services::agent::AgentInteractionMode::Work,
        ))),
        message_count: Set(0),
        archived: Set(false),
        created_at: Set(now),
        last_active_at: Set(now),
    };

    agent_sessions::Entity::insert(session)
        .exec(&db)
        .await
        .map_err(|error| session_store_http("create session", error))?;

    Ok(Json(json!({
        "id": session_id,
        "title": null,
        "messageCount": 0,
        "lastActiveAt": now.to_rfc3339(),
    })))
}

/// 会话列表查询参数
#[derive(Debug, Deserialize)]
pub struct SessionListQuery {
    #[serde(default = "default_page")]
    pub page: u64,
    #[serde(default = "default_limit")]
    pub limit: u64,
}

pub(crate) fn default_page() -> u64 {
    1
}
pub(crate) fn default_limit() -> u64 {
    20
}

const SESSION_PAGE_MIN: u64 = 1;
const SESSION_LIMIT_MIN: u64 = 1;
const SESSION_LIMIT_MAX: u64 = 100;

fn session_pagination(query: &SessionListQuery) -> Result<(u64, u64), HttpError> {
    if query.page < SESSION_PAGE_MIN {
        return Err(HttpError(AppError::bad_request("page must be >= 1")));
    }
    if query.limit < SESSION_LIMIT_MIN || query.limit > SESSION_LIMIT_MAX {
        return Err(HttpError(AppError::bad_request(format!(
            "limit must be between {SESSION_LIMIT_MIN} and {SESSION_LIMIT_MAX}"
        ))));
    }
    let page_index = query.page - 1;
    if page_index.checked_mul(query.limit).is_none() {
        return Err(HttpError(AppError::bad_request(
            "pagination offset overflow",
        )));
    }
    Ok((query.limit, page_index))
}

/// 列出最近会话
/// GET /api/agent/sessions
pub async fn list_sessions(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<SessionListQuery>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    if crate::services::agent::merope::is_logged_in_addressee(user_id) {
        crate::services::agent::merope::spawn_presence(user_id);
    }

    let (limit, page_index) = session_pagination(&query)?;
    let sessions = agent_sessions::Entity::find()
        .filter(agent_sessions::Column::UserId.eq(user_id))
        .filter(agent_sessions::Column::Archived.eq(false))
        .order_by_desc(agent_sessions::Column::LastActiveAt)
        .paginate(&db, limit)
        .fetch_page(page_index)
        .await
        .map_err(|error| session_store_http("list sessions", error))?;

    let sessions_json: Vec<Value> = sessions
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "title": s.title,
                "messageCount": s.message_count,
                "lastActiveAt": s.last_active_at.to_rfc3339(),
                "createdAt": s.created_at.to_rfc3339(),
                "mode": session_mode(s.context.as_ref()).as_str(),
            })
        })
        .collect();

    Ok(Json(json!({ "sessions": sessions_json })))
}

/// 获取会话消息
/// GET /api/agent/sessions/:id/messages
pub async fn get_session_messages(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(session_id): Path<String>,
    Query(query): Query<SessionListQuery>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;

    // 验证会话归属
    let session = agent_sessions::Entity::find_by_id(&session_id)
        .filter(agent_sessions::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|error| session_store_http("find session", error))?;

    if session.is_none() {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Session not found")),
        )));
    }

    let (limit, page_index) = session_pagination(&query)?;
    let messages = agent_messages::Entity::find()
        .filter(agent_messages::Column::SessionId.eq(&session_id))
        .order_by_asc(agent_messages::Column::CreatedAt)
        .paginate(&db, limit)
        .fetch_page(page_index)
        .await
        .map_err(|error| session_store_http("load session messages", error))?;

    let messages_json: Vec<Value> = messages
        .into_iter()
        .map(|m| {
            json!({
                "id": m.id,
                "sessionId": m.session_id,
                "taskId": m.task_id,
                "role": m.role,
                "content": m.content,
                "metadata": m.metadata,
                "createdAt": m.created_at.to_rfc3339(),
            })
        })
        .collect();

    Ok(Json(json!({ "messages": messages_json })))
}

/// 归档会话
/// DELETE /api/agent/sessions/:id
pub async fn archive_session(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;

    let session = agent_sessions::Entity::find_by_id(&session_id)
        .filter(agent_sessions::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|error| session_store_http("find session", error))?;

    if session.is_none() {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Session not found")),
        )));
    }

    let mut active: agent_sessions::ActiveModel = session.unwrap().into();
    active.archived = Set(true);
    active
        .update(&db)
        .await
        .map_err(|error| session_store_http("archive session", error))?;

    Ok(Json(json!({"success": true})))
}

/// 更新会话标题请求
#[derive(Debug, Deserialize)]
pub struct UpdateSessionRequest {
    pub title: Option<String>,
}

/// 更新会话标题
/// PATCH /api/agent/sessions/:id
pub async fn update_session(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(session_id): Path<String>,
    Json(req): Json<UpdateSessionRequest>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;

    let session = agent_sessions::Entity::find_by_id(&session_id)
        .filter(agent_sessions::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|error| session_store_http("find session", error))?;

    if session.is_none() {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Session not found")),
        )));
    }

    let mut active: agent_sessions::ActiveModel = session.unwrap().into();
    if let Some(title) = req.title {
        active.title = Set(Some(title));
    }
    let updated = active
        .update(&db)
        .await
        .map_err(|error| session_store_http("update session", error))?;

    Ok(Json(json!({
        "id": updated.id,
        "title": updated.title,
        "messageCount": updated.message_count,
        "lastActiveAt": updated.last_active_at.to_rfc3339(),
    })))
}

/// AI 生成会话标题
/// POST /api/agent/sessions/:id/generate-title
pub async fn generate_session_title(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(session_id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;

    // 验证会话属于当前用户
    let session = agent_sessions::Entity::find_by_id(&session_id)
        .filter(agent_sessions::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|error| session_store_http("find session", error))?;

    if session.is_none() {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Session not found")),
        )));
    }

    // Oldest four messages (`order_by_asc` + page 0) as title context.
    let messages = session_title_messages(
        agent_messages::Entity::find()
            .filter(agent_messages::Column::SessionId.eq(&session_id))
            .order_by_asc(agent_messages::Column::CreatedAt)
            .paginate(&db, 4)
            .fetch_page(0)
            .await,
    )
    .map_err(|error| session_store_http("load session messages for title", error))?;

    if messages.is_empty() {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("No messages in session")),
        )));
    }

    let context: String = messages
        .iter()
        .map(|m| {
            format!(
                "{}: {}",
                m.role,
                m.content.chars().take(200).collect::<String>()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    // 调用 AI 生成标题
    let analyzer =
        crate::services::ai::create_ai_analyzer_for_tier(crate::config::ModelTier::Standard).await;

    let title = if let Some(analyzer) = analyzer {
        let prompt = format!(
            "Based on the following conversation, generate a concise session title (5-15 characters, in the same language as the user). \
             Return ONLY the title text, no quotes, no explanation.\n\n{}",
            context
        );
        match crate::services::ai_cost_ledger::with_site_ai_ledger(
            user_id,
            "agent",
            "session_title",
            analyzer.analyze(&prompt),
        )
        .await
        {
            Ok(raw) => {
                // 清理：去掉首尾引号、多余空白
                let cleaned = raw
                    .trim()
                    .trim_matches('"')
                    .trim_matches('\u{300c}')
                    .trim_matches('\u{300d}')
                    .trim();
                if cleaned.is_empty() || cleaned.len() > 100 {
                    fallback_title(&messages)
                } else {
                    cleaned.to_string()
                }
            }
            Err(e) => {
                tracing::warn!("[Agent API] AI title generation failed: {}", e);
                fallback_title(&messages)
            }
        }
    } else {
        fallback_title(&messages)
    };

    // 更新数据库
    let mut active: agent_sessions::ActiveModel = session.unwrap().into();
    active.title = Set(Some(title.clone()));
    session_title_write(active.update(&db).await)
        .map_err(|error| session_store_http("update session title", error))?;

    Ok(Json(json!({ "title": title })))
}

/// Query errors must not be treated as an empty session.
pub(crate) fn session_title_messages<T, E>(result: Result<Vec<T>, E>) -> Result<Vec<T>, E> {
    result
}

/// Title writes must fail the request when persist fails.
pub(crate) fn session_title_write<T, E>(result: Result<T, E>) -> Result<T, E> {
    result
}

/// Durable user-message writes abort the turn; callers must not ignore Err.
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
    agent_messages::Entity::insert(msg)
        .exec(db)
        .await
        .map_err(|error| session_store_failed("save user message", error))?;
    Ok(())
}

pub(crate) async fn persist_assistant_message(
    db: &DatabaseConnection,
    session_id: &str,
    task_id: Option<&str>,
    content: &str,
    metadata: Option<Value>,
) -> Result<(), String> {
    let now = Utc::now().fixed_offset();
    let msg = agent_messages::ActiveModel {
        id: sea_orm::ActiveValue::NotSet,
        session_id: Set(session_id.to_string()),
        task_id: Set(task_id.map(|s| s.to_string())),
        role: Set("assistant".to_string()),
        content: Set(content.to_string()),
        metadata: Set(metadata),
        created_at: Set(now),
    };
    agent_messages::Entity::insert(msg)
        .exec(db)
        .await
        .map_err(|error| session_store_failed("save assistant message", error))?;

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

/// Return the session if id+user+mode match; otherwise insert a new row (None, missing, or mode mismatch).
pub(crate) async fn ensure_session(
    db: &DatabaseConnection,
    session_id: Option<&str>,
    user_id: i32,
    mode: crate::services::agent::AgentInteractionMode,
) -> Result<String, String> {
    if let Some(sid) = session_id {
        if let Some(session) = agent_sessions::Entity::find_by_id(sid)
            .filter(agent_sessions::Column::UserId.eq(user_id))
            .one(db)
            .await
            .map_err(|error| session_store_failed("find session", error))?
        {
            if session_mode(session.context.as_ref()) == mode {
                return Ok(sid.to_string());
            }
            tracing::info!(
                session_id = sid,
                requested_mode = mode.as_str(),
                stored_mode = session_mode(session.context.as_ref()).as_str(),
                "[Agent API] Session mode mismatch; creating an isolated session"
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
        context: Set(Some(session_context(mode))),
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

#[cfg(test)]
mod mode_tests {
    use super::*;
    use crate::services::agent::AgentInteractionMode;

    #[test]
    fn legacy_session_context_is_work() {
        assert_eq!(session_mode(None), AgentInteractionMode::Work);
        assert_eq!(session_mode(Some(&json!({}))), AgentInteractionMode::Work);
    }

    #[test]
    fn session_context_preserves_chat_mode() {
        let context = session_context(AgentInteractionMode::Chat);
        assert_eq!(context, json!({ "mode": "chat" }));
        assert_eq!(session_mode(Some(&context)), AgentInteractionMode::Chat);
    }

    fn assert_pagination_400(query: SessionListQuery) {
        let error = session_pagination(&query).expect_err("expected 400");
        assert_eq!(error.0.status_u16(), 400);
    }

    #[test]
    fn session_pagination_rejects_zero_and_overflow() {
        assert_pagination_400(SessionListQuery { page: 1, limit: 0 });
        assert_pagination_400(SessionListQuery { page: 0, limit: 20 });
        assert_pagination_400(SessionListQuery {
            page: 1,
            limit: SESSION_LIMIT_MAX + 1,
        });
        assert_pagination_400(SessionListQuery {
            page: u64::MAX,
            limit: 2,
        });

        let ok = session_pagination(&SessionListQuery { page: 2, limit: 20 }).expect("ok");
        assert_eq!(ok, (20, 1));
    }

    #[test]
    fn title_query_error_is_not_empty_session() {
        let error: Result<Vec<i32>, &str> = Err("db down");
        assert!(session_title_messages(error).is_err());
        assert!(
            session_title_messages::<i32, &str>(Ok(vec![]))
                .unwrap()
                .is_empty()
        );
    }

    #[test]
    fn title_write_error_is_not_success() {
        let error: Result<(), &str> = Err("write failed");
        assert!(session_title_write(error).is_err());
        assert!(session_title_write::<(), &str>(Ok(())).is_ok());
    }

    #[test]
    fn user_message_persist_error_aborts_the_turn() {
        let error: Result<(), &str> = Err("db down");
        assert!(require_user_message_persisted(error).is_err());
        assert!(require_user_message_persisted::<(), &str>(Ok(())).is_ok());
    }
}
