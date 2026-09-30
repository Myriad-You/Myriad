//! What the site's person is playing, seen on their Steam status.
//!
//! The site shows its owner's Steam status to anyone signed in; she sees it
//! too. Every couple of minutes she looks: what they are playing and since
//! when. Talking with them, she knows it the way a friend glancing at their
//! status would, and knows she saw it rather than was told. When they stop
//! after a proper session, she remembers what they played and for how long,
//! as something about them heard only by them, so over time she knows what
//! they have been playing.
//!
//! Only the owner's own conversations hear any of it. A group may hold people
//! from outside the site, and other people are not told what the owner does.
//! When Steam cannot be reached nothing changes: not knowing is not stopping.
//!
//! What she sees change is hers to remark on if they are here to see her,
//! like a friend glancing over: they just started a game, have been at it
//! for hours, or just stopped after a proper session. Whether to say
//! anything is decided like any passing thought (`PLAYING_EVENT` is said in
//! person or not at all). A game already running when she first looks after
//! waking is not one they just started, and she does not know how long it
//! has gone on.

use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;

use crate::services::agent::memory::unified::{self, Audience, Concept};

/// What she saw change on their Steam status; said in person or let go.
pub const PLAYING_EVENT: &str = "agent.merope.playing";
/// Hours into a session worth her noticing, each once.
const LONG_AT_HOURS: [i64; 2] = [2, 4];

/// Shorter than this is not a session worth remembering.
const WORTH_REMEMBERING: chrono::Duration = chrono::Duration::minutes(15);
/// Seen playing this recently still counts as playing now.
const STILL_PLAYING: chrono::Duration = chrono::Duration::minutes(5);
const MAX_GAME_CHARS: usize = 80;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct Session {
    game: String,
    started: DateTime<Utc>,
    /// Last time the status showed it.
    seen: DateTime<Utc>,
    /// She saw it start, rather than find it running when she first looked.
    saw_start: bool,
    /// The most hours into it she has noticed.
    noticed_hours: i64,
}

#[derive(Default)]
struct Watch {
    owner: Option<i32>,
    session: Option<Session>,
    /// She has looked before, since waking.
    looked: bool,
}

/// What she saw change on one look.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Noticed {
    Started { game: String },
    Long { game: String, hours: i64 },
    Stopped { game: String, minutes: i64 },
}

static WATCH: LazyLock<Mutex<Watch>> = LazyLock::new(|| Mutex::new(Watch::default()));

/// What the status says: playing this game, playing nothing, or unknown.
fn game_of(
    presence: Option<crate::services::steam_presence::SteamPresenceResponse>,
) -> Option<Option<String>> {
    let presence = presence?;
    Some(
        presence
            .gameextrainfo
            .filter(|_| presence.is_in_game)
            .map(|game| game.trim().chars().take(MAX_GAME_CHARS).collect::<String>())
            .filter(|game| !game.is_empty()),
    )
}

/// Move the watch on by one look; the session that just ended, if any.
fn look(watch: &mut Watch, playing: Option<String>, now: DateTime<Utc>) -> Option<Session> {
    let saw_start = watch.looked;
    watch.looked = true;
    match (&mut watch.session, playing) {
        (Some(session), Some(game)) if session.game == game => {
            session.seen = now;
            None
        }
        (session, playing) => {
            let ended = session.take();
            *session = playing.map(|game| Session {
                game,
                started: now,
                seen: now,
                saw_start,
                noticed_hours: 0,
            });
            ended
        }
    }
}

/// What she noticed on the look just taken, given the session it ended:
/// a proper session stopping, a game she saw start, or hours into one she
/// saw start. At most one thing a look.
fn notice(watch: &mut Watch, ended: Option<&Session>) -> Option<Noticed> {
    if let Some(ended) = ended {
        let length = ended.seen.signed_duration_since(ended.started);
        if watch.session.is_none() && length >= WORTH_REMEMBERING {
            return Some(Noticed::Stopped {
                game: ended.game.clone(),
                minutes: length.num_minutes(),
            });
        }
    }
    let session = watch.session.as_mut().filter(|session| session.saw_start)?;
    if session.started == session.seen {
        return Some(Noticed::Started {
            game: session.game.clone(),
        });
    }
    let hours = session
        .seen
        .signed_duration_since(session.started)
        .num_hours();
    let due = LONG_AT_HOURS
        .iter()
        .copied()
        .filter(|at| hours >= *at && session.noticed_hours < *at)
        .max()?;
    session.noticed_hours = due;
    Some(Noticed::Long {
        game: session.game.clone(),
        hours: due,
    })
}

/// What she saw, as the event she may speak from. `before` is what she
/// already knew of them playing it.
fn noticed_summary(noticed: &Noticed, before: Option<&str>) -> String {
    match noticed {
        Noticed::Started { game } => {
            let before = before.map_or_else(
                || " You have not seen them play it before.".to_string(),
                |before| format!(" What you knew of it: {before}."),
            );
            format!("On their Steam status you just saw them start playing 「{game}」.{before}")
        }
        Noticed::Long { game, hours } => format!(
            "Their Steam status says they have been playing 「{game}」 for about {hours} hours now."
        ),
        Noticed::Stopped { game, minutes } => {
            let how_long = if *minutes >= 90 {
                format!("about {} hours", (minutes + 30) / 60)
            } else {
                format!("about {minutes} minutes")
            };
            format!(
                "Their Steam status says they just stopped playing 「{game}」, after {how_long}."
            )
        }
    }
}

pub async fn tick(db: DatabaseConnection) {
    if !super::is_enabled().await {
        return;
    }
    let Some(owner) = super::call::site_owner().await else {
        return;
    };
    let Some(playing) = game_of(crate::services::steam_presence::site_presence(&db).await) else {
        return;
    };
    let Some((ended, noticed)) = WATCH.lock().ok().map(|mut watch| {
        watch.owner = Some(owner);
        let ended = look(&mut watch, playing, Utc::now());
        let noticed = notice(&mut watch, ended.as_ref());
        (ended, noticed)
    }) else {
        return;
    };
    if let Some(noticed) = noticed {
        let before = match &noticed {
            Noticed::Started { game } => {
                unified::find_active(&db, owner, "presence", &format!("《{game}》"))
                    .await
                    .ok()
                    .flatten()
                    .map(|row| row.content)
            }
            _ => None,
        };
        super::spawn_ingest(
            owner,
            PLAYING_EVENT,
            noticed_summary(&noticed, before.as_deref()),
        );
    }
    if let Some(ended) = ended {
        remember_session(&db, owner, ended).await;
    }
}

/// What she keeps about a game they played: one line per game, the latest
/// session in it, and whether they have played it before.
fn session_note(session: &Session, before: bool) -> Option<String> {
    let length = session.seen.signed_duration_since(session.started);
    if length < WORTH_REMEMBERING {
        return None;
    }
    let minutes = length.num_minutes();
    let how_long = if minutes >= 90 {
        format!("大约{}小时", (minutes + 30) / 60)
    } else {
        format!("大约{minutes}分钟")
    };
    let when = session.started.format("%m-%d");
    Some(if before {
        format!(
            "常在 Steam 上玩《{}》，最近一次是 {when}，玩了{how_long}",
            session.game
        )
    } else {
        format!(
            "在 Steam 上玩过《{}》，那次是 {when}，玩了{how_long}",
            session.game
        )
    })
}

async fn remember_session(db: &DatabaseConnection, owner: i32, session: Session) {
    // One line per game: the new session replaces what she kept of the last.
    let earlier = unified::find_active(db, owner, "presence", &format!("《{}》", session.game))
        .await
        .ok()
        .flatten();
    let Some(note) = session_note(&session, earlier.is_some()) else {
        return;
    };
    if let Some(earlier) = earlier {
        let _ = unified::retire(db, owner, &[earlier.id], "superseded").await;
    }
    let kept = unified::remember(
        db,
        unified::NewMemory {
            user_id: owner,
            kind: unified::MemoryKind::Fact,
            content: note,
            evidence: Some(format!("steam {}", session.started.format("%Y-%m-%d"))),
            speaker: unified::Speaker::Agent,
            source: "presence",
            // Theirs alone: what they do is not told to anyone else.
            audience: Audience::private(owner),
            importance: 0.3,
            concepts: vec![Concept {
                name: session.game.clone(),
                aliases: Vec::new(),
            }],
        },
    )
    .await;
    if let Err(error) = kept {
        tracing::warn!(%error, "[Merope] could not remember what they played");
    }
}

/// What they are playing right now, for a private turn with the site's
/// owner; nothing for anyone else.
pub fn now_for(user_id: i32, at: DateTime<Utc>) -> Option<String> {
    let watch = WATCH.lock().ok()?;
    if watch.owner != Some(user_id) {
        return None;
    }
    let session = watch.session.as_ref()?;
    if at.signed_duration_since(session.seen) > STILL_PLAYING {
        return None;
    }
    let minutes = at
        .signed_duration_since(session.started)
        .num_minutes()
        .max(0);
    Some(format!(
        "They are playing 「{}」 right now, about {minutes} minutes in.",
        session.game
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(minute: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-09-25T20:00:00Z")
            .unwrap()
            .with_timezone(&Utc)
            + chrono::Duration::minutes(minute)
    }

    #[test]
    fn a_session_runs_from_the_first_look_to_the_last() {
        let mut watch = Watch::default();
        assert!(look(&mut watch, Some("Elden Ring".into()), at(0)).is_none());
        assert!(look(&mut watch, Some("Elden Ring".into()), at(40)).is_none());
        let ended = look(&mut watch, None, at(42)).unwrap();
        assert_eq!((ended.started, ended.seen), (at(0), at(40)));
        assert_eq!(
            session_note(&ended, false).as_deref(),
            Some("在 Steam 上玩过《Elden Ring》，那次是 09-25，玩了大约40分钟")
        );
        // Switching games ends one session and starts the next.
        look(&mut watch, Some("Hades".into()), at(50));
        let ended = look(&mut watch, Some("Celeste".into()), at(55)).unwrap();
        assert_eq!(ended.game, "Hades");
        assert!(
            session_note(&ended, false).is_none(),
            "five minutes is not a session"
        );
    }

    #[test]
    fn she_notices_a_start_hours_in_and_a_stop_but_not_what_was_running() {
        let mut watch = Watch::default();
        let mut step = |playing: Option<&str>, minute: i64| {
            let ended = look(&mut watch, playing.map(str::to_string), at(minute));
            notice(&mut watch, ended.as_ref())
        };
        // Running when she first looks: not a start, and no hours noticed.
        assert_eq!(step(Some("Hades"), 0), None);
        assert_eq!(step(Some("Hades"), 200), None);
        // Stopping after a proper session is noticed all the same.
        assert_eq!(
            step(None, 202),
            Some(Noticed::Stopped {
                game: "Hades".into(),
                minutes: 200
            })
        );
        assert_eq!(
            step(Some("Elden Ring"), 210),
            Some(Noticed::Started {
                game: "Elden Ring".into()
            })
        );
        assert_eq!(step(Some("Elden Ring"), 250), None);
        assert_eq!(
            step(Some("Elden Ring"), 335),
            Some(Noticed::Long {
                game: "Elden Ring".into(),
                hours: 2
            })
        );
        assert_eq!(step(Some("Elden Ring"), 340), None, "each once");
        // Switching games is a start, not a stop.
        assert_eq!(
            step(Some("Celeste"), 345),
            Some(Noticed::Started {
                game: "Celeste".into()
            })
        );
        // A few minutes is not a session to remark on stopping.
        assert_eq!(step(None, 350), None);
        assert!(
            noticed_summary(
                &Noticed::Stopped {
                    game: "Hades".into(),
                    minutes: 200
                },
                None
            )
            .ends_with("after about 3 hours.")
        );
        assert!(
            noticed_summary(
                &Noticed::Started {
                    game: "Hades".into()
                },
                Some("常在 Steam 上玩《Hades》")
            )
            .contains("What you knew of it: 常在 Steam 上玩《Hades》.")
        );
    }

    #[test]
    fn long_sessions_read_in_hours() {
        let session = Session {
            game: "Factorio".into(),
            started: at(0),
            seen: at(170),
            ..Session::default()
        };
        assert_eq!(
            session_note(&session, true).as_deref(),
            Some("常在 Steam 上玩《Factorio》，最近一次是 09-25，玩了大约3小时")
        );
    }

    #[test]
    fn not_knowing_is_not_stopping() {
        assert_eq!(game_of(None), None);
    }

    #[test]
    fn only_the_owner_hears_it_and_only_while_it_is_now() {
        {
            let mut watch = WATCH.lock().unwrap();
            *watch = Watch {
                owner: Some(7),
                session: Some(Session {
                    game: "Elden Ring".into(),
                    started: at(0),
                    seen: at(30),
                    ..Session::default()
                }),
                looked: true,
            };
        }
        assert_eq!(
            now_for(7, at(32)).as_deref(),
            Some("They are playing 「Elden Ring」 right now, about 32 minutes in.")
        );
        assert!(now_for(8, at(32)).is_none(), "someone else");
        assert!(now_for(7, at(40)).is_none(), "not seen for a while");
        *WATCH.lock().unwrap() = Watch::default();
    }
}
