//! Things she does on her own.
//!
//! She is not only alive when someone talks to her. Between people she has
//! time of her own: a song from the site's playlist, a note published on
//! it, the next part of a book she follows, a question of her own to think
//! over or look into. What she picks is hers to judge, as this personality,
//! from a few things at hand and the facts of her day; she may also do
//! nothing for a while.
//!
//! This is the one loop, whatever she does: choose, take it in, write what
//! stayed with her and how it landed, keep it. Where things come from and
//! how each kind reaches her is in `sources`; nothing here tells kinds
//! apart.
//!
//! What she writes is hers: it belongs to no one, names no one, and any
//! conversation may hear of it (it is about public things). If someone can
//! see her then, she may bring it up, through the same live-only decision as
//! a passing thought. People who come by find her in the middle of something
//! and can join in.
//!
//! What she does is chosen by the judgment model and felt in her own voice,
//! billed to the site owner (without one, she does nothing). All material is
//! untrusted text, and a day holds a bounded number of things.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::call::{self, Voice};
pub use super::sources::Thing;
use super::sources::{self, Kept};
use crate::services::agent::memory::unified::{self, Concept};

mod choosing;
mod experience;
mod finishing;
mod kept;
mod songs;

pub use choosing::*;
pub use experience::*;
use finishing::*;
pub use kept::*;
pub use songs::*;

pub const DOING_EVENT: &str = "agent.merope.doing";
const PAUSE_MINUTES: std::ops::Range<i64> = 3..12;
/// When she would rather do nothing, how long before she thinks about it
/// again is hers to say, within these; otherwise `REST`.
const REST: chrono::Duration = chrono::Duration::minutes(30);
/// When the model could not be reached to choose, she did not choose to
/// rest: she comes back to it soon.
const UNREACHED_AGAIN: chrono::Duration = chrono::Duration::minutes(5);
/// She brings something up to the same person at most this often.
const TELL_EVERY: Duration = Duration::from_secs(20 * 60);
const CALL_TIMEOUT: Duration = Duration::from_secs(45);
/// A digest the model failed on in passing is asked once more after this.
const DIGEST_AGAIN_AFTER: Duration = Duration::from_secs(20);

/// What she is doing now.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Doing {
    pub thing: Thing,
    pub started: DateTime<Utc>,
    pub ends: DateTime<Utc>,
    /// Why she picked it, in her words. Hers; not shown to anyone.
    #[serde(skip)]
    pub why: String,
}

/// Lazing about: which way, since when, until when.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lazing {
    pub kind: &'static str,
    pub started: DateTime<Utc>,
    pub ends: DateTime<Utc>,
}

#[derive(Default)]
struct Life {
    now: Option<Doing>,
    lazing: Option<Lazing>,
    /// When she next looks for something to do.
    next_at: Option<DateTime<Utc>>,
    /// The clock hour counted, as hours since the epoch, and how many
    /// things she started in it.
    hour: Option<i64>,
    this_hour: u32,
    told: HashMap<i32, Instant>,
}

static LIFE: LazyLock<Mutex<Life>> = LazyLock::new(|| Mutex::new(Life::default()));

// --- her now, kept across a restart --------------------------------------------

/// A new persona starts with no time of her own behind her.
pub(super) fn forget() {
    if let Ok(mut life) = LIFE.lock() {
        *life = Life::default();
    }
    super::hearing::forget();
    super::explore::forget();
}

/// What she is in the middle of, if anything.
pub fn current() -> Option<Doing> {
    LIFE.lock().ok()?.now.clone()
}

/// How she is lazing about, if she is.
pub fn lazing() -> Option<Lazing> {
    LIFE.lock().ok()?.lazing.clone()
}

/// What she is up to on her own right now, as a line for a prompt: what she
/// is doing, or how she is lazing about.
pub fn now_text(at: DateTime<Utc>) -> Option<String> {
    if let Some(doing) = current() {
        return Some(now_line(&doing, at));
    }
    let lazing = lazing()?;
    let done = at
        .signed_duration_since(lazing.started)
        .num_minutes()
        .max(0);
    let total = lazing
        .ends
        .signed_duration_since(lazing.started)
        .num_minutes()
        .max(1);
    Some(format!(
        "You are {}, about {done} of {total} minutes in.",
        super::pace::lazing_line(lazing.kind)
    ))
}

/// Lazing about when the urge does not come: mostly daydreaming.
fn idle_kind() -> &'static str {
    let draw: f64 = rand::random();
    let index = if draw < 0.5 {
        0
    } else if draw < 0.8 {
        1
    } else {
        2
    };
    super::pace::LAZING[index].0
}

pub async fn tick(db: DatabaseConnection) {
    if !super::is_enabled().await {
        return;
    }
    let Some(owner) = super::call::site_owner().await else {
        tracing::debug!("[Merope] no site owner to bill her own time to");
        return;
    };
    restore_now(&db, owner).await;
    let now = Utc::now();
    let finished = LIFE.lock().ok().and_then(|mut life| {
        if life.now.as_ref().is_some_and(|doing| doing.ends <= now) {
            life.next_at = Some(now + chrono::Duration::minutes(rand::random_range(PAUSE_MINUTES)));
            life.now.take()
        } else {
            None
        }
    });
    // Done lazing: the time goes into the day's.
    let lazed = LIFE.lock().ok().and_then(|mut life| {
        life.lazing
            .as_ref()
            .is_some_and(|lazing| lazing.ends <= now)
            .then(|| life.lazing.take())
            .flatten()
    });
    let changed = lazed.is_some() || finished.is_some();
    if let Some(lazed) = lazed {
        let minutes = lazed
            .ends
            .signed_duration_since(lazed.started)
            .num_minutes();
        super::pace::lazed(&db, minutes.max(0) as f64).await;
    }
    if let Some(done) = finished {
        if tokio::time::timeout(Duration::from_secs(120), finish(&db, owner, done))
            .await
            .is_err()
        {
            tracing::info!("[Merope] writing down what she did ran out of time");
        }
    }
    if changed {
        keep_now(&db).await;
    }
    // After a restart the hour's count comes back from what she wrote down.
    if LIFE.lock().is_ok_and(|life| life.hour.is_none()) {
        let hour = hour_of(now);
        if let Some(start) = DateTime::<Utc>::from_timestamp(hour * 3600, 0)
            && let Ok(done) = unified::own_experiences_since(&db, start.fixed_offset()).await
            && let Ok(mut life) = LIFE.lock()
        {
            life.hour = Some(hour);
            life.this_hour = u32::try_from(done).unwrap_or(PER_HOUR);
        }
    }
    if !free_to_start(now) {
        return;
    }
    // Asleep, she does nothing of her own; she looks again when she wakes.
    if let Some(wakes_in) = super::timing::asleep_now() {
        if let Ok(mut life) = LIFE.lock() {
            life.next_at = Some(now + chrono::Duration::seconds(wakes_in as i64));
        }
        return;
    }
    let Some(hand) = at_hand(&db).await else {
        if let Ok(mut life) = LIFE.lock() {
            life.next_at = Some(now + REST);
        }
        return;
    };
    // Whether she gets up at all is not asked of the model, which says yes
    // to nearly anything: it is an urge, from how she woke up, how far past
    // her usual she is, and whether something she wants pulls her.
    let (pace, spent, lazed) = super::pace::now(&db).await;
    let load = spent / pace.usual_minutes.max(1.0);
    let urge = myriad_merope::pace::urge(pace.tone, load, hand.pulls());
    if rand::random::<f64>() >= urge {
        let minutes = myriad_merope::pace::laze_minutes(pace.tone, rand::random());
        let lazing = Lazing {
            kind: idle_kind(),
            started: now,
            ends: now + chrono::Duration::minutes(minutes),
        };
        tracing::info!(kind = lazing.kind, minutes, urge, "[Merope] lazing about");
        if let Ok(mut life) = LIFE.lock() {
            life.next_at = Some(lazing.ends);
            life.lazing = Some(lazing);
        }
        keep_now(&db).await;
        return;
    }
    let facts = myriad_merope::pace::facts(&pace, spent, lazed);
    let chosen = tokio::time::timeout(
        Duration::from_secs(120),
        choose(&db, owner, hand, &facts, pace.tone),
    )
    .await
    .unwrap_or(Err(Some(UNREACHED_AGAIN)));
    if let Ok(mut life) = LIFE.lock() {
        match chosen {
            Ok(Picked::Doing(doing)) => {
                life.this_hour += 1;
                sources::begin(&db, owner, &doing.thing);
                life.now = Some(doing);
            }
            Ok(Picked::Lazing(kind, length)) => {
                let lazing = Lazing {
                    kind,
                    started: now,
                    ends: now + length,
                };
                life.next_at = Some(lazing.ends);
                life.lazing = Some(lazing);
            }
            // Nothing she wants to do, for as long as she said, or nothing
            // at hand: a while later.
            Err(rest) => life.next_at = Some(now + rest.unwrap_or(REST)),
        }
    }
    keep_now(&db).await;
}

fn free_to_start(now: DateTime<Utc>) -> bool {
    let Ok(mut life) = LIFE.lock() else {
        return false;
    };
    let hour = hour_of(now);
    if life.hour != Some(hour) {
        life.hour = Some(hour);
        life.this_hour = 0;
    }
    life.now.is_none()
        && life.lazing.is_none()
        && life.this_hour < PER_HOUR
        && life.next_at.is_none_or(|at| at <= now)
}

// --- choosing ---------------------------------------------------------------

// --- when she is done -------------------------------------------------------

// --- what she did, read back -------------------------------------------------

use crate::models::entities::agent_memories as unified_row;
pub use myriad_merope::doing::Reaction;
use myriad_merope::doing::{
    CHOICE_SCHEMA, Choice, DIGEST_SCHEMA, PER_HOUR, REST_MINUTES, ago_text, choice_schema,
    choice_system, digest_schema, digest_system, hour_of, read_digest,
};

pub fn player_line(doing: &Doing, music: Option<&Value>) -> Option<&'static str> {
    sources::song::player_line(&doing.thing, music)
}

/// What she is in the middle of, for a prompt: what it is, how far in, and
/// why she picked it.
pub fn now_line(doing: &Doing, at: DateTime<Utc>) -> String {
    let done = at.signed_duration_since(doing.started).num_minutes().max(0);
    let total = doing
        .ends
        .signed_duration_since(doing.started)
        .num_minutes()
        .max(1);
    let why = if doing.why.trim().is_empty() {
        String::new()
    } else {
        format!(" You picked it: {}.", doing.why.trim())
    };
    let seconds_in = at.signed_duration_since(doing.started).num_milliseconds() as f32 / 1000.0;
    format!(
        "You are {} {}, about {done} of {total} minutes in.{}{why}",
        doing.thing.verb(),
        doing.thing.describe(),
        sources::so_far(&doing.thing, seconds_in)
    )
}

// --- model calls -------------------------------------------------------------

async fn soul() -> String {
    crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default()
}

/// The two calls as production sends them, for the semantic suite.
#[cfg(test)]
pub(crate) fn choice_probe_contract(soul: &str, options: usize) -> (String, Value) {
    (choice_system(soul), choice_schema(options))
}

#[cfg(test)]
pub(crate) fn digest_probe_contract(
    soul: &str,
    what: &str,
    why: &str,
    material: Option<&str>,
) -> (String, Value) {
    let intake = sources::probe_intake(what, material);
    (
        digest_system(soul, what, why, &intake.how),
        digest_schema(&intake.asks),
    )
}

#[cfg(test)]
pub(crate) fn digest_probe_input(
    material: Option<&str>,
    views: &[String],
    before: &[String],
    guessed: Option<&str>,
) -> String {
    let alongside: Vec<(String, String)> = guessed
        .map(|guess| {
            (
                "What you guessed after the last part".to_string(),
                guess.to_string(),
            )
        })
        .into_iter()
        .collect();
    digest_input(material, 12_000, views, before, &alongside)
}

/// Whether her note honors the contract for what she did (`what`), with
/// the fields its kind asks for.
#[cfg(test)]
pub(crate) fn parse_digest(raw: &str, what: &str, material: Option<&str>) -> bool {
    let asks = sources::probe_intake(what, material).asks;
    read_digest(raw, &asks).is_some_and(|(digest, wrote)| {
        !digest.impression.trim().is_empty()
            && asks.iter().all(|(field, _)| wrote.contains_key(*field))
    })
}

#[cfg(test)]
pub(crate) fn parse_choice(raw: &str) -> Option<Option<usize>> {
    call::parse::<Choice>(raw).map(|choice| choice.choice)
}

#[cfg(test)]
#[path = "doing_tests.rs"]
mod tests;

#[cfg(test)]
mod live {
    /// Offered lazing about beside what is at hand, with her pace fresh and
    /// worn out, how often she picks it: read only, MEROPE_RUNS (default 8).
    #[tokio::test]
    #[ignore = "reads the site's database and asks its model"]
    async fn does_she_laze_when_she_may() {
        let db = crate::services::agent::semantic_eval::load_configured_lite().await;
        crate::services::process_db::set_process_database(db.clone());
        let hand = super::at_hand(&db).await.expect("things at hand");
        let options = hand.options.len();
        let runs: usize = std::env::var("MEROPE_RUNS")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(8);
        let day = chrono::Local::now().date_naive();
        let fresh = myriad_merope::pace::Pace {
            day,
            usual_minutes: 240.0,
            days_past_usual: 0,
            tone: myriad_merope::pace::Tone::Even,
        };
        let worn = myriad_merope::pace::Pace {
            days_past_usual: 3,
            tone: myriad_merope::pace::Tone::Flat,
            ..fresh.clone()
        };
        let soul = super::soul().await;
        for (label, pace, spent) in [("fresh", &fresh, 120.0), ("worn", &worn, 420.0)] {
            let facts = myriad_merope::pace::facts(pace, spent, 30.0);
            let input = super::choice_input(&db, &hand, &facts).await;
            let (mut doing, mut lazing, mut nothing) = (0, 0, 0);
            let mut whys = Vec::new();
            for _ in 0..runs {
                let choice: Option<super::Choice> =
                    super::call::Ask::new(super::Voice::Judge, 1, "doing_choice")
                        .within(std::time::Duration::from_secs(60))
                        .json(
                            &super::choice_system(&soul),
                            &input,
                            super::CHOICE_SCHEMA,
                            &super::choice_schema(options + super::super::pace::LAZING.len()),
                        )
                        .await
                        .ok();
                match choice.as_ref().and_then(|choice| choice.choice) {
                    Some(index) if index >= options => lazing += 1,
                    Some(_) => doing += 1,
                    None => nothing += 1,
                }
                if let Some(why) = choice.and_then(|choice| choice.why) {
                    whys.push(why);
                }
            }
            println!("{label}: doing {doing}, lazing {lazing}, null {nothing}");
            for why in whys.iter().take(4) {
                println!("   {why}");
            }
        }
    }

    /// How she takes the same few things with the persona as it is, and as
    /// kept in MEROPE_SOUL_BEFORE (a JSON file with `personality` and
    /// `persona_json`), a few times each, printed: read only.
    #[tokio::test]
    #[ignore = "reads the site's persona and asks its model"]
    async fn the_same_things_by_two_personas() {
        let db = crate::services::agent::semantic_eval::load_configured_lite().await;
        let now = super::super::store::get_persona(&db)
            .await
            .unwrap()
            .unwrap();
        let mut souls = vec![(
            "now",
            super::super::speaking_prompts::format_persona(&now).unwrap(),
        )];
        if let Ok(path) = std::env::var("MEROPE_SOUL_BEFORE") {
            let kept: serde_json::Value =
                serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
            let mut before = now.clone();
            before.personality = kept["personality"].as_str().unwrap().to_string();
            before.persona_json = Some(kept["persona_json"].clone());
            souls.insert(
                0,
                (
                    "before",
                    super::super::speaking_prompts::format_persona(&before).unwrap(),
                ),
            );
        }
        let things = [
            (
                "listening to the song 「纸飞机」 by 小岛乐队",
                "把旧车票折成纸飞机\n从六楼往下放\n风把它带过了晾衣绳\n我在窗口数到三\n它没回来 也好\n反正我也没想去哪",
            ),
            (
                "listening to the song 「袜子去哪了」 by 午后猫",
                "洗衣机吃掉了我的左袜子\n右袜子一个人很寂寞\n我给它画了一张寻人启事\n贴在冰箱上 猫看了一眼\n猫知道 猫不说",
            ),
            (
                "reading part 3 of 「Treasure Island」 by Robert Louis Stevenson",
                "\"Pieces of eight! pieces of eight!\" cried the parrot. Long John Silver laughed and fed it a crumb. \"She's two hundred years old, Hawkins — they live for ever mostly; and if anybody's seen more wickedness, it must be the devil himself.\"",
            ),
        ];
        let runs: usize = std::env::var("MEROPE_RUNS")
            .ok()
            .and_then(|n| n.parse().ok())
            .unwrap_or(3);
        for (label, soul) in &souls {
            for (what, material) in &things {
                for _ in 0..runs {
                    let (system, schema) =
                        super::digest_probe_contract(soul, what, "", Some(material));
                    let input = super::digest_probe_input(Some(material), &[], &[], None);
                    let raw = super::call::Ask::new(super::call::Voice::Hers, 1, "doing_digest")
                        .within(std::time::Duration::from_secs(60))
                        .json_raw(&system, &input, super::DIGEST_SCHEMA, &schema)
                        .await
                        .unwrap_or_default();
                    let value: serde_json::Value = serde_json::from_str(&raw).unwrap_or_default();
                    println!(
                        "{label}\t{}\t{}\t{}",
                        what.chars().take(24).collect::<String>(),
                        value["reaction"].as_str().unwrap_or("?"),
                        value["impression"]
                            .as_str()
                            .unwrap_or(&raw)
                            .replace('\n', " ")
                    );
                }
            }
        }
    }
}
