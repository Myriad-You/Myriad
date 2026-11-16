//! Where a memory row was put, decided once (`agent_node_placements`): on a
//! node, on the person it is only about (「关于你」), or nowhere. Decided is
//! decided: it is not asked of again. A memory forgotten takes its
//! placement with it (the foreign key). This is what the old
//! `agent_memory_topics.topic_id` left empty meant, said in full.

use sea_orm::{ConnectionTrait, DatabaseBackend, QueryResult, Statement};

use super::Error;
use crate::services::agent::memory::unified::OWN_VENUE;

/// How a memory was put.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum How {
    /// On a node: a topic, a task, a group, her.
    Node,
    /// On 「关于你」: only about the person.
    About,
    /// On none: of no kind, kept so it is not asked of again.
    Nowhere,
}

impl How {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Node => "node",
            Self::About => "about",
            Self::Nowhere => "nowhere",
        }
    }

    fn of_str(raw: &str) -> Option<Self> {
        match raw {
            "node" => Some(Self::Node),
            "about" => Some(Self::About),
            "nowhere" => Some(Self::Nowhere),
            _ => None,
        }
    }
}

/// One memory's placement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Placement {
    pub memory_id: String,
    pub user_id: i32,
    pub audience: String,
    pub how: How,
    /// The node it is on; none when it is nowhere.
    pub node_id: Option<String>,
}

/// Keeps where a memory was put; a later decision for the same memory
/// takes the earlier one's place.
pub(crate) async fn place<C: ConnectionTrait>(db: &C, placement: &Placement) -> Result<(), Error> {
    if (placement.how == How::Nowhere) != placement.node_id.is_none() {
        return Err(Error::Refused(
            "a memory on a node names it, and one put nowhere names none".into(),
        ));
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_node_placements (memory_id, user_id, audience, node_id, how, decided_at) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         ON CONFLICT (memory_id) DO UPDATE SET user_id = EXCLUDED.user_id, \
         audience = EXCLUDED.audience, node_id = EXCLUDED.node_id, how = EXCLUDED.how, \
         decided_at = EXCLUDED.decided_at",
        [
            placement.memory_id.clone().into(),
            placement.user_id.into(),
            placement.audience.clone().into(),
            placement.node_id.clone().into(),
            placement.how.as_str().into(),
            crate::clock::now().fixed_offset().into(),
        ],
    ))
    .await?;
    Ok(())
}

fn placement_of(row: &QueryResult) -> Result<Placement, Error> {
    let how: String = row.try_get("", "how")?;
    Ok(Placement {
        memory_id: row.try_get("", "memory_id")?,
        user_id: row.try_get("", "user_id")?,
        audience: row.try_get("", "audience")?,
        how: How::of_str(&how)
            .ok_or_else(|| Error::Refused("a kept placement has no known way".into()))?,
        node_id: row.try_get("", "node_id")?,
    })
}

/// Where `memory_id` was put, if it was.
pub(crate) async fn placement<C: ConnectionTrait>(
    db: &C,
    memory_id: &str,
) -> Result<Option<Placement>, Error> {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT memory_id, user_id, audience, node_id, how FROM agent_node_placements \
         WHERE memory_id = $1",
        [memory_id.into()],
    ))
    .await?
    .as_ref()
    .map(placement_of)
    .transpose()
}

/// The memory rows of an audience not put anywhere yet, oldest first, at
/// most `limit`; only those of `sources` when any are given. A private
/// talk's are its account's private ones; a group's, those kept in that
/// group; her own, hers. Retired rows (replaced, forgotten, faded) are not
/// among them, nor one an entry names already: another end of hers put it
/// on a node (`logged::take_in`), and it has no placement here.
pub(crate) async fn unplaced<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    audience: &str,
    sources: &[&str],
    limit: u64,
) -> Result<Vec<String>, Error> {
    use myriad_persona_log::row::Audience;
    let (owned_by, venue) = match Audience::parse(audience)
        .map_err(|_| Error::Refused("audience is not personal:, own: or group:".into()))?
    {
        Audience::Personal(_) => (Some(user_id), "private".to_string()),
        Audience::Group(_) => (None, audience.to_string()),
        Audience::Own => (None, OWN_VENUE.to_string()),
    };
    let sources: Vec<String> = sources.iter().map(|source| source.to_string()).collect();
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id FROM agent_memories m \
             WHERE m.venue = $1 AND ($2::integer IS NULL OR m.user_id = $2) \
               AND m.invalid_at IS NULL AND m.superseded_by IS NULL \
               AND (cardinality($3::text[]) = 0 OR m.source = ANY($3)) \
               AND NOT EXISTS (SELECT 1 FROM agent_node_placements p WHERE p.memory_id = m.id) \
               AND NOT EXISTS (SELECT 1 FROM agent_node_rows r WHERE r.kind = 'node.entry' \
                               AND r.body <> '' AND (r.body::jsonb -> 'refs') ? ('memory:' || m.id)) \
             ORDER BY m.created_at, m.id LIMIT $4",
            [
                venue.into(),
                owned_by.into(),
                sources.into(),
                (limit as i64).into(),
            ],
        ))
        .await?;
    rows.iter()
        .map(|row| Ok(row.try_get::<String>("", "id")?))
        .collect()
}
