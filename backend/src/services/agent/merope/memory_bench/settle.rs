//! The settle way of the memory bench (`MEROPE_MEMORY_BENCH_WAY=settle`):
//! each session's lines taken as short notes, the session gone over once it
//! is over, and only what lasts kept for good, as production does now.

use super::*;

/// What one of their lines leaves, taken as production takes it but not
/// kept: a short note, in the settle way.
async fn note_turn(
    db: &sea_orm::DatabaseConnection,
    judge: &crate::services::analyzer::AiAnalyzer,
    user_id: i32,
    present: &Audience,
    text: &str,
    reply: &str,
    at: chrono::DateTime<chrono::FixedOffset>,
) -> Option<super::super::chat_remember::ChatMemoryUpdates> {
    let (existing, _) = super::super::store::recall_remembered_primed(
        db,
        user_id,
        present,
        Some(text),
        8,
        &Priming::default(),
        1.0,
    )
    .await
    .ok()?;
    if !super::super::chat_remember::should_extract_chat_remember_against(text, &existing) {
        return None;
    }
    let input = super::super::chat_remember::extract_input(
        text,
        reply,
        &super::super::TurnContext::default(),
        at.with_timezone(&super::super::clock::zone()),
    );
    let raw = judge
        .analyze_json(
            &myriad_merope::chat_remember::extract_system_prompt(&existing),
            &input,
            myriad_merope::chat_remember::EXTRACT_SCHEMA_NAME,
            Some(&myriad_merope::chat_remember::extract_schema()),
        )
        .await
        .ok()?;
    super::super::chat_remember::parse_chat_memory_updates(&raw, text, reply, &existing)
}

/// A session gone over once it is over (the settle way): which of its short
/// notes last, put together, kept as production keeps a fact. Returns the
/// lasting facts kept, with their evidence.
async fn settle_session(
    db: &sea_orm::DatabaseConnection,
    judge: &crate::services::analyzer::AiAnalyzer,
    user_id: i32,
    present: &Audience,
    turns: &[Turn],
    notes: &[String],
    at: chrono::DateTime<chrono::FixedOffset>,
) -> Vec<(String, String)> {
    // What she knows of them for good, newest first, as many as production
    // shows beside a stretch.
    let mut known = crate::services::agent::memory::unified::active_in(
        db,
        user_id,
        present,
        &crate::services::agent::memory::unified::MemoryKind::ABOUT_PERSON,
    )
    .await
    .unwrap_or_default();
    known.retain(|row| row.source == "chat");
    known.sort_by_key(|row| std::cmp::Reverse(row.created_at));
    let known: Vec<String> = known
        .into_iter()
        .take(SETTLE_KNOWN)
        .map(|row| super::super::ingest::compact_summary(&row.content))
        .collect();
    let theirs = turns
        .iter()
        .filter(|turn| turn.role == "user")
        .map(|turn| {
            turn.content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        })
        .collect::<Vec<_>>()
        .join("\n");
    let input = json!({
        "conversation": turns
            .iter()
            .map(|turn| json!({
                "who": if turn.role == "user" { "them" } else { "you" },
                "text": turn.content,
            }))
            .collect::<Vec<_>>(),
        "lately": notes,
        "today": at.with_timezone(&super::super::clock::zone()).format("%Y-%m-%d (%A)").to_string(),
    })
    .to_string();
    let Ok(raw) = judge
        .analyze_json(
            &myriad_merope::chat_remember::settle_system_prompt(&known),
            &input,
            myriad_merope::chat_remember::SETTLE_SCHEMA_NAME,
            Some(&myriad_merope::chat_remember::settle_schema()),
        )
        .await
    else {
        return Vec::new();
    };
    let Ok(answer) =
        serde_json::from_str::<Value>(myriad_merope::chat_remember::strip_json_fence(&raw))
    else {
        return Vec::new();
    };
    let replaces: Vec<String> = answer["replaces"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|old| old.as_str())
        .filter(|old| known.iter().any(|fact| fact == old))
        .map(str::to_string)
        .collect();
    let mut lasting = Vec::new();
    let mut updates = super::super::chat_remember::ChatMemoryUpdates::default();
    for item in answer["lasting"].as_array().into_iter().flatten() {
        let (Some(fact), Some(evidence)) = (item["fact"].as_str(), item["evidence"].as_str())
        else {
            continue;
        };
        let fact = super::super::ingest::compact_summary(fact);
        if fact.is_empty()
            || !myriad_merope::chat_remember::settle_evidence_holds(evidence, &theirs)
        {
            continue;
        }
        let concepts = serde_json::from_value(item["concepts"].clone()).unwrap_or_default();
        updates
            .updates
            .push(super::super::chat_remember::ChatMemoryUpdate {
                // When it would be worth remembering, as production keeps it.
                cues: item["whenFor"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|cue| cue.as_str().map(str::to_string))
                    .collect(),
                fact: Some(fact.clone()),
                // What it changes goes with the first: it says how it is now.
                supersedes: if updates.updates.is_empty() {
                    replaces.clone()
                } else {
                    Vec::new()
                },
                evidence: Some(evidence.to_string()),
                concepts: crate::services::agent::memory::unified::clean_concepts(concepts),
            });
        lasting.push((fact, evidence.to_string()));
    }
    if !updates.updates.is_empty() {
        apply_at(db, user_id, present, at, &updates).await;
    }
    lasting
}

/// What she knows of them for good, at most, shown beside a stretch (as
/// production's `chat_remember::settle`).
const SETTLE_KNOWN: usize = 40;

/// How long a short note is still there to recall, in the settle way: past
/// this, by the question's date, it has faded. A stand-in for a fast fading
/// memory strength, not a rule production has.
const SHORT_NOTES_LAST: chrono::Duration = chrono::Duration::days(3);

/// Every session of a question, the settle way: what is kept, and what of
/// it came from the turns that answer the question.
#[allow(clippy::too_many_arguments)]
pub(super) async fn go_over(
    db: &sea_orm::DatabaseConnection,
    judge: &crate::services::analyzer::AiAnalyzer,
    user_id: i32,
    sessions: &[super::Session<'_>],
    question_date: &str,
    answering: &mut Vec<String>,
    kept: &mut usize,
) {
    let asked_at = date_of(question_date);
    for (session_at, turns, present) in sessions {
        let (session_at, present) = (*session_at, present);
        let mut notes: Vec<String> = Vec::new();
        let mut short: Vec<(
            chrono::DateTime<chrono::FixedOffset>,
            super::super::chat_remember::ChatMemoryUpdates,
            bool,
        )> = Vec::new();
        for (index, turn) in turns.iter().enumerate() {
            if turn.role != "user" {
                continue;
            }
            let reply = turns
                .get(index + 1)
                .filter(|next| next.role == "assistant")
                .map(|next| next.content.as_str())
                .unwrap_or("");
            let at = session_at + chrono::Duration::minutes(index as i64);
            let text: String = turn
                .content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if text.chars().count() > 2_000 {
                continue;
            }
            let Some(updates) = note_turn(db, judge, user_id, present, &text, reply, at).await
            else {
                continue;
            };
            notes.extend(
                updates
                    .updates
                    .iter()
                    .filter_map(|update| update.fact.clone()),
            );
            let answer_here =
                turn.has_answer || turns.get(index + 1).is_some_and(|next| next.has_answer);
            short.push((at, updates, answer_here));
        }
        let over_at = session_at + chrono::Duration::minutes(turns.len() as i64);
        // What she said and what they suggested she try are kept as they are.
        for (at, updates, answer_here) in &short {
            let hers = super::super::chat_remember::ChatMemoryUpdates {
                updates: Vec::new(),
                said: updates.said.clone(),
                put_onto: updates.put_onto.clone(),
            };
            if (!hers.said.is_empty() || !hers.put_onto.is_empty())
                && apply_at(db, user_id, present, *at, &hers).await.is_some()
            {
                *kept += 1;
                if *answer_here {
                    answering.extend(
                        hers.said
                            .iter()
                            .map(|said| super::super::ingest::compact_summary(&said.said)),
                    );
                }
            }
        }
        if !notes.is_empty() {
            let answer_lines: String = turns
                .iter()
                .filter(|turn| turn.role == "user" && turn.has_answer)
                .map(|turn| {
                    turn.content
                        .split_whitespace()
                        .collect::<Vec<_>>()
                        .join(" ")
                })
                .collect::<Vec<_>>()
                .join("\n");
            for (fact, evidence) in
                settle_session(db, judge, user_id, present, turns, &notes, over_at).await
            {
                *kept += 1;
                if !answer_lines.is_empty()
                    && evidence
                        .split(" / ")
                        .any(|part| answer_lines.contains(part.trim()))
                {
                    answering.push(fact);
                }
            }
        }
        // Short notes still fresh when the question is asked.
        if asked_at - over_at <= SHORT_NOTES_LAST {
            for (at, updates, answer_here) in short {
                let facts = super::super::chat_remember::ChatMemoryUpdates {
                    updates: updates.updates.clone(),
                    said: Vec::new(),
                    put_onto: Vec::new(),
                };
                if !facts.updates.is_empty()
                    && apply_at(db, user_id, present, at, &facts).await.is_some()
                {
                    *kept += 1;
                    if answer_here {
                        answering.extend(
                            facts
                                .updates
                                .iter()
                                .filter_map(|update| update.fact.as_deref())
                                .map(super::super::ingest::compact_summary),
                        );
                    }
                }
            }
        }
    }
}
