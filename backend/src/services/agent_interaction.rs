//! Agent Interaction domain surface for Executor handlers.
//!
//! Requests use the shared domain registry, mailbox and expiry path directly.

use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Lifecycle state of a host-mediated Agent ↔ Tapp interaction.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum InteractionState {
    Pending,
    Accepted,
    Completed,
    Rejected,
    Expired,
    Cancelled,
}

impl InteractionState {
    pub fn terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Rejected | Self::Expired | Self::Cancelled
        )
    }
}

/// Origin of the interaction (host Agent task).
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct InteractionSource {
    pub agent_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
}

/// Serializable snapshot returned to Agent Executor and Tapp runtimes.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AgentInteractionSnapshot {
    pub version: u8,
    pub interaction_id: String,
    #[serde(rename = "type")]
    pub interaction_type: String,
    pub tapp_id: String,
    pub state: InteractionState,
    pub input: Value,
    pub input_schema: Option<String>,
    pub result_schema: Option<String>,
    pub deadline: String,
    pub source: InteractionSource,
    pub created_at: String,
    pub updated_at: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rejection_reason: Option<String>,
}

impl AgentInteractionSnapshot {
    pub fn interaction_id(&self) -> &str {
        &self.interaction_id
    }
}

/// Parameters for a trusted server-side create (Agent Executor path).
#[derive(Debug, Clone)]
pub struct CreateAgentInteractionRequest {
    pub subject_id: i32,
    pub tapp_id: String,
    pub interaction_type: String,
    pub input: Value,
    pub task_id: Option<String>,
}

/// Create an interaction from trusted server-side Agent execution code.
pub async fn create_agent_interaction(
    db: &DatabaseConnection,
    request: CreateAgentInteractionRequest,
) -> Result<AgentInteractionSnapshot, String> {
    crate::services::tapp_agent_interaction::create_from_agent(db, request)
        .await
        .map_err(|error| error.agent_message())
}

/// 自由函数签名包装，转给 [`create_agent_interaction`]。
pub async fn create_agent_interaction_internal(
    db: &DatabaseConnection,
    subject_id: i32,
    tapp_id: &str,
    interaction_type: &str,
    input: Value,
    task_id: Option<String>,
) -> Result<AgentInteractionSnapshot, String> {
    create_agent_interaction(
        db,
        CreateAgentInteractionRequest {
            subject_id,
            tapp_id: tapp_id.to_string(),
            interaction_type: interaction_type.to_string(),
            input,
            task_id,
        },
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::DatabaseConnection;

    #[test]
    fn snapshot_interaction_id_accessor() {
        let snap = AgentInteractionSnapshot {
            version: 2,
            interaction_id: "agi_test".into(),
            interaction_type: "confirm".into(),
            tapp_id: "com.example.app".into(),
            state: InteractionState::Pending,
            input: Value::Null,
            input_schema: None,
            result_schema: None,
            deadline: "2026-01-01T00:00:00Z".into(),
            source: InteractionSource {
                agent_id: "myriad.agent".into(),
                task_id: Some("task-1".into()),
            },
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            result: None,
            rejection_reason: None,
        };
        assert_eq!(snap.interaction_id(), "agi_test");
        assert!(!snap.state.terminal());
        assert!(InteractionState::Completed.terminal());
    }

    #[tokio::test]
    async fn domain_path_fails_closed_without_database() {
        let db = DatabaseConnection::default();
        let err = create_agent_interaction_internal(
            &db,
            1,
            "com.example.app",
            "confirm",
            Value::Null,
            None,
        )
        .await
        .expect_err("domain create must fail without DB/install");
        assert!(!err.is_empty(), "expected a non-empty domain error");
    }
}
