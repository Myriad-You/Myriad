//! What she was unsure where to put (`agent_node_inbox`): a span of talk
//! no node fits, kept per audience under the day it was said, until her
//! upkeep puts it on a node (then it is taken out, and the node holds it as
//! a span) or it is forgotten with the words it came from.
//!
//! An item is her few words for the span (`text`), who said what in it,
//! and where its words are (`refs`, `message:<id>`); its id is the first of
//! those, so the same span is one item however often it is kept.

use chrono::NaiveDate;
use myriad_nodes::SaidBy;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::Error;

/// One item, as kept.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Item {
    #[serde(skip)]
    pub day: Option<NaiveDate>,
    #[serde(skip)]
    pub id: String,
    pub said_by: Vec<SaidBy>,
    pub text: String,
    pub refs: Vec<String>,
    /// When it was said, Unix microseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub to: Option<i64>,
}

/// Keeps `items` in `user_id`'s `audience`, each under its day; one kept
/// already is left as it is.
pub(super) async fn add<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    items: &[Item],
) -> Result<(), Error> {
    for item in items {
        let day = item
            .day
            .ok_or_else(|| Error::Refused("an inbox item is kept under its day".into()))?;
        let value = serde_json::to_value(item)
            .map_err(|_| Error::Refused("an inbox item does not serialize".into()))?;
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_node_inbox (user_id, audience, day, id, item) \
             VALUES ($1, $2, $3, $4, $5) ON CONFLICT DO NOTHING",
            [
                user_id.into(),
                audience.into(),
                day.into(),
                item.id.clone().into(),
                value.into(),
            ],
        ))
        .await?;
    }
    Ok(())
}

/// What waits in `user_id`'s `audience`, oldest first, at most `limit`.
pub(crate) async fn waiting<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    limit: u64,
) -> Result<Vec<Item>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT day, id, item FROM agent_node_inbox \
             WHERE user_id = $1 AND audience = $2 AND placed_at IS NULL \
             ORDER BY day, created_at, id LIMIT $3",
            [user_id.into(), audience.into(), (limit as i64).into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            let value: Value = row.try_get("", "item")?;
            let mut item: Item = serde_json::from_value(value)
                .map_err(|_| Error::Refused("a kept inbox item does not read".into()))?;
            item.day = Some(row.try_get("", "day")?);
            item.id = row.try_get("", "id")?;
            Ok(item)
        })
        .collect()
}

/// Items of `user_id`'s `audience` now on a node: out of the inbox.
pub(super) async fn take_out<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    ids: &[String],
) -> Result<(), Error> {
    if ids.is_empty() {
        return Ok(());
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_node_inbox SET placed_at = $4 \
         WHERE user_id = $1 AND audience = $2 AND id = ANY($3) AND placed_at IS NULL",
        [
            user_id.into(),
            audience.into(),
            ids.to_vec().into(),
            crate::clock::now().fixed_offset().into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Every item whose words are among `refs`, whoever's: gone with them.
pub(super) async fn forget_citing<C: ConnectionTrait>(
    db: &C,
    refs: &[String],
) -> Result<(), Error> {
    if refs.is_empty() {
        return Ok(());
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM agent_node_inbox \
         WHERE EXISTS (SELECT 1 FROM jsonb_array_elements_text(item->'refs') AS cited(ref) \
                       WHERE cited.ref = ANY($1))",
        [refs.to_vec().into()],
    ))
    .await?;
    Ok(())
}
