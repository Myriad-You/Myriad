//! The past sessions of a question written as production writes them: who
//! they are, what was said and when, and the turn kept as a chat keeps it.

use super::*;

pub(super) async fn new_user(db: &sea_orm::DatabaseConnection, name: &str) -> i32 {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO users (username) VALUES ($1) RETURNING id",
        [name.into()],
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", "id")
    .unwrap()
}

/// This turn is the person's latest message, as the chat that carried it
/// would have recorded.
pub(super) async fn said_at(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    at: chrono::DateTime<chrono::FixedOffset>,
) {
    let state = super::super::store::get_or_create_state(db, user_id)
        .await
        .unwrap();
    let mut state: agent_addressee_state::ActiveModel = state.into();
    state.last_user_message_at = Set(Some(at));
    state.update(db).await.unwrap();
}

/// One turn of theirs written down as production writes it: what she
/// already knows, whether it is worth writing, the extraction on the judgment
/// model, then the same transaction, dated as the turn was. The updates
/// applied, if any.
pub(in super::super) async fn remember_turn(
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
            &myriad_merope::chat_remember::extract_system_prompt(
                &existing,
                myriad_merope::chat_remember::Examples::for_words(text),
            ),
            &input,
            myriad_merope::chat_remember::EXTRACT_SCHEMA_NAME,
            Some(&myriad_merope::chat_remember::extract_schema()),
        )
        .await
        .ok()?;
    let updates =
        super::super::chat_remember::parse_chat_memory_updates(&raw, text, reply, &existing)?;
    apply_at(db, user_id, present, at, &updates).await?;
    Some(updates)
}

/// Keep `updates` as production does, dated `at`; none when nothing was kept.
pub(super) async fn apply_at(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    present: &Audience,
    at: chrono::DateTime<chrono::FixedOffset>,
    updates: &super::super::chat_remember::ChatMemoryUpdates,
) -> Option<()> {
    said_at(db, user_id, at).await;
    let before = crate::clock::now().fixed_offset();
    let transaction = sea_orm::TransactionTrait::begin(db).await.ok()?;
    // Written as kept for good: the bench compares ways of going over talk,
    // with the short notes' fading stood in for by SHORT_NOTES_LAST.
    let applied = super::super::store::apply_chat_memory_updates_on(
        &transaction,
        user_id,
        at,
        updates,
        present,
        super::super::store::Keep::Lasting,
    )
    .await
    .unwrap_or(false);
    transaction.commit().await.ok()?;
    if !applied {
        return None;
    }
    // Dated as the turn was, not as the bench ran.
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET created_at = $1, valid_from = $1, updated_at = $1 \
         WHERE user_id = $2 AND created_at >= $3",
        [at.into(), user_id.into(), before.into()],
    ))
    .await
    .ok()?;
    Some(())
}

/// A session's turns kept as the site keeps a chat, a minute apart, for
/// scrolling back through.
pub(super) async fn keep_chat(
    db: &sea_orm::DatabaseConnection,
    session: &str,
    user_id: i32,
    at: chrono::DateTime<chrono::FixedOffset>,
    turns: &[Turn],
    here: &Audience,
) {
    let context = here
        .group_id()
        .map(|group| json!({"mode": "chat", "venue": group}));
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_sessions (id, user_id, message_count, archived, created_at, last_active_at, context) \
         VALUES ($1, $2, $3, false, $4, $4, $5)",
        [
            session.into(),
            user_id.into(),
            (turns.len() as i32).into(),
            at.into(),
            context.into(),
        ],
    ))
    .await
    .unwrap();
    for (index, turn) in turns.iter().enumerate() {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_messages (session_id, role, content, created_at) VALUES ($1, $2, $3, $4)",
            [
                session.into(),
                turn.role.clone().into(),
                turn.content.clone().into(),
                (at + chrono::Duration::minutes(index as i64)).into(),
            ],
        ))
        .await
        .unwrap();
    }
}

/// What writing a question's own sessions left: what of it answers the
/// question, and how much was kept.
#[derive(Clone, Debug, Default)]
pub(super) struct Written {
    pub(super) answering: Vec<String>,
    pub(super) kept: usize,
}

/// The writing half of [`run_for`]: the question's own sessions kept as
/// the site keeps chat and written as she would, after `answering`.
pub(super) async fn write_for(
    db: &sea_orm::DatabaseConnection,
    judge: &crate::services::analyzer::AiAnalyzer,
    question: &Question,
    name: &str,
    (user_id, tag): (i32, &str),
    mut answering: Vec<String>,
) -> Written {
    let mut kept = 0;
    let mut sessions: Vec<Session> = question
        .haystack_dates
        .iter()
        .map(|date| date_of(date))
        .zip(question.haystack_sessions.iter())
        .enumerate()
        .map(|(number, (at, turns))| (at, turns, audience_at(question.venue(number), user_id, tag)))
        .collect();
    sessions.sort_by_key(|(at, _, _)| *at);
    // The chat itself, as the site keeps it, for scrolling back through.
    for (number, (session_at, turns, here)) in sessions.iter().enumerate() {
        keep_chat(
            db,
            &format!("{name}-{number}"),
            user_id,
            *session_at,
            turns,
            here,
        )
        .await;
    }
    // What she already keeps of them, a week before they ask.
    if !question.memories.is_empty() {
        let updates = super::super::chat_remember::ChatMemoryUpdates {
            updates: question
                .memories
                .iter()
                .map(|fact| super::super::chat_remember::ChatMemoryUpdate {
                    standing: Default::default(),
                    fact: Some(fact.clone()),
                    supersedes: Vec::new(),
                    evidence: Some(fact.clone()),
                    concepts: Vec::new(),
                    topic: None,
                    cues: Vec::new(),
                })
                .collect(),
            ..Default::default()
        };
        let at = date_of(&question.question_date) - chrono::Duration::days(7);
        if apply_at(db, user_id, &Audience::private(user_id), at, &updates)
            .await
            .is_some()
        {
            kept += question.memories.len();
        }
    }
    let settle = std::env::var("MEROPE_MEMORY_BENCH_WAY").as_deref() == Ok("settle");
    let mut trail = super::note_trail::Trail::new();
    if settle {
        settle::go_over(
            db,
            judge,
            user_id,
            &sessions,
            &question.question_date,
            &mut answering,
            &mut kept,
            &mut trail,
        )
        .await;
    }
    for (session_at, turns, here) in sessions.iter().filter(|_| !settle) {
        for (index, turn) in turns.iter().enumerate() {
            if turn.role != "user" {
                continue;
            }
            let reply = turns
                .get(index + 1)
                .filter(|next| next.role == "assistant")
                .map(|next| next.content.as_str())
                .unwrap_or("");
            let at = *session_at + chrono::Duration::minutes(index as i64);
            let text: String = turn
                .content
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if text.chars().count() > 2_000 {
                continue;
            }
            let Some(updates) = remember_turn(db, judge, user_id, here, &text, reply, at).await
            else {
                continue;
            };
            kept += 1;
            // What answers the question may be in their words or hers.
            let answer_here =
                turn.has_answer || turns.get(index + 1).is_some_and(|next| next.has_answer);
            if answer_here {
                answering.extend(
                    updates
                        .updates
                        .iter()
                        .filter_map(|update| update.fact.as_deref())
                        .chain(updates.said.iter().map(|said| said.said.as_str()))
                        .map(super::super::ingest::compact_summary),
                );
            }
        }
    }
    if settle {
        settle::keep_up_nodes(db, user_id).await;
    }
    trail.finish(db, user_id, &question.question_id).await;
    Written { answering, kept }
}
