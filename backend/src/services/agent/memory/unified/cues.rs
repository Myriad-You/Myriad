//! When each memory would come to mind (`agent_memory_cues`): a few
//! situations it was kept for ("有人问周末去哪玩时"), written with it. Recall
//! reads a memory with its cues, by words and by meaning, so a question that
//! shares no word with what was said still finds it; what comes back is the
//! memory as it was said.

use std::collections::HashMap;

use sea_orm::{DatabaseBackend, Statement};

use super::*;

/// Most cues kept for one memory, and how long each may be.
pub const MAX_CUES: usize = 3;
pub const CUE_CHARS: usize = 60;

/// Cues as kept: each on one line, short, each once, at most `MAX_CUES`.
pub fn clean_cues(cues: &[String]) -> Vec<String> {
    let mut kept: Vec<String> = Vec::new();
    for cue in cues {
        let cue: String = cue
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .take(CUE_CHARS)
            .collect();
        if !cue.is_empty() && !kept.contains(&cue) {
            kept.push(cue);
        }
    }
    kept.truncate(MAX_CUES);
    kept
}

/// Keep `cues` for the memory `memory_id`, unless it has some already.
/// Whether any were kept.
pub async fn keep_cues<C: ConnectionTrait>(
    db: &C,
    memory_id: &str,
    cues: &[String],
) -> Result<bool, DbErr> {
    let cues = clean_cues(cues);
    if cues.is_empty() {
        return Ok(false);
    }
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_memory_cues (memory_id, cues, created_at) VALUES ($1, $2, $3::timestamptz) \
             ON CONFLICT (memory_id) DO NOTHING",
            [memory_id.into(), cues.join("\n").into(), crate::clock::now().into()],
        ))
        .await?;
    Ok(result.rows_affected() > 0)
}

/// The cues kept for `ids`, one per line, by memory id.
pub async fn cues_of<C: ConnectionTrait>(db: &C, ids: &[String]) -> HashMap<String, String> {
    if ids.is_empty() {
        return HashMap::new();
    }
    db.query_all_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT memory_id, cues FROM agent_memory_cues WHERE memory_id = ANY($1)",
        [ids.to_vec().into()],
    ))
    .await
    .unwrap_or_default()
    .iter()
    .filter_map(|row| {
        Some((
            row.try_get::<String>("", "memory_id").ok()?,
            row.try_get::<String>("", "cues").ok()?,
        ))
    })
    .collect()
}

/// `rows` as recall reads them, each with its cues after what it says, and
/// what each of those said, to give back (see [`as_said`]).
pub(super) async fn with_cues<C: ConnectionTrait>(
    db: &C,
    rows: Vec<agent_memories::Model>,
) -> (Vec<agent_memories::Model>, HashMap<String, String>) {
    let ids: Vec<String> = rows.iter().map(|row| row.id.clone()).collect();
    let cues = cues_of(db, &ids).await;
    let mut said = HashMap::new();
    let rows = rows
        .into_iter()
        .map(|mut row| {
            if let Some(cues) = cues.get(&row.id) {
                said.insert(row.id.clone(), row.content.clone());
                row.content = format!("{}\n{cues}", row.content);
            }
            row
        })
        .collect();
    (rows, said)
}

/// A row read with its cues, as it was said.
pub(super) fn as_said(
    mut row: agent_memories::Model,
    said: &HashMap<String, String>,
) -> agent_memories::Model {
    if let Some(content) = said.get(&row.id) {
        row.content = content.clone();
    }
    row
}

/// Up to `limit` of `user_id`'s active memories from `sources` that have no
/// cues yet, newest first.
pub async fn without_cues<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    sources: &[&str],
    limit: u64,
) -> Vec<agent_memories::Model> {
    let sources: Vec<String> = sources.iter().map(|source| source.to_string()).collect();
    agent_memories::Entity::find()
        .from_raw_sql(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.* FROM agent_memories m \
         WHERE m.user_id = $1 AND m.source = ANY($2) AND m.invalid_at IS NULL \
           AND NOT EXISTS (SELECT 1 FROM agent_memory_cues c WHERE c.memory_id = m.id) \
         ORDER BY m.created_at DESC LIMIT $3",
            [user_id.into(), sources.into(), (limit as i64).into()],
        ))
        .all(db)
        .await
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cues_are_kept_short_on_one_line_each_once() {
        let cues = clean_cues(&[
            " 有人问周末\n去哪玩时 ".into(),
            "有人问周末 去哪玩时".into(),
            String::new(),
            "长".repeat(80),
            "聊到学外语时".into(),
            "第四条".into(),
        ]);
        assert_eq!(cues.len(), MAX_CUES);
        assert_eq!(cues[0], "有人问周末 去哪玩时");
        assert_eq!(cues[1].chars().count(), CUE_CHARS);
        assert_eq!(cues[2], "聊到学外语时");
    }
}
