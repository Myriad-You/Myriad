//! What she takes from a private talk for later (decision D5, her intents):
//! after she answers each message of theirs in their own private talk, she
//! judges as herself whether it leaves her something. Their direct
//! instruction to her (「明天九点提醒我」) becomes the partner's intent, to
//! say or to do; their own plan is nothing; something she herself wants to
//! bring up becomes hers, and hers only say.
//!
//! One small JSON call on the judging model, no tools, the native host's
//! judgment (`NodeIntentTurn`): what she is given and how her answer is read
//! are the core's (`myriad_nodes::intents::prepare`, `read`): their message
//! and the lines before it, the names of her nodes their words find, what
//! waits, the time and her zone. Nothing that sits on a node goes to it.
//!
//! Every take passes the core's gate (`intents::admit`): the partner's
//! rests on their own message in their own talk (`message:<id>`, the one
//! evidence id the site names as theirs) and has something to wake it. What
//! passes is a `node.intent` row of hers on the node it is about, or on
//! 「关于你」; the partner's instruction to work stays on the site
//! (`sync: local`), the rest is shared. What only the site keeps of the
//! instruction (who, where, the grants in effect, her line back) goes
//! beside it ([`super::intent_store::Instruction`]). What their words call
//! off is dropped.
//!
//! Only for a logged-in account's private talk, only while her mind is here.

use std::time::Duration;

use chrono::{DateTime, Utc};
use myriad_nodes::{
    body::{Action, IntentStatus, Origin, Trigger},
    entries::row_id_from,
    intents::{
        self, AdmitInput, JudgeInput, Line, Note, Prepared, ReadInput, Refusal, TakeKind, Waiting,
        Who,
    },
    trust::Known,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use super::{
    Error, audience,
    intent_store::{self, Held, Instruction},
    known, write,
};
use crate::services::agent::merope::call::{Ask, Voice};
use crate::services::her_log::{NODE, pen::Thought};

/// How long the judgment may take.
const TIMEOUT: Duration = Duration::from_secs(20);
/// The judgment as billed, and as its schema goes out.
pub(crate) const OPERATION: &str = "node_intents";
/// Lines before their message sent: more than the core keeps, which cuts
/// to its own.
const LOOK_BACK: i64 = 8;

/// What one turn's judgment came to.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct Judged {
    /// The intents taken (ids).
    pub taken: Vec<String>,
    /// The waiting intents their words called off (ids).
    pub dropped: Vec<String>,
    /// Their own plans, and takes the gate refused: counted, never kept.
    pub plans: usize,
    pub refused: usize,
}

/// After she answered `request` in a private talk: the judgment, in the
/// background. Nothing waits for it.
pub(crate) fn spawn_after_turn(
    db: DatabaseConnection,
    request: &crate::services::agent::UserRequest,
) {
    let user_id = request.user_id;
    let Some(context) = request.context.as_ref() else {
        return;
    };
    if user_id <= 0 || context.venue.is_some() || context.nobody_said {
        return;
    }
    let Some(session) = context.session_id.clone() else {
        return;
    };
    if request.raw_input.trim().is_empty() {
        return;
    }
    let said = request.raw_input.clone();
    crate::services::agent::merope::background::spawn("her intents", async move {
        if !crate::services::agent::merope::is_enabled().await {
            return;
        }
        let site_owner = crate::services::principal::site_owner_id(&db)
            .await
            .ok()
            .flatten();
        let now = crate::services::agent::merope::clock::now();
        let judged = tokio::time::timeout(
            Duration::from_secs(60),
            after_turn(&db, user_id, site_owner, &session, &said, now),
        )
        .await;
        match judged {
            Ok(Ok(judged)) if !judged.taken.is_empty() || !judged.dropped.is_empty() => {
                tracing::info!(
                    taken = judged.taken.len(),
                    dropped = judged.dropped.len(),
                    "[memory-nodes] she took something for later"
                )
            }
            Ok(Ok(_)) => {}
            Ok(Err(error)) => tracing::warn!(%error, "[memory-nodes] her intents not judged"),
            Err(_) => tracing::info!("[memory-nodes] her intents ran out of time"),
        }
    });
}

/// Their message in `session` this turn answered: the one of theirs saying
/// `said`, else their latest; only in their own private talk.
async fn their_message<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    session: &str,
    said: &str,
) -> Result<Option<(i32, String)>, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id, m.content FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE m.session_id = $1 AND s.user_id = $2 AND s.context->>'mode' = 'chat' \
               AND (s.context->>'venue') IS NULL AND m.role = 'user' \
             ORDER BY (m.content = $3) DESC, m.id DESC LIMIT 1",
            [session.into(), user_id.into(), said.into()],
        ))
        .await?;
    Ok(match row {
        Some(row) => Some((row.try_get("", "id")?, row.try_get("", "content")?)),
        None => None,
    })
}

/// Their private talk before message `id`, oldest first.
async fn before<C: ConnectionTrait>(db: &C, user_id: i32, id: i32) -> Result<Vec<Line>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.role, m.content FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND s.context->>'mode' = 'chat' AND (s.context->>'venue') IS NULL \
               AND (m.metadata->>'piece') IS NULL AND m.id < $2 \
             ORDER BY m.id DESC LIMIT $3",
            [user_id.into(), id.into(), LOOK_BACK.into()],
        ))
        .await?;
    let mut lines = Vec::with_capacity(rows.len());
    for row in rows.iter().rev() {
        let role: String = row.try_get("", "role")?;
        lines.push(Line {
            // Anything but their words and hers (a system line, a helper's
            // result) is left out by the core.
            who: match role.as_str() {
                "user" => Who::Them,
                "assistant" => Who::You,
                _ => Who::Other,
            },
            text: row.try_get("", "content")?,
        });
    }
    Ok(lines)
}

/// The core's gate for one take: the partner's rests on `theirs`, their
/// own message in their own private talk, the one id the site names as
/// theirs (none: nothing here is theirs).
pub(crate) fn gate(
    origin: Origin,
    action: Action,
    message: &str,
    theirs: Option<&str>,
    trigger: Option<Trigger>,
) -> Result<Action, Error> {
    let admitted = intents::admit(AdmitInput {
        origin,
        action: Some(action),
        because: vec![message.to_string()],
        message: Some(message.to_string()),
        trigger,
        known: Known {
            partner: theirs.map(str::to_string).into_iter().collect(),
            ..Known::default()
        },
    })?;
    match (admitted.admitted, admitted.action) {
        (true, Some(action)) => Ok(action),
        (true, None) => Ok(Action::Say),
        (false, _) => Err(Error::Refused(match admitted.why {
            Some(Refusal::NotThePartners) => "not the partner's own words".into(),
            Some(Refusal::Untimed) => "an instruction with nothing to wake it".into(),
            _ => "resting on nothing of theirs".into(),
        })),
    }
}

/// [`spawn_after_turn`]'s work, at `now`: their message in `session` this
/// turn answered, judged, and what she took written.
pub(crate) async fn after_turn(
    db: &DatabaseConnection,
    user_id: i32,
    site_owner: Option<i32>,
    session: &str,
    said: &str,
    now: DateTime<Utc>,
) -> Result<Judged, Error> {
    if user_id <= 0 {
        return Ok(Judged::default());
    }
    let Some(pen) = crate::services::her_log::pen::think() else {
        tracing::info!("[memory-nodes] nothing judged for later: her mind is elsewhere");
        return Ok(Judged::default());
    };
    let Some((id, said)) = their_message(db, user_id, session, said).await? else {
        return Ok(Judged::default());
    };
    let audience = audience::private_talk(user_id, site_owner);
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let nodes: Vec<known::Known> = known::known(db, user_id, &audience)
        .await?
        .into_iter()
        .filter(|node| !node.is_person() && node.key.is_some())
        .collect();
    let topics: Vec<myriad_merope::topics::Topic> = nodes.iter().map(known::Known::topic).collect();
    let mut notes = Vec::new();
    for at in myriad_merope::topics::offered(&said, &topics) {
        let node = &nodes[at];
        notes.push(Note {
            id: node.id.clone(),
            key: node.key.clone().unwrap_or_default(),
            kind: serde_json::from_value(json!(node.kind.as_deref().unwrap_or("topic")))
                .map_err(|_| Error::Refused("a node's kind is not one the core reads".into()))?,
            name: node.name.clone(),
        });
    }
    let waiting: Vec<Held> = intent_store::held(db, user_id, &audience)
        .await?
        .into_iter()
        .filter(Held::waiting)
        .collect();
    let zone = intent_store::zone_name(now);
    let micros = now.timestamp_micros();
    let prepared: Prepared = intents::prepare(JudgeInput {
        soul,
        said: said.clone(),
        before: before(db, user_id, id).await?,
        notes,
        waiting: waiting
            .iter()
            .map(|held| Waiting {
                id: held.id.clone(),
                text: held.text.clone(),
            })
            .collect(),
        now: micros,
        zone: zone.clone(),
    })?;
    let Ok(raw) = Ask::new(Voice::Judge, user_id, OPERATION)
        .within(TIMEOUT)
        .json_raw(
            &prepared.system,
            &prepared.input.to_string(),
            OPERATION,
            &prepared.schema,
        )
        .await
    else {
        return Ok(Judged::default());
    };
    let read = intents::read(ReadInput {
        output: raw,
        notes: prepared.notes.clone(),
        waiting: prepared.waiting.clone(),
        now: micros,
        zone,
    })?;
    if read.unreadable {
        tracing::info!("[memory-nodes] her intents did not read: nothing taken");
        return Ok(Judged::default());
    }
    let message = format!("message:{id}");
    let mut judged = Judged::default();
    let mut given: Option<intents::Standing> = None;
    for (n, take) in read.takes.iter().enumerate() {
        let origin = match take.kind {
            TakeKind::Plan => {
                judged.plans += 1;
                continue;
            }
            TakeKind::Instruction => Origin::Partner,
            TakeKind::Mine => Origin::Her,
        };
        let action = match gate(
            origin,
            take.action,
            &message,
            Some(&message),
            take.trigger.clone(),
        ) {
            Ok(action) => action,
            Err(_) => {
                judged.refused += 1;
                continue;
            }
        };
        let (node, mut rows) = match &take.note {
            Some(note) => match known::landing(db, user_id, &audience, note).await? {
                Some(node) => (node, Vec::new()),
                None => intent_store::about_them(db, user_id, &audience).await?,
            },
            None => intent_store::about_them(db, user_id, &audience).await?,
        };
        let intent = row_id_from("node.intent", &node, &message, &n.to_string());
        let held = Held {
            node: node.clone(),
            id: intent.clone(),
            text: take.text.clone(),
            because: vec![message.clone()],
            status: IntentStatus::Waiting,
            trigger: take.trigger.as_ref().map(|trigger| json!(trigger)),
            origin: Some(origin),
            action: Some(action),
            said_by: json!([if origin == Origin::Partner {
                "partner"
            } else {
                "her"
            }]),
            // The partner's instruction to work runs only where it was
            // given; what she says can follow them.
            sync: if origin == Origin::Partner && action == Action::Work {
                json!({ "local": NODE })
            } else {
                json!("shared")
            },
            base: Vec::new(),
            at: micros,
        };
        rows.push(intent_store::settle_row(&held, IntentStatus::Waiting));
        if let Err(error) = write::write(db, user_id, &audience, &rows, Some(&pen)).await {
            tracing::warn!(%error, "[memory-nodes] an intent not written");
            judged.refused += 1;
            continue;
        }
        if origin == Origin::Partner {
            let standing = match &given {
                Some(standing) => standing.clone(),
                None => {
                    let standing = intent_store::standing_now(db, user_id).await;
                    given = Some(standing.clone());
                    standing
                }
            };
            let instruction = Instruction {
                intent_id: intent.clone(),
                user_id,
                audience: audience.clone(),
                node_id: node,
                message_ref: message.clone(),
                echo: take.echo.clone().unwrap_or_else(|| take.text.clone()),
                given: standing,
            };
            intent_store::keep(db, &instruction).await?;
        }
        judged.taken.push(intent);
    }
    for id in &read.drops {
        let Some(held) = waiting.iter().find(|held| &held.id == id) else {
            continue;
        };
        if call_off(db, user_id, &audience, held, &pen).await.is_ok() {
            judged.dropped.push(id.clone());
        }
    }
    // A thread their words called off goes with its intent.
    if judged
        .dropped
        .iter()
        .any(|id| id.starts_with(super::home_intents::THREAD))
    {
        super::home_intents::in_step(db, user_id).await;
    }
    Ok(judged)
}

/// `held` called off: dropped, never to run or be said.
pub(crate) async fn call_off(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    held: &Held,
    pen: &Thought,
) -> Result<(), Error> {
    let row = intent_store::settle_row(held, IntentStatus::Dropped);
    write::write(db, user_id, audience, &[row], Some(pen)).await?;
    intent_store::record(
        db,
        user_id,
        &held.id,
        &intent_store::Outcome {
            way: "cancelled",
            line: String::new(),
            succeeded: None,
            session: None,
        },
    )
    .await
}
