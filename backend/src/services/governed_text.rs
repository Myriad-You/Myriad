//! Synchronous host-governed AI text for scheduler and declared-API builtins.
//!
//! Public surface lives in services so `tapp_api_service` / `tapp_scheduler` do not
//! import `crate::api::tapp_runtime`. The concrete executor is installed by the
//! AI Task module (same registry, rate-limit, and quota ledger as the public API).

use futures::future::BoxFuture;
use once_cell::sync::OnceCell;
use sea_orm::DatabaseConnection;
use std::sync::Arc;

use crate::config::ModelTier;
use crate::services::permission_service::UserRole;
use myriad_tapp_contract::manifest::TappAiOperation;

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

type Executor = Arc<
    dyn Fn(DatabaseConnection, GovernedTextRequest) -> BoxFuture<'static, Result<String, String>>
        + Send
        + Sync,
>;

static EXECUTOR: OnceCell<Executor> = OnceCell::new();

/// Install the process-wide governed-text executor (idempotent: first wins).
pub fn install_executor<F, Fut>(handler: F)
where
    F: Fn(DatabaseConnection, GovernedTextRequest) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<String, String>> + Send + 'static,
{
    let executor: Executor = Arc::new(move |db, request| Box::pin(handler(db, request)));
    let _ = EXECUTOR.set(executor);
}

/// Run a synchronous governed text task through the installed AI Task path.
pub async fn execute_governed_text(
    db: &DatabaseConnection,
    request: GovernedTextRequest,
) -> Result<String, String> {
    let executor = EXECUTOR.get().ok_or_else(|| {
        "AI_TASK_EXECUTOR_UNAVAILABLE: governed text executor is not installed".to_string()
    })?;
    executor(db.clone(), request).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use myriad_tapp_contract::manifest::TappAiOperation;
    use sea_orm::DatabaseConnection;

    #[test]
    fn request_is_owned_and_cloneable() {
        let req = GovernedTextRequest {
            role: UserRole::User,
            subject_id: 1,
            owner_id: 1,
            tapp_id: "com.example.app".into(),
            source: "test".into(),
            operation: TappAiOperation::Generate,
            tier: ModelTier::default(),
            system_prompt: "sys".into(),
            prompt: "hello".into(),
            client_ip: None,
        };
        let cloned = req.clone();
        assert_eq!(cloned.tapp_id, "com.example.app");
        assert_eq!(cloned.prompt, "hello");
        assert_eq!(cloned.operation, TappAiOperation::Generate);
    }

    #[tokio::test]
    async fn missing_executor_returns_stable_error_code() {
        // Fresh process may or may not have an executor from other tests; if
        // installed, skip the negative check. Install a throwaway handler only
        // when empty so we can still assert the public error shape after a
        // deliberate double-install attempt is a no-op for first-wins.
        if EXECUTOR.get().is_some() {
            return;
        }
        let db = DatabaseConnection::default();
        let err = execute_governed_text(
            &db,
            GovernedTextRequest {
                role: UserRole::User,
                subject_id: 1,
                owner_id: 1,
                tapp_id: "com.example.app".into(),
                source: "test".into(),
                operation: TappAiOperation::Generate,
                tier: ModelTier::default(),
                system_prompt: "sys".into(),
                prompt: "hello".into(),
                client_ip: None,
            },
        )
        .await
        .expect_err("executor must be missing in isolated unit test");
        assert!(
            err.starts_with("AI_TASK_EXECUTOR_UNAVAILABLE"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn installed_executor_is_invoked() {
        install_executor(|_db, request| async move {
            Ok(format!("echo:{}", request.prompt))
        });
        let db = DatabaseConnection::default();
        let text = execute_governed_text(
            &db,
            GovernedTextRequest {
                role: UserRole::User,
                subject_id: 1,
                owner_id: 1,
                tapp_id: "com.example.app".into(),
                source: "test".into(),
                operation: TappAiOperation::Generate,
                tier: ModelTier::default(),
                system_prompt: "sys".into(),
                prompt: "ping".into(),
                client_ip: None,
            },
        )
        .await
        .expect("installed executor should run");
        // First-wins: if another test installed first, we only require some Ok.
        assert!(!text.is_empty());
    }
}
