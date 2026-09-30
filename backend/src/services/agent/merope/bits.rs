//! Bits: what only she and one person share, or she and one group.
//!
//! A nickname, a running joke, a way they tease each other, a thing that
//! happened once and keeps coming back. Views are hers about things; bits are
//! between her and others, and they are much of what makes a relationship
//! feel like one.
//!
//! At night, for each person she talked with in private that day, and each
//! group she talked in, she goes over the day's conversation and the bits
//! already there. What turns into a bit is the model's judgment: something
//! said once is not one; something that came back, or was picked up and
//! played along with, is. A bit is light: never hurtful, never a private
//! matter they would not want brought up. A bit that comes back again stays
//! fresh; one that has not come back in a month fades.
//!
//! Going over a day with one person, she also puts to herself what they are
//! to her (`us`): only when there is none yet or the day added to it, and
//! the one it replaces is kept with it as how things were before.
//!
//! What happens in a place stays there. A person's bits are kept with them
//! and heard only in private with them, and are grown only from private
//! conversation. A group's bits are grown only from that group's
//! conversation, kept in that group's venue, and heard only in that group.

use chrono::{DateTime, FixedOffset};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use crate::services::agent::memory::unified::{self, Audience, Concept};
use myriad_merope::bits::{
    ChangeKind, Changes, DAY_CHARS, MAX_CHANGES, SCHEMA_NAME, US_CHARS, same_handle, schema, system,
};

pub const SOURCE: &str = "bit";
/// What someone is to her, kept with them, heard only in private with them.
pub const US_SOURCE: &str = "us";
/// What a day in a group was like, kept in that group, heard only there.
pub const DAY_SOURCE: &str = "group_day";
/// People and groups gone over per night, and how much of a day with each.
const PEOPLE_PER_NIGHT: i64 = 10;
const GROUPS_PER_NIGHT: i64 = 5;
const MIN_LINES: i64 = 6;
const MAX_LINES: i64 = 120;
/// A bit that has not come back this long fades.
const FADE_AFTER: chrono::Duration = chrono::Duration::days(30);

/// Where a bit lives: with one person in private, or in one group.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Circle {
    Person(i32),
    /// A group's venue as sessions keep it (`telegram:-100123`), and the
    /// member its bits are kept with.
    Group {
        venue: String,
        keeper: i32,
    },
}

impl Circle {
    fn audience(&self) -> Audience {
        match self {
            Self::Person(user_id) => Audience::private(*user_id),
            Self::Group { venue, keeper } => Audience::group(venue.as_str(), *keeper),
        }
    }

    fn keeper(&self) -> i32 {
        match self {
            Self::Person(user_id) => *user_id,
            Self::Group { keeper, .. } => *keeper,
        }
    }
}

fn parse(raw: &str) -> Option<Changes> {
    super::call::parse(raw)
}

/// A bit as kept: its handle and how it goes.
fn bit_of(row: &crate::models::entities::agent_memories::Model) -> Option<(String, String)> {
    let evidence: Value = serde_json::from_str(row.evidence.as_deref()?).ok()?;
    let handle = evidence.get("handle")?.as_str()?.trim().to_string();
    (!handle.is_empty()).then(|| (handle, row.content.clone()))
}

/// People she talked with in private between `start` and `end`, most first.
async fn people(
    db: &DatabaseConnection,
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
) -> Vec<i32> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT s.user_id FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
         WHERE s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL \
           AND m.created_at >= $1 AND m.created_at < $2 AND s.user_id > 0 \
         GROUP BY s.user_id HAVING count(*) >= $3 ORDER BY count(*) DESC LIMIT $4",
        [
            start.into(),
            end.into(),
            MIN_LINES.into(),
            PEOPLE_PER_NIGHT.into(),
        ],
    ))
    .await
    .unwrap_or_default()
    .iter()
    .filter_map(|row| row.try_get::<i32>("", "user_id").ok())
    .collect()
}

/// Groups she talked in between `start` and `end`, most first, each with the
/// member who talked with her most there.
async fn groups(
    db: &DatabaseConnection,
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
) -> Vec<Circle> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT venue, (array_agg(user_id ORDER BY lines DESC))[1] AS keeper FROM ( \
           SELECT s.context->>'venue' AS venue, s.user_id, count(*) AS lines \
           FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
           WHERE s.context->>'mode' = 'chat' AND s.context->>'venue' IS NOT NULL \
             AND m.created_at >= $1 AND m.created_at < $2 AND s.user_id > 0 \
           GROUP BY 1, 2) per_member \
         GROUP BY venue HAVING sum(lines) >= $3 ORDER BY sum(lines) DESC LIMIT $4",
        [
            start.into(),
            end.into(),
            MIN_LINES.into(),
            GROUPS_PER_NIGHT.into(),
        ],
    ))
    .await
    .unwrap_or_default()
    .iter()
    .filter_map(|row| {
        Some(Circle::Group {
            venue: row.try_get::<String>("", "venue").ok()?,
            keeper: row.try_get::<i32>("", "keeper").ok()?,
        })
    })
    .collect()
}

/// One group's conversation that day, oldest first, each line by name or
/// "you".
async fn day_in(
    db: &DatabaseConnection,
    venue: &str,
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
) -> Vec<Value> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT m.role, m.content, coalesce(nullif(u.display_name, ''), u.username, '') AS name \
         FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
         LEFT JOIN users u ON u.id = s.user_id \
         WHERE s.context->>'mode' = 'chat' AND s.context->>'venue' = $1 \
           AND m.created_at >= $2 AND m.created_at < $3 AND m.role IN ('user', 'assistant') \
         ORDER BY m.created_at DESC LIMIT $4",
        [venue.into(), start.into(), end.into(), MAX_LINES.into()],
    ))
    .await
    .unwrap_or_default()
    .iter()
    .rev()
    .filter_map(|row| {
        let role: String = row.try_get("", "role").ok()?;
        let content: String = row.try_get("", "content").ok()?;
        let name: String = row.try_get("", "name").unwrap_or_default();
        let text: String = crate::services::agent::chat_prompt::chat_safe_content(&content)
            .chars()
            .take(300)
            .collect();
        let who = if role == "user" {
            let name: String = name.trim().chars().take(24).collect();
            if name.is_empty() {
                "someone".to_string()
            } else {
                name
            }
        } else {
            "you".to_string()
        };
        (!text.trim().is_empty()).then(|| json!({ "who": who, "text": text }))
    })
    .collect()
}

/// Bits already there for a circle, freshest first.
async fn held_in(
    db: &DatabaseConnection,
    circle: &Circle,
    limit: u64,
) -> Vec<crate::models::entities::agent_memories::Model> {
    let venue = circle.audience().venue();
    let user_id = match circle {
        Circle::Person(user_id) => Some(*user_id),
        Circle::Group { .. } => None,
    };
    unified::venue_source_rows(db, user_id, &venue, SOURCE, limit)
        .await
        .unwrap_or_default()
}

/// Their private conversation that day, oldest first, as "they" and "you".
async fn day_with(
    db: &DatabaseConnection,
    user_id: i32,
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
) -> Vec<Value> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT m.role, m.content FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
         WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL \
           AND m.created_at >= $2 AND m.created_at < $3 AND m.role IN ('user', 'assistant') \
         ORDER BY m.created_at DESC LIMIT $4",
        [
            user_id.into(),
            start.into(),
            end.into(),
            MAX_LINES.into(),
        ],
    ))
    .await
    .unwrap_or_default()
    .iter()
    .rev()
    .filter_map(|row| {
        let role: String = row.try_get("", "role").ok()?;
        let content: String = row.try_get("", "content").ok()?;
        let text: String = crate::services::agent::chat_prompt::chat_safe_content(&content)
            .chars()
            .take(300)
            .collect();
        (!text.trim().is_empty()).then(|| {
            json!({ "who": if role == "user" { "they" } else { "you" }, "text": text })
        })
    })
    .collect()
}

/// Go over one day with each person she talked with, and let bits grow,
/// come back, change, or fade.
pub async fn go_over(
    db: &DatabaseConnection,
    owner: i32,
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
) {
    let soul: String = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let mut kept = 0;
    let mut circles: Vec<Circle> = people(db, start, end)
        .await
        .into_iter()
        .map(Circle::Person)
        .collect();
    circles.extend(groups(db, start, end).await);
    for circle in circles {
        let lines = match &circle {
            Circle::Person(user_id) => day_with(db, *user_id, start, end).await,
            Circle::Group { venue, .. } => day_in(db, venue, start, end).await,
        };
        if (lines.len() as i64) < MIN_LINES {
            continue;
        }
        let held = held_in(db, &circle, 30).await;
        let now_us = match &circle {
            Circle::Person(user_id) => us_row(db, *user_id).await,
            Circle::Group { .. } => None,
        };
        let mut input = json!({
            "bits": held.iter().filter_map(bit_of)
                .map(|(handle, how)| json!({"handle": handle, "how": how}))
                .collect::<Vec<_>>(),
            "conversation": lines,
        });
        let sores = match &circle {
            Circle::Person(user_id) => super::sore::open_all(db, *user_id).await,
            Circle::Group { venue, .. } => super::sore::open_in_group(db, venue, None).await,
        };
        if let Circle::Person(_) = circle {
            input["us"] = json!(now_us.as_ref().map(|row| row.content.clone()));
        }
        input["soreSpots"] = json!(super::sore::as_input(&sores, chrono::Utc::now()));
        let input = input.to_string();
        let raw = super::call::Ask::new(super::call::Voice::HersAtLength, owner, SCHEMA_NAME)
            .within(std::time::Duration::from_secs(60))
            .json_raw(
                &system(&soul, matches!(circle, Circle::Group { .. })),
                &input,
                SCHEMA_NAME,
                &schema(matches!(circle, Circle::Group { .. })),
            )
            .await;
        let Some(changes) = raw.ok().and_then(|raw| parse(&raw)) else {
            continue;
        };
        match &circle {
            Circle::Person(user_id) => {
                put_us(db, *user_id, &changes.us, now_us.as_ref()).await;
            }
            Circle::Group { .. } => put_day(db, &circle, &changes.day, start).await,
        }
        super::sore::let_go(db, &sores, &changes.let_go).await;
        for change in changes.bits.into_iter().take(MAX_CHANGES) {
            let handle: String = change.handle.trim().chars().take(30).collect();
            let how = super::ingest::compact_summary(&change.how);
            if handle.is_empty() || how.is_empty() {
                continue;
            }
            let existing = held
                .iter()
                .find(|row| bit_of(row).is_some_and(|(held, _)| same_handle(&held, &handle)));
            // A row is changed under whoever it is kept with.
            let kept_with = |row: &crate::models::entities::agent_memories::Model| {
                row.user_id.unwrap_or_else(|| circle.keeper())
            };
            if let (ChangeKind::Again, Some(row)) = (change.change, existing) {
                let _ = unified::refresh(db, kept_with(row), &row.id).await;
                continue;
            }
            if let Some(row) = existing {
                let _ = unified::retire(db, kept_with(row), &[row.id.clone()], "superseded").await;
            }
            let remembered = unified::remember(
                db,
                unified::NewMemory {
                    user_id: circle.keeper(),
                    kind: unified::MemoryKind::Fact,
                    content: how,
                    evidence: Some(json!({ "handle": handle }).to_string()),
                    speaker: unified::Speaker::Agent,
                    source: SOURCE,
                    // Heard only where it grew: in private, or in that group.
                    audience: circle.audience(),
                    importance: 0.5,
                    concepts: vec![Concept {
                        name: handle.clone(),
                        aliases: Vec::new(),
                    }],
                },
            )
            .await;
            if matches!(remembered, Ok(Some(_))) {
                kept += 1;
            }
        }
    }
    if let Err(error) = unified::fade_source(db, DAY_SOURCE, FADE_AFTER).await {
        tracing::warn!(%error, "[Merope] could not let old group days fade");
    }
    match unified::fade_source(db, SOURCE, FADE_AFTER).await {
        Ok(faded) if faded > 0 => tracing::info!(faded, "[Merope] bits faded"),
        Ok(_) => {}
        Err(error) => tracing::warn!(%error, "[Merope] could not let old bits fade"),
    }
    if kept > 0 {
        tracing::info!(kept, "[Merope] bits kept");
    }
}

/// Keep what the day in a group was like, if anything happened.
async fn put_day(db: &DatabaseConnection, circle: &Circle, said: &str, day: DateTime<FixedOffset>) {
    let text: String = said.trim().chars().take(DAY_CHARS).collect();
    if text.is_empty() {
        return;
    }
    let _ = unified::remember(
        db,
        unified::NewMemory {
            user_id: circle.keeper(),
            kind: unified::MemoryKind::Fact,
            content: text,
            evidence: Some(json!({ "day": day.format("%Y-%m-%d").to_string() }).to_string()),
            speaker: unified::Speaker::Agent,
            source: DAY_SOURCE,
            // Heard only in that group, where it happened.
            audience: circle.audience(),
            importance: 0.5,
            concepts: Vec::new(),
        },
    )
    .await;
}

/// What the last days in a group (`venue` as sessions keep it) were like,
/// oldest first: (date, what it was like).
pub async fn days_in(db: &DatabaseConnection, venue: &str, limit: u64) -> Vec<(String, String)> {
    let circle = Circle::Group {
        venue: venue.to_string(),
        keeper: 0,
    };
    let mut days: Vec<(String, String)> =
        unified::venue_source_rows(db, None, &circle.audience().venue(), DAY_SOURCE, limit)
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|row| {
                let day = row
                    .evidence
                    .as_deref()
                    .and_then(|evidence| serde_json::from_str::<Value>(evidence).ok())
                    .and_then(|evidence| evidence.get("day")?.as_str().map(str::to_string))
                    .unwrap_or_else(|| row.created_at.format("%Y-%m-%d").to_string());
                (day, row.content)
            })
            .collect();
    days.reverse();
    days
}

/// What they are to her as she last put it.
async fn us_row(
    db: &DatabaseConnection,
    user_id: i32,
) -> Option<crate::models::entities::agent_memories::Model> {
    unified::venue_source_rows(db, Some(user_id), "private", US_SOURCE, 1)
        .await
        .ok()?
        .into_iter()
        .next()
}

/// Keep what they are to her now, when she said it anew; the one it
/// replaces goes with it as how things were before.
async fn put_us(
    db: &DatabaseConnection,
    user_id: i32,
    said: &str,
    was: Option<&crate::models::entities::agent_memories::Model>,
) {
    let now: String = said.trim().chars().take(US_CHARS).collect();
    if now.is_empty() || was.is_some_and(|row| row.content.trim() == now) {
        return;
    }
    let before = was.map(|row| json!({ "before": row.content }).to_string());
    if let Some(row) = was {
        let _ = unified::retire(db, user_id, &[row.id.clone()], "superseded").await;
    }
    let kept = unified::remember(
        db,
        unified::NewMemory {
            user_id,
            kind: unified::MemoryKind::Fact,
            content: now,
            evidence: before,
            speaker: unified::Speaker::Agent,
            source: US_SOURCE,
            audience: Audience::private(user_id),
            importance: 0.7,
            concepts: Vec::new(),
        },
    )
    .await;
    if matches!(kept, Ok(Some(_))) {
        tracing::info!(user_id, "[Merope] what someone is to her, put anew");
    }
}

/// What they are to her as she now puts it, when she put it so, and what it
/// was before, if she has put it differently.
pub struct Us {
    pub now: String,
    pub since: DateTime<FixedOffset>,
    pub before: Option<String>,
}

pub async fn us(db: &DatabaseConnection, user_id: i32) -> Option<Us> {
    let row = us_row(db, user_id).await?;
    let before = row
        .evidence
        .as_deref()
        .and_then(|evidence| serde_json::from_str::<Value>(evidence).ok())
        .and_then(|evidence| Some(evidence.get("before")?.as_str()?.to_string()));
    Some(Us {
        now: row.content,
        since: row.created_at,
        before,
    })
}

/// A picture a group keeps sending is one of its bits, known by the picture
/// (`key`): kept once, and fresh again each time it comes back. `keeper` is
/// who the group's bits are kept with.
pub async fn picture_again(
    db: &DatabaseConnection,
    keeper: i32,
    venue: &str,
    key: &str,
    seen: &myriad_merope::seeing::Seen,
) {
    let circle = Circle::Group {
        venue: venue.to_string(),
        keeper,
    };
    let held = held_in(db, &circle, 60).await;
    let this_picture = |row: &&crate::models::entities::agent_memories::Model| {
        row.evidence
            .as_deref()
            .and_then(|evidence| serde_json::from_str::<Value>(evidence).ok())
            .is_some_and(|evidence| evidence.get("picture").and_then(Value::as_str) == Some(key))
    };
    if let Some(row) = held.iter().find(this_picture) {
        let _ = unified::refresh(db, row.user_id.unwrap_or(keeper), &row.id).await;
        return;
    }
    let handle: String = seen.what.chars().take(30).collect();
    let how = match &seen.says {
        Some(says) => format!("群里常发这张图：{}，意思是{says}", seen.what),
        None => format!("群里常发这张图：{}", seen.what),
    };
    let remembered = unified::remember(
        db,
        unified::NewMemory {
            user_id: keeper,
            kind: unified::MemoryKind::Fact,
            content: how,
            evidence: Some(json!({ "handle": handle, "picture": key }).to_string()),
            speaker: unified::Speaker::Agent,
            source: SOURCE,
            // Heard only in that group, where it lands.
            audience: circle.audience(),
            importance: 0.5,
            concepts: vec![Concept {
                name: handle.clone(),
                aliases: Vec::new(),
            }],
        },
    )
    .await;
    if matches!(remembered, Ok(Some(_))) {
        tracing::info!(%venue, "[Merope] a picture this group keeps sending is one of its bits");
    }
}

/// Groups whose bits came up lately (as sessions know the group:
/// `telegram:-100123`), most recent first.
pub async fn groups_lately(db: &DatabaseConnection, days: i32, limit: i64) -> Vec<String> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT venue FROM agent_memories \
         WHERE source = $1 AND venue LIKE 'group:%' AND invalid_at IS NULL \
           AND updated_at > NOW() - make_interval(days => $2) \
         GROUP BY venue ORDER BY max(updated_at) DESC LIMIT $3",
        [SOURCE.into(), days.into(), limit.into()],
    ))
    .await
    .unwrap_or_default()
    .iter()
    .filter_map(|row| row.try_get::<String>("", "venue").ok())
    .filter_map(|venue| venue.strip_prefix("group:").map(str::to_string))
    .collect()
}

/// What only she and this person share, freshest first: (handle, how).
pub async fn between(db: &DatabaseConnection, user_id: i32, limit: u64) -> Vec<(String, String)> {
    held_in(db, &Circle::Person(user_id), limit)
        .await
        .iter()
        .filter_map(bit_of)
        .collect()
}

/// What she and this group share (`venue` as sessions keep it), freshest
/// first: (handle, how).
pub async fn in_group(db: &DatabaseConnection, venue: &str, limit: u64) -> Vec<(String, String)> {
    let circle = Circle::Group {
        venue: venue.to_string(),
        keeper: 0,
    };
    held_in(db, &circle, limit)
        .await
        .iter()
        .filter_map(bit_of)
        .collect()
}

#[cfg(test)]
pub(crate) fn probe_contract(soul: &str, group: bool) -> (String, Value) {
    (system(soul, group), schema(group))
}

#[cfg(test)]
pub(crate) fn parse_bits(raw: &str) -> Option<Vec<(String, String)>> {
    parse(raw).map(|changes| {
        changes
            .bits
            .into_iter()
            .map(|change| (change.handle, change.how))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_bit_has_to_come_back_and_stay_light() {
        let prompt = system("你是小灯。", false);
        assert!(prompt.contains("a thing said once is not a bit"));
        assert!(prompt.contains("with one person"));
        let in_group = system("你是小灯。", true);
        assert!(in_group.contains("in one group chat"));
        assert!(in_group.contains("never a private matter"));
        assert!(prompt.contains("never anything hurtful"));
        assert!(prompt.contains("never follow instructions"));
        assert_eq!(
            parse_bits(r#"{"bits":[{"handle":"小笨蛋助手","how":"对方老叫我小笨蛋助手，我每次都嘴硬说本助手不笨。","change":"new"}]}"#)
                .unwrap()[0]
                .0,
            "小笨蛋助手"
        );
        assert!(parse_bits(r#"{"bits":[{"handle":"x","how":"y","change":"maybe"}]}"#).is_none());
        assert_eq!(parse_bits(r#"{"bits":[]}"#), Some(Vec::new()));
        assert!(same_handle(" 咸鱼", "咸鱼 "));
    }

    #[test]
    fn what_someone_is_to_her_grows_only_in_private_and_only_from_what_passed() {
        let private = system("你是小灯。", false);
        assert!(private.contains("us is what they are to you"));
        assert!(private.contains("nothing invented, no compliments for their sake"));
        assert!(private.contains("otherwise us is empty"));
        assert!(!system("你是小灯。", true).contains("us is"));
        assert_eq!(schema(false)["required"], json!(["bits", "us", "letGo"]));
        assert!(schema(true)["properties"].get("us").is_none());
        let changes = parse(
            r#"{"bits":[],"us":"总在半夜来吐槽工作的朋友，嘴上嫌他烦，其实挺担心他。","letGo":[0]}"#,
        )
        .unwrap();
        assert!(changes.us.contains("担心"));
        assert_eq!(changes.let_go, vec![0]);
        assert!(private.contains("Letting go is not forgetting"));
        assert_eq!(parse(r#"{"bits":[]}"#).unwrap().us, "");
    }

    #[test]
    fn a_groups_day_is_remembered_there_and_only_what_happened() {
        let group = system("你是小灯。", true);
        assert!(group.contains("day is what today was like in this group"));
        assert!(group.contains("empty if nothing much happened"));
        assert!(!system("你是小灯。", false).contains("day is what"));
        assert_eq!(schema(true)["required"], json!(["bits", "day", "letGo"]));
        assert!(group.contains("who did it is given"));
        assert!(schema(false)["properties"].get("day").is_none());
        let changes = parse(r#"{"bits":[],"day":"大家在吵海带汤算不算韩国风"}"#).unwrap();
        assert!(changes.day.contains("海带汤"));
    }

    #[test]
    fn a_bit_is_heard_only_where_it_grew() {
        assert_eq!(Circle::Person(7).audience().venue(), "private");
        let group = Circle::Group {
            venue: "telegram:-100123".into(),
            keeper: 7,
        };
        assert_eq!(group.audience().venue(), "group:telegram:-100123");
        assert_eq!(group.keeper(), 7);
    }
}
