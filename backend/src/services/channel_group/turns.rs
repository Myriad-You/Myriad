//! The whole steps her group mind still asks of the site (D2a): who
//! someone is, whether they may talk to her, what she makes of the talk,
//! whether she speaks up, and what she says.

use myriad_merope::group::{self as core, Answer, Looked, Spoken, Target, Who};

use super::*;

/// Look at the pictures in the talk she has not seen yet, as a person
/// reads back over what was sent. Billed to the site's owner.
pub(super) async fn look(venue: &str, pictures: Vec<core::Picture>) -> Vec<Looked> {
    let Ok(db) = db() else {
        return Vec::new();
    };
    let Some(owner) = owner(&db).await else {
        return Vec::new();
    };
    let token = token(venue);
    // A sticker she looks at as she is: whether she would keep it to send.
    let soul = soul().await;
    let (db, soul, token) = (&db, soul.as_str(), token.as_str());
    futures::future::join_all(pictures.into_iter().map(|picture| async move {
        let seen = look_at(db, owner, soul, venue, token, &picture.image).await;
        seen.map(|seen| Looked {
            message_id: picture.message_id,
            index: picture.index,
            seen,
        })
    }))
    .await
    .into_iter()
    .flatten()
    .collect()
}

/// Who wrote a line there, and the site's owner.
pub(super) async fn identify(venue: &str, from: &str) -> Answer {
    let Ok(db) = db() else {
        return Answer::Identified {
            who: Who::Unknown,
            owner: None,
        };
    };
    let Some(line) = venue
        .split_once(':')
        .and_then(|(slug, chat)| {
            line_to(&Target {
                platform: slug.to_string(),
                chat: chat.to_string(),
                thread: None,
                message_id: String::new(),
            })
        })
        .map(|line| GroupLine {
            from: from.to_string(),
            ..line
        })
    else {
        return Answer::Identified {
            who: Who::Unknown,
            owner: None,
        };
    };
    let who = match lookup(&db, &line).await {
        Some(PairingLookup::Paired { user_id }) => {
            let binding = current_binding(&db, &line, user_id).await;
            let bound = binding.is_some();
            if let (Some(binding), Ok(mut bindings)) = (binding, BINDINGS.lock()) {
                bindings.insert((venue.to_string(), user_id), binding);
            }
            Who::Member { user_id, bound }
        }
        Some(_) => Who::Stranger,
        None => Who::Unknown,
    };
    Answer::Identified {
        who,
        owner: owner(&db).await,
    }
}

/// Whether `user_id` may talk to her now, with their session in this group.
pub(super) async fn prepare(venue: &str, user_id: i32) -> bool {
    // A recording without a database of its own has no sessions to keep.
    #[cfg(test)]
    if g0::on() && g0::database().is_none() {
        return true;
    }
    let Ok(db) = db() else {
        return false;
    };
    if !matches!(
        crate::services::principal::current_roles(&db, user_id).await,
        Ok(Some(_))
    ) {
        return false;
    }
    let key = (venue.to_string(), user_id);
    let mut known = SESSIONS
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(&key).cloned());
    // After a restart: the one they talked to her in here before.
    if known.is_none() {
        known = last_group_session(&db, user_id, venue).await;
    }
    // Made as the group's from the start: never read back as a private one.
    let Ok(session_id) = crate::services::agent::sessions::ensure_session_in(
        &db,
        known.as_deref(),
        user_id,
        AgentInteractionMode::Chat,
        Some(venue),
    )
    .await
    else {
        return false;
    };
    if let Ok(mut sessions) = SESSIONS.lock() {
        sessions.insert(key, session_id);
    }
    true
}

/// The session someone last talked to her in, in this group.
pub(super) async fn last_group_session(
    db: &DatabaseConnection,
    user_id: i32,
    venue: &str,
) -> Option<String> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT id FROM agent_sessions WHERE user_id = $1 AND context->>'venue' = $2 \
           AND (context->>'past') IS NULL \
           AND context->>'mode' = 'chat' ORDER BY last_active_at DESC LIMIT 1",
        [user_id.into(), venue.into()],
    ))
    .await
    .ok()
    .flatten()?
    .try_get::<String>("", "id")
    .ok()
}

pub(super) fn as_conversation(lines: &[Spoken]) -> Vec<ConversationMessage> {
    lines
        .iter()
        .map(|line| ConversationMessage {
            role: line.role.clone(),
            content: line.content.clone(),
            created_at: None,
        })
        .collect()
}

/// Open a member's turn, as one of theirs in their session in the group
/// (see `agent::group_turn`).
pub(super) async fn turn_begin(
    venue: &str,
    user_id: i32,
    input: String,
    transcript: Vec<Spoken>,
    speaker: String,
    chime: Option<String>,
    first: bool,
) -> Answer {
    let none = Answer::TurnBegun {
        turn: None,
        sections: Vec::new(),
        doing: None,
    };
    let Ok(db) = db() else {
        return none;
    };
    let Some(session_id) = SESSIONS
        .lock()
        .ok()
        .and_then(|sessions| sessions.get(&(venue.to_string(), user_id)).cloned())
    else {
        return none;
    };
    let start = crate::services::agent::group_turn::Start {
        user_id,
        venue: venue.to_string(),
        session_id,
        input,
        transcript: as_conversation(&transcript),
        speaker,
        chime,
        first,
    };
    match crate::services::agent::group_turn::begin(&db, start).await {
        Some(begun) => Answer::TurnBegun {
            turn: Some(begun.turn),
            sections: begun.sections,
            doing: begun.doing,
        },
        None => none,
    }
}

/// Whether the member she answered is still bound to her.
pub(super) async fn still_hers(venue: &str, user_id: i32) -> bool {
    let binding = BINDINGS
        .lock()
        .ok()
        .and_then(|bindings| bindings.get(&(venue.to_string(), user_id)).cloned());
    match (binding, db()) {
        (Some(binding), Ok(db)) => still_bound(&db, &binding).await,
        _ => false,
    }
}
