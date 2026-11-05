//! Her intents as the site keeps them: the `node.intent` items of an
//! audience as the core folded them onto her nodes' heads, the row that
//! settles one (said, done, dropped), and what only the site keeps of the
//! partner's instruction (`agent_node_instructions`, `migrations/memory_nodes.sql`):
//! their message, her line back at once, who, where and under which runtime
//! grants it was given, and what came of it. Who and where now, to check an
//! instruction again when it comes due ([`standing_now`]), and forgetting an
//! intent with the words it rested on ([`forget_resting_on`]).

use std::collections::{BTreeMap, BTreeSet};

use chrono::{DateTime, Offset, Utc};
use myriad_nodes::{
    body::{Action, IntentStatus, Origin},
    entries::{self, FactRowsInput, Start, row_id_from},
    intents::Standing,
};
use myriad_persona_log::Row;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use super::{Author, Error, RowIn, forget, heads, known, read, write};
use crate::services::her_log::NODE;

/// One intent on a node, as the fold has it now.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Held {
    pub node: String,
    pub id: String,
    pub text: String,
    pub because: Vec<String>,
    pub status: IntentStatus,
    pub trigger: Option<Value>,
    pub origin: Option<Origin>,
    pub action: Option<Action>,
    pub said_by: Value,
    pub sync: Value,
    /// The `base` of the next write to it.
    pub base: Vec<String>,
    /// Its current version's clock, Unix microseconds.
    pub at: i64,
}

impl Held {
    pub(crate) fn waiting(&self) -> bool {
        self.status == IntentStatus::Waiting
    }

    /// The partner's instruction that may be done, not only said.
    pub(crate) fn works(&self) -> bool {
        self.origin == Some(Origin::Partner) && self.action == Some(Action::Work)
    }
}

fn held_of(node: &str, id: &str, item: &Value) -> Option<Held> {
    let value = item.get("value")?;
    Some(Held {
        node: node.to_string(),
        id: id.to_string(),
        text: value["text"].as_str()?.to_string(),
        because: serde_json::from_value(value["because"].clone()).ok()?,
        status: serde_json::from_value(value["status"].clone()).ok()?,
        trigger: value.get("trigger").filter(|it| !it.is_null()).cloned(),
        origin: serde_json::from_value(value["origin"].clone()).ok(),
        action: serde_json::from_value(value["action"].clone()).ok(),
        said_by: item["saidBy"].clone(),
        sync: item["sync"].clone(),
        base: serde_json::from_value(item["base"].clone()).ok()?,
        at: item["at"].as_i64().unwrap_or_default(),
    })
}

/// The intents on the live nodes of `user_id`'s `audience`, newest first.
pub(crate) async fn held<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
) -> Result<Vec<Held>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT node_id, status, node FROM agent_node_heads \
             WHERE user_id = $1 AND audience = $2 AND node->'intents' IS NOT NULL",
            [user_id.into(), audience.into()],
        ))
        .await?;
    let mut out = Vec::new();
    for row in &rows {
        let status: String = row.try_get("", "status")?;
        if !known::is_live(&status) {
            continue;
        }
        let node: String = row.try_get("", "node_id")?;
        let head: Value = row.try_get("", "node")?;
        for (id, item) in head["intents"].as_object().into_iter().flatten() {
            out.extend(held_of(&node, id, item));
        }
    }
    out.sort_by(|a, b| b.at.cmp(&a.at).then_with(|| a.id.cmp(&b.id)));
    Ok(out)
}

/// The accounts and private audiences with an intent waiting.
pub(crate) async fn waiting_audiences(
    db: &DatabaseConnection,
) -> Result<Vec<(i32, String)>, Error> {
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT h.user_id, h.audience FROM agent_node_heads h \
             CROSS JOIN LATERAL jsonb_each(CASE WHEN jsonb_typeof(h.node->'intents') = 'object' \
               THEN h.node->'intents' ELSE '{}'::jsonb END) AS i(id, item) \
             WHERE h.audience LIKE 'personal:%' AND i.item->'value'->>'status' = 'waiting' \
             ORDER BY h.user_id, h.audience",
        ))
        .await?;
    rows.iter()
        .map(|row| Ok((row.try_get("", "user_id")?, row.try_get("", "audience")?)))
        .collect()
}

/// A `node.intent` body: `intent` as it is, at `status`.
pub(crate) fn body(held: &Held, status: IntentStatus) -> Value {
    let mut body = json!({
        "version": myriad_nodes::BODY_VERSION, "sync": held.sync, "saidBy": held.said_by,
        "base": held.base, "intent": held.id, "text": held.text, "because": held.because,
        "status": status,
    });
    if let Some(trigger) = &held.trigger {
        body["trigger"] = trigger.clone();
    }
    if let Some(origin) = held.origin {
        body["origin"] = json!(origin);
    }
    if let Some(action) = held.action {
        body["action"] = json!(action);
    }
    body
}

/// The row that takes `held` to `status`, from where it stands now: hers,
/// as every row of an intent is (she took it; the partner only said it).
pub(crate) fn settle_row(held: &Held, status: IntentStatus) -> RowIn {
    let word = serde_json::to_value(status)
        .ok()
        .and_then(|word| word.as_str().map(str::to_string))
        .unwrap_or_default();
    RowIn {
        id: row_id_from(
            "node.intent",
            &held.node,
            &format!("intent:{}", held.id),
            &word,
        ),
        node: held.node.clone(),
        kind: "node.intent".into(),
        body: body(held, status),
        author: Author::Her,
    }
}

/// `user_id`'s own node (「关于你」) in `audience`, and the rows that start
/// it when it is not there yet ([`super::facts`] does the same).
pub(crate) async fn about_them(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
) -> Result<(String, Vec<RowIn>), Error> {
    let key = super::audience::person_key(audience, user_id);
    let id = myriad_nodes::node_id(audience, &key)?;
    if read::node(db, user_id, audience, &id).await?.is_some() {
        return Ok((id, Vec::new()));
    }
    let made = entries::fact_rows(FactRowsInput {
        audience: audience.to_string(),
        sync: myriad_nodes::Sync::Shared,
        start: Some(Start {
            key: Some(key),
            name: super::facts::person_name(db, user_id).await?,
            aliases: Vec::new(),
        }),
        key: None,
        facts: Vec::new(),
    })?;
    let rows = made
        .rows
        .into_iter()
        .map(|row| RowIn {
            id: row.id,
            node: made.node.clone(),
            kind: row.kind,
            body: row.body,
            author: Author::Her,
        })
        .collect();
    Ok((made.node, rows))
}

/// The zone she reads times in, as the core takes it: her persona's IANA
/// name, or the site's own offset now.
pub(crate) fn zone_name(now: DateTime<Utc>) -> String {
    use crate::services::agent::merope::clock::{self, Zone};
    match clock::zone() {
        Zone::Named(zone) => zone.name().to_string(),
        Zone::Site => now.with_timezone(&chrono::Local).offset().fix().to_string(),
    }
}

/// Who she is, for an instruction's recheck: her name and personality as
/// the site keeps them, hashed. Another persona, or the same one rewritten,
/// is not who the instruction was given to.
pub(crate) async fn persona(db: &DatabaseConnection) -> String {
    match crate::services::agent::merope::get_persona(db).await {
        Ok(Some(persona)) => {
            let seed = format!(
                "{}\u{1f}{}",
                persona.name.trim(),
                persona.personality.trim()
            );
            let hash = myriad_persona_sync::canonical::sha256_hex(seed.as_bytes());
            format!("persona:{}", &hash[..32])
        }
        _ => "persona:none".into(),
    }
}

/// The runtime grants in effect for `user_id` now (none when they cannot
/// be read: fail closed).
pub(crate) async fn granted_now(db: &DatabaseConnection, user_id: i32) -> BTreeSet<String> {
    crate::services::agent::consciousness::effective_granted(db, user_id, None)
        .await
        .unwrap_or_default()
        .into_iter()
        .collect()
}

/// Who and where, under which grants, as it stands now for `user_id`.
pub(crate) async fn standing_now(db: &DatabaseConnection, user_id: i32) -> Standing {
    Standing {
        owner: user_id.to_string(),
        persona: persona(db).await,
        device: NODE.to_string(),
        granted: granted_now(db, user_id).await,
    }
}

/// The partner's instruction as the site keeps it beside the intent.
#[derive(Debug, Clone)]
pub(crate) struct Instruction {
    pub intent_id: String,
    pub user_id: i32,
    pub audience: String,
    pub node_id: String,
    /// Their message it was given in (`message:<id>`).
    pub message_ref: String,
    /// Her one line back at once (kept; shown in a later step).
    pub echo: String,
    pub given: Standing,
}

impl Instruction {
    pub(crate) fn standing(&self) -> Standing {
        self.given.clone()
    }
}

/// Keeps `instruction`; one already kept is left as it is.
pub(crate) async fn keep(db: &DatabaseConnection, instruction: &Instruction) -> Result<(), Error> {
    let granted: Vec<&String> = instruction.given.granted.iter().collect();
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_node_instructions (intent_id, user_id, audience, node_id, message_ref, \
         echo, owner, persona, device, granted) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
         ON CONFLICT (user_id, intent_id) DO NOTHING",
        [
            instruction.intent_id.clone().into(),
            instruction.user_id.into(),
            instruction.audience.clone().into(),
            instruction.node_id.clone().into(),
            instruction.message_ref.clone().into(),
            instruction.echo.clone().into(),
            instruction.given.owner.clone().into(),
            instruction.given.persona.clone().into(),
            instruction.given.device.clone().into(),
            json!(granted).into(),
        ],
    ))
    .await?;
    Ok(())
}

/// `user_id`'s instruction under `intent_id`, if it is kept.
pub(crate) async fn instruction(
    db: &DatabaseConnection,
    user_id: i32,
    intent_id: &str,
) -> Result<Option<Instruction>, Error> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT audience, node_id, message_ref, echo, owner, persona, device, granted \
             FROM agent_node_instructions WHERE user_id = $1 AND intent_id = $2",
            [user_id.into(), intent_id.into()],
        ))
        .await?;
    let Some(row) = row else {
        return Ok(None);
    };
    let granted: Value = row.try_get("", "granted")?;
    Ok(Some(Instruction {
        intent_id: intent_id.to_string(),
        user_id,
        audience: row.try_get("", "audience")?,
        node_id: row.try_get("", "node_id")?,
        message_ref: row.try_get("", "message_ref")?,
        echo: row.try_get("", "echo")?,
        given: Standing {
            owner: row.try_get("", "owner")?,
            persona: row.try_get("", "persona")?,
            device: row.try_get("", "device")?,
            granted: serde_json::from_value(granted).unwrap_or_default(),
        },
    }))
}

/// What came of an instruction: it ran (and how), her hand went up instead
/// (and why), or it was called off.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Outcome {
    pub way: &'static str,
    pub line: String,
    pub succeeded: Option<bool>,
    pub session: Option<String>,
}

pub(crate) async fn record(
    db: &DatabaseConnection,
    user_id: i32,
    intent_id: &str,
    outcome: &Outcome,
) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_node_instructions SET outcome_way = $3, outcome_line = $4, \
         outcome_at = $5, succeeded = $6, work_session_id = $7 \
         WHERE user_id = $1 AND intent_id = $2",
        [
            user_id.into(),
            intent_id.into(),
            outcome.way.into(),
            outcome.line.clone().into(),
            crate::clock::now().fixed_offset().into(),
            outcome.succeeded.into(),
            outcome.session.clone().into(),
        ],
    ))
    .await?;
    Ok(())
}

/// The text of `user_id`'s message `message:<id>`, if it is still there.
pub(crate) async fn message_text(
    db: &DatabaseConnection,
    message_ref: &str,
) -> Result<Option<String>, Error> {
    let Some(id) = message_ref
        .strip_prefix("message:")
        .and_then(|id| id.parse::<i32>().ok())
    else {
        return Ok(None);
    };
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT content FROM agent_messages WHERE id = $1 AND role = 'user'",
            [id.into()],
        ))
        .await?;
    Ok(row.map(|row| row.try_get("", "content")).transpose()?)
}

/// Forgets the intents resting on the lines `lines` (`agent_messages.id`)
/// being forgotten, on `db`'s transaction: every row of each is forgotten
/// by a forget of its own and becomes a stub, its head is folded again,
/// and what the site kept of the instruction goes.
pub(crate) async fn forget_resting_on<C: ConnectionTrait>(
    db: &C,
    lines: &[i32],
) -> Result<(), Error> {
    let refs: Vec<String> = lines.iter().map(|id| format!("message:{id}")).collect();
    if refs.is_empty() {
        return Ok(());
    }
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT DISTINCT r.user_id, r.audience, r.row_id FROM agent_node_rows r \
             CROSS JOIN LATERAL jsonb_array_elements_text( \
               CASE WHEN r.body = '' THEN '[]'::jsonb \
                    ELSE COALESCE((r.body::jsonb)->'because', '[]'::jsonb) END) AS cited(ref) \
             WHERE r.kind = 'node.intent' AND cited.ref = ANY($1)",
            [refs.clone().into()],
        ))
        .await?;
    let mut by_audience: BTreeMap<(i32, String), BTreeSet<String>> = BTreeMap::new();
    for row in &rows {
        by_audience
            .entry((row.try_get("", "user_id")?, row.try_get("", "audience")?))
            .or_default()
            .insert(row.try_get("", "row_id")?);
    }
    for ((user_id, audience), cited) in by_audience {
        write::lock(db, user_id, &audience).await?;
        let kept = heads::rows_of(db, user_id, &audience).await?;
        let doomed: Vec<Row> = kept
            .iter()
            .filter(|kept| cited.contains(&kept.row.id))
            .map(|kept| kept.row.clone())
            .collect();
        for row in doomed.iter().filter(|row| !row.forgotten) {
            let node = forget::row_node(row);
            forget::forget_one(db, user_id, &audience, &node, forget::Forgets::Row(&row.id))
                .await?;
        }
        forget::stub(db, user_id, &doomed).await?;
        let folded = heads::fold_rows(&heads::rows_of(db, user_id, &audience).await?)?;
        heads::refresh(db, user_id, &audience, &folded).await?;
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_node_instructions WHERE message_ref = ANY($1)",
        [refs.into()],
    ))
    .await?;
    Ok(())
}
