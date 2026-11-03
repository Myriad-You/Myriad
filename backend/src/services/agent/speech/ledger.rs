//! The ledger of what she said: one `speech.said` row a line, unasked or
//! called on, with how much of it went out.
//!
//! Kept on the site in `agent_speech_said`, the rows as her log keeps them
//! (`audience`, `payload`), so the core reads them as they are
//! (`Said::from_row`). While her log on the server is on and her mind is
//! here, each row is written into her log as well; across sides it travels
//! once her link speaks protocol 2.
//!
//! Migration, without touching what is there: before the ledger, what she
//! said unasked in private was kept only in `agent_proactive_messages`, and
//! when she last spoke up in `agent_addressee_state.last_proactive_at`.
//! Reading a private talk's ledger also reads that table's lines from
//! before the talk's first ledger row, so nothing she said is forgotten on
//! the day this starts; `last_proactive_at` is no longer written or read
//! for judging. The table stays the transcript of her unasked lines in a
//! private talk (her conversation on the app and in an export shows it), so
//! a line written here is copied there ([`Transcript`]); nothing judges by
//! it any more.

use chrono::{DateTime, Utc};
use myriad_agent_rules::memory::audience::{Audience, Venue};
use myriad_merope::speech::{Extent, Said, Source};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::Value;

/// How far back a judgment reads the lines to one audience.
const BACK_DAYS: i64 = 7;
/// The most rows a judgment reads.
const MOST: i64 = 128;

/// A line she said.
pub struct Line<'a> {
    pub to: &'a Audience<String>,
    pub source: Source,
    pub called: bool,
    /// The thing it was about, by its key.
    pub thing: Option<&'a str>,
    pub text: &'a str,
}

/// How it went out.
#[derive(Debug, Clone)]
pub struct Out {
    /// `web`, `app`, `group`, or a chat app's slug.
    pub place: String,
    /// The way out: `face`, `app`, `notice`, `im`, `chat`, `group`.
    pub exit: String,
    pub extent: Extent,
    /// The site's own id for it (a message's, a `say`'s).
    pub line: Option<String>,
    pub heard: Option<u32>,
    pub interrupted: bool,
    pub group_name: Option<String>,
}

impl Out {
    /// All of it went out by `exit` at `place`.
    pub fn full(place: impl Into<String>, exit: impl Into<String>) -> Self {
        Self {
            place: place.into(),
            exit: exit.into(),
            extent: Extent::Full,
            line: None,
            heard: None,
            interrupted: false,
            group_name: None,
        }
    }

    pub fn extent(mut self, extent: Extent) -> Self {
        self.extent = extent;
        self
    }

    pub fn line(mut self, line: Option<String>) -> Self {
        self.line = line;
        self
    }
}

/// A private line she said of her own accord (or for a touch) copied into
/// her transcript there
/// (`agent_proactive_messages`): the event it came of, whether it went out
/// as a notification, the `say` it went out as on an app.
pub struct Transcript<'a> {
    pub event_key: &'a str,
    pub notified: bool,
    pub say_id: Option<&'a str>,
}

/// The ledger's audience for `to`: `personal:<user id>` or `group:<venue>`.
fn audience_key(to: &Audience<String>) -> Option<String> {
    match &to.venue {
        Venue::Group(venue) => Some(format!("group:{venue}")),
        Venue::Private => super::user_of(to).map(|user| format!("personal:{user}")),
        Venue::Own => None,
    }
}

fn word<T: serde::Serialize>(value: T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

/// The row's payload: `speech.said` as her log keeps it.
fn payload(line: &Line<'_>, out: &Out, at: DateTime<Utc>) -> Value {
    serde_json::to_value(myriad_persona_log::payloads::SpeechSaid {
        at: at.timestamp_millis(),
        text: line.text.to_string(),
        place: out.place.clone(),
        source: word(line.source),
        called: line.called,
        thing: line.thing.map(str::to_string),
        exit: out.exit.clone(),
        extent: word(out.extent),
        heard: out.heard,
        interrupted: out.interrupted,
        line: out.line.clone(),
        group_name: out.group_name.clone(),
    })
    .unwrap_or_default()
}

/// Keeps `line` as she said it by `out`, now: in the ledger, in her log
/// when it is on, and, with `transcript`, in her transcript there.
/// Its id, when it was kept.
pub async fn write(
    db: &DatabaseConnection,
    line: Line<'_>,
    out: Out,
    transcript: Option<Transcript<'_>>,
) -> Option<String> {
    let text = line.text.trim();
    if text.is_empty() {
        return None;
    }
    let line = Line { text, ..line };
    let audience = audience_key(line.to)?;
    let at = crate::clock::now();
    let payload = payload(&line, &out, at);
    if let Err(error) = myriad_persona_log::payloads::check("speech.said", &payload) {
        tracing::warn!(%error, "[Speech] a line she said does not read as one");
        return None;
    }
    let id = uuid::Uuid::new_v4().simple().to_string();
    let user = super::user_of(line.to);
    let kept = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_speech_said (id, audience, user_id, thing, called, at, payload) \
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            [
                id.clone().into(),
                audience.into(),
                user.into(),
                line.thing.map(str::to_string).into(),
                line.called.into(),
                at.into(),
                payload.clone().into(),
            ],
        ))
        .await;
    if let Err(error) = kept {
        tracing::warn!(%error, "[Speech] a line she said was not kept");
        return None;
    }
    if let (Some(user), Some(transcript)) = (user, transcript)
        && out.extent.reached()
        && let Err(error) = crate::services::agent::merope::store::insert_proactive(
            db,
            user,
            text,
            Some(transcript.event_key),
            transcript.notified,
            transcript.say_id,
        )
        .await
    {
        tracing::warn!(%error, user, "[Speech] her line was not kept in her transcript");
    }
    if crate::services::her_log::enabled() {
        into_her_log(db, line.to, &id, &payload).await;
    }
    Some(id)
}

/// The row in her log on the server, when the owner's account keeps one.
async fn into_her_log(db: &DatabaseConnection, to: &Audience<String>, id: &str, payload: &Value) {
    use crate::services::her_log;
    let Ok(site_owner) = crate::services::site_owner::site_owner_user_id(db).await else {
        return;
    };
    let on = her_log::store::account(db, site_owner)
        .await
        .ok()
        .flatten()
        .is_some_and(|account| account.on());
    if !on {
        return;
    }
    let audience = match (&to.venue, super::user_of(to)) {
        (Venue::Group(venue), _) => her_log::lend::group_audience(venue),
        (Venue::Private, Some(user)) => match her_log::lend::chat_audience(db, user).await {
            Ok(audience) => audience,
            Err(_) => return,
        },
        _ => return,
    };
    let written = match her_log::owner(db, site_owner).await {
        Ok(owner) => {
            her_log::store::write_hers(
                db,
                her_log::keys::root(),
                &owner,
                &audience,
                None,
                &format!("speech:{id}"),
                "speech.said",
                payload,
            )
            .await
        }
        Err(error) => Err(error),
    };
    if let Err(error) = written {
        tracing::debug!(%error, "[Speech] her line stays on the site only");
    }
}

fn said_of(row: &sea_orm::QueryResult) -> Option<Said> {
    let audience: String = row.try_get("", "audience").ok()?;
    let payload: Value = row.try_get("", "payload").ok()?;
    Said::from_row(&audience, None, &payload)
}

/// The ledger rows that bear on saying something to `to` about `thing`:
/// the lines to `to` in the last days and the lines about `thing` anywhere,
/// newest first. A private talk's older lines come from before the ledger
/// (see the module's note on migration).
pub async fn bearing(
    db: &DatabaseConnection,
    to: &Audience<String>,
    thing: Option<&str>,
) -> Vec<Said> {
    let Some(audience) = audience_key(to) else {
        return Vec::new();
    };
    let since = crate::clock::now() - chrono::Duration::days(BACK_DAYS);
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT audience, payload FROM agent_speech_said \
             WHERE (audience = $1 AND at > $2) OR ($3::text IS NOT NULL AND thing = $3) \
             ORDER BY at DESC LIMIT $4",
            [
                audience.clone().into(),
                since.into(),
                thing.map(str::to_string).into(),
                MOST.into(),
            ],
        ))
        .await
        .unwrap_or_default();
    let mut said: Vec<Said> = rows.iter().filter_map(said_of).collect();
    if let Some(user) = super::user_of(to) {
        said.extend(before_the_ledger(db, user, &audience, since).await);
    }
    said.sort_by_key(|said| std::cmp::Reverse(said.at));
    said
}

/// What she said unasked to `user` in private after `since` and before the
/// first ledger row of that talk, from `agent_proactive_messages`.
async fn before_the_ledger(
    db: &DatabaseConnection,
    user: i32,
    audience: &str,
    since: DateTime<Utc>,
) -> Vec<Said> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT created_at, content, event_key, notified FROM agent_proactive_messages p \
             WHERE p.user_id = $1 AND p.role = 'assistant' AND p.created_at > $2 \
               AND p.created_at < COALESCE( \
                 (SELECT min(at) FROM agent_speech_said WHERE audience = $3), 'infinity') \
             ORDER BY p.created_at DESC LIMIT $4",
            [user.into(), since.into(), audience.into(), MOST.into()],
        ))
        .await
        .unwrap_or_default();
    rows.iter()
        .filter_map(|row| {
            let at = row
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .ok()?;
            let text: String = row.try_get("", "content").ok()?;
            let event_key: Option<String> = row.try_get("", "event_key").ok()?;
            let notified: bool = row.try_get("", "notified").ok()?;
            let source = super::source_of_event(event_key.as_deref().unwrap_or_default());
            Some(Said {
                at: super::micros(at.with_timezone(&Utc)),
                to: super::private(user),
                text,
                place: "web".into(),
                source: Some(source),
                called: source == Source::Touch,
                thing: None,
                exit: if notified { "notice" } else { "face" }.into(),
                extent: Extent::Full,
                heard: None,
                interrupted: false,
                line: None,
                group_name: None,
            })
        })
        .collect()
}

/// When she last spoke up to `to` unasked (her words reached them).
pub async fn last_unasked_at(
    db: &DatabaseConnection,
    to: &Audience<String>,
) -> Option<DateTime<Utc>> {
    bearing(db, to, None)
        .await
        .into_iter()
        .find(|said| !said.called && said.reached())
        .and_then(|said| DateTime::<Utc>::from_timestamp_micros(said.at))
}

/// When she last said something to `user` in private whose thing's key
/// starts with `prefix`, or, before the ledger, something of `event_key`.
pub async fn last_about(
    db: &DatabaseConnection,
    user: i32,
    prefix: &str,
    event_key: &str,
) -> Option<DateTime<Utc>> {
    let audience = audience_key(&super::private(user))?;
    let kept = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT max(at) AS at FROM agent_speech_said \
             WHERE audience = $1 AND left(thing, length($2)) = $2 \
               AND (payload->>'extent') IN ('full', 'gist', 'notice')",
            [audience.clone().into(), prefix.into()],
        ))
        .await
        .ok()
        .flatten()
        .and_then(|row| {
            row.try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "at")
                .ok()
                .flatten()
        })
        .map(|at| at.with_timezone(&Utc));
    let old = old_about(db, user, &audience, event_key).await;
    kept.into_iter().chain(old).max()
}

/// When she last said something of `event_key` to `user` before the
/// talk's first ledger row.
async fn old_about(
    db: &DatabaseConnection,
    user: i32,
    audience: &str,
    event_key: &str,
) -> Option<DateTime<Utc>> {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT max(created_at) AS at FROM agent_proactive_messages \
         WHERE user_id = $1 AND event_key = $2 \
           AND created_at < COALESCE( \
             (SELECT min(at) FROM agent_speech_said WHERE audience = $3), 'infinity')",
        [user.into(), event_key.into(), audience.into()],
    ))
    .await
    .ok()
    .flatten()?
    .try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", "at")
    .ok()
    .flatten()
    .map(|at| at.with_timezone(&Utc))
}
