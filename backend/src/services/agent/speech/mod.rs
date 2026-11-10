//! Her speech on the site, one way for every line: what she means to say is
//! a wish (`myriad_merope::speech::Wish`), whether it is said is the shared
//! core's admission ([`admit`]), one line at a time goes to one person or
//! group ([`send`]), and what went out, and how much of it, goes into the
//! ledger of what she said ([`ledger`], `speech.said`), which every
//! judgment of whether to speak reads.
//!
//! Called on (a reply, an @, her turn in a game, a touch) a line passes only
//! the audience and the grants, and goes into the ledger like any other.
//! Unasked, it passes the four rules (do-not-disturb, audience, grants, the
//! same line or thing) and the fuse, and then it is hers to judge from the
//! facts the core writes.
//!
//! The ledger is her log's `speech.said` rows: kept on the site
//! (`agent_speech_said`) and, while her log on the server is on
//! (`HER_LOG_ENABLED`) and her mind is here, written into it as well. The
//! site's older records of what she said unasked are read where the ledger
//! does not reach back yet ([`ledger`]); nothing is migrated in place.

pub mod admit;
pub mod ledger;
pub mod send;

use chrono::{DateTime, Utc};
use myriad_agent_rules::memory::audience::{Audience, Venue};

pub use myriad_merope::speech::{Admission, Extent, Source, Wish};

/// A private talk with `user_id`.
pub fn private(user_id: i32) -> Audience<String> {
    Audience {
        members: vec![user_id.to_string()],
        venue: Venue::Private,
    }
}

/// The group `venue` (`telegram:-100123`).
pub fn group(venue: &str) -> Audience<String> {
    Audience {
        members: Vec::new(),
        venue: Venue::Group(venue.to_string()),
    }
}

/// Her own: what she made or did herself, sayable anywhere.
pub fn own() -> Audience<String> {
    Audience {
        members: Vec::new(),
        venue: Venue::Own,
    }
}

/// The site user a private audience is with, when it is one person.
pub fn user_of(to: &Audience<String>) -> Option<i32> {
    match (&to.venue, to.members.as_slice()) {
        (Venue::Private, [one]) => one.parse().ok(),
        _ => None,
    }
}

/// `at` as the core keeps times: Unix microseconds.
pub fn micros(at: DateTime<Utc>) -> i64 {
    at.timestamp_micros()
}

/// A wish to say something to `to`, unasked, set off by `seed` (heard
/// where `seed_from` is), at the site's now.
pub fn wish(
    to: Audience<String>,
    source: Source,
    seed: impl Into<String>,
    seed_from: Audience<String>,
) -> Wish {
    Wish {
        to,
        source,
        seed: seed.into(),
        seed_from,
        thing: None,
        in_person: false,
        keeps_until: None,
        called: false,
        needs: Vec::new(),
        at: micros(crate::clock::now()),
        text: None,
    }
}

/// Why she speaks, from the site's event key: what set off a line that
/// went through her event decision.
pub fn source_of_event(event_key: &str) -> Source {
    use crate::services::agent::merope::{curiosity, doing, gates, playing, wander};
    match event_key {
        "agent.merope.greeting" => Source::Greeting,
        "agent.merope.touch" => Source::Touch,
        crate::services::agent::merope::reach::EVENT_KEY => Source::Reach,
        key if key == wander::THOUGHT_EVENT || key == curiosity::FOUND_OUT_EVENT => Source::Wander,
        key if key == doing::DOING_EVENT => Source::Share,
        key if key == playing::PLAYING_EVENT => Source::GameRemark,
        gates::SITUATION_EVENT | gates::SCREEN_EVENT => Source::Screen,
        gates::AI_CHAT_EVENT | gates::PEER_AGENT_EVENT => Source::OtherAi,
        key if gates::is_task_outcome(key) => Source::WorkDone,
        _ => Source::Event,
    }
}

#[cfg(test)]
mod replay;
#[cfg(test)]
mod tests;
