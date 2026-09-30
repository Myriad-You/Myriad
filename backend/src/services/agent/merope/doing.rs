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

pub const DOING_EVENT: &str = "agent.merope.doing";
const PAUSE_MINUTES: std::ops::Range<i64> = 3..12;
/// When she would rather do nothing, how long before she thinks about it
/// again is hers to say, within these; otherwise `REST`.
const REST: chrono::Duration = chrono::Duration::minutes(30);
/// When the model could not be reached to choose, she did not choose to
/// rest: she comes back to it soon.
const UNREACHED_AGAIN: chrono::Duration = chrono::Duration::minutes(5);
/// She brings something up to the same person at most this often.
const TELL_EVERY: Duration = Duration::from_secs(45 * 60);
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

/// Where what she is in the middle of is kept, so a restart does not wipe
/// it: a book half read is still half read.
pub const PRESENT_NAMESPACE: &str = "merope_present";
const PRESENT: &str = "now";
/// Something that ended while she was not running is still written down if
/// it ended at most this long ago; older, it has gone by.
const STILL_FRESH: chrono::Duration = chrono::Duration::hours(2);
static RESTORED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[derive(Debug, Default, Serialize, Deserialize)]
struct KeptNow {
    doing: Option<KeptDoing>,
    lazing: Option<KeptLazing>,
}

#[derive(Debug, Serialize, Deserialize)]
struct KeptDoing {
    thing: Thing,
    started: DateTime<Utc>,
    ends: DateTime<Utc>,
    why: String,
}

#[derive(Debug, Serialize, Deserialize)]
struct KeptLazing {
    kind: String,
    started: DateTime<Utc>,
    ends: DateTime<Utc>,
}

fn present_identity() -> crate::services::runtime_registry::RegistryIdentity<'static> {
    crate::services::runtime_registry::RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    }
}

/// Keep what she is in the middle of now.
async fn keep_now(db: &DatabaseConnection) {
    let kept = LIFE.lock().ok().map(|life| KeptNow {
        doing: life.now.as_ref().map(|doing| KeptDoing {
            thing: doing.thing.clone(),
            started: doing.started,
            ends: doing.ends,
            why: doing.why.clone(),
        }),
        lazing: life.lazing.as_ref().map(|lazing| KeptLazing {
            kind: lazing.kind.to_string(),
            started: lazing.started,
            ends: lazing.ends,
        }),
    });
    let Some(kept) = kept else {
        return;
    };
    let keep_until = (Utc::now() + chrono::Duration::days(2)).timestamp();
    if let Err(error) = crate::services::runtime_registry::put(
        db,
        PRESENT_NAMESPACE,
        PRESENT,
        present_identity(),
        &kept,
        keep_until,
    )
    .await
    {
        tracing::warn!(%error, "[Merope] could not keep what she is in the middle of");
    }
}

/// After a restart, once: pick up what she was in the middle of. What is
/// still going goes on; what ended meanwhile is written down if lately,
/// and lazing is counted.
async fn restore_now(db: &DatabaseConnection, owner: i32) {
    if RESTORED.swap(true, std::sync::atomic::Ordering::Relaxed) {
        return;
    }
    let kept =
        match crate::services::runtime_registry::get::<KeptNow>(db, PRESENT_NAMESPACE, PRESENT)
            .await
        {
            Ok(kept) => kept.unwrap_or_default(),
            Err(error) => {
                RESTORED.store(false, std::sync::atomic::Ordering::Relaxed);
                tracing::warn!(%error, "[Merope] could not read what she was in the middle of");
                return;
            }
        };
    let now = Utc::now();
    if let Some(kept) = kept.doing {
        let doing = Doing {
            thing: kept.thing,
            started: kept.started,
            ends: kept.ends,
            why: kept.why,
        };
        if doing.ends > now {
            tracing::info!(kind = %doing.thing.key(), "[Merope] back to what she was in the middle of");
            if let Ok(mut life) = LIFE.lock() {
                life.now = Some(doing);
            }
        } else if now - doing.ends <= STILL_FRESH {
            tracing::info!(kind = %doing.thing.key(), "[Merope] writing down what she finished meanwhile");
            let _ = tokio::time::timeout(Duration::from_secs(120), finish(db, owner, doing)).await;
        }
    }
    if let Some(kept) = kept.lazing {
        let kind = super::pace::LAZING
            .iter()
            .find(|(kind, _)| *kind == kept.kind)
            .map_or(super::pace::LAZING[0].0, |(kind, _)| kind);
        if kept.ends > now {
            if let Ok(mut life) = LIFE.lock() {
                life.next_at = Some(kept.ends);
                life.lazing = Some(Lazing {
                    kind,
                    started: kept.started,
                    ends: kept.ends,
                });
            }
        } else {
            let minutes = kept.ends.signed_duration_since(kept.started).num_minutes();
            super::pace::lazed(db, minutes.max(0) as f64).await;
        }
    }
    keep_now(db).await;
}

/// A new persona is in the middle of nothing.
pub async fn forget_kept<C: sea_orm::ConnectionTrait>(db: &C) -> Result<u64, sea_orm::DbErr> {
    crate::services::runtime_registry::delete_matching(
        db,
        PRESENT_NAMESPACE,
        None,
        None,
        None,
        None,
    )
    .await
}

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

/// What is at hand for a free moment, and which of it works toward
/// something she wants on her own.
struct AtHand {
    lately: Vec<unified_row::Model>,
    options: Vec<Thing>,
    advances: Vec<Option<String>>,
    taste: myriad_merope::taste::Taste,
}

impl AtHand {
    /// Something she wants pulls her toward what is at hand.
    fn pulls(&self) -> bool {
        self.advances.iter().any(Option::is_some)
    }
}

async fn at_hand(db: &DatabaseConnection) -> Option<AtHand> {
    let lately = match unified::own_experiences(db, 300).await {
        Ok(lately) => lately,
        Err(error) => {
            tracing::warn!(%error, "[Merope] could not read what she did lately");
            return None;
        }
    };
    let taste = taste(db).await;
    let options = sources::options(db, &taste).await;
    if options.is_empty() {
        tracing::info!("[Merope] nothing at hand for her own time");
        return None;
    }
    let wants: Vec<String> = super::wants::open(db)
        .await
        .into_iter()
        .filter(|want| want.reach == myriad_merope::wants::Reach::OnYourOwn && !want.longing)
        .map(|want| want.want)
        .collect();
    let advances = options
        .iter()
        .map(|thing| {
            wants
                .iter()
                .find(|want| myriad_merope::pace::advances(want, thing.title(), thing.by()))
                .cloned()
        })
        .collect();
    Some(AtHand {
        lately,
        options,
        advances,
        taste,
    })
}

/// Whose things keep getting to her lately, a line each, at most `most`.
pub async fn keeps_getting_to_her(db: &DatabaseConnection, most: usize) -> Vec<String> {
    taste(db).await.liked_by(most)
}

/// Her taste as the site's owner looks into her: whose things keep getting
/// to her and whose keep not being for her.
pub(super) async fn taste_view(db: &DatabaseConnection) -> Value {
    let taste = taste(db).await;
    json!({ "likedBy": taste.liked(5), "notForHer": taste.not_for_her(3) })
}

/// How far back her reactions make up her taste: faded experiences still
/// count until they are purged.
const TASTE_DAYS: i64 = 120;
const TASTE_ROWS: u64 = 4000;

/// Her taste, from how what she did landed with her (see
/// `myriad_merope::taste`). Empty when it cannot be read.
async fn taste(db: &DatabaseConnection) -> myriad_merope::taste::Taste {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    #[derive(Deserialize)]
    struct Row {
        thing: Thing,
        #[serde(default)]
        reaction: Option<Reaction>,
    }
    let since = Utc::now() - chrono::Duration::days(TASTE_DAYS);
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT created_at, jsonb_build_object('thing', e->'thing', 'reaction', e->'reaction')::text AS taken \
             FROM (SELECT created_at, evidence::jsonb AS e FROM agent_memories \
               WHERE user_id IS NULL AND venue = $1 AND source = $2 AND created_at >= $3 \
                 AND evidence IS JSON) own \
             ORDER BY created_at DESC LIMIT $4",
            [
                unified::OWN_VENUE.into(),
                unified::OWN_EXPERIENCE.into(),
                since.fixed_offset().into(),
                (TASTE_ROWS as i64).into(),
            ],
        ))
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "[Merope] could not read her taste");
            Vec::new()
        });
    let now = Utc::now();
    let read: Vec<(Row, f64)> = rows
        .iter()
        .filter_map(|row| {
            let at: DateTime<chrono::FixedOffset> = row.try_get("", "created_at").ok()?;
            let taken: String = row.try_get("", "taken").ok()?;
            let days = now.signed_duration_since(at).num_minutes() as f64 / 1440.0;
            Some((serde_json::from_str::<Row>(&taken).ok()?, days))
        })
        .collect();
    let taken: Vec<myriad_merope::taste::Taken> = read
        .iter()
        .map(|(row, days_ago)| myriad_merope::taste::Taken {
            thing: &row.thing,
            reaction: row.reaction,
            days_ago: *days_ago,
        })
        .collect();
    myriad_merope::taste::Taste::of(&taken)
}

/// What she chooses from, as the model reads it.
async fn choice_input(db: &DatabaseConnection, hand: &AtHand, pace: &[String]) -> String {
    let AtHand {
        lately,
        options,
        advances,
        taste,
    } = hand;
    let myself = super::self_state::current(db).await.facts_view();
    let now = Utc::now();
    let lately_view: Vec<Value> = lately
        .iter()
        .take(8)
        .filter_map(|row| {
            let experience = Experience::of(row)?;
            let ago = now.signed_duration_since(row.created_at.with_timezone(&Utc));
            Some(json!(format!(
                "{}, {}",
                experience.line_felt(),
                ago_text(ago)
            )))
        })
        .collect();
    let mut option_views = Vec::with_capacity(options.len() + super::pace::LAZING.len());
    for (index, thing) in options.iter().enumerate() {
        let mut view = sources::view(db, index, thing).await;
        if let Some(want) = &advances[index] {
            view["advances"] = json!(want);
        }
        // What she had before, she knows she had, and how it went.
        if let Some((reaction, days_ago)) = taste.last_time(thing) {
            let ago = ago_text(chrono::Duration::minutes((days_ago * 1440.0) as i64));
            view["hadBefore"] = json!(match reaction {
                Some(reaction) => format!("{}, {ago}", reaction.felt()),
                None => format!("nothing of it reached you, {ago}"),
            });
        }
        option_views.push(view);
    }
    for (offset, (_, what)) in super::pace::LAZING.iter().enumerate() {
        option_views.push(json!({
            "index": options.len() + offset,
            "kind": "lazing",
            "what": what,
        }));
    }
    let kinds: Vec<(&str, chrono::Duration)> = lately
        .iter()
        .filter_map(|row| {
            let experience = Experience::of(row)?;
            Some((
                experience.thing.kind(),
                now.signed_duration_since(row.created_at.with_timezone(&Utc)),
            ))
        })
        .collect();
    json!({
        "myself": myself,
        "lately": lately_view,
        "yourPace": pace,
        "sameThingLately": myriad_merope::doing::same_run(&kinds),
        "keepsGettingToYou": taste.liked_by(3),
        "yourViews": super::views::held(db, 5).await,
        "yourWants": super::wants::lines(&super::wants::open(db).await),
        "whoYouHaveBeen": super::self_story::current(db).await,
        "options": option_views,
    })
    .to_string()
}

/// What she took up at a free moment.
enum Picked {
    Doing(Doing),
    /// A way of lazing about (its kind), for this long.
    Lazing(&'static str, chrono::Duration),
}

/// What she picked, or how long she would rather leave it (none when she
/// did not say or could not choose). `pace` is her pace as she knows it.
async fn choose(
    db: &DatabaseConnection,
    owner: i32,
    hand: AtHand,
    pace: &[String],
    tone: myriad_merope::pace::Tone,
) -> Result<Picked, Option<chrono::Duration>> {
    let input = choice_input(db, &hand, pace).await;
    let soul = soul().await;
    let AtHand { options, .. } = hand;
    let choice: Option<Choice> = call::Ask::new(Voice::Judge, owner, "doing_choice")
        .within(CALL_TIMEOUT)
        .json(
            &choice_system(&soul),
            &input,
            CHOICE_SCHEMA,
            &choice_schema(options.len() + super::pace::LAZING.len()),
        )
        .await
        .ok();
    let Some(choice) = choice else {
        tracing::info!("[Merope] could not decide what to do on her own");
        return Err(Some(UNREACHED_AGAIN));
    };
    // A way of lazing about, picked like anything else.
    if let Some((kind, _)) = choice
        .choice
        .and_then(|index| index.checked_sub(options.len()))
        .and_then(|index| super::pace::LAZING.get(index))
    {
        let minutes = choice
            .rest_minutes
            .unwrap_or_else(|| myriad_merope::pace::laze_minutes(tone, rand::random()))
            .clamp(*REST_MINUTES.start(), *REST_MINUTES.end());
        tracing::info!(kind, minutes, "[Merope] chose to laze about");
        return Ok(Picked::Lazing(kind, chrono::Duration::minutes(minutes)));
    }
    let Some(thing) = choice.choice.and_then(|index| options.get(index)).cloned() else {
        let rest = choice.rest_minutes.map(|minutes| {
            chrono::Duration::minutes(minutes.clamp(*REST_MINUTES.start(), *REST_MINUTES.end()))
        });
        tracing::info!(
            rest_minutes = rest.map(|rest| rest.num_minutes()),
            "[Merope] chose to do nothing for a while"
        );
        return Err(rest);
    };
    // A book off the shelf is opened only now.
    let Some(thing) = sources::open(db, thing).await else {
        return Err(None);
    };
    let started = Utc::now();
    let length = sources::length(db, &thing).await;
    tracing::info!(kind = %thing.key(), "[Merope] doing something of her own");
    Ok(Picked::Doing(Doing {
        ends: started + length,
        started,
        why: choice.why.unwrap_or_default().chars().take(80).collect(),
        thing,
    }))
}

// --- when she is done -------------------------------------------------------

async fn finish(db: &DatabaseConnection, owner: i32, done: Doing) {
    let Some(intake) = sources::intake(db, owner, &done.thing).await else {
        return;
    };
    let views = own_views_on(db, &done.thing).await;
    let before = notes_before(db, &done.thing).await;
    // What she has been quietly hoping for is there when something reaches
    // her, not when she picks: it colours how things land.
    let mut alongside = intake.alongside.clone();
    if let Some(longings) = myriad_merope::wants::undercurrent(&super::wants::open(db).await) {
        alongside.push((
            "Underneath lately, not something you set out for, you have been hoping".to_string(),
            longings,
        ));
    }
    let input = digest_input(
        intake.material.as_deref(),
        intake.limit,
        &views,
        &before,
        &alongside,
    );
    let soul = soul().await;
    let what = format!("{} {}", done.thing.verb(), done.thing.describe());
    let system = digest_system(&soul, &what, &done.why, &intake.how);
    let schema = digest_schema(&intake.asks);
    let ask = || {
        call::Ask::new(Voice::Hers, owner, "doing_digest")
            .within(CALL_TIMEOUT)
            .json_raw(&system, &input, DIGEST_SCHEMA, &schema)
    };
    // What she spent the while on is not lost to one bad call.
    let raw = match ask().await {
        Err(failure) if failure.retryable() => {
            tokio::time::sleep(DIGEST_AGAIN_AFTER).await;
            ask().await
        }
        raw => raw,
    };
    let Ok(raw) = raw else {
        return;
    };
    let Some((digest, wrote)) = read_digest(&raw, &intake.asks) else {
        tracing::warn!(kind = %done.thing.key(), "[Merope] what stayed with her came back unreadable; not kept");
        return;
    };
    let impression = super::ingest::compact_summary(&digest.impression);
    if impression.is_empty() {
        return;
    }
    let reached = intake.reached;
    let kept = sources::after(db, owner, &done.thing, &wrote, intake.carry).await;
    let evidence = Experience {
        key: done.thing.key(),
        thing: done.thing.clone(),
        // Nothing reached her: no taste to keep.
        reaction: reached.then_some(digest.reaction),
        tell: digest.tell,
        kept,
    };
    match unified::remember_own(
        db,
        &impression,
        &serde_json::to_string(&evidence).unwrap_or_default(),
        digest.concepts,
        unified::OWN_EXPERIENCE,
    )
    .await
    {
        Ok(Some(_)) => {}
        Ok(None) => return,
        Err(error) => {
            tracing::warn!(%error, kind = %done.thing.key(), "[Merope] could not keep what she did");
            return;
        }
    }
    if digest.tell {
        tell_whoever_is_here(&done.thing, &impression);
        // The groups she is in may hear it too, if one is where she would
        // say it.
        let what = evidence
            .noted(&impression)
            .trim_start_matches("- ")
            .to_string();
        super::background::spawn("share_first", async move {
            crate::services::channel_group::share_first(owner, what).await;
        });
    }
}

/// What she takes it in with: the material, what its kind shows beside it,
/// her views that touch it, and what she wrote before.
fn digest_input(
    material: Option<&str>,
    limit: usize,
    views: &[String],
    before: &[String],
    alongside: &[(String, String)],
) -> String {
    let mut input = match material {
        Some(text) if !text.trim().is_empty() => myriad_agent_rules::untrusted_block(
            "material",
            &text.chars().take(limit).collect::<String>(),
        ),
        _ => "(no material)".to_string(),
    };
    for (heading, text) in alongside {
        input.push_str(&format!("\n\n{heading}:\n"));
        input.push_str(&myriad_agent_rules::untrusted_block("alongside", text));
    }
    if !views.is_empty() {
        input.push_str("\n\nYour views:\n");
        input.push_str(&myriad_agent_rules::untrusted_block(
            "your_views",
            &views.join("\n"),
        ));
    }
    if !before.is_empty() {
        input.push_str("\n\nWhat you wrote when you had this same one before:\n");
        input.push_str(&myriad_agent_rules::untrusted_block(
            "this_one_before",
            &before.join("\n"),
        ));
    }
    input
}

/// Her views that touch this thing. Only those: a view she brings to
/// everything becomes the words she says about everything.
async fn own_views_on(db: &DatabaseConnection, thing: &Thing) -> Vec<String> {
    let words = format!("{} {}", thing.title(), thing.by().unwrap_or_default());
    super::views::touched(db, &words, 3)
        .await
        .into_iter()
        .map(|(about, view)| format!("{about}: {view}"))
        .collect()
}

/// What she wrote the times she had this same thing before: her memory of
/// it, most recent first. Not her notes on other things: shown those, she
/// writes them again.
async fn notes_before(db: &DatabaseConnection, thing: &Thing) -> Vec<String> {
    const BEFORE: usize = 2;
    let key = thing.key();
    let now = Utc::now();
    unified::own_experiences(db, 300)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| Some((Experience::of(row)?, row)))
        .filter(|(experience, _)| experience.key == key)
        .take(BEFORE)
        .map(|(experience, row)| {
            format!(
                "{} ({})",
                experience.noted(&row.content),
                ago(now, row.created_at.with_timezone(&Utc))
            )
        })
        .collect()
}

/// Someone who can see her may hear about it; the decision is hers, live.
fn tell_whoever_is_here(thing: &Thing, impression: &str) {
    let summary = format!(
        "你刚自己{}{}：{impression}",
        thing.done_verb(),
        thing.title()
    );
    let people: Vec<i32> = {
        let Ok(mut life) = LIFE.lock() else {
            return;
        };
        life.told.retain(|_, at| at.elapsed() < TELL_EVERY);
        let people: Vec<i32> = crate::services::agent::consciousness::present_users()
            .into_iter()
            .filter(|user_id| *user_id > 0 && !life.told.contains_key(user_id))
            .collect();
        for user_id in &people {
            life.told.insert(*user_id, Instant::now());
        }
        people
    };
    for user_id in people {
        super::spawn_ingest(user_id, DOING_EVENT, summary.clone());
    }
}

// --- what she did, read back -------------------------------------------------

use crate::models::entities::agent_memories as unified_row;
pub use myriad_merope::doing::Reaction;
use myriad_merope::doing::{
    CHOICE_SCHEMA, Choice, DIGEST_SCHEMA, PER_HOUR, REST_MINUTES, ago_text, choice_schema,
    choice_system, digest_schema, digest_system, hour_of, read_digest,
};

/// A thing she did and when, read back from her memory.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Experience {
    key: String,
    thing: Thing,
    /// How it landed with her.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    reaction: Option<Reaction>,
    /// She would want to tell someone about it.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    tell: bool,
    /// What only its kind keeps (see `sources`).
    #[serde(flatten)]
    kept: Kept,
}

/// The key of what a row of her own experience was, and the thing.
pub(super) fn key_of(row: &unified_row::Model) -> Option<(String, Thing)> {
    Experience::of(row).map(|experience| (experience.key, experience.thing))
}

/// What a row of her own experience was and how it landed, as a line
/// ("listening to … (you liked it)"): her views grow out of these.
pub(super) fn experience_line(row: &unified_row::Model) -> Option<String> {
    Experience::of(row).map(|experience| experience.line_felt())
}

/// A row of her own experience for looking back: the line with how it
/// landed and what she wrote, and whether it did not go well (only fine,
/// not for her, a guess that did not hold, a question left unanswered).
pub(super) fn experience_record(row: &unified_row::Model) -> Option<(String, bool)> {
    let experience = Experience::of(row)?;
    let (more, went_wrong) = experience.kept.looking_back();
    let missed = went_wrong
        || matches!(
            experience.reaction,
            Some(Reaction::Fine | Reaction::NotForMe)
        );
    Some((
        format!("{}: {}{more}", experience.line_felt(), row.content),
        missed,
    ))
}

impl Experience {
    fn of(row: &unified_row::Model) -> Option<Self> {
        serde_json::from_str(row.evidence.as_deref()?).ok()
    }

    fn line(&self) -> String {
        format!("{} {}", self.thing.verb(), self.thing.describe())
    }

    /// The line with how it landed, when she said.
    fn line_felt(&self) -> String {
        match self.reaction {
            Some(reaction) => format!("{} ({})", self.line(), reaction.felt()),
            None => self.line(),
        }
    }

    /// "「晴天」 by 周杰伦 (you liked it): what she wrote".
    fn noted(&self, content: &str) -> String {
        format!("- {}: {content}", self.line_felt())
    }
}

/// What she did lately, and older things their words touch, for a prompt:
/// (what it was and when, what stayed with her), most recent first.
pub async fn recalled(
    db: &DatabaseConnection,
    words: Option<&str>,
    recent: usize,
    related: usize,
) -> Vec<(String, String)> {
    let Ok(rows) = unified::own_experiences(db, 120).await else {
        return Vec::new();
    };
    let now = Utc::now();
    let mut picked: Vec<usize> = rows
        .iter()
        .enumerate()
        .filter(|(_, row)| now.signed_duration_since(row.created_at) < chrono::Duration::hours(24))
        .map(|(index, _)| index)
        .take(recent)
        .collect();
    if let Some(words) = words.filter(|words| !words.trim().is_empty()) {
        let concepts: Vec<Vec<Concept>> = rows
            .iter()
            .map(|row| serde_json::from_value(row.concepts.clone()).unwrap_or_default())
            .collect();
        let texts: Vec<String> = rows
            .iter()
            .map(|row| {
                let what = Experience::of(row)
                    .map(|experience| experience.thing.describe())
                    .unwrap_or_default();
                format!("{what} {}", row.content)
            })
            .collect();
        let documents: Vec<crate::services::agent::memory::lexical::Document> = texts
            .iter()
            .zip(&concepts)
            .map(
                |(text, concepts)| crate::services::agent::memory::lexical::Document {
                    text,
                    concepts,
                },
            )
            .collect();
        let scores = crate::services::agent::memory::lexical::score_all(words, &documents);
        let mut touched: Vec<(usize, f64)> = scores
            .iter()
            .enumerate()
            .filter(|(index, score)| score.strong && !picked.contains(index))
            .map(|(index, score)| (index, score.value))
            .collect();
        touched.sort_by(|a, b| b.1.total_cmp(&a.1));
        picked.extend(touched.into_iter().take(related).map(|(index, _)| index));
    }
    picked
        .into_iter()
        .filter_map(|index| {
            let row = &rows[index];
            let experience = Experience::of(row)?;
            // What she heard in it is in what she wrote, as a listener says
            // it; the measurements it came from are not talk.
            Some((
                format!(
                    "{} ({})",
                    experience.line_felt(),
                    ago(now, row.created_at.with_timezone(&Utc))
                ),
                row.content.clone(),
            ))
        })
        .collect()
}

/// What she did on her own between `start` and `end`, oldest first, each with
/// what stayed with her: for her diary.
pub async fn during(
    db: &DatabaseConnection,
    start: DateTime<chrono::FixedOffset>,
    end: DateTime<chrono::FixedOffset>,
    limit: usize,
) -> Vec<String> {
    let mut lines: Vec<String> = unified::own_experiences(db, 120)
        .await
        .unwrap_or_default()
        .iter()
        .filter(|row| row.created_at >= start && row.created_at < end)
        .filter_map(|row| {
            Some(format!(
                "{}: {}",
                Experience::of(row)?.line_felt(),
                row.content
            ))
        })
        .take(limit)
        .collect();
    lines.reverse();
    lines
}

fn ago(now: DateTime<Utc>, at: DateTime<Utc>) -> String {
    let minutes = now.signed_duration_since(at).num_minutes().max(0);
    match minutes {
        0..=9 => "just now".into(),
        10..=89 => format!("{minutes} minutes ago"),
        90..=1439 => format!("{} hours ago", minutes / 60),
        1440..=2879 => "yesterday".into(),
        _ => format!("{} days ago", minutes / 1440),
    }
}

/// For the player section of a private chat: whether they are already
/// listening with her, or how she can put her song on for them.
/// What she did on her own after `since` (within the last day) that she
/// would want to tell someone, most recent first: what it was, how it
/// landed and what she wrote.
pub async fn would_tell(db: &DatabaseConnection, since: Option<DateTime<Utc>>) -> Vec<String> {
    const WITHIN: chrono::Duration = chrono::Duration::hours(24);
    const AT_MOST: usize = 3;
    let now = Utc::now();
    unified::own_experiences(db, 60)
        .await
        .unwrap_or_default()
        .iter()
        .filter(|row| {
            let at = row.created_at.with_timezone(&Utc);
            now.signed_duration_since(at) < WITHIN && since.is_none_or(|since| at > since)
        })
        .filter_map(|row| Some((Experience::of(row)?, row)))
        .filter(|(experience, _)| experience.tell)
        .take(AT_MOST)
        .map(|(experience, row)| format!("{}: {}", experience.line_felt(), row.content))
        .collect()
}

/// Songs she could play for someone: ones she listened to on her own
/// lately and liked, most recent first, each with what she wrote then.
/// What she says about a song she plays comes from here, not from nowhere.
pub async fn songs_to_share(db: &DatabaseConnection) -> Vec<(Thing, String)> {
    const WITHIN: chrono::Duration = chrono::Duration::days(14);
    const AT_MOST: usize = 8;
    let now = Utc::now();
    let rows = unified::own_experiences(db, 300).await.unwrap_or_default();
    let mut seen = std::collections::HashSet::new();
    rows.iter()
        .filter(|row| now.signed_duration_since(row.created_at) < WITHIN)
        .filter_map(|row| Some((Experience::of(row)?, row)))
        .filter(|(experience, _)| {
            matches!(experience.thing, Thing::Song { .. })
                && matches!(experience.reaction, Some(Reaction::Liked | Reaction::Moved))
        })
        .filter(|(experience, _)| seen.insert(experience.key.clone()))
        .take(AT_MOST)
        .map(|(experience, row)| {
            let line = format!(
                "{} {}, {}: {}",
                experience.thing.describe(),
                experience
                    .reaction
                    .map(|reaction| format!("({})", reaction.felt()))
                    .unwrap_or_default(),
                ago_text(now.signed_duration_since(row.created_at.with_timezone(&Utc))),
                gist(&row.content)
            );
            (experience.thing, line)
        })
        .collect()
}

/// The start of what she wrote, enough to know which song it was to her:
/// the whole of it is in her own time when that comes to mind.
fn gist(note: &str) -> String {
    const GIST_CHARS: usize = 40;
    let mut gist: String = note.chars().take(GIST_CHARS).collect();
    if note.chars().count() > GIST_CHARS {
        gist.push('…');
    }
    gist
}

/// The songs last offered in each conversation, so the number she picks
/// is the song she saw, even if she liked another one since.
static OFFERED: LazyLock<Mutex<HashMap<(i32, String), Vec<Thing>>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn offer_songs(user_id: i32, session_id: &str, songs: Vec<Thing>) {
    if let Ok(mut offered) = OFFERED.lock() {
        if offered.len() > 1024 {
            offered.clear();
        }
        offered.insert((user_id, session_id.to_string()), songs);
    }
}

/// The song numbered `number` (from 1) in what was offered in this
/// conversation.
pub fn offered_song(user_id: i32, session_id: &str, number: u8) -> Option<Thing> {
    let offered = OFFERED.lock().ok()?;
    offered
        .get(&(user_id, session_id.to_string()))?
        .get(usize::from(number).checked_sub(1)?)
        .cloned()
}

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
