//! Sending, thinly: one line at a time to one person or group, and when
//! several of her lines are waiting for the same one, which goes is hers to
//! choose (`myriad_merope::speech::choice`). Which way out (her face, the
//! app, a notification, a chat app) stays with each path; it is named in
//! the ledger row it writes.
//!
//! A path writes its line, then takes its turn ([`turn`]) with it; holding
//! the turn it checks the line again (admission with the words in it) and
//! sends it, and the turn passes on when it is dropped. Lines that came up
//! while another was going out wait as hands; when the turn frees and more
//! than one is up, one call picks the one she says now, and the rest are
//! let go. Nothing here is kept across a restart: a line that was waiting
//! is not said.

use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock, Mutex};

use myriad_agent_rules::memory::audience::{Audience, Venue};
use myriad_merope::speech::Source;
use myriad_merope::speech::choice::{self, Hand, Held};
use tokio::sync::{Mutex as AsyncMutex, OwnedMutexGuard};

/// Audiences kept at most; past this, idle ones are let go.
const MOST: usize = 4096;

#[derive(Default)]
struct Waiting {
    /// Hands up, in the order they went up.
    hands: Vec<Hand>,
    /// What she chose for hands that were up together: said or let go.
    chosen: HashMap<String, bool>,
}

struct Slot {
    turn: Arc<AsyncMutex<()>>,
    waiting: Arc<Mutex<Waiting>>,
}

static SLOTS: LazyLock<Mutex<HashMap<String, Slot>>> = LazyLock::new(|| Mutex::new(HashMap::new()));

fn key(to: &Audience<String>) -> String {
    match &to.venue {
        Venue::Group(venue) => format!("group:{venue}"),
        Venue::Private => format!("personal:{}", to.members.join(",")),
        Venue::Own => "own".into(),
    }
}

/// Her turn to say something to one audience: while it is held, nothing
/// else of hers goes to them.
pub struct Turn {
    _turn: OwnedMutexGuard<()>,
}

/// Whether the line `hand` may go out now: waits for whatever of hers is
/// going out to the same audience, then, when other lines came up meanwhile,
/// for her choice among them. `None`: she let it go for another.
pub async fn turn(owner: i32, to: &Audience<String>, hand: Hand) -> Option<Turn> {
    let (lock, waiting) = {
        let mut slots = SLOTS.lock().ok()?;
        if slots.len() >= MOST {
            slots.retain(|_, slot| Arc::strong_count(&slot.turn) > 1);
        }
        let slot = slots.entry(key(to)).or_insert_with(|| Slot {
            turn: Arc::new(AsyncMutex::new(())),
            waiting: Arc::new(Mutex::new(Waiting::default())),
        });
        (slot.turn.clone(), slot.waiting.clone())
    };
    if let Ok(guard) = lock.clone().try_lock_owned() {
        return Some(Turn { _turn: guard });
    }
    let id = hand.id.clone();
    if let Ok(mut waiting) = waiting.lock() {
        waiting.hands.push(hand);
    }
    let guard = lock.lock_owned().await;
    let decided = waiting
        .lock()
        .ok()
        .and_then(|mut waiting| waiting.chosen.remove(&id));
    if let Some(said) = decided {
        return said.then_some(Turn { _turn: guard });
    }
    let hands = waiting
        .lock()
        .ok()
        .map(|mut waiting| std::mem::take(&mut waiting.hands))
        .unwrap_or_default();
    let say = if hands.len() > 1 {
        choose(owner, &hands).await
    } else {
        Some(id.clone())
    };
    if let Ok(mut waiting) = waiting.lock() {
        for hand in &hands {
            if hand.id != id {
                waiting
                    .chosen
                    .insert(hand.id.clone(), say.as_deref() == Some(hand.id.as_str()));
            }
        }
    }
    (say.as_deref() == Some(id.as_str())).then_some(Turn { _turn: guard })
}

/// A hand for `line`, up now.
pub fn hand(source: Source, line: &str) -> Hand {
    Hand {
        id: uuid::Uuid::new_v4().simple().to_string(),
        source,
        line: line.to_string(),
        raised_at: super::micros(crate::clock::now()),
    }
}

/// The one of `hands` she says now, as she chooses (one small call, billed
/// to `owner`); when she cannot be asked, the first that went up.
async fn choose(owner: i32, hands: &[Hand]) -> Option<String> {
    let first = hands.first().map(|hand| hand.id.clone());
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let system = choice::system(&soul, "on their site", false);
    let input = choice::input(
        hands,
        &[Held::HerSpeaking],
        &BTreeMap::new(),
        None,
        None,
        super::micros(crate::clock::now()),
    )
    .to_string();
    let timeout = std::time::Duration::from_secs(choice::TIMEOUT_SECS);
    let Some(analyzer) =
        crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(timeout)).await
    else {
        return first;
    };
    let schema = choice::schema();
    let call = analyzer.analyze_json(&system, &input, choice::SCHEMA_NAME, Some(&schema));
    let raw = match tokio::time::timeout(
        timeout,
        crate::services::ai_cost_ledger::with_site_ai_ledger(
            owner,
            "merope",
            "speech_choice",
            call,
        ),
    )
    .await
    {
        Ok(Ok(raw)) => raw,
        _ => return first,
    };
    match choice::parse(&raw, hands) {
        Some(chosen) => chosen.say,
        None => first,
    }
}
