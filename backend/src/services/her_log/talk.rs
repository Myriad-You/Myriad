//! What the partner says to her in private, as rows of her log (D6): the
//! end that hears a line writes it, wherever her mind is, so another end
//! her mind moves to knows what was said there.
//!
//! - Only the owner's private chat with her (a chat session of the site
//!   owner's with no venue): groups, others' talk and Work are not written.
//! - Only the partner's half: hers is already `speech.said`.
//! - One segment a conversation, `segment:site_<session id>`, opened by a
//!   `segment.open` before its first line; each line is a `segment.line`
//!   with the message's id as its `line`.
//! - The site's picture links are taken out of the text and counted
//!   (`pictures`): the pictures stay here.
//!
//! What another end heard and what she said there come back the other way
//! ([`fold`], D6-e): copies in the site's own conversations, which every
//! reader of them (her prompt, the panel, search) reads as any line. A copy
//! is never written to the log again.
//!
//! Forgetting a line is forgetting it on every end (D6-g): a line forgotten
//! here is a forget of its row ([`forget_lines`]); a row forgotten anywhere
//! takes its line here with what it taught her ([`fold`]).

use chrono::{DateTime, Duration, Utc};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use myriad_persona_log::{
    Author, Row,
    payloads::{SegmentLine, SegmentOpen, SpeechSaid},
};

use super::{Error, NODE, keys, store};
use crate::services::agent::memory::origins::Gone;

/// The partner said `text` (message `message` of session `session`) at
/// `at`, where `place` names (`web`, `app`, a chat app): kept in her log
/// when it is on and the owner syncs it. Never fails the message: a line
/// the log does not take stays on the site only.
pub async fn partner_said(
    db: &DatabaseConnection,
    session: &str,
    message: i32,
    text: &str,
    at: DateTime<Utc>,
    place: &str,
) {
    if !super::enabled() {
        return;
    }
    let Ok(site_owner) = crate::services::site_owner::site_owner_user_id(db).await else {
        return;
    };
    if let Err(error) = keep(
        db,
        keys::root(),
        site_owner,
        session,
        message,
        text,
        at,
        place,
    )
    .await
    {
        tracing::debug!(%error, "[her-log] the partner's line stays on the site only");
    }
}

/// [`partner_said`] for the site owner `site_owner`: the line's row, or
/// none when it is not one the log keeps.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn keep(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    site_owner: i32,
    session: &str,
    message: i32,
    text: &str,
    at: DateTime<Utc>,
    place: &str,
) -> Result<Option<Row>, Error> {
    if !private_with(db, session, site_owner).await? {
        return Ok(None);
    }
    if !store::account(db, site_owner)
        .await?
        .is_some_and(|account| account.on())
    {
        return Ok(None);
    }
    let (text, pictures) = without_pictures(text);
    if text.is_empty() && pictures == 0 {
        return Ok(None);
    }
    let owner = super::owner(db, site_owner).await?;
    let audience = owner.personal();
    let object = format!("segment:{NODE}_{session}");
    let at = at.timestamp_millis();
    if !opened(db, site_owner, &audience, &object).await? {
        let open = SegmentOpen {
            at,
            place: place.to_string(),
        };
        store::write_owners(
            db,
            root,
            &owner,
            &audience,
            None,
            &object,
            "segment.open",
            &serde_json::to_value(open).unwrap_or_default(),
        )
        .await?;
    }
    let line = SegmentLine {
        at,
        text,
        line: Some(message.to_string()),
        reply_to: None,
        pictures: (pictures > 0).then_some(pictures),
    };
    let row = store::write_owners(
        db,
        root,
        &owner,
        &audience,
        None,
        &object,
        "segment.line",
        &serde_json::to_value(line).unwrap_or_default(),
    )
    .await?;
    // The line knows its row: forgotten anywhere, it goes here too.
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_messages SET log_row = $1 WHERE id = $2 AND session_id = $3",
        [row.id.clone().into(), message.into(), session.into()],
    ))
    .await?;
    Ok(Some(row))
}

/// Whether `session` is `user`'s private chat with her.
async fn private_with<C: ConnectionTrait>(db: &C, session: &str, user: i32) -> Result<bool, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT context FROM agent_sessions WHERE id = $1 AND user_id = $2",
            [session.into(), user.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(false);
    };
    let context: Option<Value> = row.try_get("", "context")?;
    let context = context.unwrap_or_default();
    Ok(context.get("mode").and_then(Value::as_str) == Some("chat")
        && context.get("venue").is_none_or(Value::is_null))
}

/// Whether the segment `object` was opened in `audience` already.
async fn opened<C: ConnectionTrait>(
    db: &C,
    user: i32,
    audience: &str,
    object: &str,
) -> Result<bool, Error> {
    Ok(db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 AS one FROM agent_log_rows WHERE user_id = $1 AND audience = $2 \
             AND object = $3 AND kind = 'segment.open' LIMIT 1",
            [user.into(), audience.into(), object.into()],
        ))
        .await?
        .is_some())
}

/// How long after a conversation's last line a line said elsewhere still
/// goes on with it (D6-f); past that, it is a new conversation.
const GOES_ON_WITHIN_HOURS: i64 = 6;

/// Copies the lines of the site owner's private talk with her that another
/// of their ends wrote into her log (the partner's `segment.line`s, her
/// `speech.said`s) into the site's conversations, and forgets here the lines
/// whose row was forgotten (copies or not), with what they taught her. Each segment goes into one conversation, settled
/// the first time the site sees it: the private chat last talked in within
/// [`GOES_ON_WITHIN_HOURS`] before its first line, or a new one. Her lines go
/// into the conversation talked in last before she said them, or the latest
/// one. Each copy is at its row's time and says where it was said. Done
/// again any time, it copies nothing twice. How many copies it made.
pub async fn fold(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    site_owner: i32,
) -> Result<usize, Error> {
    let owner = super::owner(db, site_owner).await?;
    let audience = owner.personal();
    forget_forgotten(db, site_owner).await?;
    let mut rows = store::rows_of(
        db.query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT r.envelope FROM agent_log_rows r \
             WHERE r.user_id = $1 AND r.audience = $2 AND r.origin <> $3 AND NOT r.forgotten \
               AND r.kind IN ('segment.line', 'speech.said') \
               AND NOT EXISTS (SELECT 1 FROM agent_messages m WHERE m.log_row = r.id)",
            [site_owner.into(), audience.clone().into(), NODE.into()],
        ))
        .await?,
    )?;
    rows.retain(|row| {
        matches!(
            (row.kind.as_str(), row.author),
            ("segment.line", Author::Owner) | ("speech.said", Author::Her)
        )
    });
    if rows.is_empty() {
        return Ok(0);
    }
    rows.sort_by(|a, b| a.hlc.cmp(&b.hlc).then_with(|| a.id.cmp(&b.id)));
    let held: Vec<&Row> = rows.iter().collect();
    let bodies = store::bodies(db, root, site_owner, &audience, &held).await?;
    let mut copied = 0;
    for row in &rows {
        // A body still to come is copied when it comes.
        let Some(body) = bodies.get(&row.id) else {
            continue;
        };
        let at = DateTime::<Utc>::from_timestamp_millis(row.hlc.wall_ms as i64).unwrap_or_default();
        let name = crate::services::persona_sync::lease::device_name(db, &row.origin)
            .await
            .ok()
            .flatten();
        let copy = match row.kind.as_str() {
            "segment.line" => {
                let Ok(line) = serde_json::from_value::<SegmentLine>(body.clone()) else {
                    continue;
                };
                let place = place_of(db, root, site_owner, &audience, &row.object).await?;
                let session = segment_session(db, site_owner, &row.object, at).await?;
                Copy {
                    session,
                    role: "user",
                    text: line.text,
                    metadata: json!({
                        "said": { "on": row.origin, "name": name, "place": place },
                        "pictures": line.pictures,
                    }),
                }
            }
            _ => {
                let Ok(said) = serde_json::from_value::<SpeechSaid>(body.clone()) else {
                    continue;
                };
                // Said here for her mind on a PC: the line is here already.
                if bound_to_lent(db, site_owner, row, said.line.as_deref()).await? {
                    continue;
                }
                let session = match last_talked(db, site_owner, at, true).await? {
                    Some(session) => session,
                    None => new_private_chat(db, site_owner).await?,
                };
                let mut metadata = json!({
                    "said": { "on": row.origin, "name": name, "place": said.place },
                });
                use crate::services::agent::chat_prompt::{CUT_OFF_KEY, CUT_OFF_UNSENT};
                if said.extent == "none" {
                    metadata[CUT_OFF_KEY] = json!(CUT_OFF_UNSENT);
                } else if said.interrupted {
                    metadata[CUT_OFF_KEY] =
                        json!(crate::services::agent::chat_prompt::cut_off_kind(true));
                    if let Some(heard) = said.heard {
                        metadata["heardChars"] = json!(heard);
                    }
                }
                if said.source != "reply" {
                    metadata["unprompted"] = json!(true);
                }
                Copy {
                    session,
                    role: "assistant",
                    text: said.text,
                    metadata,
                }
            }
        };
        copied += copy.write(db, row, at).await? as usize;
    }
    Ok(copied)
}

/// The metadata key of a line the site sent for her mind on a PC
/// (`channel_work::held::say_lent`): `<device>/<say id>`.
pub const LENT_SAY: &str = "lentSay";

/// Whether `row`, her `speech.said` of a say `line` she sent through the
/// site from her mind on a PC, is the line the site kept of that say: it
/// becomes that line's row (so forgetting either takes the other), and no
/// copy is made. Only a line of the site owner's, from the row's own device,
/// not yet any row's.
async fn bound_to_lent(
    db: &DatabaseConnection,
    site_owner: i32,
    row: &Row,
    line: Option<&str>,
) -> Result<bool, Error> {
    let Some(line) = line.filter(|line| !line.is_empty()) else {
        return Ok(false);
    };
    let bound = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_messages m SET log_row = $1 \
             FROM agent_sessions s \
             WHERE m.session_id = s.id AND s.user_id = $2 AND m.role = 'assistant' \
               AND m.log_row IS NULL AND (m.metadata->>$3) = $4",
            [
                row.id.clone().into(),
                site_owner.into(),
                LENT_SAY.into(),
                format!("{}/{line}", row.origin).into(),
            ],
        ))
        .await?
        .rows_affected();
    Ok(bound > 0)
}

/// Has her forget, on her log, the lines of the site owner's private talk
/// forgotten here (D6-g): a forget of each one's row, which every end takes,
/// and each forgets there what the line taught her. A line's row is the one
/// it is a copy of, the one it was written as (the partner's), or, hers, the
/// `speech.said` row of what she said (`agent_speech_said`, by `line`). A
/// line the log never had, or whose row is forgotten already, is forgotten
/// here only. Never fails the forgetting here: a forget the log does not
/// take is only logged.
pub async fn forget_lines(db: &DatabaseConnection, lines: &[Gone]) {
    if !super::enabled() || lines.is_empty() {
        return;
    }
    let Ok(site_owner) = crate::services::site_owner::site_owner_user_id(db).await else {
        return;
    };
    for line in lines {
        if let Err(error) = forget_row_of(db, site_owner, line).await {
            tracing::warn!(%error, line = line.id, "[her-log] a line forgotten here is not forgotten on her log");
        }
    }
}

/// [`forget_lines`] of one line, for the site owner `site_owner`: the
/// forget written, or none when there is none to write.
pub(crate) async fn forget_row_of(
    db: &DatabaseConnection,
    site_owner: i32,
    line: &Gone,
) -> Result<Option<Row>, Error> {
    if !private_with(db, &line.session_id, site_owner).await?
        || !store::account(db, site_owner)
            .await?
            .is_some_and(|account| account.on())
    {
        return Ok(None);
    }
    let row = match &line.log_row {
        Some(row) => Some(row.clone()),
        None if line.role == "assistant" => db
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT r.id FROM agent_speech_said d JOIN agent_log_rows r \
                   ON r.user_id = $1 AND r.object = 'speech:' || d.id AND r.origin = $3 \
                 WHERE d.user_id = $1 AND d.payload->>'line' = $2 LIMIT 1",
                [site_owner.into(), line.id.to_string().into(), NODE.into()],
            ))
            .await?
            .map(|row| row.try_get("", "id"))
            .transpose()?,
        None => None,
    };
    let Some(row) = row else {
        return Ok(None);
    };
    let forgotten = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT forgotten FROM agent_log_rows WHERE user_id = $1 AND id = $2",
            [site_owner.into(), row.clone().into()],
        ))
        .await?
        .map(|kept| kept.try_get::<bool>("", "forgotten"))
        .transpose()?;
    if forgotten != Some(false) {
        return Ok(None);
    }
    let owner = super::owner(db, site_owner).await?;
    store::forget(db, &owner, &owner.personal(), &format!("row:{row}"))
        .await
        .map(Some)
}

/// The site owner's lines whose row of her log is forgotten (by an end
/// that forgot it there, or here before the line went), forgotten here too
/// with what they taught her ([`crate::services::agent::memory::manage::forget_line`]).
async fn forget_forgotten(db: &DatabaseConnection, site_owner: i32) -> Result<(), Error> {
    let lines = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.session_id, m.id FROM agent_log_rows r \
             JOIN agent_messages m ON m.log_row = r.id \
             JOIN agent_sessions s ON s.id = m.session_id AND s.user_id = $1 \
             WHERE r.user_id = $1 AND r.forgotten \
             UNION \
             SELECT m.session_id, m.id FROM agent_log_rows r \
             JOIN agent_speech_said d ON r.object = 'speech:' || d.id AND d.user_id = $1 \
             JOIN agent_messages m ON m.id::text = d.payload->>'line' AND m.role = 'assistant' \
               AND m.log_row IS NULL \
             JOIN agent_sessions s ON s.id = m.session_id AND s.user_id = $1 \
             WHERE r.user_id = $1 AND r.forgotten AND r.origin = $2 AND r.kind = 'speech.said'",
            [site_owner.into(), NODE.into()],
        ))
        .await?;
    for line in lines {
        let session: String = line.try_get("", "session_id")?;
        let id: i32 = line.try_get("", "id")?;
        crate::services::agent::memory::manage::forget_line(db, site_owner, &session, id).await?;
    }
    Ok(())
}

/// One line said elsewhere, as the site's conversation keeps it.
struct Copy {
    session: String,
    role: &'static str,
    text: String,
    metadata: Value,
}

impl Copy {
    /// Writes it as `row`'s copy at `at`: whether it was not there yet.
    async fn write(
        self,
        db: &DatabaseConnection,
        row: &Row,
        at: DateTime<Utc>,
    ) -> Result<bool, Error> {
        let written = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO agent_messages \
                 (session_id, role, content, metadata, created_at, origin, log_row) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT DO NOTHING",
                [
                    self.session.clone().into(),
                    self.role.into(),
                    self.text.into(),
                    self.metadata.into(),
                    at.into(),
                    row.origin.clone().into(),
                    row.id.clone().into(),
                ],
            ))
            .await?
            .rows_affected()
            > 0;
        if written {
            db.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "UPDATE agent_sessions SET last_active_at = GREATEST(last_active_at, $2), \
                 message_count = (SELECT COUNT(*) FROM agent_messages WHERE session_id = $1) \
                 WHERE id = $1",
                [self.session.into(), at.into()],
            ))
            .await?;
        }
        Ok(written)
    }
}

/// Where segment `object` is, as its `segment.open` says (`app`, a chat
/// app); none when that has not come.
async fn place_of(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    user: i32,
    audience: &str,
    object: &str,
) -> Result<Option<String>, Error> {
    let opens = store::rows_of(
        db.query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT envelope FROM agent_log_rows WHERE user_id = $1 AND audience = $2 \
             AND object = $3 AND kind = 'segment.open' AND NOT forgotten LIMIT 1",
            [user.into(), audience.into(), object.into()],
        ))
        .await?,
    )?;
    let held: Vec<&Row> = opens.iter().collect();
    Ok(store::bodies(db, root, user, audience, &held)
        .await?
        .into_values()
        .next()
        .and_then(|body| serde_json::from_value::<SegmentOpen>(body).ok())
        .map(|open| open.place))
}

/// The site's conversation segment `object` goes in, settled now when it is
/// not yet: its first line came at `at`.
async fn segment_session(
    db: &DatabaseConnection,
    user: i32,
    object: &str,
    at: DateTime<Utc>,
) -> Result<String, Error> {
    if let Some(session) = settled(db, user, object).await? {
        return Ok(session);
    }
    let session = match last_talked(db, user, at, false).await? {
        Some(session) => session,
        None => new_private_chat(db, user).await?,
    };
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_her_segments (user_id, object, session_id) VALUES ($1, $2, $3) \
         ON CONFLICT (user_id, object) DO NOTHING",
        [user.into(), object.into(), session.into()],
    ))
    .await?;
    // Another fold may have settled it first.
    settled(db, user, object)
        .await?
        .ok_or_else(|| Error::Refused("a segment's conversation was not kept".into()))
}

async fn settled(
    db: &DatabaseConnection,
    user: i32,
    object: &str,
) -> Result<Option<String>, Error> {
    Ok(db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT session_id FROM agent_her_segments WHERE user_id = $1 AND object = $2",
            [user.into(), object.into()],
        ))
        .await?
        .map(|row| row.try_get("", "session_id"))
        .transpose()?)
}

/// `user`'s private chat with her that has the last line before `at`, when
/// that line is less than [`GOES_ON_WITHIN_HOURS`] old then; with `any`,
/// whichever was talked in last when none is.
async fn last_talked(
    db: &DatabaseConnection,
    user: i32,
    at: DateTime<Utc>,
    any: bool,
) -> Result<Option<String>, Error> {
    const PRIVATE: &str = "s.user_id = $1 AND s.context->>'mode' = 'chat' \
        AND s.context->>'venue' IS NULL AND NOT s.archived";
    let within = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT s.id FROM agent_sessions s JOIN agent_messages m ON m.session_id = s.id \
                 WHERE {PRIVATE} AND m.created_at <= $2 AND m.created_at > $3 \
                 ORDER BY m.created_at DESC, m.id DESC LIMIT 1"
            ),
            [
                user.into(),
                at.into(),
                (at - Duration::hours(GOES_ON_WITHIN_HOURS)).into(),
            ],
        ))
        .await?;
    let row = match within {
        Some(row) => Some(row),
        None if any => {
            db.query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                format!("SELECT s.id FROM agent_sessions s WHERE {PRIVATE} ORDER BY s.last_active_at DESC LIMIT 1"),
                [user.into()],
            ))
            .await?
        }
        None => None,
    };
    Ok(row.map(|row| row.try_get("", "id")).transpose()?)
}

async fn new_private_chat(db: &DatabaseConnection, user: i32) -> Result<String, Error> {
    crate::services::agent::sessions::ensure_session_in(
        db,
        None,
        user,
        crate::services::agent::AgentInteractionMode::Chat,
        None,
    )
    .await
    .map_err(Error::Refused)
}

/// `text` without the pictures in it (`![…](…)`), trimmed, and how many
/// there were.
fn without_pictures(text: &str) -> (String, u32) {
    static PICTURE: std::sync::LazyLock<regex::Regex> =
        std::sync::LazyLock::new(|| regex::Regex::new(r"!\[[^\]]*\]\([^)]*\)").unwrap());
    let pictures = PICTURE.find_iter(text).count() as u32;
    (PICTURE.replace_all(text, "").trim().to_string(), pictures)
}

#[cfg(test)]
mod tests {
    use super::without_pictures;

    #[test]
    fn pictures_are_counted_and_taken_out() {
        assert_eq!(
            without_pictures("看 ![猫](/api/media/1) 和 ![](/api/media/2)"),
            ("看  和".to_string(), 2)
        );
        assert_eq!(without_pictures("  早  "), ("早".to_string(), 0));
        assert_eq!(without_pictures("![a](b)"), (String::new(), 1));
    }
}
