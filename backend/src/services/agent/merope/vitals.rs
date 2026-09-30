//! Her vital signs (see `myriad_merope::vitals`): each day of her counted
//! from what was kept, without any model, and kept for looking back; the
//! night after a day counts it once, today is counted when looked at.

use chrono::{DateTime, FixedOffset, Local, NaiveDate, TimeZone};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::Value;

use crate::services::agent::memory::unified;
pub use myriad_merope::vitals::Day;
use myriad_merope::vitals::{alerts, ends_asking, leaned_on, quantile};

pub const NAMESPACE: &str = "merope_vitals";
const KEEP_DAYS: i64 = 400;
/// Days looked back on for what she leans on, and for what is usual.
const LEANING_DAYS: i64 = 3;
const USUAL_DAYS: u64 = 7;
/// A phrase is listed from this share of her notes or replies.
const LISTED_FROM: f64 = 0.15;

fn identity() -> crate::services::runtime_registry::RegistryIdentity<'static> {
    crate::services::runtime_registry::RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    }
}

fn key(day: NaiveDate) -> String {
    format!("day:{day}")
}

fn bounds(
    from: NaiveDate,
    to: NaiveDate,
) -> Option<(DateTime<FixedOffset>, DateTime<FixedOffset>)> {
    let at = |day: NaiveDate| {
        Local
            .from_local_datetime(&day.and_hms_opt(0, 0, 0)?)
            .earliest()
            .map(|at| at.fixed_offset())
    };
    Some((at(from)?, at(to.succ_opt()?)?))
}

async fn rows(
    db: &DatabaseConnection,
    sql: &str,
    start: DateTime<FixedOffset>,
    end: DateTime<FixedOffset>,
) -> Vec<sea_orm::QueryResult> {
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        sql,
        [start.into(), end.into()],
    ))
    .await
    .unwrap_or_default()
}

/// One day of her, counted.
pub async fn count(db: &DatabaseConnection, day: NaiveDate) -> Day {
    let mut counted = Day {
        day: day.to_string(),
        ..Day::default()
    };
    let Some((start, end)) = bounds(day, day) else {
        return counted;
    };
    // What she cost.
    for row in rows(
        db,
        "SELECT operation, count(*) AS calls, \
           count(*) FILTER (WHERE status <> 'completed') AS failed, \
           coalesce(sum(input_tokens), 0) AS tokens \
         FROM ai_cost_ledger WHERE source = 'merope' AND occurred_at >= $1 AND occurred_at < $2 \
         GROUP BY operation ORDER BY calls DESC",
        start,
        end,
    )
    .await
    {
        let calls = row.try_get::<i64>("", "calls").unwrap_or(0) as u64;
        counted.calls += calls;
        counted.failed_calls += row.try_get::<i64>("", "failed").unwrap_or(0) as u64;
        counted.input_tokens += row.try_get::<i64>("", "tokens").unwrap_or(0) as u64;
        if counted.busiest.len() < 5 {
            counted.busiest.push((
                row.try_get::<String>("", "operation").unwrap_or_default(),
                calls,
            ));
        }
    }
    // What was kept.
    for row in rows(
        db,
        "SELECT source, count(*) AS kept FROM agent_memories \
         WHERE created_at >= $1 AND created_at < $2 GROUP BY source",
        start,
        end,
    )
    .await
    {
        if let (Ok(source), Ok(kept)) = (
            row.try_get::<String>("", "source"),
            row.try_get::<i64>("", "kept"),
        ) {
            counted.kept.insert(source, kept as u64);
        }
    }
    // Her own time.
    let own = unified::own_rows_since(db, unified::OWN_EXPERIENCE, start, 3000)
        .await
        .unwrap_or_default();
    for row in own.iter().filter(|row| row.created_at < end) {
        match super::doing::key_of(row) {
            Some((_, thing)) => {
                counted.things += 1;
                counted.own_minutes += thing.minutes() as f64;
                let reaction = row
                    .evidence
                    .as_deref()
                    .and_then(|evidence| serde_json::from_str::<Value>(evidence).ok())
                    .and_then(|evidence| evidence.get("reaction")?.as_str().map(str::to_string))
                    .unwrap_or_else(|| "unsaid".to_string());
                *counted.landed.entry(reaction).or_default() += 1;
            }
            None => counted.unreadable += 1,
        }
    }
    counted.lazed_minutes = super::pace::lazed_on(db, day).await;
    // How fast she answered.
    let waits: Vec<f64> = rows(
        db,
        "SELECT extract(epoch FROM m.created_at - prev.created_at)::float8 AS wait \
         FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
         JOIN LATERAL (SELECT p.role, p.created_at FROM agent_messages p \
           WHERE p.session_id = m.session_id AND p.created_at < m.created_at \
           ORDER BY p.created_at DESC LIMIT 1) prev ON true \
         WHERE s.context->>'mode' = 'chat' AND m.role = 'assistant' AND prev.role = 'user' \
           AND m.created_at >= $1 AND m.created_at < $2",
        start,
        end,
    )
    .await
    .iter()
    .filter_map(|row| row.try_get::<f64>("", "wait").ok())
    .filter(|wait| *wait < 600.0)
    .collect();
    counted.replies = waits.len() as u64;
    counted.reply_p50 = quantile(&waits, 0.5).map(|wait| (wait * 10.0).round() / 10.0);
    counted.reply_p90 = quantile(&waits, 0.9).map(|wait| (wait * 10.0).round() / 10.0);
    // What she leans on lately.
    if let Some((from, to)) = bounds(day - chrono::Duration::days(LEANING_DAYS - 1), day) {
        let notes: Vec<String> = unified::own_rows_since(db, unified::OWN_EXPERIENCE, from, 3000)
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|row| row.created_at < to)
            .map(|row| row.content)
            .collect();
        counted.notes_lean_on = leaned_on(&notes, LISTED_FROM, 5);
        let replies: Vec<String> = rows(
            db,
            "SELECT m.content FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.context->>'mode' = 'chat' AND m.role = 'assistant' \
               AND m.created_at >= $1 AND m.created_at < $2",
            from,
            to,
        )
        .await
        .iter()
        .filter_map(|row| row.try_get::<String>("", "content").ok())
        .collect();
        counted.replies_lean_on = leaned_on(&replies, LISTED_FROM, 5);
        counted.replies_asking = (replies.len() >= 5).then(|| {
            let asking = replies.iter().filter(|reply| ends_asking(reply)).count();
            (asking as f64 / replies.len() as f64 * 100.0).round() / 100.0
        });
    }
    counted
}

/// The days kept before `day`, oldest first, at most `limit`.
async fn kept_before(db: &DatabaseConnection, day: NaiveDate, limit: u64) -> Vec<Day> {
    let mut days = Vec::new();
    for back in (1..=limit).rev() {
        let Some(then) = day.checked_sub_days(chrono::Days::new(back)) else {
            continue;
        };
        if let Ok(Some(kept)) =
            crate::services::runtime_registry::get::<Day>(db, NAMESPACE, &key(then)).await
        {
            days.push(kept);
        }
    }
    days
}

/// One day counted, with what is worth raising against the days before.
async fn count_with_alerts(db: &DatabaseConnection, day: NaiveDate) -> Day {
    let mut counted = count(db, day).await;
    counted.alerts = alerts(&counted, &kept_before(db, day, USUAL_DAYS).await);
    counted
}

/// At night: yesterday counted and kept, once.
pub async fn go_over(db: &DatabaseConnection) {
    let Some(yesterday) = Local::now().date_naive().pred_opt() else {
        return;
    };
    if let Ok(Some(_)) =
        crate::services::runtime_registry::get::<Day>(db, NAMESPACE, &key(yesterday)).await
    {
        return;
    }
    let counted = count_with_alerts(db, yesterday).await;
    if !counted.alerts.is_empty() {
        tracing::warn!(alerts = ?counted.alerts, day = %yesterday, "[Merope] her vital signs raise something");
    }
    let keep_until = (chrono::Utc::now() + chrono::Duration::days(KEEP_DAYS)).timestamp();
    if let Err(error) = crate::services::runtime_registry::put(
        db,
        NAMESPACE,
        &key(yesterday),
        identity(),
        &counted,
        keep_until,
    )
    .await
    {
        tracing::warn!(%error, "[Merope] could not keep her vital signs");
    }
}

/// The last week of her, oldest first: the days kept, and today so far.
pub async fn week(db: &DatabaseConnection) -> Vec<Day> {
    let today = Local::now().date_naive();
    let mut days = kept_before(db, today, USUAL_DAYS - 1).await;
    days.push(count_with_alerts(db, today).await);
    days
}

/// A new persona has none of this behind her.
pub async fn forget<C: ConnectionTrait>(db: &C) -> Result<u64, sea_orm::DbErr> {
    crate::services::runtime_registry::delete_matching(db, NAMESPACE, None, None, None, None).await
}
