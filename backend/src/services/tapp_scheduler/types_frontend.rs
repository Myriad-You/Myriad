use chrono::{TimeZone, Utc};
use sea_orm::{ActiveModelTrait, ColumnTrait, DatabaseConnection, EntityTrait};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::services::tapp_registry::{self as shared_registry, RegistryIdentity};
use myriad_tapp_contract::manifest::{
    TappAiManifest, TappAiModelTier, TappAiOperation, TappAiOutputFormat,
};

use crate::config::ModelTier;
use crate::models::entities::tapp_scheduled_tasks::{BackendAction, BackendActionWrapper};
use crate::services::permission_service::{TappPermission, UserRole};

pub(crate) const MAX_SCHEDULER_FETCH_RESPONSE_BYTES: usize = 2 * 1024 * 1024;
pub const MAX_SCHEDULER_BACKEND_ACTIONS: usize = 8;
pub const MAX_SCHEDULER_RETRIES: i32 = 2;
pub const MAX_SCHEDULER_RETRY_DELAY_MS: i64 = 60_000;
pub(crate) const SCHEDULER_ACTION_BUDGET_MS: i64 = 130_000;
pub(crate) const MIN_SCHEDULER_LEASE_MINUTES: i64 = 15;
pub(crate) const MAX_SCHEDULER_LEASE_MINUTES: i64 = 360;
pub(crate) const SCHEDULER_PRESENCE_NAMESPACE: &str = "scheduler_ws_presence";
pub(crate) const SCHEDULER_MAILBOX_CHANNEL: &str = "scheduler_frontend";
pub(crate) const SCHEDULER_PRESENCE_TTL_SECONDS: i64 = 75;
pub(crate) const SCHEDULER_MESSAGE_TTL_SECONDS: i64 = 5 * 60;
pub(crate) const MAX_SCHEDULER_CONNECTIONS_PER_SUBJECT: usize = 8;
pub const SCHEDULER_PRESENCE_REFRESH_SECONDS: u64 = 20;
pub const SCHEDULER_MAILBOX_POLL_MILLIS: u64 = 500;
pub const SCHEDULER_MAILBOX_BATCH_SIZE: i64 = 32;

pub(crate) static SCHEDULER_DISPATCHED: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCHEDULER_DELIVERY_FAILURES: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCHEDULER_REQUEUED: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCHEDULER_COMPLETED: AtomicU64 = AtomicU64::new(0);
pub(crate) static SCHEDULER_TIMEOUTS: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SchedulerCounters {
    pub dispatched: u64,
    pub delivery_failures: u64,
    pub requeued: u64,
    pub completed: u64,
    pub timeouts: u64,
}

pub fn scheduler_counters() -> SchedulerCounters {
    SchedulerCounters {
        dispatched: SCHEDULER_DISPATCHED.load(Ordering::Relaxed),
        delivery_failures: SCHEDULER_DELIVERY_FAILURES.load(Ordering::Relaxed),
        requeued: SCHEDULER_REQUEUED.load(Ordering::Relaxed),
        completed: SCHEDULER_COMPLETED.load(Ordering::Relaxed),
        timeouts: SCHEDULER_TIMEOUTS.load(Ordering::Relaxed),
    }
}

/// Normalize the public SDK action shape (`type`) to the persisted Rust enum
/// tag (`action`) and validate every action before a task is stored.
pub fn normalize_backend_actions(
    actions: Option<serde_json::Value>,
) -> Result<Option<serde_json::Value>, String> {
    let Some(serde_json::Value::Array(actions)) = actions else {
        return match actions {
            None => Ok(None),
            Some(_) => Err("backendActions must be an array".to_string()),
        };
    };
    if actions.len() > MAX_SCHEDULER_BACKEND_ACTIONS {
        return Err(format!(
            "backendActions exceeds the maximum of {MAX_SCHEDULER_BACKEND_ACTIONS}"
        ));
    }

    let mut normalized = Vec::with_capacity(actions.len());
    for mut value in actions {
        let object = value
            .as_object_mut()
            .ok_or_else(|| "Each backend action must be an object".to_string())?;
        if !object.contains_key("action") {
            let action_type = object
                .remove("type")
                .ok_or_else(|| "Backend action requires type".to_string())?;
            object.insert("action".to_string(), action_type);
        }

        serde_json::from_value::<BackendActionWrapper>(value.clone())
            .map_err(|e| format!("Invalid backend action: {e}"))?;
        normalized.push(value);
    }

    Ok(Some(serde_json::Value::Array(normalized)))
}

/// Resolve the dynamic Tapp permissions needed by a validated backend action
/// pipeline. Registration and delayed execution both use this list.
pub fn backend_action_permissions(
    actions: &Option<serde_json::Value>,
) -> Result<Vec<TappPermission>, String> {
    let Some(serde_json::Value::Array(actions)) = actions else {
        return Ok(Vec::new());
    };

    let mut permissions = Vec::new();
    for value in actions {
        let wrapper: BackendActionWrapper = serde_json::from_value(value.clone())
            .map_err(|e| format!("Invalid backend action: {e}"))?;
        let permission = match wrapper.action {
            BackendAction::PlatformSync { .. } => Some(TappPermission::PlatformWrite),
            BackendAction::StorageSet { .. }
            | BackendAction::StorageDelete { .. }
            | BackendAction::StorageGet { .. } => Some(TappPermission::Storage),
            BackendAction::AiGenerate { .. } => Some(TappPermission::AiGenerate),
            BackendAction::Fetch { .. } => Some(TappPermission::NetworkFetch),
            BackendAction::NotificationQueue { .. } => Some(TappPermission::UiNotification),
            BackendAction::Transform { .. } => None,
        };
        if let Some(permission) = permission {
            permissions.push(permission);
        }
    }
    permissions.sort_by_key(|permission| permission.as_str());
    permissions.dedup();
    Ok(permissions)
}

/// Validate delayed backend actions against the installed Manifest contract.
/// Permissions alone are insufficient: AI execution must also be declared in
/// manifest.ai so every entry path uses the same declared operation/model boundary.
pub fn validate_backend_action_declarations(
    manifest: &serde_json::Value,
    actions: &Option<serde_json::Value>,
) -> Result<Option<ModelTier>, String> {
    let Some(serde_json::Value::Array(actions)) = actions else {
        return Ok(None);
    };
    let uses_ai_generate = actions.iter().try_fold(false, |uses_ai, value| {
        let wrapper: BackendActionWrapper = serde_json::from_value(value.clone())
            .map_err(|error| format!("Invalid backend action: {error}"))?;
        Ok::<_, String>(uses_ai || matches!(wrapper.action, BackendAction::AiGenerate { .. }))
    })?;
    if !uses_ai_generate {
        return Ok(None);
    }

    let declaration: TappAiManifest = manifest
        .get("ai")
        .cloned()
        .ok_or_else(|| "ai.generate backend action requires manifest.ai".to_string())
        .and_then(|value| {
            serde_json::from_value(value)
                .map_err(|_| "Stored manifest.ai declaration is invalid".to_string())
        })?;
    if declaration.protocol_version != 2
        || !declaration.operations.contains(&TappAiOperation::Generate)
        || !declaration
            .output_formats
            .contains(&TappAiOutputFormat::Text)
    {
        return Err(
            "ai.generate backend action requires protocolVersion 2 with generate and text output"
                .to_string(),
        );
    }
    Ok(Some(match declaration.model_tier {
        TappAiModelTier::Standard => ModelTier::Standard,
        TappAiModelTier::Pro => ModelTier::Pro,
    }))
}

#[derive(Clone, Copy)]
pub(crate) struct ScheduledExecutionAuthority {
    pub(crate) role: UserRole,
    pub(crate) owner_id: i32,
    pub(crate) ai_model_tier: Option<ModelTier>,
}

/// 任务执行上下文（发送给前端）
///
/// Times are RFC3339 strings (same as outer `FrontendTaskMessage.scheduled_at`)
/// so clients never mix epoch ms with ISO.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskExecutionContext {
    pub id: i32,
    pub task_id: String,
    pub tapp_id: String,
    /// 注册任务的用户 ID
    pub user_id: i32,
    /// 任务作用域: user, tapp, global
    pub scope: String,
    pub scheduled_at: String,
    pub executed_at: String,
    pub is_compensation: bool,
    pub payload: Option<serde_json::Value>,
}

/// 前端任务推送消息
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FrontendTaskMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub task: TaskExecutionContext,
    pub payload: Option<serde_json::Value>,
    pub scheduled_at: String,
    pub execution_id: i32,
    /// 目标用户列表（空表示广播给所有该 Tapp 的用户）
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target_users: Option<Vec<i32>>,
}

pub(crate) fn scheduler_mailbox_recipient(connection_id: &str) -> String {
    format!("connection:{connection_id}")
}

pub async fn register_frontend_connection(
    db: &DatabaseConnection,
    user_id: i32,
    connection_id: &str,
) -> Result<(), String> {
    let expires_at = Utc::now().timestamp() + SCHEDULER_PRESENCE_TTL_SECONDS;
    let inserted = shared_registry::put_with_subject_limit(
        db,
        SCHEDULER_PRESENCE_NAMESPACE,
        connection_id,
        RegistryIdentity {
            subject_id: Some(user_id),
            owner_id: None,
            tapp_id: None,
            runtime_id: Some(connection_id),
        },
        &json!({ "connectedAt": Utc::now().to_rfc3339() }),
        expires_at,
        MAX_SCHEDULER_CONNECTIONS_PER_SUBJECT,
    )
    .await
    .map_err(|error| format!("Failed to register scheduler connection: {error}"))?;
    if inserted {
        Ok(())
    } else {
        Err(format!(
            "Scheduler connection limit exceeded ({MAX_SCHEDULER_CONNECTIONS_PER_SUBJECT})"
        ))
    }
}

pub async fn unregister_frontend_connection(
    db: &DatabaseConnection,
    connection_id: &str,
) -> Result<(), String> {
    shared_registry::delete(db, SCHEDULER_PRESENCE_NAMESPACE, connection_id)
        .await
        .map(|_| ())
        .map_err(|error| format!("Failed to unregister scheduler connection: {error}"))
}

pub async fn drain_frontend_messages(
    db: &DatabaseConnection,
    connection_id: &str,
) -> Result<Vec<FrontendTaskMessage>, String> {
    shared_registry::drain(
        db,
        SCHEDULER_MAILBOX_CHANNEL,
        &scheduler_mailbox_recipient(connection_id),
        SCHEDULER_MAILBOX_BATCH_SIZE,
    )
    .await
    .map_err(|error| format!("Failed to drain scheduler mailbox: {error}"))
}

pub async fn requeue_frontend_message(
    db: &DatabaseConnection,
    connection_id: &str,
    message: &FrontendTaskMessage,
) -> Result<(), String> {
    shared_registry::enqueue(
        db,
        SCHEDULER_MAILBOX_CHANNEL,
        &scheduler_mailbox_recipient(connection_id),
        message,
        Utc::now().timestamp() + SCHEDULER_MESSAGE_TTL_SECONDS,
    )
    .await
    .map_err(|error| format!("Failed to requeue scheduler message: {error}"))?;
    SCHEDULER_REQUEUED.fetch_add(1, Ordering::Relaxed);
    Ok(())
}

pub async fn active_frontend_subject_count(db: &DatabaseConnection) -> Result<usize, String> {
    shared_registry::list_subject_ids(db, SCHEDULER_PRESENCE_NAMESPACE)
        .await
        .map(|subjects| subjects.len())
        .map_err(|error| format!("Failed to count scheduler subjects: {error}"))
}

pub async fn scheduler_mailbox_depth(db: &DatabaseConnection) -> Result<i64, String> {
    shared_registry::mailbox_depth(db, SCHEDULER_MAILBOX_CHANNEL)
        .await
        .map_err(|error| format!("Failed to count scheduler mailbox: {error}"))
}

/// 调度引擎
pub struct TappSchedulerEngine {
    pub(crate) db: DatabaseConnection,
    /// 是否正在运行
    pub(crate) running: Arc<RwLock<bool>>,
}
