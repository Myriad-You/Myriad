//! A group's days: what each day there was like, as the group itself heard it.

use super::*;

/// What she makes of a stretch of a group's talk while it is still at hand
/// (the lines themselves are kept only a few hours), in one reading (see
/// `myriad_merope::group_reading`): what today there is like so far, the
/// group's bits, how she comes across there, stings let go, and what she
/// heard about the world. `said` is (when, who, what, hers), oldest first;
/// `keeper` is who the group's rows are kept with (the site's owner, who
/// brought her there) and who the reading is billed to.
pub async fn go_over_stretch(
    db: &DatabaseConnection,
    keeper: i32,
    venue: &str,
    said: Vec<(DateTime<FixedOffset>, String, String, bool)>,
) {
    let said: Vec<_> = said
        .into_iter()
        .filter(|(_, _, text, _)| !text.trim().is_empty())
        .collect();
    if (said.len() as i64) < MIN_LINES || said.iter().all(|(_, _, _, hers)| *hers) {
        return;
    }
    let lines = reading_lines(&said);
    let circle = Circle::Group {
        venue: venue.to_string(),
        keeper,
    };
    let today = super::super::clock::local_now()
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|midnight| {
            chrono::TimeZone::from_local_datetime(&super::super::clock::zone(), &midnight)
                .earliest()
        })
        .map(|midnight| midnight.fixed_offset());
    let Some(today) = today else {
        return;
    };
    let before = day_line(db, &circle, today).await;
    read_group(db, &circle, venue, &lines, today, before.as_deref()).await;
}

/// The stretch as the reading takes it: who (bounded), what (bounded), and
/// on her lines how many seconds until someone else wrote that day.
fn reading_lines(
    said: &[(DateTime<FixedOffset>, String, String, bool)],
) -> Vec<myriad_merope::group_reading::Line> {
    said.iter()
        .enumerate()
        .map(|(index, (at, name, text, hers))| {
            let after = hers
                .then(|| {
                    said[index + 1..]
                        .iter()
                        .find(|(_, _, _, later_hers)| !later_hers)
                        .map(|(later, ..)| (*later - *at).num_seconds().max(0))
                })
                .flatten();
            let who: String = name.trim().chars().take(24).collect();
            myriad_merope::group_reading::Line {
                who: if who.is_empty() {
                    "someone".to_string()
                } else {
                    who
                },
                text: text.chars().take(300).collect(),
                hers: *hers,
                after,
            }
        })
        .collect()
}

/// Read the stretch once and keep what holds up of it.
async fn read_group(
    db: &DatabaseConnection,
    circle: &Circle,
    venue: &str,
    lines: &[myriad_merope::group_reading::Line],
    today: DateTime<FixedOffset>,
    day_before: Option<&str>,
) {
    use myriad_merope::group_reading::{AtHand, SCHEMA_NAME, input, parse, schema, system};
    let held = held_in(db, circle, 30).await;
    let wants: Vec<String> = super::super::wants::open(db)
        .await
        .into_iter()
        .map(|want| want.want)
        .collect();
    let sores = super::super::sore::open_in_group(db, venue, None).await;
    let lands_now = lands_row(db, circle).await;
    let shown_as = crate::services::channel_group::shown_as(venue);
    let at_hand = AtHand {
        bits: held.iter().filter_map(bit_of).collect(),
        lands: lands_now.as_ref().map(|row| row.content.as_str()),
        day_before,
        sore_spots: json!(super::super::sore::as_input(&sores, chrono::Utc::now())),
        shown_as: shown_as.as_deref(),
        wants,
    };
    let soul: String = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let Ok(raw) = super::super::call::Ask::new(
        super::super::call::Voice::Reading,
        circle.keeper(),
        SCHEMA_NAME,
    )
    .within(READING_TIMEOUT)
    .json_raw(
        &system(&soul),
        &input(lines, &at_hand),
        SCHEMA_NAME,
        &schema(),
    )
    .await
    else {
        return;
    };
    let Some(reading) = parse(&raw, lines, &at_hand) else {
        tracing::warn!("[Merope] a group stretch's reading came back unreadable");
        return;
    };
    // What she kept on a mistake someone pointed out goes before anything
    // new is kept from the same reading.
    if !reading.corrected.is_empty() {
        recheck(db, circle, venue, lines, &reading.corrected, &soul).await;
    }
    put_day(db, circle, &reading.day, today).await;
    put_lands(db, circle, &reading.lands, lands_now.as_ref()).await;
    super::super::sore::let_go(db, &sores, &reading.let_go).await;
    let kept = apply_bits(db, circle, &held, reading.bits).await;
    if kept > 0 {
        tracing::info!(kept, "[Merope] bits kept from a group's talk");
    }
    super::super::heard::keep(db, venue, reading.heard).await;
    for explained in &reading.explained {
        super::super::jargon::told(db, venue, explained).await;
    }
}

/// What she kept from a group lately and goes back over when told she got
/// the talk wrong, at most; and how far back.
const RECHECKED_AT_MOST: u64 = 40;
const RECHECKED_WITHIN_DAYS: i32 = 14;
/// What she kept there that a recheck may take back.
const RECHECKED: [&str; 7] = [
    super::super::strangers::SOURCE,
    super::super::making_sense::SOURCE,
    SOURCE,
    super::super::jargon::SOURCE,
    super::super::recognizing::SOURCE,
    DAY_SOURCE,
    LANDS_SOURCE,
];

/// Someone told her she got the talk wrong: go back over what she kept from
/// that group lately, and take back what rests on that mistake (kept as
/// retired, `misread`, never deleted).
async fn recheck(
    db: &DatabaseConnection,
    circle: &Circle,
    venue: &str,
    lines: &[myriad_merope::group_reading::Line],
    corrected: &[myriad_merope::group_reading::Correction],
    soul: &str,
) {
    use myriad_merope::group_reading::{
        Kept, RECHECK_SCHEMA_NAME, parse_recheck, recheck_input, recheck_schema, recheck_system,
    };
    let rows = lately_kept_in(db, &circle.audience().venue(), venue).await;
    if rows.is_empty() {
        return;
    }
    let kept: Vec<Kept> = rows
        .iter()
        .map(|(_, content, rests_on)| Kept {
            what: content,
            rests_on: rests_on.as_deref(),
        })
        .collect();
    let Ok(raw) = super::super::call::Ask::new(
        super::super::call::Voice::Reading,
        circle.keeper(),
        RECHECK_SCHEMA_NAME,
    )
    .within(READING_TIMEOUT)
    .json_raw(
        &recheck_system(soul),
        &recheck_input(lines, corrected, &kept),
        RECHECK_SCHEMA_NAME,
        &recheck_schema(),
    )
    .await
    else {
        return;
    };
    let wrong: Vec<String> = parse_recheck(&raw, rows.len(), corrected)
        .into_iter()
        .map(|k| rows[k].0.clone())
        .collect();
    if wrong.is_empty() {
        return;
    }
    match take_back(db, wrong).await {
        Ok(taken_back) => tracing::info!(
            %venue,
            taken_back,
            "[Merope] told she got a group's talk wrong, took back what rested on it"
        ),
        Err(error) => tracing::warn!(%error, "[Merope] could not take back what she misread"),
    }
}

/// What she kept from the group (`stored`: the venue as memories keep it;
/// `venue`: as sessions keep it) lately, freshest first: (id, what it says,
/// the words it rests on).
async fn lately_kept_in(
    db: &DatabaseConnection,
    stored: &str,
    venue: &str,
) -> Vec<(String, String, Option<String>)> {
    let heard_here = format!("\"group\":{}", Value::String(venue.to_string()));
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, content, evidence FROM agent_memories \
             WHERE invalid_at IS NULL AND updated_at > now() - make_interval(days => $4) \
               AND ((venue = $1 AND source = ANY($2)) \
                 OR (source = $5 AND evidence LIKE '%' || $3 || '%')) \
             ORDER BY updated_at DESC LIMIT $6",
            [
                stored.into(),
                RECHECKED
                    .iter()
                    .map(|source| source.to_string())
                    .collect::<Vec<_>>()
                    .into(),
                heard_here.into(),
                RECHECKED_WITHIN_DAYS.into(),
                super::super::heard::SOURCE.into(),
                (RECHECKED_AT_MOST as i64).into(),
            ],
        ))
        .await
        .unwrap_or_default();
    let rows: Vec<(String, String, Option<String>)> = rows
        .iter()
        .filter_map(|row| {
            let id: String = row.try_get("", "id").ok()?;
            let content: String = row.try_get("", "content").ok()?;
            let evidence: Option<String> = row.try_get("", "evidence").ok().flatten();
            Some((id, content, evidence.as_deref().and_then(rests_on)))
        })
        .collect();
    rows
}

/// Take back rows she kept on a misreading: retired as `misread`, kept.
async fn take_back(db: &DatabaseConnection, ids: Vec<String>) -> Result<u64, sea_orm::DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET invalid_at = now(), invalid_reason = $2, updated_at = now() \
         WHERE id = ANY($1) AND invalid_at IS NULL",
        [ids.into(), MISREAD.into()],
    ))
    .await
    .map(|done| done.rows_affected())
}

/// Why a row was taken back after someone pointed out the mistake it rests on.
pub const MISREAD: &str = "misread";

/// The words a kept row was kept from, as its evidence carries them: its
/// quote, or the quotes of its facts.
fn rests_on(evidence: &str) -> Option<String> {
    let evidence: Value = serde_json::from_str(evidence).ok()?;
    if let Some(quote) = evidence.get("quote").and_then(Value::as_str) {
        return Some(quote.to_string());
    }
    let quotes: Vec<&str> = evidence
        .get("facts")?
        .as_array()?
        .iter()
        .filter_map(|fact| fact.get("quote")?.as_str())
        .collect();
    (!quotes.is_empty()).then(|| quotes.join(" / "))
}

/// How long the reading of a stretch may take: the stronger model, thinking.
const READING_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(120);

/// What she already wrote about `day` in this group, if anything.
async fn day_line(
    db: &DatabaseConnection,
    circle: &Circle,
    day: DateTime<FixedOffset>,
) -> Option<String> {
    let marker = format!("\"day\":\"{}\"", day.format("%Y-%m-%d"));
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT content FROM agent_memories WHERE source = $1 AND venue = $2 \
           AND invalid_at IS NULL AND evidence LIKE '%' || $3 || '%' \
         ORDER BY created_at DESC LIMIT 1",
        [
            DAY_SOURCE.into(),
            circle.audience().venue().into(),
            marker.into(),
        ],
    ))
    .await
    .ok()
    .flatten()
    .and_then(|row| row.try_get::<String>("", "content").ok())
}

/// On each of her lines, how many seconds until someone else wrote after it
/// that day (null: nobody did): how it was taken, as far as timing shows.
pub(super) fn with_after(lines: Vec<(DateTime<FixedOffset>, Value)>) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::with_capacity(lines.len());
    for (index, (at, line)) in lines.iter().enumerate() {
        let mut line = line.clone();
        if line["who"] == "you" {
            let next = lines[index + 1..]
                .iter()
                .find(|(_, later)| later["who"] != "you")
                .map(|(later, _)| (*later - *at).num_seconds().max(0));
            line["after"] = json!(next);
        }
        out.push(line);
    }
    out
}

/// Bits already there for a circle, freshest first.
pub(super) async fn held_in(
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
pub(super) async fn day_with(
    db: &DatabaseConnection,
    user_id: i32,
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
) -> Vec<Value> {
    let lines = db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT m.role, m.content, m.created_at FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
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
        let at = row.try_get::<DateTime<FixedOffset>>("", "created_at").ok()?;
        (!text.trim().is_empty()).then(|| {
            (
                at,
                json!({ "who": if role == "user" { "they" } else { "you" }, "text": text }),
            )
        })
    })
    .collect::<Vec<_>>();
    with_after(lines)
}

/// Keep what the day in a group was like, if anything happened.
pub(super) async fn put_day(
    db: &DatabaseConnection,
    circle: &Circle,
    said: &str,
    day: DateTime<FixedOffset>,
) {
    let text: String = said.trim().chars().take(DAY_CHARS).collect();
    if text.is_empty() {
        return;
    }
    // One line a day: what she writes of a day later replaces what she wrote
    // of it earlier, each earlier line pointing at the new one.
    let marker = format!("\"day\":\"{}\"", day.format("%Y-%m-%d"));
    let earlier: Vec<String> = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM agent_memories \
             WHERE user_id = $1 AND source = $2 AND venue = $3 AND invalid_at IS NULL \
               AND evidence LIKE '%' || $4 || '%'",
            [
                circle.keeper().into(),
                DAY_SOURCE.into(),
                circle.audience().venue().into(),
                marker.into(),
            ],
        ))
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| row.try_get::<String>("", "id").ok())
        .collect();
    let _ = unified::remember_in_place_of(
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
        unified::InPlaceOf {
            user_id: circle.keeper(),
            ids: &earlier,
            reason: "superseded",
        },
    )
    .await;
}

/// What days in a group (`venue` as sessions keep it) were like, oldest
/// first: the last `limit`, and any older one what was `said` brings back:
/// (date, what it was like).
pub async fn days_in(
    db: &DatabaseConnection,
    venue: &str,
    limit: u64,
    said: Option<&str>,
) -> Vec<(String, String)> {
    let circle = Circle::Group {
        venue: venue.to_string(),
        keeper: 0,
    };
    let mut days: Vec<(String, String)> = unified::fresh_and_brought(
        db,
        &circle.audience().venue(),
        DAY_SOURCE,
        limit as usize,
        said,
        super::BROUGHT_BACK,
    )
    .await
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
    // Freshest first, then what was brought back: by date, oldest first.
    days.sort_by(|left, right| left.0.cmp(&right.0));
    days
}

#[cfg(test)]
mod tests {
    use super::*;
    use sea_orm::EntityTrait;

    /// Told she misread a group, she goes back over what she kept from that
    /// group lately (here, and what she heard there), with the words each
    /// rests on, and what she takes back is retired as misread, not deleted.
    #[tokio::test]
    async fn what_she_misread_in_a_group_is_taken_back_and_kept() {
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let isolated = crate::db::IsolatedSchema::migrated(&url, "group_recheck").await;
        let db = isolated.db.clone();
        let venue = "onebot:1065";
        let stored = unified::Audience::group(venue, 0).venue();
        let elsewhere = unified::Audience::group("onebot:2077", 0).venue();
        let note = unified::remember_in_venue(
            &db,
            &stored,
            "阿明，笑我不会玩",
            &json!({ "who": "onebot:1", "facts": [{ "fact": "笑我不会玩", "quote": "你不会玩吧" }] })
                .to_string(),
            super::super::super::strangers::SOURCE,
        )
        .await
        .unwrap()
        .unwrap();
        let word = unified::remember_in_venue(
            &db,
            &stored,
            "书包：开地铁的",
            &json!({ "term": "书包", "quote": "书包是开地铁的" }).to_string(),
            super::super::super::jargon::SOURCE,
        )
        .await
        .unwrap()
        .unwrap();
        let old = unified::remember_in_venue(
            &db,
            &stored,
            "很早以前的一条",
            "{}",
            super::super::super::jargon::SOURCE,
        )
        .await
        .unwrap()
        .unwrap();
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_memories SET updated_at = now() - interval '20 days' WHERE id = $1",
            [old.clone().into()],
        ))
        .await
        .unwrap();
        unified::remember_in_venue(
            &db,
            &elsewhere,
            "别的群的说法",
            "{}",
            super::super::super::jargon::SOURCE,
        )
        .await
        .unwrap();
        let heard = unified::remember_own(
            &db,
            "武汉大学的樱花三月最好看",
            &json!({ "group": venue }).to_string(),
            Vec::new(),
            super::super::super::heard::SOURCE,
        )
        .await
        .unwrap()
        .unwrap();
        let kept = lately_kept_in(&db, &stored, venue).await;
        let mut ids: Vec<&str> = kept.iter().map(|(id, ..)| id.as_str()).collect();
        ids.sort_unstable();
        let mut expected = vec![note.as_str(), word.as_str(), heard.as_str()];
        expected.sort_unstable();
        assert_eq!(
            ids, expected,
            "this group's, lately, and what she heard there"
        );
        let rests = |id: &str| {
            kept.iter()
                .find(|(kept, ..)| kept == id)
                .and_then(|(.., rests_on)| rests_on.clone())
        };
        assert_eq!(rests(&note).as_deref(), Some("你不会玩吧"));
        assert_eq!(rests(&word).as_deref(), Some("书包是开地铁的"));
        assert_eq!(take_back(&db, vec![word.clone()]).await.unwrap(), 1);
        assert_eq!(take_back(&db, vec![word.clone()]).await.unwrap(), 0);
        let row = crate::models::entities::agent_memories::Entity::find_by_id(word)
            .one(&db)
            .await
            .unwrap()
            .expect("kept, not deleted");
        assert_eq!(row.invalid_reason.as_deref(), Some(MISREAD));
        assert_eq!(lately_kept_in(&db, &stored, venue).await.len(), 2);
        isolated.drop().await;
    }
}
