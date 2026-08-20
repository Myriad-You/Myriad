//! Agent API — sessions
use super::*;
use crate::error::HttpError;

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
        context: Set(None),
        message_count: Set(0),
        archived: Set(false),
        created_at: Set(now),
        last_active_at: Set(now),
    };

    agent_sessions::Entity::insert(session)
        .exec(&db)
        .await
        .map_err(|e| HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                { tracing::error!("Failed to create session: {}", e); Json(json!({"error": "Database error"})) },
            )))?;

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

/// 列出最近会话
/// GET /api/agent/sessions
pub async fn list_sessions(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Query(query): Query<SessionListQuery>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;
    if crate::services::agent::life::is_logged_in_addressee(user_id) {
        crate::services::agent::life::spawn_presence(user_id);
    }

    let sessions = agent_sessions::Entity::find()
        .filter(agent_sessions::Column::UserId.eq(user_id))
        .filter(agent_sessions::Column::Archived.eq(false))
        .order_by_desc(agent_sessions::Column::LastActiveAt)
        .paginate(&db, query.limit)
        .fetch_page(query.page.saturating_sub(1))
        .await
        .map_err(|e| HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                { tracing::error!("Failed to list sessions: {}", e); Json(json!({"error": "Database error"})) },
            )))?;

    let sessions_json: Vec<Value> = sessions
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "title": s.title,
                "messageCount": s.message_count,
                "lastActiveAt": s.last_active_at.to_rfc3339(),
                "createdAt": s.created_at.to_rfc3339(),
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
        .map_err(|_e| HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )))?;

    if session.is_none() {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Session not found"})),
        )));
    }

    let messages = agent_messages::Entity::find()
        .filter(agent_messages::Column::SessionId.eq(&session_id))
        .order_by_asc(agent_messages::Column::CreatedAt)
        .paginate(&db, query.limit)
        .fetch_page(query.page.saturating_sub(1))
        .await
        .map_err(|e| HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                { tracing::error!("Failed to list messages: {}", e); Json(json!({"error": "Database error"})) },
            )))?;

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
        .map_err(|_e| HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )))?;

    if session.is_none() {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Session not found"})),
        )));
    }

    let mut active: agent_sessions::ActiveModel = session.unwrap().into();
    active.archived = Set(true);
    active.update(&db).await.map_err(|e| HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            { tracing::error!("Failed to archive session: {}", e); Json(json!({"error": "Database error"})) },
        )))?;

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
        .map_err(|_e| HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )))?;

    if session.is_none() {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Session not found"})),
        )));
    }

    let mut active: agent_sessions::ActiveModel = session.unwrap().into();
    if let Some(title) = req.title {
        active.title = Set(Some(title));
    }
    let updated = active.update(&db).await.map_err(|e| HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            { tracing::error!("Failed to update session: {}", e); Json(json!({"error": "Database error"})) },
        )))?;

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
        .map_err(|_e| HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": "Database error"})),
            )))?;

    if session.is_none() {
        return Err(HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({"error": "Session not found"})),
        )));
    }

    // 加载最近几条消息作为标题生成上下文
    let messages = agent_messages::Entity::find()
        .filter(agent_messages::Column::SessionId.eq(&session_id))
        .order_by_asc(agent_messages::Column::CreatedAt)
        .paginate(&db, 4)
        .fetch_page(0)
        .await
        .unwrap_or_default();

    if messages.is_empty() {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({"error": "No messages in session"})),
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
        match analyzer.analyze(&prompt).await {
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
    let _ = active.update(&db).await;

    Ok(Json(json!({ "title": title })))
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
        .map_err(|e| { tracing::error!("Failed to persist user message: {}", e); "Database error".to_string() })?;
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
        .map_err(|e| { tracing::error!("Failed to persist assistant message: {}", e); "Database error".to_string() })?;

    // 更新会话消息计数和最后活跃时间
    if let Ok(Some(session)) = agent_sessions::Entity::find_by_id(session_id).one(db).await {
        let mut active: agent_sessions::ActiveModel = session.into();
        active.last_active_at = Set(now);
        // message_count 用 raw SQL 更新可能更好，但这里简单处理
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

/// 加载会话历史消息作为对话上下文
pub(crate) async fn load_session_history(
    db: &DatabaseConnection,
    session_id: &str,
    max_messages: u64,
) -> Vec<crate::services::agent::ConversationMessage> {
    use crate::services::agent::ConversationMessage;

    let messages = agent_messages::Entity::find()
        .filter(agent_messages::Column::SessionId.eq(session_id))
        .order_by_desc(agent_messages::Column::CreatedAt)
        .paginate(db, max_messages)
        .fetch_page(0)
        .await
        .unwrap_or_default();

    // 反转为时间正序，assistant 消息附带 metadata 摘要
    messages
        .into_iter()
        .rev()
        .map(|m| {
            let mut content = m.content.clone();
            // 将 metadata 中的关键信息追加到 assistant 内容，让 planner 了解上轮输出
            if m.role == "assistant" {
                if let Some(ref meta) = m.metadata {
                    let mut extras = Vec::new();
                    if let Some(data) = meta.get("data") {
                        if !data.is_null() {
                            // 截取摘要，避免过长
                            let s = data.to_string();
                            if s.len() > 2 && s != "null" {
                                let truncated: String = s.chars().take(500).collect();
                                extras.push(format!("[输出数据: {}]", truncated));
                            }
                        }
                    }
                    if let Some(dd) = meta.get("dataDisplay") {
                        if let Some(display_type) = dd.get("type").and_then(|v| v.as_str()) {
                            extras.push(format!("[展示类型: {}]", display_type));
                        }
                    }
                    if let Some(fa) = meta.get("frontendAction") {
                        if let Some(action) = fa.get("action").and_then(|v| v.as_str()) {
                            extras.push(format!("[前端动作: {}]", action));
                        }
                    }
                    if !extras.is_empty() {
                        content.push_str(&format!("\n{}", extras.join(" ")));
                    }
                }
            }
            ConversationMessage {
                role: m.role,
                content,
                created_at: Some(m.created_at.to_rfc3339()),
            }
        })
        .collect()
}

/// 确保会话存在，如果 session_id 为 None 则自动创建
pub(crate) async fn ensure_session(
    db: &DatabaseConnection,
    session_id: Option<&str>,
    user_id: i32,
) -> Result<String, String> {
    if let Some(sid) = session_id {
        // 验证会话存在且属于当前用户
        if agent_sessions::Entity::find_by_id(sid)
            .filter(agent_sessions::Column::UserId.eq(user_id))
            .one(db)
            .await
            .map_err(|e| { tracing::error!("DB error: {}", e); "Database error".to_string() })?
            .is_some()
        {
            return Ok(sid.to_string());
        }
    }

    // 自动创建新会话
    let new_id = format!("ses_{}", uuid::Uuid::new_v4().simple());
    let now = Utc::now().fixed_offset();
    let session = agent_sessions::ActiveModel {
        id: Set(new_id.clone()),
        user_id: Set(user_id),
        title: Set(None),
        context: Set(None),
        message_count: Set(0),
        archived: Set(false),
        created_at: Set(now),
        last_active_at: Set(now),
    };
    agent_sessions::Entity::insert(session)
        .exec(db)
        .await
        .map_err(|e| { tracing::error!("Failed to create session: {}", e); "Database error".to_string() })?;

    Ok(new_id)
}


