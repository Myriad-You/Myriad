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
    session_context_in(mode, None)
}

fn session_context_in(
    mode: crate::services::agent::AgentInteractionMode,
    venue: Option<&str>,
) -> Value {
    match venue {
        Some(venue) => json!({ "mode": mode.as_str(), "venue": venue }),
        None => json!({ "mode": mode.as_str() }),
    }
}

/// The group a session belongs to, if it is a group's.
fn session_venue(context: Option<&Value>) -> Option<String> {
    context
        .and_then(|value| value.get("venue"))
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// Sessions that are the user's own conversations, not a group's.
pub(crate) fn private_sessions() -> sea_orm::sea_query::SimpleExpr {
    sea_orm::sea_query::Expr::cust("(agent_sessions.context::jsonb ->> 'venue') IS NULL")
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
        return Err(HttpError(
            AppError::bad_request(format!(
                "limit must be between {SESSION_LIMIT_MIN} and {SESSION_LIMIT_MAX}"
            ))
            .with_code("pagination_invalid"),
        ));
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
        // A group's conversation is not one of theirs to open and continue.
        .filter(private_sessions())
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
    let messages = agent_messages::Entity::find()
        .filter(agent_messages::Column::SessionId.eq(&session_id))
        .order_by_asc(agent_messages::Column::CreatedAt)
        .paginate(&db, 4)
        .fetch_page(0)
        .await
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
    active
        .update(&db)
        .await
        .map_err(|error| session_store_http("update session title", error))?;

    Ok(Json(json!({ "title": title })))
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
async fn bind_message_media(
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
    fn user_message_persist_error_aborts_the_turn() {
        let error: Result<(), &str> = Err("db down");
        assert!(require_user_message_persisted(error).is_err());
        assert!(require_user_message_persisted::<(), &str>(Ok(())).is_ok());
    }
}

#[cfg(test)]
mod title_endpoint_tests {
    use super::*;
    use sea_orm::{ConnectOptions, ConnectionTrait, Database};

    #[tokio::test]
    async fn title_endpoint_propagates_query_and_write_failures() {
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let schema = format!("title_test_{}", uuid::Uuid::new_v4().simple());
        let admin = Database::connect(&url).await.unwrap();
        admin
            .execute_unprepared(&format!("CREATE SCHEMA {schema}"))
            .await
            .unwrap();
        let mut options = ConnectOptions::new(url);
        options.set_schema_search_path(&schema).sqlx_logging(false);
        let db = Database::connect(options).await.unwrap();
        db.execute_unprepared("CREATE TABLE agent_sessions (id TEXT PRIMARY KEY, user_id INT NOT NULL, title TEXT, context JSONB, message_count INT NOT NULL DEFAULT 0, archived BOOLEAN NOT NULL DEFAULT false, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), last_active_at TIMESTAMPTZ NOT NULL DEFAULT NOW()); INSERT INTO agent_sessions(id,user_id) VALUES ('test',1)").await.unwrap();
        let claims = crate::middleware::auth::mint_session_claims(1, "test", false, false, 0);
        let query_error = generate_session_title(
            State(db.clone()),
            Extension(claims.clone()),
            Path("test".into()),
        )
        .await;
        assert_eq!(
            query_error.unwrap_err().0.status_u16(),
            500,
            "a missing messages table must not become an empty conversation"
        );
        db.execute_unprepared("CREATE TABLE agent_messages (id SERIAL PRIMARY KEY, session_id TEXT NOT NULL, task_id TEXT, role TEXT NOT NULL, content TEXT NOT NULL, metadata JSONB, created_at TIMESTAMPTZ NOT NULL DEFAULT NOW())").await.unwrap();
        assert_eq!(
            generate_session_title(
                State(db.clone()),
                Extension(claims.clone()),
                Path("test".into())
            )
            .await
            .unwrap_err()
            .0
            .status_u16(),
            400
        );
        db.execute_unprepared("INSERT INTO agent_messages(session_id,role,content) VALUES ('test','user','A title'); ALTER TABLE agent_sessions ADD CONSTRAINT refuse_title CHECK (title IS NULL)").await.unwrap();
        assert_eq!(
            generate_session_title(
                State(db.clone()),
                Extension(claims.clone()),
                Path("test".into())
            )
            .await
            .unwrap_err()
            .0
            .status_u16(),
            500,
            "a failed title write must not return success"
        );
        db.execute_unprepared("ALTER TABLE agent_sessions DROP CONSTRAINT refuse_title")
            .await
            .unwrap();
        let Json(response) =
            generate_session_title(State(db.clone()), Extension(claims), Path("test".into()))
                .await
                .unwrap();
        assert_eq!(response["title"], "A title");
        db.close().await.unwrap();
        admin
            .execute_unprepared(&format!("DROP SCHEMA {schema} CASCADE"))
            .await
            .unwrap();
        admin.close().await.unwrap();
    }
}

#[cfg(test)]
mod message_media_tests {
    use super::*;
    use crate::services::media::{
        MediaActor, MediaContext, MediaExposure, MediaService, MediaSource, NewMediaBytes,
        active_count,
    };
    use sea_orm::ConnectionTrait;

    fn png() -> Vec<u8> {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD
            .decode("iVBORw0KGgoAAAANSUhEUgAAAAIAAAABCAYAAAD0In+KAAAACXBIWXMAAAPoAAAD6AG1e1JrAAAADklEQVQImWNw6fj/H4QBFnsFlbfmtiMAAAAASUVORK5CYII=")
            .unwrap()
    }

    async fn upload(
        db: &DatabaseConnection,
        service: &MediaService,
        owner: i32,
    ) -> crate::services::media::MediaAsset {
        service
            .create_from_bytes(
                db,
                MediaContext::user(MediaActor::user(owner).unwrap(), MediaSource::Generated)
                    .unwrap(),
                NewMediaBytes {
                    bytes: png().into(),
                    claimed_mime: "image/png".into(),
                    filename: "g.png".into(),
                    max_bytes: 1024 * 1024,
                    derived_from_id: None,
                    exposure: MediaExposure::Public,
                },
            )
            .await
            .unwrap()
    }

    #[tokio::test]
    async fn stored_messages_protect_the_owners_media_only() {
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let isolated = crate::db::IsolatedSchema::migrated(&url, "message_media").await;
        let db = isolated.db.clone();
        db.execute_unprepared(
            "INSERT INTO users (id, username) VALUES (1, 'owner'), (2, 'other');
             INSERT INTO agent_sessions (id, user_id, created_at, last_active_at) VALUES ('s1', 1, NOW(), NOW())",
        )
        .await
        .unwrap();
        let service = MediaService::new(
            std::env::temp_dir().join(format!("message_media_{}", uuid::Uuid::new_v4().simple())),
        );
        let own = upload(&db, &service, 1).await;
        let foreign = upload(&db, &service, 2).await;
        let dead = format!("/api/phantasi/image-cache/ab/ab{}.png", "0".repeat(62));
        let content = format!("![a]({}) ![b]({}) ![c]({dead})", own.url, foreign.url);
        persist_assistant_message(&db, "s1", None, &content, None)
            .await
            .expect("a dead link never fails the message");
        assert_eq!(active_count(&db, own.id).await.unwrap(), 1);
        assert_eq!(
            active_count(&db, foreign.id).await.unwrap(),
            0,
            "a message cannot pin another user's media"
        );
        let _ = tokio::fs::remove_dir_all(service.store().root()).await;
    }
}

#[cfg(test)]
mod venue_tests {
    use super::*;
    use crate::services::agent::AgentInteractionMode;
    use sea_orm::ConnectionTrait;

    /// A group's session is the group's from the moment it exists; a private
    /// request never continues it, and nothing private picks it up.
    #[tokio::test]
    async fn a_group_session_never_turns_private() {
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "session_venue").await;
        let db = &schema.db;
        db.execute_unprepared("INSERT INTO users (id, username) VALUES (21, 'owner')")
            .await
            .unwrap();
        let private = ensure_session(db, None, 21, AgentInteractionMode::Chat)
            .await
            .unwrap();
        let group = ensure_session_in(
            db,
            None,
            21,
            AgentInteractionMode::Chat,
            Some("telegram:-100123"),
        )
        .await
        .unwrap();
        let stored = agent_sessions::Entity::find_by_id(&group)
            .one(db)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            session_venue(stored.context.as_ref()).as_deref(),
            Some("telegram:-100123")
        );
        // The group keeps its session; a private request gets its own.
        let again = ensure_session_in(
            db,
            Some(&group),
            21,
            AgentInteractionMode::Chat,
            Some("telegram:-100123"),
        )
        .await
        .unwrap();
        assert_eq!(again, group);
        let from_web = ensure_session(db, Some(&group), 21, AgentInteractionMode::Chat)
            .await
            .unwrap();
        assert_ne!(from_web, group);
        let other_group = ensure_session_in(
            db,
            Some(&group),
            21,
            AgentInteractionMode::Chat,
            Some("discord:22"),
        )
        .await
        .unwrap();
        assert_ne!(other_group, group);
        // Newest is a group's; the private listing never shows or picks it.
        db.execute_unprepared(&format!(
            "UPDATE agent_sessions SET last_active_at = NOW() + interval '1 hour' WHERE id = '{group}'"
        ))
        .await
        .unwrap();
        let listed: Vec<String> = agent_sessions::Entity::find()
            .filter(agent_sessions::Column::UserId.eq(21))
            .filter(private_sessions())
            .all(db)
            .await
            .unwrap()
            .into_iter()
            .map(|session| session.id)
            .collect();
        assert!(listed.contains(&private) && listed.contains(&from_web));
        assert!(!listed.contains(&group) && !listed.contains(&other_group));
        let (latest, _) = crate::services::agent::merope::store::latest_open_session(db, 21)
            .await
            .unwrap()
            .unwrap();
        assert_ne!(latest, group);
        schema.drop().await;
    }
}
