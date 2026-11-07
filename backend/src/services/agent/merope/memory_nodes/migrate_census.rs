//! The count by audience after moving her memory ([`super::migrate`]):
//! for each private talk and each group, the memories still held, those
//! that belong on a node (of the sources and kinds sorted onto one,
//! [`super::sorting::SORTED`] and [`super::sorting::KINDS`], or of a
//! source hung where it is written, [`super::migrate_sources::HUNG`]), how
//! many of those are on one (an entry naming the memory, or a placement on
//! a node or on 「关于你」), how many were put nowhere, and how many are
//! neither: not decided. What does not belong on a node is counted by source and kind,
//! so why each is not there can be read. Her own memories are no
//! audience's and are not moved. Counts only, never the words.

use std::collections::BTreeMap;

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde::Serialize;

use super::{Error, migrate_sources, sorting};

/// One audience's count.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Counted {
    /// `private:<user id>` or the group's venue.
    pub audience: String,
    pub held: usize,
    /// Sorted onto a node, or hung on one.
    pub belong: usize,
    pub on_node: usize,
    pub nowhere: usize,
    /// Belong on a node, and are on none and not put nowhere.
    pub undecided: usize,
    /// Neither sorted nor hung, by `source/kind`.
    pub not_sorted: BTreeMap<String, usize>,
}

/// Every audience's count; `undecided` summed.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Census {
    pub audiences: Vec<Counted>,
    pub undecided: usize,
}

impl Census {
    /// The audiences with something undecided, as `audience: n`.
    pub(crate) fn undecided_where(&self) -> String {
        self.audiences
            .iter()
            .filter(|counted| counted.undecided > 0)
            .map(|counted| format!("{}: {}", counted.audience, counted.undecided))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

pub(crate) async fn census<C: ConnectionTrait>(db: &C) -> Result<Census, Error> {
    let kinds: Vec<String> = sorting::KINDS
        .iter()
        .map(|kind| kind.as_str().to_string())
        .collect();
    let sources: Vec<String> = sorting::SORTED.iter().map(|s| s.to_string()).collect();
    let hung: Vec<String> = migrate_sources::HUNG
        .iter()
        .map(|s| s.to_string())
        .collect();
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "WITH named AS ( \
               SELECT DISTINCT substring(r FROM 8) AS memory_id \
               FROM agent_node_rows, jsonb_array_elements_text(body::jsonb -> 'refs') r \
               WHERE kind = 'node.entry' AND r LIKE 'memory:%'), \
             held AS ( \
               SELECT m.id, m.user_id, m.venue, COALESCE(m.source, '') AS source, m.kind::text AS kind \
               FROM agent_memories m \
               WHERE m.invalid_at IS NULL AND m.superseded_by IS NULL \
                 AND ((m.venue = 'private' AND m.user_id > 0) OR m.venue LIKE 'group:%')) \
             SELECT CASE WHEN h.venue = 'private' THEN 'private:' || h.user_id ELSE h.venue END \
                      AS audience, \
                    h.source, h.kind, \
                    COALESCE((h.source = ANY($1) AND h.kind = ANY($2)) OR h.source = ANY($3), false) AS belongs, \
                    COALESCE(n.memory_id IS NOT NULL OR p.how IN ('node', 'about'), false) AS on_node, \
                    COALESCE(n.memory_id IS NULL AND p.how = 'nowhere', false) AS nowhere, \
                    count(*) AS n \
             FROM held h \
             LEFT JOIN agent_node_placements p ON p.memory_id = h.id \
             LEFT JOIN named n ON n.memory_id = h.id \
             GROUP BY 1, 2, 3, 4, 5, 6 ORDER BY 1",
            [sources.into(), kinds.into(), hung.into()],
        ))
        .await?;
    let mut by_audience: BTreeMap<String, Counted> = BTreeMap::new();
    for row in rows {
        let audience: String = row.try_get("", "audience")?;
        let source: String = row.try_get("", "source")?;
        let kind: String = row.try_get("", "kind")?;
        let belongs: bool = row.try_get("", "belongs")?;
        let on_node: bool = row.try_get("", "on_node")?;
        let nowhere: bool = row.try_get("", "nowhere")?;
        let n = row.try_get::<i64>("", "n")? as usize;
        let counted = by_audience
            .entry(audience.clone())
            .or_insert_with(|| Counted {
                audience,
                ..Counted::default()
            });
        counted.held += n;
        if !belongs {
            *counted
                .not_sorted
                .entry(format!("{source}/{kind}"))
                .or_default() += n;
            continue;
        }
        counted.belong += n;
        if on_node {
            counted.on_node += n;
        } else if nowhere {
            counted.nowhere += n;
        } else {
            counted.undecided += n;
        }
    }
    let audiences: Vec<Counted> = by_audience.into_values().collect();
    let undecided = audiences.iter().map(|counted| counted.undecided).sum();
    Ok(Census {
        audiences,
        undecided,
    })
}
