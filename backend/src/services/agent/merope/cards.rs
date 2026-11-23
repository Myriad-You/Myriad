//! Her cards in her private talk with the owner on the site: what she puts
//! before them, one thing at a time (`myriad_nodes::cards`), kept with her
//! lines there (`agent_proactive_messages`).
//!
//! A card comes into the talk as soon as it is made, on a line of its own
//! (role `card`, no text): it never waits for her to say something of it.
//! When her line about it goes out, that line carries the card (the same
//! id), and the card as it stands moves to it: only the latest line that
//! carries a card holds it whole (`card`: the card, its version, the
//! owner's answer to that version, whether it waits on them); the earlier
//! ones keep only its id, and show as a line pointing to it.
//!
//! A newer version of the card takes the place of the old on that line,
//! unless it asks the owner again (the one before asked nothing, or was
//! answered): then it comes on a new line. The same card again (all but
//! when it was made) changes nothing.
//!
//! The owner answers on the site or on a child (`card.answer`). The same
//! card, version and action is one answer however often it comes; an
//! answer to an older version is kept as the answer to what they saw then,
//! and the latest still waits. Each answer is told to her as a fact she
//! can check, and she answers it as she would.
//!
//! Every metadata change announces itself on `myriad_conversations`, so a
//! child kept current hears of it as of a new line.

use myriad_persona_log::payloads::{CardAction, CardAnswered, CardVersion};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DatabaseTransaction, DbErr, Statement,
    TransactionTrait,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::services::her_log;

mod forget;
mod lately;
pub(crate) use forget::forget_about;
pub(crate) use lately::lately;

/// The event a line that is only a card is kept under.
pub(crate) const CARD_EVENT: &str = "agent.merope.card";
/// The role of a line that is only a card: never one of her lines, so
/// nothing that counts what she said counts it.
pub(crate) const CARD_ROLE: &str = "card";
/// What she is told of, at most, of the cards before the owner now.
pub(crate) const LATELY: i64 = 5;

/// One version of a card as it was put before the owner: its id, and what
/// it let them answer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Version {
    pub id: String,
    #[serde(default)]
    pub actions: Vec<CardAction>,
    /// It went into her log as its row, under this id.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub logged: bool,
}

/// One answer of the owner's.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Answer {
    pub at: i64,
    pub action: String,
    pub version: String,
}

/// The card as it stands, as the latest line that carries it holds it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct Now {
    pub card: CardVersion,
    pub version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answered: Option<CardAnswered>,
    pub waiting: bool,
}

/// What the latest line that carries a card keeps of it.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Held {
    card: Now,
    #[serde(default)]
    card_versions: Vec<Version>,
    #[serde(default)]
    card_answers: Vec<Answer>,
}

impl Held {
    fn into_fields(self) -> serde_json::Map<String, Value> {
        match serde_json::to_value(self) {
            Ok(Value::Object(fields)) => fields,
            _ => serde_json::Map::new(),
        }
    }

    fn of(metadata: &Value) -> Option<Self> {
        serde_json::from_value(json!({
            "card": metadata.get("card")?,
            "cardVersions": metadata.get("cardVersions").cloned().unwrap_or(json!([])),
            "cardAnswers": metadata.get("cardAnswers").cloned().unwrap_or(json!([])),
        }))
        .ok()
    }
}

fn with_held(metadata: Option<Value>, held: Held) -> Value {
    let mut metadata = metadata
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    if let Some(fields) = metadata.as_object_mut() {
        fields.extend(held.into_fields());
    }
    metadata
}

fn new_id(prefix: &str) -> String {
    format!("{prefix}_{}", uuid::Uuid::new_v4().simple())
}

/// A card as two of its versions compare: all but when it was made.
fn same(one: &CardVersion, other: &CardVersion) -> bool {
    let mut one = one.clone();
    one.at = other.at;
    one == *other
}

/// One card's lines are changed by one writer at a time.
async fn lock(txn: &DatabaseTransaction, owner: i32, card: &str) -> Result<(), DbErr> {
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtext($1))",
        [format!("card:{owner}:{card}").into()],
    ))
    .await
    .map(|_| ())
}

/// Her conversations with the owner changed: a child kept current reads
/// them again (an insert announces itself; a changed metadata does not).
async fn announce<C: ConnectionTrait>(db: &C, owner: i32) -> Result<(), DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_notify($1, $2)",
        [
            migration::CONVERSATIONS_CHANNEL.into(),
            owner.to_string().into(),
        ],
    ))
    .await
    .map(|_| ())
}

/// The latest line that holds `card` whole: its id and metadata.
async fn holder<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    card: &str,
) -> Result<Option<(i64, Value)>, DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, metadata FROM agent_proactive_messages \
             WHERE user_id = $1 AND metadata->>'cardId' = $2 AND metadata->'card' IS NOT NULL \
             ORDER BY created_at DESC, id DESC LIMIT 1",
            [owner.into(), card.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    Ok(Some((
        row.try_get("", "id")?,
        row.try_get::<Option<Value>>("", "metadata")?
            .unwrap_or_else(|| json!({})),
    )))
}

/// Sets `fields` on line `id`'s metadata, leaving its other fields as
/// they are now: what else marks the line (cut off while said aloud) is
/// written beside the card, never over it.
async fn merge<C: ConnectionTrait>(db: &C, id: i64, fields: Value) -> Result<(), DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_proactive_messages \
         SET metadata = (CASE WHEN jsonb_typeof(metadata) = 'object' THEN metadata \
           ELSE '{}'::jsonb END) || $2::jsonb WHERE id = $1",
        [id.into(), fields.into()],
    ))
    .await
    .map(|_| ())
}

/// The whole card goes from line `id`: it keeps only the card's id.
async fn let_go<C: ConnectionTrait>(db: &C, id: i64) -> Result<(), DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_proactive_messages \
         SET metadata = metadata - 'card' - 'cardVersions' - 'cardAnswers' WHERE id = $1",
        [id.into()],
    ))
    .await
    .map(|_| ())
}

/// A new line that is only `held`'s card.
async fn card_line<C: ConnectionTrait>(db: &C, owner: i32, held: Held) -> Result<(), DbErr> {
    let card = held.card.card.id.clone();
    let metadata = with_held(
        Some(json!({ "cardId": card, "line": new_id("card") })),
        held,
    );
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_proactive_messages (user_id, role, content, event_key, notified, \
         created_at, metadata) VALUES ($1, $2, '', $3, false, $4, $5)",
        [
            owner.into(),
            CARD_ROLE.into(),
            CARD_EVENT.into(),
            crate::clock::now().fixed_offset().into(),
            metadata.into(),
        ],
    ))
    .await
    .map(|_| ())
}

/// Runs `work` in one transaction that ends either way before this
/// returns: committed when it went well, else rolled back. A transaction
/// only dropped would hold its card's lock until its connection is next
/// used.
async fn in_one<T, E, F>(db: &DatabaseConnection, work: F) -> Result<T, E>
where
    E: From<DbErr>,
    F: for<'t> FnOnce(
        &'t DatabaseTransaction,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<T, E>> + Send + 't>,
    >,
{
    let txn = db.begin().await?;
    match work(&txn).await {
        Ok(done) => {
            txn.commit().await?;
            Ok(done)
        }
        Err(error) => {
            if let Err(rollback) = txn.rollback().await {
                tracing::warn!(%rollback, "[cards] a card's change was not rolled back");
            }
            Err(error)
        }
    }
}

/// What became of a card put before the owner.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Put {
    /// On a new line of its own.
    New,
    /// In the place of the version before, on the line that holds it.
    Changed,
    /// The same as the version before: nothing changed.
    Same,
}

/// Puts `card` before the owner, checked first as the wire would take it
/// (its words, ids and bounds, and the secret scan).
pub(crate) async fn put(
    db: &DatabaseConnection,
    owner: i32,
    card: CardVersion,
) -> Result<Put, anyhow::Error> {
    let card = myriad_nodes::cards::check_call(myriad_nodes::cards::CardCheckInput {
        card: serde_json::to_value(&card)?,
    })
    .map_err(|error| anyhow::anyhow!("{error}"))?
    .card;
    let log = her_log::cards::card_into_log(db, owner, &card)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(%error, "[cards] whether cards go into her log is not known");
            None
        });
    let (id, logged) = (card.id.clone(), log.is_some());
    let put = in_one(db, move |txn| Box::pin(put_in(txn, owner, card, logged))).await?;
    if let Some(log) = log
        && put != Put::Same
        && let Some(now) = now(db, owner, &id).await?
        && let Err(error) = her_log::cards::version(db, &log, &now.card, &now.version).await
    {
        tracing::warn!(%error, "[cards] a card's version is not in her log");
    }
    Ok(put)
}

async fn put_in(
    txn: &DatabaseTransaction,
    owner: i32,
    card: CardVersion,
    logged: bool,
) -> Result<Put, anyhow::Error> {
    lock(txn, owner, &card.id).await?;
    let version = Version {
        id: new_id("v"),
        actions: card.actions.clone(),
        logged,
    };
    let asks = myriad_nodes::cards::asks(&card);
    let now = Now {
        card,
        version: version.id.clone(),
        answered: None,
        waiting: asks,
    };
    Ok(match holder(txn, owner, &now.card.id).await? {
        None => {
            card_line(
                txn,
                owner,
                Held {
                    card: now,
                    card_versions: vec![version],
                    card_answers: Vec::new(),
                },
            )
            .await?;
            Put::New
        }
        Some((id, metadata)) => {
            let Some(mut held) = Held::of(&metadata) else {
                return Err(anyhow::anyhow!("a card's line does not hold it"));
            };
            if same(&now.card, &held.card.card) {
                return Ok(Put::Same);
            }
            // The version before asked nothing, or was answered.
            let before_done =
                !myriad_nodes::cards::asks(&held.card.card) || held.card.answered.is_some();
            held.card_versions.push(version);
            held.card = now;
            if asks && before_done {
                // It asks them again: anew, on a line of its own.
                let_go(txn, id).await?;
                card_line(txn, owner, held).await?;
                Put::New
            } else {
                merge(txn, id, Value::Object(held.into_fields())).await?;
                announce(txn, owner).await?;
                Put::Changed
            }
        }
    })
}

/// The card as it stands now, if the owner has it.
pub(crate) async fn now<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    card: &str,
) -> Result<Option<Now>, DbErr> {
    Ok(holder(db, owner, card)
        .await?
        .and_then(|(_, metadata)| Held::of(&metadata))
        .map(|held| held.card))
}

/// Her line `line` (its row `row`), just kept, carries `card`: the card as
/// it stands moves to it. Nothing when the owner has no such card (it was
/// not let through).
pub(crate) async fn carry(
    db: &DatabaseConnection,
    owner: i32,
    row: i64,
    card: &str,
    line: &str,
) -> Result<(), DbErr> {
    let (card, line) = (card.to_string(), line.to_string());
    in_one(db, move |txn| {
        Box::pin(carry_in(txn, owner, row, card, line))
    })
    .await
}

async fn carry_in(
    txn: &DatabaseTransaction,
    owner: i32,
    row: i64,
    card: String,
    line: String,
) -> Result<(), DbErr> {
    let card = card.as_str();
    lock(txn, owner, card).await?;
    let Some((id, metadata)) = holder(txn, owner, card).await? else {
        return Ok(());
    };
    if id != row {
        let Some(held) = Held::of(&metadata) else {
            return Ok(());
        };
        let_go(txn, id).await?;
        let mut carried = held.into_fields();
        carried.insert("cardId".into(), card.into());
        carried.insert("line".into(), line.into());
        merge(txn, row, Value::Object(carried)).await?;
        announce(txn, owner).await?;
    }
    Ok(())
}

/// What became of an answer, as `card.answered` says it.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Answered {
    pub kept: bool,
    /// Why not, or `card_stale`.
    pub code: Option<&'static str>,
    /// The card as it now stands.
    pub now: Option<Now>,
}

impl Answered {
    fn refused(code: &'static str) -> Self {
        Self {
            kept: false,
            code: Some(code),
            now: None,
        }
    }
}

/// The owner answered `card`'s `version` with `action` (on the site, or on
/// a child). Kept once however often it comes; told to her once; written
/// into her log when the version is there.
pub(crate) async fn answer(
    db: &DatabaseConnection,
    owner: i32,
    card: &str,
    version: &str,
    action: &str,
) -> Result<Answered, DbErr> {
    answer_as(db, owner, card, version, action, true).await
}

/// [`answer`]; `write` false for an answer that came from her log.
pub(crate) async fn answer_as(
    db: &DatabaseConnection,
    owner: i32,
    card: &str,
    version: &str,
    action: &str,
    write: bool,
) -> Result<Answered, DbErr> {
    let id = card.to_string();
    let (card, version, action) = (card.to_string(), version.to_string(), action.to_string());
    let (answered, told) = in_one(db, move |txn| {
        Box::pin(answer_in(txn, owner, card, version, action))
    })
    .await?;
    if let (Some((chosen, latest, version, logged)), Some(now)) = (told, &answered.now) {
        if write && logged {
            log_answer(db, owner, &id, &chosen.id, &version).await;
        }
        tell_her(db, owner, &now.card, &chosen, latest, &version);
    }
    Ok(answered)
}

async fn log_answer(db: &DatabaseConnection, owner: i32, card: &str, action: &str, version: &str) {
    let answer = CardAnswered {
        at: crate::clock::now().timestamp_millis(),
        action: action.to_string(),
        version: version.to_string(),
    };
    let written = match her_log::cards::into_log(db, owner).await {
        Ok(Some(log)) => her_log::cards::answered(db, &log, card, &answer).await,
        Ok(None) => Ok(()),
        Err(error) => Err(error),
    };
    if let Err(error) = written {
        tracing::warn!(%error, "[cards] an answer is not in her log");
    }
}

/// The answer kept, and what to tell her of it when it is new: what they
/// chose, whether on the latest version, which version, and whether that
/// version is in her log.
type Kept = (Answered, Option<(CardAction, bool, String, bool)>);

async fn answer_in(
    txn: &DatabaseTransaction,
    owner: i32,
    card: String,
    version: String,
    action: String,
) -> Result<Kept, DbErr> {
    let (card, version, action) = (card.as_str(), version.as_str(), action.as_str());
    lock(txn, owner, card).await?;
    let Some((id, metadata)) = holder(txn, owner, card).await? else {
        return Ok((Answered::refused("card_unknown"), None));
    };
    let Some(mut held) = Held::of(&metadata) else {
        return Ok((Answered::refused("card_unknown"), None));
    };
    let Some(answered) = held
        .card_versions
        .iter()
        .find(|known| known.id == version)
        .cloned()
    else {
        return Ok((Answered::refused("card_unknown"), None));
    };
    if !answered
        .actions
        .iter()
        .any(|known| known.id != myriad_nodes::cards::OPEN)
    {
        return Ok((Answered::refused("card_closed"), None));
    }
    let Some(chosen) = answered
        .actions
        .iter()
        .find(|known| known.id == action && known.id != myriad_nodes::cards::OPEN)
        .cloned()
    else {
        return Ok((Answered::refused("card_action_unknown"), None));
    };
    let latest = held.card.version == version;
    let code = (!latest).then_some("card_stale");
    if held
        .card_answers
        .iter()
        .any(|known| known.version == version && known.action == action)
    {
        return Ok((
            Answered {
                kept: true,
                code,
                now: Some(held.card),
            },
            None,
        ));
    }
    let at = crate::clock::now().timestamp_millis();
    held.card_answers.push(Answer {
        at,
        action: action.to_string(),
        version: version.to_string(),
    });
    if latest {
        held.card.answered = Some(CardAnswered {
            at,
            action: action.to_string(),
            version: version.to_string(),
        });
        held.card.waiting = false;
    }
    let now = held.card.clone();
    merge(txn, id, Value::Object(held.into_fields())).await?;
    announce(txn, owner).await?;
    Ok((
        Answered {
            kept: true,
            code,
            now: Some(now),
        },
        Some((chosen, latest, version.to_string(), answered.logged)),
    ))
}

/// The owner's answer, as a fact she can check.
pub(crate) fn fact(card: &CardVersion, chosen: &CardAction, latest: bool) -> String {
    let mut fact = format!("站长在卡片「{}」上选了：{}", card.title, chosen.label);
    if !latest {
        fact.push_str("（答的是这张卡更早的一版）");
    }
    fact
}

/// Her the owner's answer, to answer as she would.
fn tell_her(
    db: &DatabaseConnection,
    owner: i32,
    card: &CardVersion,
    chosen: &CardAction,
    latest: bool,
    version: &str,
) {
    use crate::services::agent::consciousness::{
        EventUrgency, enqueue_speak_intent, new_speak_intent,
    };
    let fact = fact(card, chosen, latest);
    let mut intent = new_speak_intent(
        owner,
        format!("card:{}:{version}:{}", card.id, chosen.id),
        crate::services::agent::merope::gates::PEER_AGENT_EVENT.to_string(),
        fact.clone(),
        EventUrgency::Normal,
        None,
    );
    intent.happened = Some(fact);
    enqueue_speak_intent(intent);
    if cfg!(test) {
        return;
    }
    let db = db.clone();
    crate::services::agent::merope::background::spawn("speak intents", async move {
        crate::services::agent::merope::ingest::tick_speak_intents(db).await;
    });
}

/// A line that is a card or carries one, as a conversation shows it: the
/// card's id and the line's own id, and on the latest that carries it the
/// card as it stands.
pub(crate) fn shown(metadata: Option<&Value>) -> Option<Value> {
    let metadata = metadata?;
    let card = metadata.get("cardId")?.as_str()?;
    let mut shown = json!({ "cardId": card });
    if let Some(line) = metadata.get("line").and_then(Value::as_str) {
        shown["line"] = line.into();
    }
    if let Some(now) = metadata.get("card").filter(|now| now.is_object()) {
        shown["card"] = now.clone();
    }
    Some(shown)
}

/// The lines of the owner's private chat `session` that are a card or
/// carry one, oldest first, each where the chat stood when it came: the
/// chat with the newest line at or before it. As the site's chat shows
/// them: `{id, role, content, createdAt, cardId, line, card?}`.
pub(crate) async fn lines_in(
    db: &DatabaseConnection,
    owner: i32,
    session: &str,
) -> Result<Vec<Value>, DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT p.id, p.role, p.content, p.created_at, p.metadata, \
               (SELECT m.session_id FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
                 WHERE s.user_id = p.user_id AND s.context->>'mode' = 'chat' \
                   AND s.context->>'venue' IS NULL AND NOT s.archived \
                   AND m.created_at <= p.created_at \
                 ORDER BY m.created_at DESC, m.id DESC LIMIT 1) AS placed \
             FROM agent_proactive_messages p \
             WHERE p.user_id = $1 AND p.metadata->>'cardId' IS NOT NULL \
             ORDER BY p.created_at DESC, p.id DESC LIMIT $2",
            [owner.into(), (LINES as i64).into()],
        ))
        .await?;
    let mut lines = Vec::new();
    for row in rows.iter().rev() {
        let placed: Option<String> = row.try_get("", "placed")?;
        if placed.as_deref() != Some(session) {
            continue;
        }
        let metadata: Option<Value> = row.try_get("", "metadata")?;
        let Some(Value::Object(card)) = shown(metadata.as_ref()) else {
            continue;
        };
        let id: i64 = row.try_get("", "id")?;
        let at: chrono::DateTime<chrono::FixedOffset> = row.try_get("", "created_at")?;
        let mut line = json!({
            "id": format!("pro_{id}"),
            "role": row.try_get::<String>("", "role")?,
            "content": row.try_get::<String>("", "content")?,
            "createdAt": at.to_rfc3339(),
        });
        if let Some(fields) = line.as_object_mut() {
            fields.extend(card);
        }
        lines.push(line);
    }
    Ok(lines)
}

/// The newest lines that are a card or carry one, looked at for a chat.
const LINES: usize = 30;

#[cfg(test)]
mod tests;
