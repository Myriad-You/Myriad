//! What they say in a talk, put in line to be kept (`memory_jobs`): a line at
//! a time, or the stretch it belongs to.

use std::time::Duration;

use super::super::ingest::compact_summary;
use super::super::is_logged_in_addressee;
use super::{memory_user_text, settle_raw, should_extract_chat_remember};

pub async fn enqueue_chat_remember(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    user_text: String,
    reply: String,
    input_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    present: crate::services::agent::memory::unified::Audience,
    turn: super::super::TurnContext,
) {
    queue_chat_words(
        db,
        user_id,
        user_text,
        compact_summary(&reply),
        input_at,
        present,
        turn,
    )
    .await;
}

/// Their words as they land, before she answers: held for what she keeps of
/// them, so a line she is cut off from answering (they said more first) is
/// still gone over, with the next. `work`: said to her at work, not in talk.
pub(in crate::services::agent::merope) async fn hold_chat_words(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    user_text: String,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    present: crate::services::agent::memory::unified::Audience,
    turn: super::super::TurnContext,
    work: bool,
) {
    let reply = String::new();
    if work && !present.is_group() {
        queue_line(db, user_id, user_text, reply, input_at, present, turn).await;
        return;
    }
    queue_chat_words(db, user_id, user_text, reply, Some(input_at), present, turn).await;
}

async fn queue_chat_words(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    user_text: String,
    reply: String,
    input_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    present: crate::services::agent::memory::unified::Audience,
    turn: super::super::TurnContext,
) {
    let Some(input_at) = input_at else {
        return;
    };
    if !is_logged_in_addressee(user_id) {
        return;
    }
    // In a private talk nothing is taken line by line: the talk itself is
    // what she has of it for now, and the stretch is gone over from what
    // was said (`settle_raw`), however long a line. Elsewhere, one job per
    // person and place: their lines fold into it in order.
    let private = !present.is_group();
    // One too long to be written down there is of its stretch all the same.
    let Some(user_text) = settle_raw::words_of(&user_text) else {
        return;
    };
    let answered = !reply.trim().is_empty();
    // In private a stretch is of their messages, each as it was kept; a
    // line of a game is noted as that there.
    let mut said_at = input_at;
    if private {
        if let Some((message, at)) =
            settle_raw::their_message(db, user_id, &present, &user_text, input_at).await
        {
            said_at = at;
            if turn.in_game {
                settle_raw::of_a_game(db, message).await;
            }
        }
    } else {
        queue_line(
            db,
            user_id,
            user_text.clone(),
            reply,
            input_at,
            present.clone(),
            turn,
        )
        .await;
    }
    // And the stretch it belongs to, gone over once it is over (or once
    // the box is full, or a line is needed at once).
    let id = super::super::memory_jobs::key(&["settle", &user_id.to_string(), &present.venue()]);
    let at_once = private && myriad_merope::settle_raw::at_once(&user_text).is_some();
    let data = super::super::memory_jobs::Payload::Settle {
        present: present.clone(),
        from: said_at,
        last: said_at,
        heard: vec![(said_at, answered)],
    };
    if !matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            super::super::store::memory_jobs::enqueue(db, &id, user_id, data, None)
        )
        .await,
        Ok(Ok(()))
    ) {
        tracing::warn!(user_id, outcome = "enqueue_failed", "[Merope] chat settle");
    }
    // Needed at once: gone over now, with this line (nothing is judged so
    // yet, `myriad_merope::settle_raw::at_once`). Nobody waits on it here.
    if at_once && answered {
        let db = db.clone();
        super::super::background::spawn("a line needed at once, gone over", async move {
            super::super::memory_jobs::hurry(&db, user_id, &present).await;
        });
    }
}

/// A line written down on its own: in a group, its gist a short note, gone
/// over with its stretch; in private, what was said to her at work, which is
/// no talk to go over and so is kept for good.
async fn queue_line(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    user_text: String,
    reply: String,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    present: crate::services::agent::memory::unified::Audience,
    turn: super::super::TurnContext,
) {
    let Some(user_text) = memory_user_text(&user_text) else {
        return;
    };
    if !is_logged_in_addressee(user_id) || !should_extract_chat_remember(&user_text) {
        return;
    }
    let id = super::super::memory_jobs::key(&["chat", &user_id.to_string(), &present.venue()]);
    let data = super::super::memory_jobs::Payload::Chat {
        user_text,
        reply,
        input_at,
        for_good: !present.is_group(),
        present,
        turn,
        lines: vec![input_at],
    };
    if !matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            super::super::store::memory_jobs::enqueue(db, &id, user_id, data, None)
        )
        .await,
        Ok(Ok(()))
    ) {
        tracing::warn!(user_id, outcome = "enqueue_failed", "[Merope] chat memory");
    }
}
