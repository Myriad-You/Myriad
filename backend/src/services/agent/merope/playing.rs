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

use std::sync::{LazyLock, Mutex};

use chrono::{DateTime, Utc};
use sea_orm::DatabaseConnection;

use crate::services::agent::memory::unified::{self, Audience, Concept};

/// Shorter than this is not a session worth remembering.
const WORTH_REMEMBERING: chrono::Duration = chrono::Duration::minutes(15);
/// Seen playing this recently still counts as playing now.
const STILL_PLAYING: chrono::Duration = chrono::Duration::minutes(5);
const MAX_GAME_CHARS: usize = 80;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Session {
    game: String,
    started: DateTime<Utc>,
    /// Last time the status showed it.
    seen: DateTime<Utc>,
}

#[derive(Default)]
struct Watch {
    owner: Option<i32>,
    session: Option<Session>,
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
            });
            ended
        }
    }
}

pub async fn tick(db: DatabaseConnection) {
    if !super::is_enabled().await {
        return;
    }
    let Ok(owner) = crate::services::ai_cost_ledger::resolve_site_owner_id().await else {
        return;
    };
    let Some(playing) = game_of(crate::services::steam_presence::site_presence(&db).await) else {
        return;
    };
    let ended = WATCH.lock().ok().and_then(|mut watch| {
        watch.owner = Some(owner);
        look(&mut watch, playing, Utc::now())
    });
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
    fn long_sessions_read_in_hours() {
        let session = Session {
            game: "Factorio".into(),
            started: at(0),
            seen: at(170),
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
                }),
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
