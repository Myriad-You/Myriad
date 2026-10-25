//! SSE 事件发送辅助
//!
//! 消除 execute / DAG streaming / resume_with_answer 三条路径中
//! 大量重复的 StepStarted / StepCompleted / StepDebug 事件构造代码。

use crate::services::agent::types::*;
use serde_json::Value;
use tokio::sync::mpsc::Sender;

/// 步骤事件发送器
///
/// 封装 progress_tx 通道，提供简洁的事件发送方法。
/// 所有方法在 tx 为 None 时静默返回。
pub struct StepEventEmitter {
    tx: Option<Sender<AgentProgressEvent>>,
}

impl StepEventEmitter {
    pub fn new(tx: Option<Sender<AgentProgressEvent>>) -> Self {
        Self { tx }
    }

    pub fn is_live(&self) -> bool {
        self.tx.is_some()
    }

    /// 发送 StepStarted 事件
    pub async fn step_started(
        &self,
        step_id: &str,
        step_index: u32,
        total_steps: u32,
        capability_id: &str,
        capability_name: &str,
        description: String,
    ) {
        if let Some(ref tx) = self.tx {
            let _ = tx
                .send(AgentProgressEvent::StepStarted {
                    step_id: step_id.to_string(),
                    step_index,
                    total_steps,
                    capability_name: capability_name.to_string(),
                    capability_id: capability_id.to_string(),
                    description,
                })
                .await;
        }
    }

    /// 发送 StepCompleted 事件（成功）
    pub async fn step_succeeded(
        &self,
        step_id: &str,
        step_index: u32,
        duration_ms: u64,
        output_summary: Option<String>,
        image_url: Option<String>,
        frontend_actions: Vec<Value>,
    ) {
        if let Some(ref tx) = self.tx {
            let _ = tx
                .send(AgentProgressEvent::StepCompleted {
                    step_id: step_id.to_string(),
                    step_index,
                    success: true,
                    duration_ms,
                    output_summary,
                    image_url,
                    frontend_actions,
                })
                .await;
        }
    }

    /// 成功步骤：摘要、图片 URL、frontendActions 从输出里抽。
    pub async fn step_output_succeeded(
        &self,
        step_id: &str,
        step_index: u32,
        duration_ms: u64,
        output: &Value,
    ) {
        self.step_succeeded(
            step_id,
            step_index,
            duration_ms,
            super::summarize_output(output),
            super::extract_image_url(output),
            crate::services::agent::collect_step_frontend_actions(std::iter::once(output)),
        )
        .await;
    }

    /// 发送 StepCompleted 事件（失败）
    pub async fn step_failed(
        &self,
        step_id: &str,
        step_index: u32,
        duration_ms: u64,
        error_msg: &str,
    ) {
        if let Some(ref tx) = self.tx {
            let _ = tx
                .send(AgentProgressEvent::StepCompleted {
                    step_id: step_id.to_string(),
                    step_index,
                    success: false,
                    duration_ms,
                    output_summary: Some(error_msg.to_string()),
                    image_url: None,
                    frontend_actions: Vec::new(),
                })
                .await;
        }
    }

    /// 发送 WaitingForInput 事件
    pub async fn waiting_for_input(&self, task_id: &str, question: &UserQuestion) {
        if let Some(ref tx) = self.tx {
            let _ = tx
                .send(AgentProgressEvent::WaitingForInput {
                    task_id: task_id.to_string(),
                    question_id: question.question_id.clone(),
                    question_type: serde_json::to_value(&question.question_type)
                        .ok()
                        .and_then(|v| v.as_str().map(String::from))
                        .unwrap_or_else(|| "free_text".to_string()),
                    question: question.question.clone(),
                    context: if question.context.is_empty() {
                        None
                    } else {
                        Some(question.context.clone())
                    },
                    options: question.options.as_ref().map(|opts: &Vec<QuestionOption>| {
                        opts.iter()
                            .map(|o| QuestionOptionCompact {
                                value: o.value.clone(),
                                label: o.label.clone(),
                                description: o.description.clone(),
                            })
                            .collect()
                    }),
                    required: question.required,
                    default_value: question.default_value.clone(),
                    detail: question.detail.clone(),
                })
                .await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn step_completed_serializes_frontend_actions() {
        let event = AgentProgressEvent::StepCompleted {
            step_id: "s1".into(),
            step_index: 0,
            success: true,
            duration_ms: 12,
            output_summary: None,
            image_url: None,
            frontend_actions: vec![json!({
                "type": "navigate",
                "path": "/phantasi",
                "timestamp": 1
            })],
        };
        let value = serde_json::to_value(&event).unwrap();
        assert_eq!(value["type"], "step_completed");
        assert_eq!(value["frontendActions"][0]["type"], "navigate");
    }

    #[test]
    fn step_completed_omits_empty_frontend_actions() {
        let event = AgentProgressEvent::StepCompleted {
            step_id: "s1".into(),
            step_index: 0,
            success: true,
            duration_ms: 1,
            output_summary: None,
            image_url: None,
            frontend_actions: Vec::new(),
        };
        let value = serde_json::to_value(&event).unwrap();
        assert!(value.get("frontendActions").is_none());
    }
}
