//! The raw-chat arm (`MEROPE_MEMORY_BENCH_RAW_ARM=<n>`): each question also
//! answered the wide way with the raw private chat searched every time, by
//! meaning, whatever the cues said: the `n` messages nearest what was asked,
//! whole, beside what the look back found, from the same memories. Only a
//! bench arm: production scrolls back only when the words or cues reach
//! back.
//!
//! Unlike the look back, the latest talk is not left out: the bench shows
//! none of it in front of her.

use myriad_merope::remembering::Who;

use super::*;

tokio::task_local! {
    /// Inside the raw arm's answer: how many messages to bring.
    pub(super) static RAW: usize;
}

/// The arm asked for: how many raw messages each answer brings.
pub(super) fn raw_arm() -> Option<usize> {
    std::env::var("MEROPE_MEMORY_BENCH_RAW_ARM")
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .filter(|found| *found > 0)
}

/// Inside the raw arm's answer, how many raw messages it brings; outside
/// it, none.
pub(super) fn raw_way() -> Option<usize> {
    RAW.try_with(|found| *found).ok()
}

/// At most this many texts to one embedding call, and about this many
/// characters.
const BATCH_TEXTS: usize = 48;
const BATCH_CHARS: usize = 60_000;

/// `found` with the `wanted` private messages nearest `asked` by meaning
/// added, each once, oldest first. Nothing is added when what was asked or
/// the chat cannot be embedded.
pub(super) async fn with_raw(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    asked: &str,
    wanted: usize,
    found: Vec<(String, Who, String)>,
) -> Vec<(String, Who, String)> {
    let chat = private_chat(db, user_id, asked).await;
    let nearest = nearest(asked, &chat, wanted).await;
    merged(
        found,
        nearest
            .into_iter()
            .map(|index| {
                let (at, hers, content) = &chat[index];
                let who = if *hers { Who::You } else { Who::They };
                (at.clone(), who, content.clone())
            })
            .collect(),
    )
}

/// Their private chat, oldest first, as `(day, hers, text)`; what they
/// just said is not in it.
async fn private_chat(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    asked: &str,
) -> Vec<(String, bool, String)> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.role, m.content, m.created_at FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND (s.context->>'venue') IS NULL \
               AND m.role IN ('user', 'assistant') AND (m.metadata->>'piece') IS NULL \
             ORDER BY m.created_at, m.id",
            [user_id.into()],
        ))
        .await
        .unwrap_or_default();
    rows.iter()
        .filter_map(|row| {
            let role = row.try_get::<String>("", "role").ok()?;
            let content = row
                .try_get::<String>("", "content")
                .ok()
                .filter(|content| !content.trim().is_empty() && content.trim() != asked.trim())?;
            let at = row
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .ok()?;
            Some((
                at.format("%Y-%m-%d").to_string(),
                role == "assistant",
                content,
            ))
        })
        .collect()
}

/// The `wanted` messages of `chat` nearest `asked`, by index; none when
/// what was asked could not be embedded. A message whose batch could not
/// be embedded is left out.
async fn nearest(asked: &str, chat: &[(String, bool, String)], wanted: usize) -> Vec<usize> {
    if chat.is_empty() {
        return Vec::new();
    }
    let Some(embedder) = crate::services::ai::create_lite_embedding_analyzer_with_timeout(Some(
        Duration::from_secs(60),
    ))
    .await
    else {
        return Vec::new();
    };
    let texts: Vec<&str> = std::iter::once(asked)
        .chain(chat.iter().map(|(_, _, content)| content.trim()))
        .collect();
    let mut batches: Vec<Vec<usize>> = vec![Vec::new()];
    let mut size = 0;
    for (index, text) in texts.iter().enumerate() {
        let length = text.chars().count();
        let last = batches.last().expect("one batch at least");
        if !last.is_empty() && (last.len() >= BATCH_TEXTS || size + length > BATCH_CHARS) {
            batches.push(Vec::new());
            size = 0;
        }
        batches.last_mut().expect("one batch at least").push(index);
        size += length;
    }
    let (embedder, texts) = (&embedder, &texts);
    let embedded: Vec<(Vec<usize>, Option<Vec<Vec<f32>>>)> = futures::stream::iter(batches)
        .map(|batch| async move {
            let inputs: Vec<String> = batch
                .iter()
                .map(|index| texts[*index].to_string())
                .collect();
            let vectors = embedder
                .embed(&inputs)
                .await
                .ok()
                .filter(|vectors| vectors.len() == inputs.len());
            (batch, vectors)
        })
        .buffer_unordered(4)
        .collect()
        .await;
    let mut vectors: Vec<Option<Vec<f32>>> = vec![None; texts.len()];
    for (batch, got) in embedded {
        for (index, vector) in batch.into_iter().zip(got.into_iter().flatten()) {
            vectors[index] = Some(vector);
        }
    }
    let Some(query) = vectors[0].take() else {
        return Vec::new();
    };
    let mut scored: Vec<(usize, f64)> = vectors[1..]
        .iter()
        .enumerate()
        .filter_map(|(index, vector)| {
            vector.as_ref().map(|vector| {
                (
                    index,
                    crate::services::agent::memory::meaning::cosine(&query, vector),
                )
            })
        })
        .collect();
    scored.sort_by(|left, right| right.1.total_cmp(&left.1).then(left.0.cmp(&right.0)));
    scored
        .into_iter()
        .take(wanted)
        .map(|(index, _)| index)
        .collect()
}

/// `found`, with each of `more` added once, oldest first: a message the
/// look back already showed cut is shown whole in its place, one shown
/// whole is not shown again.
fn merged(
    found: Vec<(String, Who, String)>,
    more: Vec<(String, Who, String)>,
) -> Vec<(String, Who, String)> {
    // Letters and digits only: an excerpt joins lines with " / " and marks
    // what it cut with "…".
    let bare = |text: &str| -> String {
        text.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let mut all = found;
    for (date, who, text) in more {
        let whole = bare(&text);
        let shown = all.iter().position(|(was_on, was_who, was)| {
            *was_on == date && !matches!(was_who, Who::TheDay) && whole.contains(&bare(was))
        });
        match shown {
            Some(index) => all[index].2 = text,
            None => all.push((date, who, text)),
        }
    }
    all.sort_by(|left, right| left.0.cmp(&right.0));
    all
}

#[test]
fn a_raw_message_already_found_is_shown_once_and_whole() {
    let found = vec![
        (
            "2023-05-02".to_string(),
            Who::They,
            "which bakery did you say".to_string(),
        ),
        (
            "2023-05-01".to_string(),
            Who::You,
            "… the one on Elm / Street …".to_string(),
        ),
        (
            "2023-05-01".to_string(),
            Who::TheDay,
            "bakeries".to_string(),
        ),
    ];
    let more = vec![
        (
            "2023-05-01".to_string(),
            Who::You,
            "Try the one on Elm\nStreet, open till six".to_string(),
        ),
        (
            "2023-04-30".to_string(),
            Who::They,
            "I love sourdough".to_string(),
        ),
        (
            "2023-05-02".to_string(),
            Who::They,
            "which bakery did you say".to_string(),
        ),
    ];
    let all = merged(found, more);
    let texts: Vec<(&str, &str)> = all
        .iter()
        .map(|(day, _, text)| (day.as_str(), text.as_str()))
        .collect();
    assert_eq!(
        texts,
        [
            ("2023-04-30", "I love sourdough"),
            ("2023-05-01", "Try the one on Elm\nStreet, open till six"),
            ("2023-05-01", "bakeries"),
            ("2023-05-02", "which bakery did you say"),
        ]
    );
}

#[tokio::test]
async fn outside_the_raw_arm_nothing_raw_is_brought() {
    assert_eq!(raw_way(), None);
    assert_eq!(RAW.scope(5, async { raw_way() }).await, Some(5));
}
