//! Agent API — presets
use super::*;
use crate::error::HttpError;

// 任务预设 API

/// 获取任务预设列表
/// GET /api/agent/presets
pub async fn list_presets(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
) -> Result<Json<TaskPresetListResponse>, HttpError> {
    use sea_orm::QuerySelect;

    let user_id = parse_user_id(&claims)?;

    // 并行发起两次查询，减少串行等待时间
    let (favorites_result, history_result) = tokio::join!(
        agent_task_presets::Entity::find()
            .filter(agent_task_presets::Column::UserId.eq(user_id))
            .filter(agent_task_presets::Column::PresetType.eq("favorite"))
            .order_by_desc(agent_task_presets::Column::LastUsedAt)
            .all(&db),
        agent_task_presets::Entity::find()
            .filter(agent_task_presets::Column::UserId.eq(user_id))
            .filter(agent_task_presets::Column::PresetType.eq("history"))
            .order_by_desc(agent_task_presets::Column::LastUsedAt)
            .limit(20) // DB 层限制，避免拉取全量到内存
            .all(&db),
    );

    let favorites = favorites_result
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to fetch favorites: {}", e);
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(
                    json!({ "error": "Failed to fetch favorites", "code": "preset_fetch_failed" }),
                ),
            ))
        })?
        .into_iter()
        .map(TaskPresetResponse::from)
        .collect();

    let history = history_result
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to fetch history: {}", e);
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to fetch history", "code": "preset_fetch_failed" })),
            ))
        })?
        .into_iter()
        .map(TaskPresetResponse::from)
        .collect();

    Ok(Json(TaskPresetListResponse { favorites, history }))
}

/// 创建或更新任务预设
/// POST /api/agent/presets
pub async fn create_preset(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Json(req): Json<CreatePresetRequest>,
) -> Result<Json<TaskPresetResponse>, HttpError> {
    let user_id = parse_user_id(&claims)?;

    // 验证预设类型
    if req.preset_type != "favorite" && req.preset_type != "history" {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Invalid preset type, must be 'favorite' or 'history'" })),
        )));
    }

    // 输入长度校验
    validate_input(&req.input)?;
    if let Some(ref title) = req.title {
        if title.len() > 200 {
            return Err(HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Title is too long", "code": "preset_title_too_long" })),
            )));
        }
    }
    if let Some(ref summary) = req.intent_summary {
        if summary.len() > 500 {
            return Err(HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Summary is too long", "code": "preset_summary_too_long" })),
            )));
        }
    }
    if let Some(ref steps) = req.parsed_steps {
        let steps_size = serde_json::to_string(steps).unwrap_or_default().len();
        if steps_size > 102_400 {
            return Err(HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(
                    json!({ "error": "Parsed steps are too large", "code": "preset_steps_too_large" }),
                ),
            )));
        }
    }
    if let Some(ref conv) = req.conversation_data {
        if conv.len() > MAX_HISTORY_ITEMS {
            return Err(HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(
                    json!({ "error": "Conversation history is too long", "code": "preset_history_too_long" }),
                ),
            )));
        }
    }

    let now = Utc::now().fixed_offset();

    // 检查是否已存在相同的输入
    let existing = agent_task_presets::Entity::find()
        .filter(agent_task_presets::Column::UserId.eq(user_id))
        .filter(agent_task_presets::Column::Input.eq(&req.input))
        .one(&db)
        .await
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to check existing preset: {}", e);
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error", "code": "database_error" })),
            ))
        })?;

    if let Some(existing_preset) = existing {
        // 更新已存在的预设
        let mut active_model: agent_task_presets::ActiveModel = existing_preset.clone().into();
        // 只有当现有预设是 history 类型时才更新类型（保留 favorite 状态）
        if existing_preset.preset_type == "history" {
            active_model.preset_type = Set(req.preset_type);
        }
        active_model.last_used_at = Set(now);
        // 从原始 Model 获取 use_count 避免 ActiveValue::unwrap() panic
        active_model.use_count = Set(existing_preset.use_count + 1);
        if req.parsed_steps.is_some() {
            active_model.parsed_steps = Set(req.parsed_steps);
        }
        if req.intent_summary.is_some() {
            active_model.intent_summary = Set(req.intent_summary);
        }
        // 更新对话数据
        if req.title.is_some() {
            active_model.title = Set(req.title);
        }
        if req.conversation_data.is_some() {
            active_model.conversation_data = Set(req
                .conversation_data
                .map(|c| serde_json::to_value(&c).unwrap_or_default()));
        }

        let updated = active_model.update(&db).await.map_err(|e| {
            tracing::error!("[Agent Presets] Failed to update preset: {}", e);
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to update preset", "code": "preset_update_failed" })),
            ))
        })?;

        return Ok(Json(TaskPresetResponse::from(updated)));
    }

    // 创建新预设
    let new_preset = agent_task_presets::ActiveModel {
        id: sea_orm::ActiveValue::NotSet,
        user_id: Set(user_id),
        input: Set(req.input),
        preset_type: Set(req.preset_type.clone()),
        parsed_steps: Set(req.parsed_steps),
        intent_summary: Set(req.intent_summary),
        last_used_at: Set(now),
        use_count: Set(1),
        created_at: Set(now),
        title: Set(req.title),
        conversation_data: Set(req
            .conversation_data
            .map(|c| serde_json::to_value(&c).unwrap_or_default())),
    };

    let created = new_preset.insert(&db).await.map_err(|e| {
        tracing::error!("[Agent Presets] Failed to create preset: {}", e);
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to create preset", "code": "preset_update_failed" })),
        ))
    })?;

    // 如果是历史记录，清理超过 20 条的旧记录
    if req.preset_type == "history" {
        cleanup_old_history(&db, user_id).await;
    }

    Ok(Json(TaskPresetResponse::from(created)))
}

/// 清理超过 20 条的历史记录
///
/// 只查询超出部分的 ID（加 LIMIT+OFFSET），避免拉取全量数据到内存
pub(crate) async fn cleanup_old_history(db: &DatabaseConnection, user_id: i32) {
    use sea_orm::{PaginatorTrait, QuerySelect};

    // 先计算总数，若不超出则跳过
    let count = agent_task_presets::Entity::find()
        .filter(agent_task_presets::Column::UserId.eq(user_id))
        .filter(agent_task_presets::Column::PresetType.eq("history"))
        .count(db)
        .await
        .unwrap_or(0);

    if count <= 20 {
        return;
    }

    // 只查询第 21 条起的 ID，在 DB 层做 LIMIT/OFFSET
    let to_delete_ids: Vec<i32> = agent_task_presets::Entity::find()
        .filter(agent_task_presets::Column::UserId.eq(user_id))
        .filter(agent_task_presets::Column::PresetType.eq("history"))
        .order_by_desc(agent_task_presets::Column::LastUsedAt)
        .offset(20)
        .limit(count)
        .all(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|p| p.id)
        .collect();

    if !to_delete_ids.is_empty() {
        let _ = agent_task_presets::Entity::delete_many()
            .filter(agent_task_presets::Column::Id.is_in(to_delete_ids))
            .exec(db)
            .await;
    }
}

/// 删除任务预设（仅限历史类型，收藏类型不允许直接删除）
/// DELETE /api/agent/presets/{id}
pub async fn delete_preset(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(preset_id): Path<i32>,
) -> Result<Json<Value>, HttpError> {
    let user_id = parse_user_id(&claims)?;

    // 确保只能删除自己的预设
    let preset = agent_task_presets::Entity::find_by_id(preset_id)
        .filter(agent_task_presets::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to find preset: {}", e);
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error", "code": "database_error" })),
            ))
        })?;

    let preset = preset.ok_or_else(|| {
        HttpError::from((
            StatusCode::NOT_FOUND,
            Json(json!({ "error": "Preset not found" })),
        ))
    })?;

    // 只允许删除历史类型的预设，收藏类型需要先取消收藏
    if preset.preset_type == "favorite" {
        return Err(HttpError::from((
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Cannot delete favorite preset, please unfavorite first" })),
        )));
    }

    agent_task_presets::Entity::delete_by_id(preset_id)
        .exec(&db)
        .await
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to delete preset: {}", e);
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Failed to delete preset", "code": "preset_update_failed" })),
            ))
        })?;

    Ok(Json(json!({ "success": true })))
}

/// 切换收藏状态
/// POST /api/agent/presets/{id}/toggle-favorite
pub async fn toggle_favorite(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(preset_id): Path<i32>,
) -> Result<Json<TaskPresetResponse>, HttpError> {
    let user_id = parse_user_id(&claims)?;

    // 查找预设
    let preset = agent_task_presets::Entity::find_by_id(preset_id)
        .filter(agent_task_presets::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to find preset: {}", e);
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error", "code": "database_error" })),
            ))
        })?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Preset not found" })),
            ))
        })?;

    // 切换类型
    let new_type = if preset.preset_type == "favorite" {
        "history"
    } else {
        "favorite"
    };

    let mut active_model: agent_task_presets::ActiveModel = preset.into();
    active_model.preset_type = Set(new_type.to_string());

    let updated = active_model.update(&db).await.map_err(|e| {
        tracing::error!("[Agent Presets] Failed to toggle favorite: {}", e);
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to toggle favorite", "code": "preset_update_failed" })),
        ))
    })?;

    Ok(Json(TaskPresetResponse::from(updated)))
}

/// 更新预设使用时间（每次使用时调用）
/// POST /api/agent/presets/{id}/use
pub async fn use_preset(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(preset_id): Path<i32>,
) -> Result<Json<TaskPresetResponse>, HttpError> {
    let user_id = parse_user_id(&claims)?;

    let now = Utc::now().fixed_offset();

    // 查找预设
    let preset = agent_task_presets::Entity::find_by_id(preset_id)
        .filter(agent_task_presets::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to find preset: {}", e);
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error", "code": "database_error" })),
            ))
        })?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Preset not found" })),
            ))
        })?;

    let new_use_count = preset.use_count + 1;
    let mut active_model: agent_task_presets::ActiveModel = preset.into();
    active_model.last_used_at = Set(now);
    active_model.use_count = Set(new_use_count);

    let updated = active_model.update(&db).await.map_err(|e| {
        tracing::error!("[Agent Presets] Failed to update use time: {}", e);
        HttpError::from((
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": "Failed to update preset", "code": "preset_update_failed" })),
        ))
    })?;

    Ok(Json(TaskPresetResponse::from(updated)))
}

/// 执行任务预设
/// POST /api/agent/presets/{id}/execute
///
/// 直接执行已保存的预设，跳过意图分析步骤
pub async fn execute_preset(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    Path(preset_id): Path<i32>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, HttpError> {
    let user_id = parse_user_id_with_agent_access(&claims, &db).await?;

    // 查找预设
    let preset = agent_task_presets::Entity::find_by_id(preset_id)
        .filter(agent_task_presets::Column::UserId.eq(user_id))
        .one(&db)
        .await
        .map_err(|e| {
            tracing::error!("[Agent Presets] Failed to find preset: {}", e);
            HttpError::from((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({ "error": "Database error", "code": "database_error" })),
            ))
        })?
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::NOT_FOUND,
                Json(json!({ "error": "Preset not found" })),
            ))
        })?;

    // 检查是否有保存的 recipe
    let mut recipe: crate::services::agent::types::Recipe = preset
        .parsed_steps
        .as_ref()
        .and_then(|v| serde_json::from_value(v.clone()).ok())
        .ok_or_else(|| {
            HttpError::from((
                StatusCode::BAD_REQUEST,
                Json(json!({ "error": "Preset has no saved recipe, please run the task first" })),
            ))
        })?;

    // 重要：清除保存的 page_context，让步骤重新执行获取最新数据
    // 这确保 "获取最新文章 → AI总结" 这样的流程会获取当时的最新内容
    // 而不是使用保存时的旧数据
    recipe.page_context = None;

    tracing::info!(
        user_id = user_id,
        preset_id = preset_id,
        recipe_id = %recipe.id,
        "[Agent API] Executing preset with saved recipe"
    );

    // 更新使用时间
    let now = Utc::now().fixed_offset();
    let new_use_count = preset.use_count + 1;
    let mut active_model: agent_task_presets::ActiveModel = preset.into();
    active_model.last_used_at = Set(now);
    active_model.use_count = Set(new_use_count);
    let _ = active_model.update(&db).await;

    // 创建进度通道
    let (tx, rx) = tokio::sync::mpsc::channel::<ProgressEvent>(32);

    // 在后台执行任务
    let db_clone = db.clone();
    let queue = LANE_QUEUE.clone();
    let lane_key = LaneQueue::make_lane_key(user_id, None);
    tokio::spawn(async move {
        // 获取 Lane Queue 执行许可
        let _guard = match queue
            .acquire_timeout(
                &lane_key,
                std::time::Duration::from_secs(LaneQueue::DEFAULT_ACQUIRE_TIMEOUT_SECS),
            )
            .await
        {
            Ok(guard) => guard,
            Err(e) => {
                let _ = tx
                    .send(ProgressEvent::Error {
                        task_id: None,
                        message: e,
                        code: "QUEUE_FULL".to_string(),
                    })
                    .await;
                return;
            }
        };

        let agent = crate::services::agent::Agent::new(db_clone).await;

        // TaskCreated 在 execute_saved_recipe 内 mint 新 run id 后发送，保证与 task_id 一致
        match agent
            .execute_saved_recipe(&recipe, user_id, tx.clone())
            .await
        {
            Ok(response) => {
                let api_response: ApiResponse = response.into();
                let task_id = api_response
                    .task
                    .as_ref()
                    .map(|t| t.task_id.clone())
                    .unwrap_or_default();
                let success = api_response.success;
                let response_value = serde_json::to_value(&api_response)
                    .unwrap_or_else(|_| json!({"error": "serialization failed"}));
                tracing::info!(
                    "[Agent API] Preset execution completed, sending TaskCompleted event"
                );
                let _ = tx
                    .send(AgentProgressEvent::TaskCompleted {
                        task_id,
                        success,
                        response: Box::new(response_value),
                    })
                    .await;
            }
            Err(e) => {
                let code = agent_stream_error_code(&e, "EXECUTION_ERROR");
                if code == "EXECUTION_ERROR" {
                    tracing::error!(error = %e, "[Agent API] Preset execution failed");
                }
                let _ = tx
                    .send(ProgressEvent::Error {
                        task_id: None,
                        message: e.clone(),
                        code,
                    })
                    .await;
            }
        }
    });

    // 将通道转换为 SSE 流
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx).map(|event| {
        let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
        Ok(Event::default().data(data))
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15))))
}
