//! Whether it is them typing (see `myriad_merope::style`): the last few
//! lines someone sent, against how they usually type. A friend's phone in
//! someone else's hands reads wrong within a few lines; she notices as a
//! friend would, and what she makes of it is hers. Nothing here decides who
//! may know what: that stays with pairing and who is present.
//!
//! Where they type is weighed on its own: lines in private against how they
//! type in private, lines in a group against how they type in that group.
//! Someone who fires off short bursts among friends and writes whole
//! sentences to her alone is not someone else in either place.

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

/// Per person and place (`None` in private, else the group).
static SEEN: LazyLock<Mutex<HashMap<(i32, Option<String>), (Instant, Option<f64>)>>> =
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

/// How unlike their usual way of typing in this place (`venue`: none in
/// private, else the group) their last few lines there are, in their own
/// deviations; none when it cannot be told, as when they have typed too
/// little there.
pub async fn unlike_them(
    db: &DatabaseConnection,
    user_id: i32,
    venue: Option<&str>,
) -> Option<f64> {
    if user_id <= 0 {
        return None;
    }
    let key = (user_id, venue.map(str::to_string));
    if let Some((_, seen)) = SEEN
        .lock()
        .ok()?
        .get(&key)
        .filter(|(at, _)| at.elapsed() < FRESH_FOR)
    {
        return *seen;
    }
    let now = crate::clock::now();
    let lines = super::store::their_lines_in(db, user_id, venue, LINES)
        .await
        .ok()?;
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
        all.insert(key, (Instant::now(), seen));
    }
    seen
}

/// The section she reads when their last lines here do not read like them.
pub async fn section(db: &DatabaseConnection, user_id: i32, venue: Option<&str>) -> Option<String> {
    unlike_them(db, user_id, venue)
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
            as_typed("（回复你之前说的「好」）哈哈哈").as_deref(),
            Some("哈哈哈")
        );
        assert_eq!(as_typed("[表情：歪头]"), None);
        assert_eq!(as_typed("  好的  ").as_deref(), Some("好的"));
    }

    /// Someone who writes whole sentences to her alone and fires off short
    /// bursts in a group reads like themselves in the group, though the
    /// same bursts are far from how they write to her in private. On a real
    /// database (an isolated schema; skipped without a test database).
    #[tokio::test]
    async fn lines_in_a_group_are_weighed_against_that_group() {
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let isolated = crate::db::IsolatedSchema::migrated(&url, "likeness_by_place").await;
        let db = isolated.db.clone();
        let user_id: i32 = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "INSERT INTO users (username, display_name) VALUES ('bursts', 'bursts') RETURNING id"
                    .to_string(),
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "id")
            .unwrap();
        let group = "telegram:-1007788";
        let said = |venue: Option<&str>, minutes_ago: i64, text: String| {
            let db = db.clone();
            let context = match venue {
                Some(venue) => serde_json::json!({ "mode": "chat", "venue": venue }),
                None => serde_json::json!({ "mode": "chat" }),
            };
            async move {
                let at =
                    (crate::clock::now() - chrono::Duration::minutes(minutes_ago)).fixed_offset();
                let session = uuid::Uuid::new_v4().to_string();
                db.execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "INSERT INTO agent_sessions (id, user_id, title, context, message_count, \
                     archived, created_at, last_active_at) VALUES ($1, $2, 't', $3, 1, false, $4, $4)",
                    [
                        session.clone().into(),
                        user_id.into(),
                        context.into(),
                        at.into(),
                    ],
                ))
                .await
                .unwrap();
                db.execute_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "INSERT INTO agent_messages (session_id, role, content, created_at) \
                     VALUES ($1, 'user', $2, $3)",
                    [session.into(), text.into(), at.into()],
                ))
                .await
                .unwrap();
            }
        };
        let topics = [
            "工作",
            "周末",
            "那本书",
            "做饭",
            "跑步",
            "搬家",
            "考试",
            "天气",
        ];
        let bursts = [
            "哈哈哈哈",
            "笑死",
            "草",
            "绝了",
            "6",
            "？？",
            "好家伙",
            "离谱",
        ];
        for n in 0..120_usize {
            let topic = topics[n % topics.len()];
            let private = format!(
                "我仔细想了一下{topic}的事情，觉得还是应该慢慢来。第{n}次说这个了，你别嫌我啰嗦。"
            );
            said(None, 3000 + n as i64, private).await;
            let burst = bursts[n % bursts.len()];
            said(
                Some(group),
                3000 + n as i64,
                format!("{burst}{burst} {topic}"),
            )
            .await;
        }
        let mut recent = Vec::new();
        for n in 0..6_usize {
            let burst = bursts[(n * 3) % bursts.len()];
            let line = format!("{burst}{burst} {}", topics[(n + 2) % topics.len()]);
            said(Some(group), 5 - n as i64, line.clone()).await;
            recent.push(line);
        }
        let in_group = unlike_them(&db, user_id, Some(group)).await;
        assert!(
            in_group.is_some_and(|far| far < NOTICED_AT),
            "like themselves in the group: {in_group:?}"
        );
        let private: Vec<String> = super::super::store::their_lines_in(&db, user_id, None, LINES)
            .await
            .unwrap()
            .into_iter()
            .map(|(_, line)| line)
            .collect();
        assert_eq!(private.len(), 120, "only what they wrote her in private");
        let against_private = unlike(&private, &recent);
        assert!(
            against_private.is_some_and(|far| far >= NOTICED_AT),
            "the same bursts against how they write in private: {against_private:?}"
        );
        // Nothing new in private: nothing to tell there.
        assert_eq!(unlike_them(&db, user_id, None).await, None);
        isolated.drop().await;
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
