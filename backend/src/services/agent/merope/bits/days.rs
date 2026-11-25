//! A group's days: what each day there was like, as the group itself heard it.

use sea_orm::TransactionTrait;

use super::*;
use crate::services::agent::merope::store::memory_jobs::Own;

/// What she makes of a stretch of a group's talk while it is still at hand
/// (the lines themselves are kept only a few hours), in one reading (see
/// `myriad_merope::group_reading`): what today there is like so far, the
/// group's bits, how she comes across there, stings let go, and what she
/// heard about the world. `said` is (when, who, what, hers), oldest first;
/// `keeper` is who the group's rows are kept with (the site's owner, who
/// brought her there) and who the reading is billed to; `people`, those in
/// it she has an account of, each by the name they go by there (none: could
/// not be read). An `Err`: what they asked her not to keep could not be told
/// of it, and none of it is kept till it is, with what it was read as
/// (`again`, the next time).
pub(crate) async fn go_over_stretch(
    db: &DatabaseConnection,
    keeper: i32,
    venue: &str,
    said: Vec<(DateTime<FixedOffset>, String, String, bool)>,
    people: Option<&[(i32, String)]>,
    again: Option<Again<'_>>,
) -> Result<(), NotTold> {
    let said: Vec<_> = said
        .into_iter()
        .filter(|(_, _, text, _)| !text.trim().is_empty())
        .collect();
    if (said.len() as i64) < MIN_LINES || said.iter().all(|(_, _, _, hers)| *hers) {
        return Ok(());
    }
    let lines = reading_lines(&said);
    let circle = Circle::Group {
        venue: venue.to_string(),
        keeper,
    };
    let day = match again {
        Some(again) => (again.read.at)
            .with_timezone(&super::super::clock::zone())
            .date_naive(),
        None => super::super::clock::local_now().date_naive(),
    };
    let today = day
        .and_hms_opt(0, 0, 0)
        .and_then(|midnight| {
            chrono::TimeZone::from_local_datetime(&super::super::clock::zone(), &midnight)
                .earliest()
        })
        .map(|midnight| midnight.fixed_offset());
    let Some(today) = today else {
        return Ok(());
    };
    let before = day_line(db, &circle, today).await;
    read_group(db, &circle, venue, &lines, (today, before), people, again).await
}

/// What a stretch was read as, and when.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub(crate) struct Read {
    pub raw: String,
    pub at: DateTime<FixedOffset>,
}

/// A stretch read before, gone over again: what it was read as, and the job
/// it waited in, under which alone anything of it is kept.
#[derive(Clone, Copy)]
pub(crate) struct Again<'a> {
    pub read: &'a Read,
    pub own: Own<'a>,
}

/// A stretch read, and what they asked her not to keep of it not told.
pub(crate) struct NotTold {
    pub read: Read,
    pub(in crate::services::agent::merope) failure: super::super::memory_jobs::Failure,
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

/// Read the stretch once (`again`, when it was) and keep what holds up of it;
/// of one read before, nothing she has found again since, and all of it
/// together or none, while its job is its own still.
async fn read_group(
    db: &DatabaseConnection,
    circle: &Circle,
    venue: &str,
    lines: &[myriad_merope::group_reading::Line],
    (today, day_before): (DateTime<FixedOffset>, Option<DayLine>),
    people: Option<&[(i32, String)]>,
    again: Option<Again<'_>>,
) -> Result<(), NotTold> {
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
        day_before: day_before.as_ref().map(|(line, _)| line.as_str()),
        sore_spots: json!(super::super::sore::as_input(&sores, crate::clock::now())),
        shown_as: shown_as.as_deref(),
        wants,
    };
    let soul: String = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let now_read;
    let (read, first) = match again {
        Some(again) => (again.read, false),
        None => {
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
                return Ok(());
            };
            now_read = Read {
                raw,
                at: crate::clock::now().fixed_offset(),
            };
            (&now_read, true)
        }
    };
    let Some(mut reading) = parse(&read.raw, lines, &at_hand) else {
        tracing::warn!("[Merope] a group stretch's reading came back unreadable");
        return Ok(());
    };
    // What she kept on a mistake someone pointed out goes before anything
    // new is kept from the same reading. It says which by their places now,
    // and goes whatever is kept.
    if first {
        if !reading.corrected.is_empty() {
            recheck(db, circle, venue, lines, &reading.corrected, &soul).await;
        }
        super::super::sore::let_go(db, &sores, &reading.let_go).await;
    }
    let told = match people {
        Some(people) => super::not_kept::leave_out(db, people, &mut reading).await,
        None => Err(super::super::memory_jobs::Failure::Storage),
    };
    if let Err(failure) = told {
        return Err(NotTold {
            read: read.clone(),
            failure,
        });
    }
    let since = |at: DateTime<FixedOffset>| !first && at > read.at;
    if day_before.is_some_and(|(_, at)| since(at)) {
        reading.day.clear();
    }
    if lands_now.as_ref().is_some_and(|row| since(row.created_at)) {
        reading.lands.clear();
    }
    reading.bits.retain(|bit| {
        !held.iter().any(|row| {
            since(row.updated_at)
                && bit_of(row).is_some_and(|(handle, _)| same_handle(&handle, bit.handle.trim()))
        })
    });
    let Some(again) = again else {
        put_day(db, circle, &reading.day, today).await;
        put_lands(db, circle, &reading.lands, lands_now.as_ref()).await;
        let kept = apply_bits(db, circle, &held, reading.bits).await;
        if kept > 0 {
            tracing::info!(kept, "[Merope] bits kept from a group's talk");
        }
        super::super::heard::keep(db, venue, reading.heard).await;
        for explained in &reading.explained {
            super::super::jargon::told(db, venue, explained).await;
        }
        return Ok(());
    };
    let explained: Vec<String> = (reading.explained.iter())
        .map(|one| one.term.clone())
        .collect();
    let lands = lands_now.as_ref();
    match keep_again(db, again.own, circle, venue, reading, (today, lands, &held)).await {
        Ok(None) => Ok(()),
        Ok(Some(kept)) => {
            for id in kept {
                super::super::memory_nodes::hanging::kept(db, &id).await;
            }
            for term in explained {
                super::super::jargon::no_longer_watched(db, venue, &term).await;
            }
            Ok(())
        }
        Err(error) => {
            tracing::warn!(%error, "[Merope] a group's stretch could not be kept");
            Err(NotTold {
                read: read.clone(),
                failure: super::super::memory_jobs::Failure::Storage,
            })
        }
    }
}

/// What a stretch read before came to, kept all together, while its job is
/// its `own` still: the rows to go on her nodes, or `None` when it no longer
/// is, and nothing of it is kept.
async fn keep_again(
    db: &DatabaseConnection,
    own: Own<'_>,
    circle: &Circle,
    venue: &str,
    reading: myriad_merope::group_reading::Reading,
    (today, lands, held): (
        DateTime<FixedOffset>,
        Option<&crate::models::entities::agent_memories::Model>,
        &[crate::models::entities::agent_memories::Model],
    ),
) -> anyhow::Result<Option<Vec<String>>> {
    let heard = if super::super::is_enabled().await {
        reading.heard
    } else {
        Vec::new()
    };
    let tx = db.begin().await?;
    if !own.still(&tx).await? {
        return Ok(None);
    }
    let mut kept = Vec::new();
    kept.extend(write_day(&tx, circle, &reading.day, today).await?);
    kept.extend(write_lands(&tx, circle, &reading.lands, lands).await?);
    for change in reading.bits.into_iter().take(MAX_CHANGES) {
        kept.extend(write_bit(&tx, circle, held, change).await?);
    }
    super::super::heard::write(&tx, venue, heard).await?;
    for explained in &reading.explained {
        kept.extend(super::super::jargon::write_told(&tx, venue, explained).await?);
    }
    tx.commit().await?;
    Ok(Some(kept))
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
             WHERE invalid_at IS NULL AND updated_at > $7::timestamptz - make_interval(days => $4) \
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
                crate::clock::now().into(),
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
        "UPDATE agent_memories SET invalid_at = $3::timestamptz, invalid_reason = $2, updated_at = $3::timestamptz \
         WHERE id = ANY($1) AND invalid_at IS NULL",
        [ids.into(), MISREAD.into(), crate::clock::now().into()],
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

/// What she wrote of a day in a group, and when.
type DayLine = (String, DateTime<FixedOffset>);

/// What she already wrote about `day` in this group, if anything.
async fn day_line(
    db: &DatabaseConnection,
    circle: &Circle,
    day: DateTime<FixedOffset>,
) -> Option<DayLine> {
    let marker = format!("\"day\":\"{}\"", day.format("%Y-%m-%d"));
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT content, created_at FROM agent_memories WHERE source = $1 AND venue = $2 \
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
    .and_then(|row| {
        Some((
            row.try_get("", "content").ok()?,
            row.try_get("", "created_at").ok()?,
        ))
    })
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
        "SELECT m.role, m.content, m.created_at, m.metadata->>'piece' AS piece FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
         WHERE (s.context->>'past') IS NULL AND s.user_id = $1 AND s.context->>'mode' = 'chat' AND s.context->>'venue' IS NULL \
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
        // A piece's result is her helper's, not what she said.
        let piece = row.try_get::<Option<String>>("", "piece").ok().flatten();
        let who = match (role.as_str(), piece) {
            ("user", _) => "they",
            (_, Some(_)) => "your helper",
            _ => "you",
        };
        (!text.trim().is_empty()).then(|| (at, json!({ "who": who, "text": text })))
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
    if let Ok(Some(id)) = write_day(db, circle, said, day).await {
        super::super::memory_nodes::hanging::kept(db, &id).await;
    }
}

/// [`put_day`]'s row, when there is one to write.
async fn write_day<C: ConnectionTrait>(
    db: &C,
    circle: &Circle,
    said: &str,
    day: DateTime<FixedOffset>,
) -> Result<Option<String>, sea_orm::DbErr> {
    let text: String = said.trim().chars().take(DAY_CHARS).collect();
    if text.is_empty() {
        return Ok(None);
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
        .await?
        .iter()
        .filter_map(|row| row.try_get::<String>("", "id").ok())
        .collect();
    unified::remember_in_place_of(
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
    .await
    .map(|replaced| replaced.id)
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
    .map(day_of_row)
    .collect();
    // Freshest first, then what was brought back: by date, oldest first.
    days.sort_by(|left, right| left.0.cmp(&right.0));
    days
}

/// A day in a group as its row keeps it: (date, what it was like).
pub(crate) fn day_of_row(row: crate::models::entities::agent_memories::Model) -> (String, String) {
    let day = row
        .evidence
        .as_deref()
        .and_then(|evidence| serde_json::from_str::<Value>(evidence).ok())
        .and_then(|evidence| evidence.get("day")?.as_str().map(str::to_string))
        .unwrap_or_else(|| row.created_at.format("%Y-%m-%d").to_string());
    (day, row.content)
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
        let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
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
