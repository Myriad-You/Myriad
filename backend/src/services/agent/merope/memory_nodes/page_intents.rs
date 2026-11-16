//! Her intents on a node of the site owner's page ([`super::page`]): the
//! waiting ones, then the newest few settled, each with when it comes due
//! and what the site kept of the owner's instruction (their message, her
//! line back at once, what came of it).

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde::Serialize;

use super::{Error, intent_store, read};

/// Settled intents shown after the waiting ones, newest first, at most.
const SETTLED_SHOWN: usize = 3;

/// When an intent comes due.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct When {
    /// Unix microseconds.
    pub at: Option<i64>,
    pub zone: Option<String>,
    /// The node it waits on, by name, and for what (`changed`, `done`).
    pub node: Option<String>,
    pub change: Option<String>,
}

/// What the site kept of the owner's instruction beside an intent.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Instruction {
    /// Her line back at once.
    pub echo: String,
    /// Their message, or none when it was deleted.
    pub message: Option<String>,
    /// `ran`, `raised` or `cancelled`, with its line.
    pub way: Option<String>,
    pub line: Option<String>,
}

/// One intent of hers.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) struct IntentLine {
    pub id: String,
    pub text: String,
    pub status: String,
    /// `partner` or `her`.
    pub origin: Option<String>,
    /// `say` or `work`.
    pub action: Option<String>,
    pub at: i64,
    pub when: Option<When>,
    pub instruction: Option<Instruction>,
}

/// What the site kept of the instructions on `node`, by intent.
async fn instructions(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
    node: &str,
) -> Result<Vec<(String, String, String, Option<String>, Option<String>)>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT intent_id, message_ref, echo, outcome_way, outcome_line \
             FROM agent_node_instructions WHERE user_id = $1 AND audience = $2 AND node_id = $3",
            [owner.into(), audience.into(), node.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            Ok((
                row.try_get("", "intent_id")?,
                row.try_get("", "message_ref")?,
                row.try_get("", "echo")?,
                row.try_get("", "outcome_way")?,
                row.try_get("", "outcome_line")?,
            ))
        })
        .collect()
}

/// Her intents on `node`: the waiting ones, then the newest few settled.
pub(super) async fn intents_of(
    db: &DatabaseConnection,
    owner: i32,
    audience: &str,
    node: &str,
) -> Result<Vec<IntentLine>, Error> {
    let held: Vec<intent_store::Held> = intent_store::held(db, owner, audience)
        .await?
        .into_iter()
        .filter(|held| held.node == node)
        .collect();
    let kept = instructions(db, owner, audience, node).await?;
    let (waiting, settled): (Vec<_>, Vec<_>) = held.into_iter().partition(|held| held.waiting());
    let mut out = Vec::new();
    for held in waiting
        .into_iter()
        .chain(settled.into_iter().take(SETTLED_SHOWN))
    {
        let when = match &held.trigger {
            Some(trigger) => Some(When {
                at: trigger["at"].as_i64(),
                zone: trigger["zone"].as_str().map(str::to_string),
                node: match trigger["node"].as_str() {
                    Some(key) => match myriad_nodes::node_id(audience, key) {
                        Ok(id) => read::node(db, owner, audience, &id)
                            .await?
                            .and_then(|head| head["name"]["value"].as_str().map(str::to_string)),
                        Err(_) => None,
                    },
                    None => None,
                },
                change: trigger["change"].as_str().map(str::to_string),
            }),
            None => None,
        };
        let instruction = match kept.iter().find(|kept| kept.0 == held.id) {
            Some((_, message, echo, way, line)) => Some(Instruction {
                echo: echo.clone(),
                message: intent_store::message_text(db, message).await?,
                way: way.clone(),
                line: line.clone().filter(|line| !line.is_empty()),
            }),
            None => None,
        };
        out.push(IntentLine {
            status: word(&held.status).unwrap_or_default(),
            origin: held.origin.and_then(|origin| word(&origin)),
            action: held.action.and_then(|action| word(&action)),
            id: held.id,
            text: held.text,
            at: held.at,
            when,
            instruction,
        });
    }
    Ok(out)
}

/// A value the core writes as one word.
fn word<T: Serialize>(value: &T) -> Option<String> {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
}
