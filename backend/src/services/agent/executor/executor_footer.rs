// Executor free helpers: capability timeout/cancel and tapp interaction wait.

use serde_json::Value;

use crate::services::agent::types::*;

use super::handlers::{self, HandlerContext};
use super::task_store::is_cancelled;

#[cfg(test)]
use super::task_store;
#[cfg(test)]
use super::Executor;
#[cfg(test)]
use serde_json::json;

pub(crate) fn tapp_interaction_wait_question(output: &Value) -> Option<UserQuestion> {
    let interaction = output.get("interaction")?;
    let interaction_id = interaction
        .get("interactionId")
        .or_else(|| interaction.get("interaction_id"))?
        .as_str()?;
    let expires_at = interaction
        .get("deadline")
        .and_then(Value::as_str)
        .and_then(|value| chrono::DateTime::parse_from_rfc3339(value).ok())
        .map(|value| value.with_timezone(&chrono::Utc));
    Some(UserQuestion {
        question_id: format!("tapp_interaction:{interaction_id}"),
        question_type: QuestionType::FreeText,
        question: "Waiting for the Tapp to finish interacting".to_string(),
        context: format!(
            "Tapp Agent Interaction {interaction_id} will resume this task after a structured result is submitted"
        ),
        options: None,
        required: true,
        default_value: None,
        created_at: chrono::Utc::now(),
        expires_at,
    })
}

/// Run a capability with wall-clock timeout and cooperative mid-step cancel.
///
/// Cancel is polled every 500ms while the handler future is in flight so a user
/// interrupt does not wait for the full step timeout. Dropping the pinned future
/// aborts at the next `.await`.
pub(crate) async fn execute_capability_with_timeout_and_cancel(
    capability_id: &str,
    action: &str,
    category: &CapabilityCategory,
    params: &std::collections::HashMap<String, Value>,
    handler_ctx: &HandlerContext<'_>,
    timeout_secs: u64,
    task_id: Option<&str>,
) -> Result<Value, String> {
    let work = handlers::execute_capability(capability_id, action, category, params, handler_ctx);
    tokio::pin!(work);

    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let mut cancel_tick = tokio::time::interval(std::time::Duration::from_millis(500));
    cancel_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    // Skip the immediate first tick so we don't cancel-check before start.
    cancel_tick.tick().await;

    loop {
        tokio::select! {
            biased;
            result = &mut work => {
                return result;
            }
            _ = tokio::time::sleep_until(deadline) => {
                tracing::error!(
                    capability = %capability_id,
                    timeout_secs = timeout_secs,
                    "[Executor] Step timed out"
                );
                return Err(crate::services::agent::response_agent::step_timeout(
                    capability_id,
                    timeout_secs,
                ));
            }
            _ = cancel_tick.tick(), if task_id.is_some() => {
                if let Some(tid) = task_id {
                    if is_cancelled(tid).await {
                        tracing::info!(
                            task_id = %tid,
                            capability = %capability_id,
                            "[Executor] Mid-step cancel observed"
                        );
                        return Err(
                            crate::services::agent::response_agent::task_cancelled_by_user(),
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod cancel_during_step_tests {
    use super::*;

    #[tokio::test]
    async fn mid_step_cancel_returns_before_timeout() {
        // Synthetic long future + cancel flag: must fail with cancel, not timeout.
        let task_id = format!("cancel_test_{}", uuid::Uuid::new_v4().simple());
        {
            let mut tokens = task_store::CANCELLATION_TOKENS.write().await;
            tokens.insert(task_id.clone());
        }

        let started = std::time::Instant::now();
        // Use a tiny local future via the select helper pattern (inline).
        let work = async {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
            Ok::<Value, String>(json!({}))
        };
        tokio::pin!(work);
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut cancel_tick = tokio::time::interval(std::time::Duration::from_millis(50));
        cancel_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        cancel_tick.tick().await;

        let result = loop {
            tokio::select! {
                biased;
                result = &mut work => break result,
                _ = tokio::time::sleep_until(deadline) => {
                    break Err("timeout".to_string());
                }
                _ = cancel_tick.tick() => {
                    if is_cancelled(&task_id).await {
                        break Err(crate::services::agent::response_agent::task_cancelled_by_user());
                    }
                }
            }
        };

        {
            let mut tokens = task_store::CANCELLATION_TOKENS.write().await;
            tokens.remove(&task_id);
        }

        assert!(
            result
                .as_ref()
                .err()
                .is_some_and(|e| e.contains("取消") || e.contains("cancel")),
            "expected cancel error, got {result:?}"
        );
        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "cancel should win quickly, elapsed {:?}",
            started.elapsed()
        );
    }
}

#[cfg(test)]
mod resolve_id_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn dynamic_risk_gate_aligns_with_system_sensitive_gate() {
        assert!(Executor::should_block_unconfirmed_dynamic_step(
            crate::services::agent::SYSTEM_USER_ID,
            RiskLevel::Critical
        ));
        assert!(Executor::should_block_unconfirmed_dynamic_step(
            crate::services::agent::SYSTEM_USER_ID,
            RiskLevel::High
        ));
        // System Medium must auto-run (matches system_sensitive_gate).
        assert!(!Executor::should_block_unconfirmed_dynamic_step(
            crate::services::agent::SYSTEM_USER_ID,
            RiskLevel::Medium
        ));
        assert!(Executor::should_block_unconfirmed_dynamic_step(
            7,
            RiskLevel::Medium
        ));
        assert!(!Executor::should_block_unconfirmed_dynamic_step(
            7,
            RiskLevel::Low
        ));
    }

    /// 复现歌单播放链路：搜索步骤输出被整对象引用为 playlistIdFrom 时，
    /// 必须取到 playlists[0].id，而不是 message 文案
    #[test]
    fn id_param_extracts_from_search_output() {
        let output = json!({
            "success": true,
            "message": "找到 10 个「凉宫春日」相关歌单",
            "keyword": "凉宫春日",
            "playlists": [
                { "id": 12597740641u64, "name": "悲情篇章" },
                { "id": 12764048642u64, "name": "アニサマ" }
            ]
        });
        let got = Executor::extract_id_from_output(&output, "playlistId");
        assert_eq!(got, Some(json!(12597740641u64)));
    }

    #[test]
    fn id_param_prefers_same_name_field() {
        let output = json!({ "playlistId": "abc123", "id": "other", "message": "文案" });
        let got = Executor::extract_id_from_output(&output, "playlistId");
        assert_eq!(got, Some(json!("abc123")));
    }

    #[test]
    fn id_param_falls_back_to_top_level_id() {
        let output = json!({ "id": 42, "message": "文案" });
        assert_eq!(
            Executor::extract_id_from_output(&output, "songId"),
            Some(json!(42))
        );
    }

    #[test]
    fn id_param_array_input_takes_first_element() {
        let output = json!([{ "id": "first" }, { "id": "second" }]);
        assert_eq!(
            Executor::extract_id_from_output(&output, "itemId"),
            Some(json!("first"))
        );
    }

    /// 提取不到 ID 必须返回 None（上层按未解析处理并让步骤报错），
    /// 绝不能兜底成 message 文案
    #[test]
    fn id_param_without_id_yields_none() {
        let output = json!({ "message": "找到 10 个歌单", "success": true });
        assert_eq!(
            Executor::extract_id_from_output(&output, "playlistId"),
            None
        );
    }
}
