use super::*;
use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};

pub(super) const MAX_ROUNDS: u32 = 32;
pub(super) const MAX_CALLS: usize = 64;
pub(super) const MAX_INPUT_CHARS: usize = 1_000_000;
pub(super) const MAX_ACTIVE_MS: u64 = 20 * 60 * 1000;
pub(super) const RESULT_CHARS: usize = 12_000;

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct PendingCall {
    pub call: ToolCall,
    /// Resolved at model-response time; tool discovery cannot reinterpret it.
    pub capability_id: Option<String>,
    pub approval: Option<String>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(super) enum Wait {
    Answer { call: ToolCall },
    Approval { fingerprint: String },
    Interaction { call: ToolCall, step_id: String },
    Recovery,
}

#[derive(Clone, Serialize, Deserialize)]
pub(super) struct Checkpoint {
    #[serde(default)]
    pub budget: Option<super::budget::Budget>,
    #[serde(default)]
    pub recipe_run: Option<super::recipes::RecipeRun>,
    pub version: u32,
    pub revision: u64,
    pub lease_id: String,
    pub user_id: i32,
    pub request: UserRequest,
    pub task: TaskState,
    pub history: Vec<ToolMessage>,
    pub selected: Vec<String>,
    pub pending: VecDeque<PendingCall>,
    pub inflight: Option<String>,
    pub wait: Option<Wait>,
    pub denied: HashSet<String>,
    pub attempted_effects: HashSet<String>,
    pub call_counts: std::collections::HashMap<String, u32>,
    pub rounds: u32,
    pub calls: usize,
    pub input_chars: usize,
    pub active_ms: u64,
    pub plan: Value,
    pub final_text: String,
    /// The memories the run was given, and its one recall where it is stuck.
    #[serde(default)]
    pub recall: super::stuck::Recall,
}

impl Checkpoint {
    pub fn context(&self) -> ExecutionContext {
        self.task.execution_context.clone().unwrap_or_default()
    }

    pub fn tool_result(&mut self, call: ToolCall, output: &Value) {
        self.record_result(&call, output);
        if self
            .recipe_run
            .as_ref()
            .and_then(|frame| frame.active_call.as_ref())
            == Some(&call.id)
        {
            return;
        }
        self.history.push(ToolMessage::Tool {
            call,
            content: preview(output, RESULT_CHARS),
        });
    }

    /// Local questions, declined calls and recovery results must also be
    /// retrievable after their history text is compacted.
    fn record_result(&mut self, call: &ToolCall, output: &Value) {
        self.task
            .step_results
            .entry(call.id.clone())
            .or_insert_with(|| StepResult {
                step_id: call.id.clone(),
                success: output.get("error").is_none() && output.get("cancelled").is_none(),
                output: Some(output.clone()),
                error: output
                    .get("error")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
                duration_ms: 0,
                retry_count: 0,
            });
    }

    /// A recipe started directly (preset API) answers no assistant tool call,
    /// so its aggregate joins the user turn instead of a tool message. Every
    /// provider then sees a well-formed history before the first model turn.
    pub fn direct_recipe_result(&mut self, call: ToolCall, output: &Value) {
        self.record_result(&call, output);
        let text =
            myriad_agent_rules::untrusted_block("recipe_result", &preview(output, RESULT_CHARS));
        match self.history.last_mut() {
            Some(ToolMessage::User { content }) => {
                content.push_str("\n\n");
                content.push_str(&text);
            }
            _ => self.history.push(ToolMessage::User { content: text }),
        }
    }

    pub fn budget_error(&self) -> Option<&'static str> {
        if self
            .budget
            .as_ref()
            .is_some_and(|budget| budget.remaining() == 0)
        {
            Some("The task reached its token budget")
        } else if self.pending.is_empty() && self.rounds >= MAX_ROUNDS {
            Some("The task reached its model-turn limit")
        } else if self.calls >= MAX_CALLS {
            Some("The task reached its tool-call limit")
        } else if self.pending.is_empty() && self.input_chars >= MAX_INPUT_CHARS {
            Some("The task reached its context budget")
        } else if self.active_ms >= MAX_ACTIVE_MS {
            Some("The task reached its active-time limit")
        } else {
            None
        }
    }

    pub fn validate_answer(&self, answer: &UserAnswer) -> Result<(), String> {
        let question = self
            .task
            .pending_question
            .as_ref()
            .ok_or("This task has no pending question")?;
        if self.task.status != TaskStatus::WaitingForInput
            || answer.task_id != self.task.task_id
            || answer.question_id != question.question_id
        {
            return Err("The answer does not match the current task question".into());
        }
        // Tapp expiry is itself an authenticated terminal result, verified
        // against its persisted interaction before apply_answer is called.
        if !matches!(self.wait, Some(Wait::Interaction { .. }))
            && question.is_expired(chrono::Utc::now())
        {
            return Err("The question has expired".into());
        }
        let answer_limit = if matches!(self.wait, Some(Wait::Interaction { .. })) {
            128_000
        } else {
            16_000
        };
        if answer.answer.chars().count() > answer_limit {
            return Err("The answer is too long".into());
        }
        if matches!(self.wait, Some(Wait::Approval { .. }))
            && !answer.skipped
            && !matches!(answer.answer.as_str(), "yes" | "no")
        {
            return Err("Select yes or no for this operation".into());
        }
        Ok(())
    }

    /// Trim result bodies, never assistant signatures or unresolved call pairs.
    /// Full results remain in the server checkpoint and read_result can page them.
    pub fn prune_results(&mut self) {
        let older = self.history.len().saturating_sub(8);
        for message in self.history.iter_mut().take(older) {
            if let ToolMessage::Tool { call, content } = message {
                if content.chars().count() > 1200 {
                    *content = format!(
                        "{}\n[Saved result {}: use read_result to retrieve more.]",
                        content.chars().take(1000).collect::<String>(),
                        call.id
                    );
                }
            }
        }
    }
}

pub(super) fn preview(value: &Value, limit: usize) -> String {
    let text = value.to_string();
    if text.chars().count() <= limit {
        return text;
    }
    format!(
        "{}\n[Result truncated; use read_result with the call id to inspect the saved output.]",
        text.chars().take(limit).collect::<String>()
    )
}

pub(super) fn fingerprint(capability: &Capability, params: &Value) -> String {
    use sha2::{Digest, Sha256};
    // Binds approval to arguments AND the tool's current policy/schema.
    hex::encode(Sha256::digest(
        serde_json::to_vec(&json!({"capability":capability,"params":params})).unwrap(),
    ))
}

pub(super) fn operation_key(id: &str, params: &Value) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(
        json!({"tool":id,"params":params}).to_string().as_bytes(),
    ))
}

pub(crate) fn is_work_recipe(recipe: &Recipe) -> bool {
    recipe.engine == AgentEngine::WorkLoop
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn approval_cannot_survive_argument_or_policy_changes() {
        let mut cap = Capability::default();
        cap.id = "note.create".into();
        let first = fingerprint(&cap, &json!({"title":"one"}));
        assert_ne!(first, fingerprint(&cap, &json!({"title":"two"})));
        cap.required_permissions.push("new:permission".into());
        assert_ne!(first, fingerprint(&cap, &json!({"title":"one"})));
    }
    #[test]
    fn bounded_results_preserve_unicode_and_indicate_retrieval() {
        let value = json!("東京".repeat(30));
        let text = preview(&value, 10);
        assert!(text.starts_with("\"東京東京"));
        assert!(text.contains("read_result"));
    }
}
