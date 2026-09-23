//! Server-governed AI Task for Tapp runtimes.
//!
//! Tasks are scoped to the subject/owner/Tapp identity carried by a Runtime
//! Grant. The registry intentionally contains only short-lived execution state;
//! quota usage remains authoritative and persistent in PostgreSQL.

use std::{collections::VecDeque, convert::Infallible, time::Duration};

use axum::{
    Extension, Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    response::sse::{Event, KeepAlive, Sse},
};
use chrono::Utc;
use futures::Stream;
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use crate::error::HttpError;
use crate::{
    api::tapp_store::{TappAiModelTier, TappAiOperation},
    middleware::auth::Claims,
    services::{
        ai_config::{get_ai_config_for_tier, get_ai_image_config},
        ai_quota::{AiQuotaError, get_ai_usage, reserve_ai_quota, rollback_ai_quota_reservation},
        ai_task_context::{AiContextError, AiContextSubject, resolve_context},
        ai_task_execute::{
            AiTaskExecution, PreparedModel, default_output, execute_task, hash_request,
            parse_ai_manifest, prepare_task, validate_output,
        },
        ai_task_image::{load_image_references, validate_task_input},
        ai_task_prepare::{AiTaskLogicError, permission_for_operation, validate_idempotency_key},
        ai_task_registry::{
            AI_CANCEL_NAMESPACE, AI_TASK_MAILBOX_CHANNEL, AI_TASK_NAMESPACE, AiTaskRegistration,
            AiTaskStatus, MAX_ACTIVE_TASKS_PER_SUBJECT, MAX_RETAINED_TASKS_PER_SUBJECT,
            PersistedAiTask, register_ai_task_atomically, task_id_for_request,
        },
        ai_task_runtime::{
            LocalAiTask, TaskBroadcast, cancel_local_task, insert_local, local_subject_counts,
        },
        permission_service::{TappPermission, UserRole},
    },
};

use super::{
    RuntimeGrantContext,
    common::{
        check_anonymous_rate_limit, check_rate_limit, current_tapp_user_role, parse_user_id,
        resolve_accessible_tapp, validate_prompt_security,
    },
    shared_registry::{self, RegistryIdentity},
};

// Domain types (HTTP request/response surfaces).
// AiContextRef lives in services::ai_task_context; not re-exported here.
pub use crate::services::ai_task_execute::{AiTaskOutputRequest, CreateAiTaskRequest};
pub use crate::services::ai_task_registry::{AiTaskDelivery, AiTaskSnapshot};

type ApiError = HttpError;

fn api_error(status: StatusCode, code: &str, message: impl Into<String>) -> ApiError {
    HttpError::from((
        status,
        Json(json!({
            "error": message.into(),
            "code": code
        })),
    ))
}

fn quota_api_error(err: AiQuotaError) -> ApiError {
    let status = if err.is_client_limit() {
        StatusCode::TOO_MANY_REQUESTS
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    };
    api_error(status, err.code(), err.message())
}

fn logic_api_error(err: AiTaskLogicError) -> ApiError {
    let status = match err.code.as_str() {
        "AI_TASK_PROMPT_LIMIT" | "AI_TASK_INPUT_LIMIT" | "AI_IMAGE_REFERENCE_LIMIT" => {
            StatusCode::PAYLOAD_TOO_LARGE
        }
        "AI_V2_NOT_DECLARED" => StatusCode::FORBIDDEN,
        "INVALID_AI_V2_MANIFEST" => StatusCode::UNPROCESSABLE_ENTITY,
        "AI_OUTPUT_NOT_DECLARED" => StatusCode::FORBIDDEN,
        _ => StatusCode::BAD_REQUEST,
    };
    api_error(status, &err.code, err.message)
}

fn context_api_error(err: AiContextError) -> ApiError {
    let status = match err.status_hint {
        403 => StatusCode::FORBIDDEN,
        404 => StatusCode::NOT_FOUND,
        413 => StatusCode::PAYLOAD_TOO_LARGE,
        500 => StatusCode::INTERNAL_SERVER_ERROR,
        _ => StatusCode::BAD_REQUEST,
    };
    api_error(status, &err.code, err.message)
}

// Cancel helpers for runtime_grant (local + durable shared registry).
pub(super) use crate::services::ai_task_runtime::{
    cancel_all_tapp_ai_tasks, cancel_runtime_ai_tasks, cancel_tapp_ai_tasks,
};

fn authorize_persisted<'a>(
    task: &'a PersistedAiTask,
    runtime: &RuntimeGrantContext,
) -> Result<&'a PersistedAiTask, ApiError> {
    if task.subject_id != runtime.subject_id()
        || task.owner_id != runtime.owner_id()
        || task.tapp_id != runtime.tapp_id()
    {
        return Err(api_error(
            StatusCode::NOT_FOUND,
            "AI_TASK_NOT_FOUND",
            "AI task was not found or has expired",
        ));
    }
    Ok(task)
}

/// POST /api/tapp/ai/v2/tasks
pub async fn create_ai_task(
    State(db): State<DatabaseConnection>,
    Extension(claims): Extension<Claims>,
    runtime: RuntimeGrantContext,
    headers: HeaderMap,
    axum::extract::ConnectInfo(addr): axum::extract::ConnectInfo<std::net::SocketAddr>,
    Json(mut request): Json<CreateAiTaskRequest>,
) -> Result<(StatusCode, Json<AiTaskSnapshot>), ApiError> {
    if request.version != 2 {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "UNSUPPORTED_AI_TASK_VERSION",
            "AI task version must be 2",
        ));
    }
    validate_task_input(request.operation, &request.input).map_err(logic_api_error)?;
    if request
        .idempotency_key
        .as_deref()
        .is_some_and(|value| !validate_idempotency_key(value))
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_IDEMPOTENCY_KEY",
            "idempotencyKey must use 1-128 safe ASCII characters",
        ));
    }

    let operation_permission = permission_for_operation(request.operation);
    runtime.require(operation_permission)?;
    // The grant extractor already verified the subject and rebound this request
    // to the live installation; reuse those facts instead of re-querying.
    let user_id = runtime.subject_id();
    let declaration =
        parse_ai_manifest(&runtime.installation().manifest).map_err(logic_api_error)?;
    if declaration.protocol_version != 2 || !declaration.operations.contains(&request.operation) {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "AI_OPERATION_NOT_DECLARED",
            "AI operation is not declared by this Tapp",
        ));
    }
    let output = request
        .output
        .clone()
        .unwrap_or_else(|| default_output(request.operation));
    validate_output(&declaration, request.operation, &output).map_err(logic_api_error)?;
    if request.delivery == AiTaskDelivery::Stream
        && matches!(
            request.operation,
            TappAiOperation::Image | TappAiOperation::Search
        )
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "INVALID_AI_TASK_DELIVERY",
            "Image and search tasks do not support token streaming",
        ));
    }
    if matches!(
        request.operation,
        TappAiOperation::Image | TappAiOperation::Search
    ) && !request.context.is_empty()
    {
        return Err(api_error(
            StatusCode::BAD_REQUEST,
            "UNSUPPORTED_AI_TASK_CONTEXT",
            "Image and search tasks do not currently accept context references",
        ));
    }

    let request_hash = hash_request(&request).map_err(logic_api_error)?;
    let shared_tasks =
        shared_registry::list(&db, AI_TASK_NAMESPACE, Some(runtime.subject_id()), None)
            .await
            .map_err(|_| {
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "AI_TASK_REGISTRY_UNAVAILABLE",
                    "AI task registry is unavailable",
                )
            })?
            .into_iter()
            .filter_map(|row| serde_json::from_value::<PersistedAiTask>(row.payload).ok())
            .collect::<Vec<_>>();
    if let Some(key) = request.idempotency_key.as_deref() {
        if let Some(existing) = shared_tasks.iter().find(|task| {
            task.owner_id == runtime.owner_id()
                && task.tapp_id == runtime.tapp_id()
                && task.idempotency_key.as_deref() == Some(key)
        }) {
            if existing.request_hash != request_hash {
                return Err(api_error(
                    StatusCode::CONFLICT,
                    "IDEMPOTENCY_KEY_REUSED",
                    "idempotencyKey was already used for a different AI task",
                ));
            }
            return Ok((StatusCode::OK, Json(existing.snapshot.clone())));
        }
    }
    let active_shared = shared_tasks
        .iter()
        .filter(|task| !task.snapshot.status.terminal())
        .count();
    if active_shared >= MAX_ACTIVE_TASKS_PER_SUBJECT
        || shared_tasks.len() >= MAX_RETAINED_TASKS_PER_SUBJECT
    {
        return Err(api_error(
            StatusCode::TOO_MANY_REQUESTS,
            "AI_TASK_CONCURRENCY_LIMIT",
            "Too many active or retained AI tasks",
        ));
    }
    // Local process preflight (authoritative limit is still register_ai_task_atomically).
    let (active_local, retained_local) = local_subject_counts(runtime.subject_id()).await;
    if active_local >= MAX_ACTIVE_TASKS_PER_SUBJECT
        || retained_local >= MAX_RETAINED_TASKS_PER_SUBJECT
    {
        return Err(api_error(
            StatusCode::TOO_MANY_REQUESTS,
            "AI_TASK_CONCURRENCY_LIMIT",
            "Too many active or retained AI tasks",
        ));
    }

    let role = runtime.role();
    let context_subject = AiContextSubject {
        subject_id: runtime.subject_id(),
        username: claims.username.clone(),
        role,
        tapp_id: runtime.tapp_id().to_string(),
        grant_platform_read: runtime.has(TappPermission::PlatformRead),
        grant_report_read: runtime.has(TappPermission::ReportRead),
    };
    let (context, provenance) =
        resolve_context(&db, &context_subject, &declaration, &request.context)
            .await
            .map_err(context_api_error)?;
    let mut prepared = prepare_task(&request, context, provenance).map_err(logic_api_error)?;
    if request.operation == TappAiOperation::Image {
        prepared.image_references = load_image_references(&request.input)
            .await
            .map_err(logic_api_error)?;
        // The original sources already participate in request_hash. Keep only
        // resolved bytes during execution instead of also retaining their base64.
        if let Some(input) = request.input.as_object_mut() {
            input.remove("referenceImages");
        }
    }
    let tier = match declaration.model_tier {
        TappAiModelTier::Standard => crate::config::ModelTier::Standard,
        TappAiModelTier::Pro => crate::config::ModelTier::Pro,
    };
    let model = match request.operation {
        TappAiOperation::Image => {
            PreparedModel::Image(get_ai_image_config().await.map_err(|error| {
                api_error(
                    StatusCode::SERVICE_UNAVAILABLE,
                    "AI_NOT_CONFIGURED",
                    error.message(),
                )
            })?)
        }
        TappAiOperation::Search => PreparedModel::Search,
        _ => PreparedModel::Text(get_ai_config_for_tier(tier).await.map_err(|error| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "AI_NOT_CONFIGURED",
                error.message(),
            )
        })?),
    };

    check_rate_limit(&db, user_id, runtime.tapp_id(), "ai.task").await?;
    let client_ip = crate::middleware::client_ip::client_ip_from_parts(
        &headers,
        Some(addr.ip()),
        crate::middleware::client_ip::trusted_proxy_headers_enabled(),
    )
    .map(|ip| ip.to_string());
    if role == UserRole::Guest {
        check_anonymous_rate_limit(&db, client_ip.as_deref(), runtime.tapp_id()).await?;
    }
    let estimated_tokens = if matches!(
        request.operation,
        TappAiOperation::Image | TappAiOperation::Search
    ) {
        0
    } else {
        prepared.prompt.len() / 4 + 1_000
    };
    let reservation = reserve_ai_quota(
        &db,
        role,
        runtime.subject_id(),
        runtime.owner_id(),
        runtime.tapp_id(),
        estimated_tokens,
        client_ip.as_deref(),
    )
    .await
    .map_err(quota_api_error)?;
    let usage = match get_ai_usage(
        &db,
        role,
        runtime.subject_id(),
        runtime.owner_id(),
        runtime.tapp_id(),
    )
    .await
    {
        Ok(usage) => usage,
        Err(error) => {
            if let Err(rollback_error) = rollback_ai_quota_reservation(&db, &reservation).await {
                tracing::error!(
                    ?rollback_error,
                    "[TAPP] Failed to roll back AI quota after usage read failure"
                );
            }
            return Err(quota_api_error(error));
        }
    };

    let task_id = task_id_for_request(
        runtime.subject_id(),
        runtime.owner_id(),
        runtime.tapp_id(),
        request.idempotency_key.as_deref(),
    );
    let now = Utc::now().to_rfc3339();
    let snapshot = AiTaskSnapshot {
        task_id: task_id.clone(),
        status: AiTaskStatus::Queued,
        operation: request.operation,
        delivery: request.delivery,
        created_at: now.clone(),
        updated_at: now,
        result: None,
        error: None,
        usage,
    };
    let (stored, cancel_receiver) = LocalAiTask::new(
        runtime.runtime_id().to_string(),
        runtime.subject_id(),
        runtime.owner_id(),
        runtime.tapp_id().to_string(),
        request.idempotency_key.clone(),
        request_hash,
        snapshot.clone(),
    );
    let persisted = stored.to_persisted();
    let registration = register_ai_task_atomically(&db, &persisted).await;
    match registration {
        Ok(AiTaskRegistration::Inserted) => {}
        Ok(AiTaskRegistration::Existing(existing)) => {
            if let Err(error) = rollback_ai_quota_reservation(&db, &reservation).await {
                tracing::error!(?error, task_id = %task_id, "[TAPP] Failed to roll back duplicate AI task quota");
            }
            return Ok((StatusCode::OK, Json(*existing)));
        }
        Ok(AiTaskRegistration::IdempotencyConflict) => {
            if let Err(error) = rollback_ai_quota_reservation(&db, &reservation).await {
                tracing::error!(?error, task_id = %task_id, "[TAPP] Failed to roll back conflicting AI task quota");
            }
            return Err(api_error(
                StatusCode::CONFLICT,
                "IDEMPOTENCY_KEY_REUSED",
                "idempotencyKey was already used for a different AI task",
            ));
        }
        Ok(AiTaskRegistration::LimitReached) => {
            if let Err(error) = rollback_ai_quota_reservation(&db, &reservation).await {
                tracing::error!(?error, task_id = %task_id, "[TAPP] Failed to roll back limited AI task quota");
            }
            return Err(api_error(
                StatusCode::TOO_MANY_REQUESTS,
                "AI_TASK_CONCURRENCY_LIMIT",
                "Too many active or retained AI tasks",
            ));
        }
        Err(error) => {
            if let Err(rollback_error) = rollback_ai_quota_reservation(&db, &reservation).await {
                tracing::error!(?rollback_error, task_id = %task_id, "[TAPP] Failed to roll back unregistered AI task quota");
            }
            tracing::error!(%error, task_id = %task_id, "[TAPP] Failed to register AI task");
            return Err(api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "AI_TASK_REGISTRY_UNAVAILABLE",
                "AI task registry is unavailable",
            ));
        }
    }
    insert_local(stored).await;

    tokio::spawn(execute_task(AiTaskExecution {
        task_id,
        db,
        role,
        subject_id: runtime.subject_id(),
        owner_id: runtime.owner_id(),
        tapp_id: runtime.tapp_id().to_string(),
        request,
        prepared,
        model,
        system_prompt: None,
        reservation,
        cancel: cancel_receiver,
        ledger_source: "runtime".to_string(),
    }));

    Ok((StatusCode::ACCEPTED, Json(snapshot)))
}

/// GET /api/tapp/ai/v2/tasks/{task_id}
pub async fn get_ai_task(
    State(db): State<DatabaseConnection>,
    runtime: RuntimeGrantContext,
    Path(task_id): Path<String>,
) -> Result<Json<AiTaskSnapshot>, ApiError> {
    let task = shared_registry::get::<PersistedAiTask>(&db, AI_TASK_NAMESPACE, &task_id)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "AI_TASK_REGISTRY_UNAVAILABLE",
                "AI task registry is unavailable",
            )
        })?
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "AI_TASK_NOT_FOUND",
                "AI task was not found or has expired",
            )
        })?;
    authorize_persisted(&task, &runtime)?;
    Ok(Json(task.snapshot))
}

/// DELETE /api/tapp/ai/v2/tasks/{task_id}
pub async fn cancel_ai_task(
    State(db): State<DatabaseConnection>,
    runtime: RuntimeGrantContext,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, ApiError> {
    let task = shared_registry::get::<PersistedAiTask>(&db, AI_TASK_NAMESPACE, &task_id)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "AI_TASK_REGISTRY_UNAVAILABLE",
                "AI task registry is unavailable",
            )
        })?
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "AI_TASK_NOT_FOUND",
                "AI task was not found or has expired",
            )
        })?;
    authorize_persisted(&task, &runtime)?;
    if task.snapshot.status.terminal() {
        return Err(api_error(
            StatusCode::CONFLICT,
            "AI_TASK_ALREADY_TERMINAL",
            "AI task is already in a terminal state",
        ));
    }
    shared_registry::put(
        &db,
        AI_CANCEL_NAMESPACE,
        &task_id,
        RegistryIdentity {
            subject_id: Some(task.subject_id),
            owner_id: Some(task.owner_id),
            tapp_id: Some(&task.tapp_id),
            runtime_id: Some(&task.runtime_id),
        },
        &true,
        task.retain_until,
    )
    .await
    .map_err(|_| {
        api_error(
            StatusCode::SERVICE_UNAVAILABLE,
            "AI_TASK_REGISTRY_UNAVAILABLE",
            "AI cancellation registry is unavailable",
        )
    })?;
    let _ = cancel_local_task(&task_id).await;
    Ok(Json(json!({ "success": true, "taskId": task_id })))
}

/// GET /api/tapp/ai/v2/tasks/{task_id}/events
pub async fn stream_ai_task_events(
    State(db): State<DatabaseConnection>,
    runtime: RuntimeGrantContext,
    Path(task_id): Path<String>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let task = shared_registry::get::<PersistedAiTask>(&db, AI_TASK_NAMESPACE, &task_id)
        .await
        .map_err(|_| {
            api_error(
                StatusCode::SERVICE_UNAVAILABLE,
                "AI_TASK_REGISTRY_UNAVAILABLE",
                "AI task registry is unavailable",
            )
        })?
        .ok_or_else(|| {
            api_error(
                StatusCode::NOT_FOUND,
                "AI_TASK_NOT_FOUND",
                "AI task was not found or has expired",
            )
        })?;
    authorize_persisted(&task, &runtime)?;
    let snapshot = task.snapshot;
    let initial = serde_json::to_value(&snapshot).unwrap_or(Value::Null);
    let snapshot_event = Ok(Event::default()
        .event("snapshot")
        .json_data(initial)
        .unwrap_or_default());
    let stream = futures::stream::unfold(
        Some(AiTaskFeed {
            pending: VecDeque::from([snapshot_event]),
            last_updated: snapshot.updated_at.clone(),
            poll: !snapshot.status.terminal(),
            db,
            task_id,
        }),
        |feed| async move {
            let mut feed = feed?;
            if let Some(event) = feed.pending.pop_front() {
                return Some((event, Some(feed)));
            }
            if !feed.poll {
                return None;
            }
            loop {
                tokio::time::sleep(Duration::from_millis(250)).await;
                let drained = shared_registry::drain::<TaskBroadcast>(
                    &feed.db,
                    AI_TASK_MAILBOX_CHANNEL,
                    &feed.task_id,
                    128,
                )
                .await;
                let events = drained.unwrap_or_default();
                for event in events {
                    let terminal = matches!(event.kind.as_str(), "result" | "error" | "cancelled");
                    feed.pending.push_back(Ok(Event::default()
                        .event(event.kind)
                        .json_data(event.payload)
                        .unwrap_or_default()));
                    if terminal {
                        feed.poll = false;
                        break;
                    }
                }
                if let Some(event) = feed.pending.pop_front() {
                    return Some((event, Some(feed)));
                }
                let fetched = shared_registry::get::<PersistedAiTask>(
                    &feed.db,
                    AI_TASK_NAMESPACE,
                    &feed.task_id,
                )
                .await;
                let task = fetched.ok().flatten()?;
                if task.snapshot.updated_at == feed.last_updated {
                    continue;
                }
                feed.last_updated = task.snapshot.updated_at.clone();
                let kind = match task.snapshot.status {
                    AiTaskStatus::Completed => "result",
                    AiTaskStatus::Cancelled => "cancelled",
                    AiTaskStatus::Failed => "error",
                    _ => "state",
                };
                let terminal = task.snapshot.status.terminal();
                if terminal {
                    feed.poll = false;
                }
                return Some((
                    Ok(Event::default()
                        .event(kind)
                        .json_data(task.snapshot)
                        .unwrap_or_default()),
                    Some(feed),
                ));
            }
        },
    );
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

struct AiTaskFeed {
    pending: VecDeque<Result<Event, Infallible>>,
    last_updated: String,
    poll: bool,
    db: DatabaseConnection,
    task_id: String,
}

/// GET /api/tapp/ai/v2/usage
pub async fn ai_usage(
    State(db): State<DatabaseConnection>,
    runtime: RuntimeGrantContext,
) -> Result<Json<Value>, ApiError> {
    if ![
        TappPermission::AiGenerate,
        TappPermission::AiAnalyze,
        TappPermission::AiChat,
        TappPermission::AiImage,
        TappPermission::AiSearch,
    ]
    .into_iter()
    .any(|permission| runtime.has(permission))
    {
        return Err(api_error(
            StatusCode::FORBIDDEN,
            "RUNTIME_GRANT_PERMISSION_DENIED",
            "Runtime Grant has no AI capability",
        ));
    }
    let role = runtime.role();
    let usage = get_ai_usage(
        &db,
        role,
        runtime.subject_id(),
        runtime.owner_id(),
        runtime.tapp_id(),
    )
    .await
    .map_err(quota_api_error)?;
    Ok(Json(json!({ "success": true, "usage": usage })))
}

#[cfg(test)]
mod tests {
    use super::task_id_for_request;

    #[test]
    fn idempotent_task_ids_are_stable_and_identity_scoped() {
        let first = task_id_for_request(7, 1, "com.example.app", Some("daily"));
        assert_eq!(
            first,
            task_id_for_request(7, 1, "com.example.app", Some("daily"))
        );
        assert_ne!(
            first,
            task_id_for_request(8, 1, "com.example.app", Some("daily"))
        );
        assert_ne!(
            first,
            task_id_for_request(7, 2, "com.example.app", Some("daily"))
        );
        assert_ne!(
            task_id_for_request(7, 1, "com.example.app", None),
            task_id_for_request(7, 1, "com.example.app", None)
        );
    }
}
