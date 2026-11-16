//! The decisions questions put to a chat model instead: it answers each in
//! the shape a decisions model does, so the plan comes out the same way.
use myriad_merope::motion::questions::{chat_schema, chat_system_prompt};
use myriad_merope::motion::strip_motion_json;

use super::*;

/// Asks a chat model the decisions questions about this moment; the plan as
/// the chat director would have written it, for the shared parser.
pub(in crate::services::agent::merope::motion) async fn direct_by_chat(
    context: &Moment,
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
    let answers: Value = serde_json::from_str(strip_motion_json(&raw))?;
    anyhow::ensure!(answers.is_object(), "answers must be an object");
    Ok(answers)
}
