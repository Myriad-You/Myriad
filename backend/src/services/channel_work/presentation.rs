//! Work event projection. The run/task is authoritative; prompts are a transport view.
use super::*;

pub(crate) fn map_progress(
    event: &AgentProgressEvent,
    original_input: &str,
    caps: &myriad_agent_rules::channel::ChannelCapabilities,
) -> Option<(ChannelEvent, Option<PendingPrompt>)> {
    match event {
        AgentProgressEvent::WaitingForInput {
            task_id,
            question_id,
            question_type,
            question,
            options,
            ..
        } => {
            if !caps.interactive {
                return Some((ChannelEvent::ConfirmationRequired, None));
            }
            let mut prompt = PendingPrompt {
                id: String::new(),
                kind: PendingKind::Answer {
                    task_id: task_id.clone(),
                    question_id: question_id.clone(),
                    question_type: question_type.clone(),
                },
                question: question.clone(),
                options: options
                    .as_ref()
                    .map(|rows| {
                        rows.iter()
                            .map(|row| PendingOption {
                                value: row.value.clone(),
                                label: if row.label.trim().is_empty() {
                                    row.value.clone()
                                } else {
                                    row.label.clone()
                                },
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
                expires_at_unix: None,
            };
            ensure_pending_id(&mut prompt);
            Some((
                ChannelEvent::Answer {
                    message: format_pending_prompt(&prompt),
                    image_urls: Vec::new(),
                },
                Some(prompt),
            ))
        }
        AgentProgressEvent::Error { message, .. } => Some((
            ChannelEvent::Error {
                message: message.clone(),
            },
            None,
        )),
        AgentProgressEvent::TaskCompleted { response, .. } => {
            Some(map_completed(response, original_input, caps))
        }
        AgentProgressEvent::TaskCreated { total_steps, .. } => Some((
            ChannelEvent::TaskStarted {
                total_steps: *total_steps,
            },
            None,
        )),
        AgentProgressEvent::StepCompleted {
            frontend_actions, ..
        } if !frontend_actions.is_empty() => Some((ChannelEvent::FrontendAction, None)),
        AgentProgressEvent::ThinkingToken { .. }
        | AgentProgressEvent::StepStarted { .. }
        | AgentProgressEvent::StepCompleted { .. }
        | AgentProgressEvent::Progress { .. }
        | AgentProgressEvent::StepRetrying { .. }
        | AgentProgressEvent::RunStarted { .. }
        | AgentProgressEvent::SessionCreated { .. }
        | AgentProgressEvent::SessionTitleUpdated { .. }
        | AgentProgressEvent::SummaryToken { .. }
        | AgentProgressEvent::TaskAssigned { .. }
        | AgentProgressEvent::PlannerDecision { .. }
        | AgentProgressEvent::StepDebug { .. }
        | AgentProgressEvent::PerformancePlan { .. }
        | AgentProgressEvent::MeropeStateChanged { .. }
        | AgentProgressEvent::OutfitOverlay { .. }
        | AgentProgressEvent::MusicControl { .. } => None,
    }
}

pub(crate) fn map_completed(
    response: &Value,
    original_input: &str,
    caps: &myriad_agent_rules::channel::ChannelCapabilities,
) -> (ChannelEvent, Option<PendingPrompt>) {
    if response.get("frontendAction").is_some() {
        let session_id = response.get("sessionId").and_then(Value::as_str);
        let task_id = response
            .get("task")
            .and_then(|task| task.get("taskId"))
            .and_then(Value::as_str);
        return (
            ChannelEvent::Error {
                message: panel_entry_reply(session_id, task_id),
            },
            None,
        );
    }
    if response.pointer("/task/status").and_then(Value::as_str) == Some("waiting_for_input") {
        if let Some(prompt) = task_pending_prompt(response) {
            return (
                ChannelEvent::Answer {
                    message: format_pending_prompt(&prompt),
                    image_urls: Vec::new(),
                },
                Some(prompt),
            );
        }
        return (ChannelEvent::ConfirmationRequired, None);
    }
    let response_type = response
        .get("responseType")
        .and_then(Value::as_str)
        .unwrap_or("");
    if response_type == "confirmation_required" || response.get("confirmation").is_some() {
        if !caps.interactive {
            return (ChannelEvent::ConfirmationRequired, None);
        }
        if let Some(prompt) = confirmation_prompt(response) {
            return (
                ChannelEvent::Answer {
                    message: format_pending_prompt(&prompt),
                    image_urls: Vec::new(),
                },
                Some(prompt),
            );
        }
        return (ChannelEvent::ConfirmationRequired, None);
    }
    if response_type == "clarification" {
        if !caps.interactive {
            return (ChannelEvent::ConfirmationRequired, None);
        }
        let prompt = clarification_prompt(response, original_input);
        return (
            ChannelEvent::Answer {
                message: format_pending_prompt(&prompt),
                image_urls: Vec::new(),
            },
            Some(prompt),
        );
    }
    let message = response
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if let Some(prompt) = pending_prompt_from_model_json(message, original_input) {
        if caps.interactive {
            return (
                ChannelEvent::Answer {
                    message: format_pending_prompt(&prompt),
                    image_urls: Vec::new(),
                },
                Some(prompt),
            );
        }
    }
    let image_urls = if caps.outbound_image {
        collect_channel_image_urls(response)
    } else {
        Vec::new()
    };
    let data = if image_urls.is_empty() || response.get("dataDisplay").is_some() {
        response.get("data")
    } else {
        None
    };
    let rendered = format_channel_result(message, data, response.get("dataDisplay"));
    if response.get("success").and_then(Value::as_bool) == Some(false) || response_type == "error" {
        return (
            ChannelEvent::Error {
                message: if rendered.is_empty() {
                    "办事失败。".into()
                } else {
                    rendered
                },
            },
            None,
        );
    }
    let event = ChannelEvent::Answer {
        message: if rendered.is_empty() && image_urls.is_empty() {
            PANEL_REQUIRED_REPLY.to_string()
        } else {
            rendered
        },
        image_urls,
    };
    if !channel_can_finish(caps, &event) {
        return (ChannelEvent::FrontendAction, None);
    }
    (event, None)
}

fn task_pending_prompt(response: &Value) -> Option<PendingPrompt> {
    let task = response.get("task")?;
    let question = task.get("pendingQuestion")?;
    let mut prompt = PendingPrompt {
        id: String::new(),
        kind: PendingKind::Answer {
            task_id: task.get("taskId")?.as_str()?.to_string(),
            question_id: question.get("questionId")?.as_str()?.to_string(),
            question_type: question
                .get("type")
                .or_else(|| question.get("questionType"))
                .and_then(Value::as_str)
                .unwrap_or("free_text")
                .to_string(),
        },
        question: question.get("question")?.as_str()?.to_string(),
        options: question
            .get("options")
            .and_then(Value::as_array)
            .map(|rows| {
                rows.iter()
                    .filter_map(|row| {
                        Some(PendingOption {
                            value: row.get("value")?.as_str()?.to_string(),
                            label: row
                                .get("label")
                                .or_else(|| row.get("value"))?
                                .as_str()?
                                .to_string(),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default(),
        expires_at_unix: None,
    };
    ensure_pending_id(&mut prompt);
    Some(prompt)
}

fn confirmation_prompt(response: &Value) -> Option<PendingPrompt> {
    let confirmation = response.get("confirmation")?;
    let confirmation_id = confirmation
        .get("confirmationId")
        .and_then(Value::as_str)
        .filter(|id| !id.is_empty())?;
    let question = response
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let step = confirmation
        .get("pendingSteps")
        .and_then(Value::as_array)
        .and_then(|steps| steps.first())
        .and_then(|step| step.get("message"))
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    let question = if question.is_empty() {
        step.to_string()
    } else if step.is_empty() || question.contains(step) {
        question.to_string()
    } else {
        format!("{question}\n{step}")
    };
    let expires_in = confirmation
        .get("expiresInSeconds")
        .and_then(Value::as_i64)
        .filter(|secs| *secs > 0);
    let mut prompt = PendingPrompt {
        id: String::new(),
        kind: PendingKind::Confirm {
            confirmation_id: confirmation_id.to_string(),
        },
        question,
        options: Vec::new(),
        expires_at_unix: expires_in.map(|secs| Utc::now().timestamp().saturating_add(secs)),
    };
    ensure_pending_id(&mut prompt);
    Some(prompt)
}

fn clarification_prompt(response: &Value, original_input: &str) -> PendingPrompt {
    let question = response
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim()
        .to_string();
    let mut options = json_string_options(response.get("suggestions"));
    if options.is_empty() {
        options = json_string_options(
            response
                .get("clarification")
                .and_then(|value| value.get("options")),
        );
    }
    let mut prompt = PendingPrompt {
        id: String::new(),
        kind: PendingKind::Clarify {
            original_input: clarify_base_input(original_input).to_string(),
        },
        question: question.clone(),
        options,
        expires_at_unix: None,
    };
    if prompt.options.is_empty() {
        if let Some(from_json) = pending_prompt_from_model_json(&question, original_input) {
            return from_json;
        }
    }
    ensure_pending_id(&mut prompt);
    prompt
}

fn json_string_options(value: Option<&Value>) -> Vec<PendingOption> {
    value
        .and_then(Value::as_array)
        .map(|rows| {
            rows.iter()
                .filter_map(|row| {
                    let text = row.as_str().or_else(|| {
                        row.get("label")
                            .and_then(Value::as_str)
                            .or_else(|| row.get("value").and_then(Value::as_str))
                    })?;
                    let trimmed = text.trim();
                    (!trimmed.is_empty()).then(|| PendingOption {
                        value: trimmed.to_string(),
                        label: trimmed.to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn telegram_caps() -> myriad_agent_rules::channel::ChannelCapabilities {
        telegram_dm_capabilities()
    }

    #[test]
    fn task_created_becomes_a_start_notice_and_keeps_the_turn_open() {
        let (event, parked) = map_progress(
            &AgentProgressEvent::TaskCreated {
                task_id: "task_1".into(),
                message: String::new(),
                total_steps: 3,
                step_descriptions: Vec::new(),
            },
            "整理表格",
            &qq_c2c_capabilities(),
        )
        .expect("task start is mapped");
        assert_eq!(event, ChannelEvent::TaskStarted { total_steps: 3 });
        assert!(parked.is_none());
    }

    #[test]
    fn feishu_has_no_typing_so_task_start_is_delivered() {
        let sink = ChannelTransport::Feishu {
            chat_id: "oc_chat".into(),
        };
        let ctx = sink.delivery_context();
        assert!(!ctx.typing);
        assert_eq!(
            plan_delivery(&ChannelEvent::TaskStarted { total_steps: 2 }, &ctx),
            DeliveryPlan::ActiveText {
                content: task_started_reply(2),
                image_urls: Vec::new(),
            }
        );
    }

    #[test]
    fn completed_answer_uses_message() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "answer",
                "message": "票已订好"
            }),
            "订票",
            &telegram_caps(),
        );
        assert_eq!(
            event,
            ChannelEvent::Answer {
                message: "票已订好".into(),
                image_urls: Vec::new(),
            }
        );
        assert!(parked.is_none());
    }

    #[test]
    fn completed_image_keeps_message_and_urls() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "answer",
                "message": "图片已经生成好了",
                "data": {
                    "format": "image",
                    "value": {
                        "url": "/api/brew/image-cache/ab/abcd.png",
                        "width": 1,
                        "height": 1
                    }
                },
                "task": {
                    "stepHistory": [{ "imageUrl": "https://cdn.example/a.png" }]
                }
            }),
            "画一只猫",
            &telegram_caps(),
        );
        let ChannelEvent::Answer {
            message,
            image_urls,
        } = event
        else {
            panic!("{event:?}");
        };
        assert_eq!(message, "图片已经生成好了");
        assert!(!message.contains("format"));
        assert_eq!(
            image_urls,
            vec![
                "/api/brew/image-cache/ab/abcd.png",
                "https://cdn.example/a.png",
            ]
        );
        assert!(parked.is_none());
    }

    #[test]
    fn completed_image_without_message_does_not_send_panel_entry() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "answer",
                "message": "",
                "data": {
                    "format": "image",
                    "value": { "url": "/api/brew/image-cache/ab/abcd.png" }
                }
            }),
            "画一只猫",
            &telegram_caps(),
        );
        let ChannelEvent::Answer {
            message,
            image_urls,
        } = event
        else {
            panic!("{event:?}");
        };
        assert!(message.is_empty());
        assert_eq!(image_urls, vec!["/api/brew/image-cache/ab/abcd.png"]);
        assert!(parked.is_none());
    }

    #[test]
    fn table_result_is_appended() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "answer",
                "message": "查到了",
                "data": [{"name": "A"}],
                "dataDisplay": {
                    "type": "table",
                    "columns": [{"field": "name", "title": "名称"}]
                }
            }),
            "查",
            &telegram_caps(),
        );
        let ChannelEvent::Answer {
            message,
            image_urls,
        } = event
        else {
            panic!("{event:?}");
        };
        assert!(image_urls.is_empty());
        assert!(message.contains("查到了"));
        assert!(message.contains("名称"));
        assert!(message.contains("A"));
        assert!(parked.is_none());
    }

    #[test]
    fn confirmation_asks_yes_or_no() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "confirmation_required",
                "message": "要删掉这篇文章吗？",
                "confirmation": {
                    "confirmationId": "c1",
                    "expiresInSeconds": 60
                }
            }),
            "删除文章",
            &telegram_caps(),
        );
        let ChannelEvent::Answer {
            message,
            image_urls,
        } = event
        else {
            panic!("{event:?}");
        };
        assert!(image_urls.is_empty());
        assert!(message.contains("要删掉这篇文章吗？"));
        let prompt = parked.expect("confirmation parks");
        assert!(!prompt.id.is_empty());
        assert!(matches!(
            prompt.kind,
            PendingKind::Confirm { confirmation_id } if confirmation_id == "c1"
        ));
    }

    #[test]
    fn frontend_action_points_at_the_session() {
        let (event, parked) = map_completed(
            &serde_json::json!({
                "success": true,
                "responseType": "answer",
                "message": "打开页面",
                "sessionId": "ses_1",
                "task": {"taskId": "t9"},
                "frontendAction": {"type": "navigate"}
            }),
            "打开",
            &telegram_caps(),
        );
        let ChannelEvent::Error { message } = event else {
            panic!("{event:?}");
        };
        assert!(message.contains("ses_1"));
        assert!(message.contains("t9"));
        assert!(parked.is_none());
    }
}

#[cfg(test)]
mod continuation_tests {
    use super::*;
    #[test]
    fn completed_waiting_response_preserves_question_identity_and_option_values() {
        let response = serde_json::json!({
            "success": true, "responseType": "answer", "message": "需要更多信息", "streamTerminal": false,
            "task": { "taskId": "task2", "status": "waiting_for_input", "pendingQuestion": {
                "questionId": "pre_param:step1:city", "questionType": "single_choice", "question": "去哪里？",
                "options": [{"value": "tokyo", "label": "东京"}]
            }}
        });
        let (_, pending) = map_completed(&response, "旅行", &telegram_dm_capabilities());
        let pending = pending.unwrap();
        assert_eq!(pending.options[0].value, "tokyo");
        assert!(
            matches!(pending.kind, PendingKind::Answer { ref task_id, ref question_id, .. } if task_id == "task2" && question_id == "pre_param:step1:city")
        );
        assert!(
            matches!(decide_pending_reply(&pending, "1", 0), PendingDecision::Resume { answer, .. } if answer == "tokyo")
        );
    }
}
