//! Agent API — sessions
use super::*;
use crate::error::HttpError;
pub(crate) use crate::services::agent::sessions::*;
use myriad_error::AppError;

fn session_store_http(context: &'static str, error: impl std::fmt::Display) -> HttpError {
    tracing::error!(%error, context, "agent session store failed");
    HttpError(AppError::internal(format!("Failed to {context}")))
}

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
