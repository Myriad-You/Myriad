//! Destroy runtime capabilities after the installed code changes.
use sea_orm::DatabaseConnection;
pub(crate) async fn revoke_installation(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
) -> usize {
    let revoked =
        crate::services::tapp_runtime_grant::revoke_all_tapp_runtime_grants(db, owner_id, tapp_id)
            .await;
    crate::services::ai_task_runtime::cancel_all_tapp_ai_tasks(owner_id, tapp_id).await;
    crate::services::tapp_events::disconnect_all_tapp_events(owner_id, tapp_id).await;
    crate::services::tapp_data_exchange::cancel_all_tapp_data_exchanges(owner_id, tapp_id).await;
    revoked
}
