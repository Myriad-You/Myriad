//! Host-governed text generation for scheduler and declared-API builtins.
//! Shares AI Task registration, execution, rate limits and the quota ledger.

use chrono::Utc;
use myriad_prompt_security::validate_prompt_security;
use myriad_tapp_contract::manifest::{TappAiOperation, TappAiOutputFormat};
use sea_orm::DatabaseConnection;
use serde_json::Value;

use crate::config::ModelTier;
use crate::services::{
    ai_config::get_ai_config_for_tier,
    ai_quota::{get_ai_usage, reserve_ai_quota, rollback_ai_quota_reservation},
    ai_task_execute::{
        execute_task, hash_request, AiTaskExecution, AiTaskOutputRequest, CreateAiTaskRequest,
        PreparedModel, PreparedTask,
    },
    ai_task_prepare::MAX_INPUT_BYTES,
    ai_task_registry::{
        register_ai_task_atomically, task_id_for_request, AiTaskDelivery, AiTaskRegistration,
        AiTaskSnapshot, AiTaskStatus,
    },
    ai_task_runtime::{insert_local, local_snapshot, LocalAiTask},
    permission_service::UserRole,
    tapp_rate_limit::{check_anonymous_rate_limit, check_rate_limit},
};

/// Owned request for a synchronous governed text generation.
#[derive(Debug, Clone)]
pub struct GovernedTextRequest {
    pub role: UserRole,
    pub subject_id: i32,
    pub owner_id: i32,
    pub tapp_id: String,
    pub source: String,
    pub operation: TappAiOperation,
    pub tier: ModelTier,
    pub system_prompt: String,
    pub prompt: String,
    pub client_ip: Option<String>,
}

/// Execute a synchronous host action through the same registry, concurrency,
/// rate-limit and quota ledger used by the public AI Task API. Scheduler and
/// declared builtin adapters wait for the task result, but do not get a second
/// provider path that can bypass governance.
pub async fn execute_governed_text(
    db: &DatabaseConnection,
    request: GovernedTextRequest,
) -> Result<String, String> {
    let GovernedTextRequest {
        role,
        subject_id,
        owner_id,
        tapp_id,
        source,
        operation,
        tier,
        system_prompt,
        prompt,
        client_ip,
    } = request;
    if !matches!(
        operation,
        TappAiOperation::Generate | TappAiOperation::Analyze | TappAiOperation::Chat
    ) {
        return Err(
            "AI_TASK_UNSUPPORTED_OPERATION: synchronous image tasks are unsupported".to_string(),
        );
    }
    if prompt.trim().is_empty()
        || prompt.len() > MAX_INPUT_BYTES
        || validate_prompt_security(&prompt).is_some()
    {
        return Err("UNSAFE_AI_TASK_INPUT: prompt is empty, too large, or unsafe".to_string());
    }

    check_rate_limit(db, subject_id, &tapp_id, "ai.task")
        .await
        .map_err(|error| format!("{}: {}", error.code(), error.message()))?;
    if role == UserRole::Guest {
        check_anonymous_rate_limit(db, client_ip.as_deref(), &tapp_id)
            .await
            .map_err(|error| format!("{}: {}", error.code(), error.message()))?;
    }
    let model = PreparedModel::Text(
        get_ai_config_for_tier(tier)
            .await
            .map_err(|error| format!("AI_NOT_CONFIGURED: {}", error.message()))?,
    );
    let estimated_tokens = (system_prompt.len() + prompt.len()) / 4 + 1_000;
    let reservation = reserve_ai_quota(
        db,
        role,
        subject_id,
        owner_id,
        &tapp_id,
        estimated_tokens,
        client_ip.as_deref(),
    )
    .await
    .map_err(|error| error.to_string())?;
    let usage = match get_ai_usage(db, role, subject_id, owner_id, &tapp_id).await {
        Ok(usage) => usage,
        Err(error) => {
            if let Err(rollback_error) = rollback_ai_quota_reservation(db, &reservation).await {
                tracing::error!(
                    ?rollback_error,
                    %tapp_id,
                    "[TAPP] Failed to roll back internal AI quota after usage read failure"
                );
            }
            return Err(error.to_string());
        }
    };

    let request = CreateAiTaskRequest {
        version: 2,
        operation,
        input: Value::String(prompt.clone()),
        context: Vec::new(),
        output: Some(AiTaskOutputRequest {
            format: TappAiOutputFormat::Text,
            schema: None,
        }),
        delivery: AiTaskDelivery::Result,
        idempotency_key: None,
    };
    let request_hash = match hash_request(&request) {
        Ok(hash) => hash,
        Err(error) => {
            let _ = rollback_ai_quota_reservation(db, &reservation).await;
            return Err(error.to_string());
        }
    };
    let task_id = task_id_for_request(subject_id, owner_id, &tapp_id, None);
    let now = Utc::now().to_rfc3339();
    let snapshot = AiTaskSnapshot {
        task_id: task_id.clone(),
        status: AiTaskStatus::Queued,
        operation,
        delivery: AiTaskDelivery::Result,
        created_at: now.clone(),
        updated_at: now,
        result: None,
        error: None,
        usage,
    };
    let (stored, cancel_receiver) = LocalAiTask::new(
        format!("internal:{source}:{task_id}"),
        subject_id,
        owner_id,
        tapp_id.clone(),
        None,
        request_hash,
        snapshot,
    );
    let persisted = stored.to_persisted();
    match register_ai_task_atomically(db, &persisted).await {
        Ok(AiTaskRegistration::Inserted) => {}
        Ok(AiTaskRegistration::LimitReached) => {
            let _ = rollback_ai_quota_reservation(db, &reservation).await;
            return Err(
                "AI_TASK_CONCURRENCY_LIMIT: too many active or retained AI tasks".to_string(),
            );
        }
        Ok(AiTaskRegistration::Existing(_) | AiTaskRegistration::IdempotencyConflict) => {
            let _ = rollback_ai_quota_reservation(db, &reservation).await;
            return Err("AI_TASK_REGISTRATION_CONFLICT: internal task ID conflict".to_string());
        }
        Err(error) => {
            let _ = rollback_ai_quota_reservation(db, &reservation).await;
            tracing::error!(%error, %task_id, "[TAPP] Failed to register internal AI task");
            return Err("AI_TASK_REGISTRY_UNAVAILABLE: task registry is unavailable".to_string());
        }
    }
    insert_local(stored).await;

    execute_task(AiTaskExecution {
        task_id: task_id.clone(),
        db: db.clone(),
        role,
        subject_id,
        owner_id,
        tapp_id: tapp_id.clone(),
        request,
        prepared: PreparedTask {
            prompt,
            output: AiTaskOutputRequest {
                format: TappAiOutputFormat::Text,
                schema: None,
            },
            provenance: Vec::new(),
            image_references: Vec::new(),
        },
        model,
        system_prompt: Some(system_prompt),
        reservation,
        cancel: cancel_receiver,
        ledger_source: format!("internal:{source}"),
    })
    .await;

    let snapshot = local_snapshot(&task_id)
        .await
        .ok_or_else(|| "AI_TASK_RESULT_MISSING: internal task disappeared".to_string())?;
    if snapshot.status == AiTaskStatus::Completed {
        return snapshot
            .result
            .as_ref()
            .and_then(|result| result.get("value"))
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| "AI_TASK_INVALID_RESULT: text result is missing".to_string());
    }
    let error = snapshot
        .error
        .as_ref()
        .and_then(|error| error.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("AI task failed");
    Err(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> GovernedTextRequest {
        GovernedTextRequest {
            role: UserRole::User,
            subject_id: 1,
            owner_id: 1,
            tapp_id: "com.example.app".into(),
            source: "test".into(),
            operation: TappAiOperation::Generate,
            tier: ModelTier::default(),
            system_prompt: "system".into(),
            prompt: "hello".into(),
            client_ip: None,
        }
    }

    #[tokio::test]
    async fn validates_requests_without_installing_an_executor() {
        let db = DatabaseConnection::default();
        let mut empty = request();
        empty.prompt.clear();
        assert!(execute_governed_text(&db, empty)
            .await
            .unwrap_err()
            .starts_with("UNSAFE_AI_TASK_INPUT:"));
        let mut image = request();
        image.operation = TappAiOperation::Image;
        assert!(execute_governed_text(&db, image)
            .await
            .unwrap_err()
            .starts_with("AI_TASK_UNSUPPORTED_OPERATION:"));
    }

    #[tokio::test]
    async fn enforces_shared_rate_limit_before_provider_execution() {
        let db = DatabaseConnection::default();
        assert_eq!(
            execute_governed_text(&db, request()).await.unwrap_err(),
            "RATE_LIMITER_UNAVAILABLE: Rate limiter unavailable"
        );
    }
}
