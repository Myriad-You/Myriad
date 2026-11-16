//! Her lines in a group, through the shared core's admission and into the
//! ledger of what she said (`agent::speech`), like every line of hers.
//!
//! Her group mind lives in the shared core and only says "send this"; what
//! kind of turn a message belongs to is known here by what started it: a
//! turn the site asks for (bringing something up, coming back to it), a
//! judgment of whether to chime in (`group_chime`), or a message she
//! answers (the first piece of an answer quotes it). Anything not started
//! unasked is her being called on, and passes only the audience. An unasked
//! turn not sent within [`UNASKED_FOR`] is forgotten.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};

use myriad_merope::group::Piece;

use crate::services::agent::speech::{
    self, Extent, Source,
    admit::{admit_in, goes_on, group_scene},
    ledger::{self, Line, Out},
};

/// How long a turn started unasked stays one, waiting for its first piece.
const UNASKED_FOR: std::time::Duration = std::time::Duration::from_secs(5 * 60);

/// A turn of hers started unasked: why, about what, since when.
#[derive(Clone)]
pub(super) struct Unasked {
    pub source: Source,
    pub thing: Option<String>,
    at: std::time::Instant,
}

static UNASKED: LazyLock<Mutex<HashMap<String, Unasked>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// The group's next turn is hers, unasked, for `source`, about `thing`.
pub(super) fn unasked(venue: &str, source: Source, thing: Option<String>) {
    if let Ok(mut unasked) = UNASKED.lock() {
        unasked.retain(|_, turn| turn.at.elapsed() < UNASKED_FOR);
        unasked.insert(
            venue.to_string(),
            Unasked {
                source,
                thing,
                at: std::time::Instant::now(),
            },
        );
    }
}

/// Why the piece now going out is said: unasked, as its turn was started,
/// or called on (`None`). A piece that quotes a message answers it.
fn turn_of(venue: &str, quote: bool) -> Option<Unasked> {
    let Ok(mut unasked) = UNASKED.lock() else {
        return None;
    };
    if quote {
        unasked.remove(venue);
        return None;
    }
    unasked
        .get(venue)
        .filter(|turn| turn.at.elapsed() < UNASKED_FOR)
        .cloned()
}

/// The words of a piece as it reads in the group.
pub(super) fn text_of(pieces: &[Piece]) -> String {
    pieces
        .iter()
        .map(|piece| match piece {
            Piece::Text { text } => text.clone(),
            Piece::Mention { name, .. } => format!("@{name}"),
        })
        .collect()
}

/// A piece about to go out: the turn it belongs to (`None`: she was
/// called on), and whether it may go. An unasked piece is checked as
/// written (the same line or thing again, a loop); a called one passes.
pub(super) async fn may_send(venue: &str, quote: bool, text: &str) -> (Option<Unasked>, bool) {
    let Some(turn) = turn_of(venue, quote) else {
        return (None, true);
    };
    let Ok(db) = super::db() else {
        return (Some(turn), true);
    };
    let to = speech::group(venue);
    let mut wish = speech::wish(to.clone(), turn.source, text, to);
    wish.thing = turn.thing.clone();
    wish.text = Some(text.to_string());
    let scene = group_scene(&db, venue, None).await;
    let may = goes_on(&admit_in(&db, &wish, &scene).await);
    (Some(turn), may)
}

/// A piece she sent there, or that did not go: into the ledger.
pub(super) async fn sent(venue: &str, turn: Option<&Unasked>, text: &str, ok: bool) {
    let Ok(db) = super::db() else {
        return;
    };
    let place = venue.split(':').next().unwrap_or("group").to_string();
    ledger::write(
        &db,
        Line {
            to: &speech::group(venue),
            source: turn.map_or(Source::GroupAt, |turn| turn.source),
            called: turn.is_none(),
            thing: turn.and_then(|turn| turn.thing.as_deref()),
            text,
        },
        Out::full(place, "group").extent(if ok { Extent::Full } else { Extent::None }),
        None,
    )
    .await;
}

/// Whether something she would bring up first in `venue` (`source`, about
/// `thing`) may be said there at all: the rules only, as she has not
/// written it yet and what she weighs is in her judgment's own input.
pub(super) async fn may_bring_up(
    db: &sea_orm::DatabaseConnection,
    venue: &str,
    source: Source,
    seed: &str,
    seed_from: myriad_agent_rules::memory::audience::Audience<String>,
    thing: &str,
) -> bool {
    let mut wish = speech::wish(speech::group(venue), source, seed, seed_from);
    wish.thing = Some(thing.to_string());
    let scene = group_scene(db, venue, None).await;
    goes_on(&admit_in(db, &wish, &scene).await)
}
