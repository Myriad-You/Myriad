//! Pure resume / answer validation and pending-question rules.
//!
//! Store locks, CAS claim, SSE, and step re-execution stay in
//! `executor/mod.rs`. Domain owns:
//! - answer `question_id` match policy (single vs multi pending)
//! - question expiry checks and queue pruning
//! - answer → context var / output / decision plan
//! - free-text required empty placeholder
//! - cancel/skip/retry/confirmation skip disposition
//! - pre_param queue-head detection and apply qid selection

use crate::services::agent::types::{
    DecisionType, QuestionOption, QuestionType, UserAnswer, UserQuestion,
};
use chrono::{DateTime, Utc};
use serde_json::{json, Value};

/// Stored when a required free-text question gets an empty answer.
pub const EMPTY_REQUIRED_ANSWER_PLACEHOLDER: &str = "（用户未提供输入）";

/// Result of checking whether an answer targets the active pending question.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnswerIdValidation {
    /// IDs match exactly.
    Ok,
    /// IDs differ but only one active question exists — accept for legacy clients.
    AcceptedMismatch {
        expected: String,
        actual: String,
    },
    /// IDs differ with more questions queued — reject.
    RejectMismatch {
        expected: String,
        actual: String,
    },
}

impl AnswerIdValidation {
    pub fn is_reject(&self) -> bool {
        matches!(self, Self::RejectMismatch { .. })
    }

    pub fn mismatch_error_message(&self) -> Option<String> {
        match self {
            Self::RejectMismatch { expected, actual } => Some(format!(
                "Question ID mismatch: expected {expected}, got {actual}"
            )),
            _ => None,
        }
    }
}

/// Validate answer.question_id against the current pending question.
///
/// `queued_pending_empty` is true when `context.pending_questions` has no
/// additional queued items (only the active pending_question exists).
pub fn validate_answer_question_id(
    answer_qid: &str,
    expected_qid: &str,
    queued_pending_empty: bool,
) -> AnswerIdValidation {
    if answer_qid == expected_qid {
        return AnswerIdValidation::Ok;
    }
    if queued_pending_empty {
        AnswerIdValidation::AcceptedMismatch {
            expected: expected_qid.to_string(),
            actual: answer_qid.to_string(),
        }
    } else {
        AnswerIdValidation::RejectMismatch {
            expected: expected_qid.to_string(),
            actual: answer_qid.to_string(),
        }
    }
}

/// Whether a question's `expires_at` is strictly in the past.
pub fn is_question_expired(expires_at: Option<DateTime<Utc>>, now: DateTime<Utc>) -> bool {
    expires_at.is_some_and(|exp| now > exp)
}

/// Whether a pending question is still askable at `now`.
pub fn is_pending_question_still_valid(
    expires_at: Option<DateTime<Utc>>,
    now: DateTime<Utc>,
) -> bool {
    expires_at.is_none_or(|exp| now <= exp)
}

/// Drop expired items from a pending-question queue; returns count removed.
pub fn retain_unexpired_pending(
    questions: &mut Vec<UserQuestion>,
    now: DateTime<Utc>,
) -> usize {
    let before = questions.len();
    questions.retain(|q| is_pending_question_still_valid(q.expires_at, now));
    before.saturating_sub(questions.len())
}

/// Storage key for per-question answers in context variables / step outputs.
pub fn answer_storage_key(question_id: &str) -> String {
    format!("answer_{question_id}")
}

/// Step-output payload for `xxxFrom` references to a user answer.
pub fn answer_step_output(answer: &str, question_text: &str) -> Value {
    json!({
        "answer": answer,
        "question": question_text,
    })
}

/// Control answers used by error-recovery choice UI.
pub fn is_control_choice_answer(answer: &str) -> bool {
    matches!(answer, "cancel" | "skip" | "retry")
}

/// Whether resume should clear a failed step for re-execution.
pub fn is_retry_answer(answer: &str) -> bool {
    answer == "retry"
}

/// Head of deferred queue is a pre_param fill-in (do not start execution yet).
pub fn is_pre_param_question_id(question_id: &str) -> bool {
    question_id.starts_with("pre_param")
}

/// Which question_id should drive pre_param recipe write-back (if either parses).
pub fn resolve_pre_param_apply_qid(answer_qid: &str, question_qid: &str) -> Option<String> {
    if crate::services::agent::parse_pre_param_question_id(answer_qid).is_some() {
        return Some(answer_qid.to_string());
    }
    if crate::services::agent::parse_pre_param_question_id(question_qid).is_some() {
        return Some(question_qid.to_string());
    }
    None
}

/// Free-text / numeric storage value (required empty → placeholder).
pub fn free_text_user_input(answer: &str, required: bool) -> String {
    if required && answer.trim().is_empty() {
        EMPTY_REQUIRED_ANSWER_PLACEHOLDER.to_string()
    } else {
        answer.to_string()
    }
}

/// Whether a choice answer matches an option value.
pub fn is_valid_option_value(answer: &str, options: &[QuestionOption]) -> bool {
    options.iter().any(|o| o.value == answer)
}

/// Label for a matching option, if any.
pub fn selected_option_label(answer: &str, options: &[QuestionOption]) -> Option<String> {
    options
        .iter()
        .find(|o| o.value == answer)
        .map(|o| o.label.clone())
}

/// Planned decision record (timestamp applied by adapter).
#[derive(Debug, Clone, PartialEq)]
pub struct PlannedDecision {
    pub decision_type: DecisionType,
    pub description: String,
    pub reasoning: String,
}

/// Pure plan of context mutations + skip disposition for one user answer.
#[derive(Debug, Clone)]
pub struct AnswerProcessPlan {
    /// Cancel / confirmation-reject → complete without further steps.
    pub should_skip_remaining: bool,
    /// Context variables to set (key, value).
    pub vars: Vec<(String, Value)>,
    /// Key under which to store the answer step output.
    pub answer_output_key: String,
    /// Answer step-output payload.
    pub answer_output: Value,
    /// Decision history entries to append.
    pub decisions: Vec<PlannedDecision>,
    /// When set, adapter writes answer into recipe via pre_param helper.
    pub pre_param_qid: Option<String>,
    /// Choice answer not in options and not a control verb.
    pub warn_invalid_option: bool,
    /// Required free-text was empty (placeholder used).
    pub used_empty_required_placeholder: bool,
}

/// Plan how a validated, non-expired answer mutates execution context / recipe.
pub fn plan_user_answer_effects(
    answer: &UserAnswer,
    question: &UserQuestion,
) -> AnswerProcessPlan {
    let answer_output_key = answer_storage_key(&answer.question_id);
    let mut plan = AnswerProcessPlan {
        should_skip_remaining: false,
        vars: vec![(
            answer_output_key.clone(),
            json!(answer.answer.clone()),
        )],
        answer_output_key: answer_output_key.clone(),
        answer_output: answer_step_output(&answer.answer, &question.question),
        decisions: Vec::new(),
        pre_param_qid: resolve_pre_param_apply_qid(&answer.question_id, &question.question_id),
        warn_invalid_option: false,
        used_empty_required_placeholder: false,
    };

    match question.question_type {
        QuestionType::SingleChoice | QuestionType::MultipleChoice => {
            plan.vars
                .push(("user_choice".into(), json!(answer.answer.clone())));

            if let Some(options) = &question.options {
                if !options.is_empty() {
                    if is_valid_option_value(&answer.answer, options) {
                        if let Some(label) = selected_option_label(&answer.answer, options) {
                            plan.vars
                                .push(("selected_option_label".into(), json!(label)));
                        }
                    } else if !is_control_choice_answer(&answer.answer) {
                        plan.warn_invalid_option = true;
                    }
                }
            }

            match answer.answer.as_str() {
                "cancel" => {
                    plan.decisions.push(PlannedDecision {
                        decision_type: DecisionType::SkipStep,
                        description: "用户选择取消任务".into(),
                        reasoning: "用户选择不执行该操作".into(),
                    });
                    plan.should_skip_remaining = true;
                }
                "skip" => {
                    plan.decisions.push(PlannedDecision {
                        decision_type: DecisionType::SkipStep,
                        description: "用户选择跳过错误步骤".into(),
                        reasoning: "跳过当前步骤继续执行".into(),
                    });
                }
                "retry" => {
                    plan.decisions.push(PlannedDecision {
                        decision_type: DecisionType::ModifyParams,
                        description: "用户选择重试失败步骤".into(),
                        reasoning: "重新执行出错的步骤".into(),
                    });
                }
                _ => {
                    plan.decisions.push(PlannedDecision {
                        decision_type: DecisionType::ModifyParams,
                        description: format!("用户选择了: {}", answer.answer),
                        reasoning: "根据用户选择调整执行参数".into(),
                    });
                }
            }
        }
        QuestionType::FreeText => {
            let input = free_text_user_input(&answer.answer, question.required);
            plan.used_empty_required_placeholder =
                question.required && answer.answer.trim().is_empty();
            plan.vars.push(("user_input".into(), json!(input)));
            plan.decisions.push(PlannedDecision {
                decision_type: DecisionType::ModifyParams,
                description: format!("用户输入了: {}", answer.answer),
                reasoning: "使用用户提供的信息".into(),
            });
        }
        QuestionType::Confirmation => {
            if answer.answer == "yes" {
                plan.decisions.push(PlannedDecision {
                    decision_type: DecisionType::GenerateSteps,
                    description: "用户确认继续执行".into(),
                    reasoning: "用户确认了操作".into(),
                });
            } else {
                plan.decisions.push(PlannedDecision {
                    decision_type: DecisionType::SkipStep,
                    description: "用户拒绝，跳过相关操作".into(),
                    reasoning: "用户选择不执行该操作".into(),
                });
                plan.should_skip_remaining = true;
            }
        }
        QuestionType::Numeric | QuestionType::Date => {
            plan.vars
                .push(("user_input".into(), json!(answer.answer.clone())));
            plan.decisions.push(PlannedDecision {
                decision_type: DecisionType::ModifyParams,
                description: format!("用户输入了: {}", answer.answer),
                reasoning: "使用用户提供的信息".into(),
            });
        }
    }

    plan
}

/// Decision recorded when a pending question expired before the answer landed.
pub fn expired_answer_decision() -> PlannedDecision {
    PlannedDecision {
        decision_type: DecisionType::SkipStep,
        description: "用户回答已过期，跳过该问题".into(),
        reasoning: "问题超时未回答，继续执行剩余步骤".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    fn choice_q(id: &str, options: Vec<(&str, &str)>) -> UserQuestion {
        UserQuestion {
            question_id: id.into(),
            question_type: QuestionType::SingleChoice,
            question: "pick".into(),
            context: String::new(),
            options: Some(
                options
                    .into_iter()
                    .map(|(v, l)| QuestionOption {
                        value: v.into(),
                        label: l.into(),
                        description: None,
                    })
                    .collect(),
            ),
            required: true,
            default_value: None,
            created_at: Utc::now(),
            expires_at: None,
        }
    }

    fn answer(qid: &str, body: &str) -> UserAnswer {
        UserAnswer {
            question_id: qid.into(),
            task_id: "t1".into(),
            answer: body.into(),
            skipped: false,
        }
    }

    #[test]
    fn question_id_match_and_legacy_accept() {
        assert_eq!(
            validate_answer_question_id("q1", "q1", true),
            AnswerIdValidation::Ok
        );
        assert!(matches!(
            validate_answer_question_id("old", "q1", true),
            AnswerIdValidation::AcceptedMismatch { .. }
        ));
        let rej = validate_answer_question_id("old", "q1", false);
        assert!(rej.is_reject());
        assert!(rej.mismatch_error_message().unwrap().contains("mismatch"));
    }

    #[test]
    fn expiry_and_retain_pending() {
        let now = Utc::now();
        assert!(!is_question_expired(None, now));
        assert!(!is_question_expired(Some(now + Duration::minutes(5)), now));
        assert!(is_question_expired(Some(now - Duration::seconds(1)), now));

        let mut qs = vec![
            UserQuestion {
                question_id: "alive".into(),
                question_type: QuestionType::FreeText,
                question: "a".into(),
                context: String::new(),
                options: None,
                required: false,
                default_value: None,
                created_at: now,
                expires_at: Some(now + Duration::hours(1)),
            },
            UserQuestion {
                question_id: "dead".into(),
                question_type: QuestionType::FreeText,
                question: "b".into(),
                context: String::new(),
                options: None,
                required: false,
                default_value: None,
                created_at: now,
                expires_at: Some(now - Duration::hours(1)),
            },
        ];
        assert_eq!(retain_unexpired_pending(&mut qs, now), 1);
        assert_eq!(qs.len(), 1);
        assert_eq!(qs[0].question_id, "alive");
    }

    #[test]
    fn cancel_and_confirmation_skip_remaining() {
        let q = choice_q("err", vec![("retry", "重试"), ("cancel", "取消")]);
        let plan = plan_user_answer_effects(&answer("err", "cancel"), &q);
        assert!(plan.should_skip_remaining);
        assert_eq!(plan.decisions[0].decision_type, DecisionType::SkipStep);

        let conf = UserQuestion {
            question_id: "c1".into(),
            question_type: QuestionType::Confirmation,
            question: "ok?".into(),
            context: String::new(),
            options: None,
            required: true,
            default_value: None,
            created_at: Utc::now(),
            expires_at: None,
        };
        assert!(
            plan_user_answer_effects(&answer("c1", "no"), &conf).should_skip_remaining
        );
        assert!(
            !plan_user_answer_effects(&answer("c1", "yes"), &conf).should_skip_remaining
        );
    }

    #[test]
    fn choice_option_label_and_invalid_warn() {
        let q = choice_q("c", vec![("a", "选项A"), ("b", "选项B")]);
        let plan = plan_user_answer_effects(&answer("c", "a"), &q);
        assert!(!plan.warn_invalid_option);
        assert!(plan.vars.iter().any(|(k, v)| {
            k == "selected_option_label" && v.as_str() == Some("选项A")
        }));

        let bad = plan_user_answer_effects(&answer("c", "zzz"), &q);
        assert!(bad.warn_invalid_option);
        // control verbs do not warn
        assert!(
            !plan_user_answer_effects(&answer("c", "retry"), &q).warn_invalid_option
        );
    }

    #[test]
    fn free_text_required_empty_placeholder() {
        let q = UserQuestion {
            question_id: "f1".into(),
            question_type: QuestionType::FreeText,
            question: "name?".into(),
            context: String::new(),
            options: None,
            required: true,
            default_value: None,
            created_at: Utc::now(),
            expires_at: None,
        };
        let plan = plan_user_answer_effects(&answer("f1", "  "), &q);
        assert!(plan.used_empty_required_placeholder);
        let input = plan
            .vars
            .iter()
            .find(|(k, _)| k == "user_input")
            .map(|(_, v)| v.as_str().unwrap())
            .unwrap();
        assert_eq!(input, EMPTY_REQUIRED_ANSWER_PLACEHOLDER);
    }

    #[test]
    fn pre_param_qid_and_retry_helpers() {
        assert!(is_pre_param_question_id("pre_param:step:tappId"));
        assert!(is_pre_param_question_id("pre_param_url"));
        assert!(!is_pre_param_question_id("user_q"));
        assert_eq!(
            resolve_pre_param_apply_qid("pre_param:s1:p", "other"),
            Some("pre_param:s1:p".into())
        );
        assert_eq!(
            resolve_pre_param_apply_qid("other", "pre_param:s1:p"),
            Some("pre_param:s1:p".into())
        );
        assert!(resolve_pre_param_apply_qid("a", "b").is_none());
        assert!(is_retry_answer("retry"));
        assert!(!is_retry_answer("skip"));
        assert_eq!(answer_storage_key("q9"), "answer_q9");
    }

    #[test]
    fn expired_decision_and_output_shape() {
        let d = expired_answer_decision();
        assert_eq!(d.decision_type, DecisionType::SkipStep);
        assert!(d.description.contains("过期"));
        let out = answer_step_output("hi", "what?");
        assert_eq!(out["answer"], json!("hi"));
        assert_eq!(out["question"], json!("what?"));
    }
}
