//! Whether it is them typing (see `myriad_merope::style`): the last few
//! lines someone sent, against how they usually type. A friend's phone in
//! someone else's hands reads wrong within a few lines; she notices as a
//! friend would, and what she makes of it is hers. Nothing here decides who
//! may know what: that stays with pairing and who is present.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use chrono::Utc;
use sea_orm::DatabaseConnection;

use myriad_merope::style::{NOTICED_AT, unlike};

/// Their lines looked at: the newest this many.
const LINES: i64 = 1500;
/// Lines within this long are the ones now being written.
const NOW: chrono::Duration = chrono::Duration::minutes(15);
/// Lines older than this are how they usually type.
const USUAL: chrono::Duration = chrono::Duration::hours(1);
/// The newest lines weighed at once.
const RECENT: usize = 6;
/// Looked at again no sooner than this, per person.
const FRESH_FOR: Duration = Duration::from_secs(60);

static SEEN: LazyLock<Mutex<HashMap<i32, (Instant, Option<f64>)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// What a line says as typed: without the quoted line it replies to, and
/// nothing for a picture or sticker alone.
fn as_typed(content: &str) -> Option<String> {
    let text = content.trim();
    let text = match text.strip_prefix('（') {
        Some(rest) if rest.starts_with("回复") => {
            rest.split_once('）').map_or(text, |(_, after)| after)
        }
        _ => text,
    }
    .trim();
    (!text.is_empty() && !(text.starts_with('[') && text.ends_with(']'))).then(|| text.to_string())
}

/// How unlike their usual way of typing their last few lines are, in their
/// own deviations; none when it cannot be told.
pub async fn unlike_them(db: &DatabaseConnection, user_id: i32) -> Option<f64> {
    if user_id <= 0 {
        return None;
    }
    if let Some((_, seen)) = SEEN
        .lock()
        .ok()?
        .get(&user_id)
        .filter(|(at, _)| at.elapsed() < FRESH_FOR)
    {
        return *seen;
    }
    let now = Utc::now();
    let lines = super::store::their_lines(db, user_id, LINES).await.ok()?;
    let recent: Vec<String> = lines
        .iter()
        .take_while(|(at, _)| now - at.with_timezone(&Utc) < NOW)
        .filter_map(|(_, content)| as_typed(content))
        .take(RECENT)
        .collect();
    let history: Vec<String> = lines
        .iter()
        .filter(|(at, _)| now - at.with_timezone(&Utc) >= USUAL)
        .filter_map(|(_, content)| as_typed(content))
        .collect();
    let seen = unlike(&history, &recent);
    if let Ok(mut all) = SEEN.lock() {
        if all.len() > 4096 {
            all.clear();
        }
        all.insert(user_id, (Instant::now(), seen));
    }
    seen
}

/// The section she reads when their last lines do not read like them.
pub async fn section(db: &DatabaseConnection, user_id: i32) -> Option<String> {
    unlike_them(db, user_id)
        .await
        .filter(|far| *far >= NOTICED_AT)
        .map(|_| myriad_merope::style::NOTICED.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_they_typed_is_the_line_itself() {
        assert_eq!(
            as_typed("（回复你说的：好）哈哈哈").as_deref(),
            Some("哈哈哈")
        );
        assert_eq!(as_typed("[表情：歪头]"), None);
        assert_eq!(as_typed("  好的  ").as_deref(), Some("好的"));
    }
}

/// On the site's own messages, over a read-only connection: how far windows
/// of someone's own held-out lines stray from them, and how far windows of
/// her own lines (someone else typing) do.
/// `cargo test … likeness_on_the_site -- --ignored --nocapture`, optional
/// `MEROPE_LIKENESS_USER` (default 1).
#[cfg(test)]
mod live {
    use super::*;

    #[tokio::test]
    #[ignore = "reads the site's database"]
    async fn likeness_on_the_site() {
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
        let db = crate::services::agent::semantic_eval::load_configured_lite().await;
        let user_id: i32 = std::env::var("MEROPE_LIKENESS_USER")
            .ok()
            .and_then(|id| id.parse().ok())
            .unwrap_or(1);
        let theirs: Vec<String> = super::super::store::their_lines(&db, user_id, 3000)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|(_, content)| as_typed(&content))
            .collect();
        let hers: Vec<String> = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT m.content FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
                 WHERE m.role = 'assistant' AND s.context->>'mode' = 'chat' \
                 ORDER BY m.created_at DESC LIMIT 600"
                    .to_string(),
            ))
            .await
            .unwrap()
            .iter()
            .filter_map(|row| row.try_get::<String>("", "content").ok())
            .flat_map(|content| content.lines().map(str::to_string).collect::<Vec<_>>())
            .filter_map(|line| as_typed(&line))
            .collect();
        // Newest fifth held out as their own new lines; the rest is usual.
        let held = theirs.len() / 5;
        let (new, usual) = theirs.split_at(held);
        for window in [3, 4, 6] {
            let score = |lines: &[String]| -> Vec<f64> {
                lines
                    .chunks(window)
                    .filter_map(|chunk| unlike(usual, chunk))
                    .collect()
            };
            let own = score(new);
            let other = score(&hers);
            let over = |scores: &[f64]| {
                scores.iter().filter(|z| **z >= NOTICED_AT).count() as f64
                    / scores.len().max(1) as f64
            };
            let quantiles = |scores: &[f64]| {
                let mut sorted = scores.to_vec();
                sorted.sort_by(f64::total_cmp);
                [0.1, 0.5, 0.9].map(|q| {
                    sorted
                        .get(((sorted.len() as f64 - 1.0) * q) as usize)
                        .copied()
                        .unwrap_or(f64::NAN)
                })
            };
            println!(
                "  own z p10/p50/p90 {:?}; hers {:?}",
                quantiles(&own),
                quantiles(&other)
            );
            println!(
                "window {window}: their own new lines noticed {:.0}% of {} windows; her lines noticed {:.0}% of {}",
                over(&own) * 100.0,
                own.len(),
                over(&other) * 100.0,
                other.len()
            );
        }
        println!(
            "their lines {}, usual {}, hers {}",
            theirs.len(),
            usual.len(),
            hers.len()
        );
    }
}
