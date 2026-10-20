//! The decisions questions put to a chat model instead: it answers each in
//! the shape a decisions model does, so the plan comes out the same way.
use super::*;

/// The same questions put to a chat model, which answers each in the shape a
/// decisions model does; the plan then comes out of the same answers.
pub(in crate::services::agent::merope::motion) fn chat_system_prompt(questions: &Value) -> String {
    let mut prompt = String::from(
        "You judge how a character acts at one moment of a chat: the input says who she is, what they said, what she replies and her mood. \
Answer every question below about her, as JSON. `confidence` is how sure you are, from 0 to 1: \
low when the moment does not show it plainly; what she does not show cannot be wrong. \
For a yes/no question, `noul` is the probability that the answer is true. \
For a scale, `score` runs from 0 (its first level) to the number of its last level.\n",
    );
    for (key, question) in questions.as_object().into_iter().flatten() {
        prompt.push_str(&format!(
            "\n{key}: {}\n",
            question["instructions"].as_str().unwrap_or_default()
        ));
        match &question["criteria"] {
            Value::Object(options) => {
                for (option, means) in options {
                    prompt.push_str(&format!(
                        "  - {option}: {}\n",
                        means.as_str().unwrap_or_default()
                    ));
                }
            }
            Value::Array(levels) => {
                for (level, means) in levels.iter().enumerate() {
                    prompt.push_str(&format!(
                        "  {level}: {}\n",
                        means.as_str().unwrap_or_default()
                    ));
                }
            }
            _ => {}
        }
    }
    prompt
}

/// The answer each question takes from a chat model, constrained.
pub(in crate::services::agent::merope::motion) fn chat_schema(questions: &Value) -> Value {
    let unit = json!({"type": "number", "minimum": 0, "maximum": 1});
    let mut properties = Map::new();
    for (key, question) in questions.as_object().into_iter().flatten() {
        let answer = match question["type"].as_str() {
            Some("noul") => json!({"type": "object", "additionalProperties": false,
                "required": ["noul"], "properties": {"noul": unit}}),
            Some("score") => {
                let top = question["criteria"]
                    .as_array()
                    .map_or(0, |levels| levels.len().saturating_sub(1));
                json!({"type": "object", "additionalProperties": false,
                    "required": ["score", "confidence"],
                    "properties": {"score": {"type": "number", "minimum": 0, "maximum": top}, "confidence": unit}})
            }
            _ => {
                let options: Vec<&String> = question["criteria"]
                    .as_object()
                    .map(|options| options.keys().collect())
                    .unwrap_or_default();
                json!({"type": "object", "additionalProperties": false,
                    "required": ["choice", "confidence"],
                    "properties": {"choice": {"type": "string", "enum": options}, "confidence": unit}})
            }
        };
        properties.insert(key.clone(), answer);
    }
    let required: Vec<String> = properties.keys().cloned().collect();
    json!({"type": "object", "additionalProperties": false, "required": required, "properties": properties})
}

/// Asks a chat model the decisions questions about this moment; the plan as
/// the chat director would have written it, for the shared parser.
pub(in crate::services::agent::merope::motion) async fn direct_by_chat(
    context: &MotionContext,
    state: Option<&RigStateSummary>,
    persona: Option<&str>,
    analyzer: &crate::services::analyzer::AiAnalyzer,
    timeout: std::time::Duration,
) -> anyhow::Result<String> {
    let started = std::time::Instant::now();
    let (asked_state, questions) = asked(context, state, persona, false);
    let answers = chat_answers(analyzer, &asked_state, &questions).await?;
    let mut plan = plan(&answers, context, state, false);
    // Two questions take a chat model no longer than the whole set did; it
    // waits no longer than that for the way, so a slow one cannot hold the look.
    let first = started.elapsed();
    let left = timeout
        .saturating_sub(first)
        .min(first.max(std::time::Duration::from_millis(1500)));
    ask_the_way(
        &mut plan,
        &answers,
        context,
        state,
        left,
        |way_state, way, _| async move { chat_answers(analyzer, &way_state, &way).await },
    )
    .await;
    Ok(plan.to_string())
}

/// One round of questions put to a chat model; its answers, shaped as a
/// decisions model gives them.
async fn chat_answers(
    analyzer: &crate::services::analyzer::AiAnalyzer,
    state: &Value,
    questions: &Value,
) -> anyhow::Result<Value> {
    let raw = analyzer
        .analyze_json(
            &chat_system_prompt(questions),
            &state.to_string(),
            "merope_acting_answers",
            Some(&chat_schema(questions)),
        )
        .await?;
    let answers: Value = serde_json::from_str(super::super::strip_motion_json(&raw))?;
    anyhow::ensure!(answers.is_object(), "answers must be an object");
    Ok(answers)
}
