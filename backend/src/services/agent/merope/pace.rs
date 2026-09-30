//! How hard she is going at her own things (see `myriad_merope::pace`): her
//! usual, kept from day to day; what went into today, counted from what she
//! did; the time she lazed about; and whether she gets up at a free moment.
//!
//! Her usual and each day's lazing are kept in the runtime registry, so a
//! restart does not reset her habit, and go with the persona.

use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, FixedOffset, Local, NaiveDate, TimeZone};
use sea_orm::DatabaseConnection;

use crate::services::agent::memory::unified;
pub use myriad_merope::pace::{LAZING, Pace, lazing_line};
use myriad_merope::pace::{next_day, section};

pub const NAMESPACE: &str = "merope_pace";
const PACE: &str = "pace";
/// Kept this long, so a week's lazing can be looked back on.
const KEEP_DAYS: i64 = 400;

static TODAY: LazyLock<Mutex<Option<Pace>>> = LazyLock::new(|| Mutex::new(None));

fn identity() -> crate::services::runtime_registry::RegistryIdentity<'static> {
    crate::services::runtime_registry::RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    }
}

async fn put<T: serde::Serialize>(db: &DatabaseConnection, record: &str, value: &T) {
    let keep_until = (chrono::Utc::now() + chrono::Duration::days(KEEP_DAYS)).timestamp();
    if let Err(error) =
        crate::services::runtime_registry::put(db, NAMESPACE, record, identity(), value, keep_until)
            .await
    {
        tracing::warn!(%error, "[Merope] could not keep her pace");
    }
}

fn lazed_key(day: NaiveDate) -> String {
    format!("lazed:{day}")
}

/// The day on the host clock, as a span.
fn bounds(day: NaiveDate) -> Option<(DateTime<FixedOffset>, DateTime<FixedOffset>)> {
    let start = Local
        .from_local_datetime(&day.and_hms_opt(0, 0, 0)?)
        .earliest()?
        .fixed_offset();
    let end = Local
        .from_local_datetime(&day.succ_opt()?.and_hms_opt(0, 0, 0)?)
        .earliest()?
        .fixed_offset();
    Some((start, end))
}

/// Minutes that went into things of her own on `day`; None when she did
/// nothing of her own then (she was not running, or a new persona).
pub async fn spent_on(db: &DatabaseConnection, day: NaiveDate) -> Option<f64> {
    let (start, end) = bounds(day)?;
    let rows = unified::own_rows_since(db, unified::OWN_EXPERIENCE, start, 3000)
        .await
        .ok()?;
    let minutes: Vec<i64> = rows
        .iter()
        .filter(|row| row.created_at < end)
        .filter_map(|row| super::doing::key_of(row).map(|(_, thing)| thing.minutes()))
        .collect();
    (!minutes.is_empty()).then(|| minutes.iter().sum::<i64>() as f64)
}

/// Minutes she lazed about on `day`.
pub async fn lazed_on(db: &DatabaseConnection, day: NaiveDate) -> f64 {
    crate::services::runtime_registry::get::<f64>(db, NAMESPACE, &lazed_key(day))
        .await
        .ok()
        .flatten()
        .unwrap_or(0.0)
}

/// She lazed about for `minutes` today.
pub async fn lazed(db: &DatabaseConnection, minutes: f64) {
    let day = Local::now().date_naive();
    let so_far = lazed_on(db, day).await;
    put(db, &lazed_key(day), &(so_far + minutes)).await;
}

/// Her pace today: kept, or, on a new day, moved on from yesterday.
pub async fn today(db: &DatabaseConnection) -> Pace {
    let day = Local::now().date_naive();
    if let Some(pace) = TODAY
        .lock()
        .ok()
        .and_then(|today| today.clone())
        .filter(|pace| pace.day == day)
    {
        return pace;
    }
    let kept = crate::services::runtime_registry::get::<Pace>(db, NAMESPACE, PACE)
        .await
        .ok()
        .flatten();
    let pace = match kept {
        Some(kept) if kept.day == day => kept,
        kept => {
            let yesterday = match day.pred_opt() {
                Some(yesterday) => spent_on(db, yesterday).await,
                None => None,
            };
            let pace = next_day(kept.as_ref(), day, yesterday);
            put(db, PACE, &pace).await;
            tracing::info!(
                usual_minutes = pace.usual_minutes.round(),
                days_past_usual = pace.days_past_usual,
                tone = ?pace.tone,
                "[Merope] a new day of her own"
            );
            pace
        }
    };
    if let Ok(mut today) = TODAY.lock() {
        *today = Some(pace.clone());
    }
    pace
}

/// Her pace and what went into today so far: (pace, minutes on her own
/// things, minutes lazing).
pub async fn now(db: &DatabaseConnection) -> (Pace, f64, f64) {
    let pace = today(db).await;
    let day = pace.day;
    let spent = spent_on(db, day).await.unwrap_or(0.0);
    let lazed = lazed_on(db, day).await;
    (pace, spent, lazed)
}

/// How much of `day` went into things of her own and into lazing, against
/// her usual, as she would know it writing her diary.
pub async fn day_line(db: &DatabaseConnection, day: NaiveDate) -> String {
    let pace = today(db).await;
    let spent = spent_on(db, day).await.unwrap_or(0.0);
    let lazed = lazed_on(db, day).await;
    myriad_merope::pace::facts(&pace, spent, lazed)
        .into_iter()
        .next()
        .unwrap_or_default()
        .replacen("today so far", "that day", 1)
}

/// Her pace for a conversation, when it says something.
pub async fn conversation_section(db: &DatabaseConnection) -> Option<String> {
    let (pace, spent, lazed) = now(db).await;
    section(&pace, spent, lazed)
}

/// A new persona has no habit of her own yet.
pub async fn forget<C: sea_orm::ConnectionTrait>(db: &C) -> Result<u64, sea_orm::DbErr> {
    if let Ok(mut today) = TODAY.lock() {
        *today = None;
    }
    crate::services::runtime_registry::delete_matching(db, NAMESPACE, None, None, None, None).await
}
