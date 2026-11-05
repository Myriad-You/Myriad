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

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::Value;

use myriad_persona_log::{
    Row,
    payloads::{SegmentLine, SegmentOpen},
};

use super::{Error, NODE, keys, store};

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
    store::write_owners(
        db,
        root,
        &owner,
        &audience,
        None,
        &object,
        "segment.line",
        &serde_json::to_value(line).unwrap_or_default(),
    )
    .await
    .map(Some)
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
