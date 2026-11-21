//! Which other notes she is offered beside the one she goes over (节点整理):
//! the nodes that already hold a session's earlier work, then those of the
//! same name, then by the words of both, so one thing stays one node.

use myriad_nodes::limits::UPKEEP_CANDIDATES;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use super::{Error, inbox, known};

/// The nodes of `audience` that hold a span placed from one of the
/// owner's coding-agent sessions in `inbox`: where the rest of that work
/// went before.
pub(super) async fn holding_sessions<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    inbox: &[inbox::Item],
) -> Result<Vec<String>, Error> {
    let sessions: Vec<String> = inbox
        .iter()
        .filter(|item| item.id.starts_with("session:"))
        .map(|item| format!("%\"{}\"%", item.id))
        .collect();
    if sessions.is_empty() {
        return Ok(Vec::new());
    }
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT node_id, max(at) AS last FROM agent_node_rows \
             WHERE user_id = $1 AND audience = $2 AND kind = 'node.span' \
               AND body LIKE ANY($3) \
             GROUP BY node_id ORDER BY last DESC, node_id",
            [user_id.into(), audience.into(), sessions.into()],
        ))
        .await?;
    rows.iter()
        .map(|row| Ok(row.try_get("", "node_id")?))
        .collect()
}

/// The other nodes of the audience she is offered beside `target`: what
/// may be the same thing, or where an inbox item belongs. First the nodes
/// that hold the earlier work of a session in the inbox (`holding`), and
/// those named as the target is, so one thing stays one node; then by the
/// words of both. Never a person's own node.
pub(super) fn candidates(
    known: &[known::Known],
    target: Option<&str>,
    inbox: &[inbox::Item],
    holding: &[String],
) -> Vec<String> {
    let pool: Vec<&known::Known> = known
        .iter()
        .filter(|node| Some(node.id.as_str()) != target && !node.is_person())
        .collect();
    let named = target
        .and_then(|id| known.iter().find(|node| node.id == id))
        .map(|node| node.name.trim().to_lowercase());
    let mut first: Vec<String> = Vec::new();
    for id in holding {
        if pool.iter().any(|node| &node.id == id) && !first.contains(id) {
            first.push(id.clone());
        }
    }
    if let Some(named) = named.filter(|named| !named.is_empty()) {
        for node in &pool {
            if node.name.trim().to_lowercase() == named && !first.contains(&node.id) {
                first.push(node.id.clone());
            }
        }
    }
    let mut query: Vec<String> = Vec::new();
    if let Some(node) = target.and_then(|id| known.iter().find(|node| node.id == id)) {
        query.push(node.name.clone());
        query.extend(node.aliases.iter().cloned());
        query.extend(node.holds.iter().cloned());
    }
    query.extend(inbox.iter().map(|item| item.text.clone()));
    let topics: Vec<myriad_merope::topics::Topic> = pool.iter().map(|node| node.topic()).collect();
    let order = myriad_merope::topics::offered(&query.join("\n"), &topics);
    for at in order {
        if !first.contains(&pool[at].id) {
            first.push(pool[at].id.clone());
        }
    }
    first.truncate(UPKEEP_CANDIDATES * 2);
    first
}
