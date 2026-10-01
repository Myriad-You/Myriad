//! Rows read by source, person or text, for the callers that keep their own kind of memory here, and keeping those rows fresh or letting them fade.

use super::*;

/// When she last learned anything, about anyone or from something she did on
/// her own, if ever. Her days are not learning.
pub async fn last_learned_at<C: ConnectionTrait>(
    db: &C,
) -> Result<Option<chrono::DateTime<chrono::FixedOffset>>, DbErr> {
    Ok(agent_memories::Entity::find()
        .filter(
            sea_orm::Condition::any()
                .add(agent_memories::Column::UserId.is_not_null())
                .add(agent_memories::Column::Kind.eq(MemoryKind::Knowledge.as_str())),
        )
        .filter(agent_memories::Column::Source.is_not_in(NOTED_IN_PASSING))
        .order_by_desc(agent_memories::Column::CreatedAt)
        .one(db)
        .await?
        .map(|row| row.created_at))
}

/// Sources of what she noted about someone in passing rather than learned
/// from them: what they played, games she played with them.
pub const NOTED_IN_PASSING: [&str; 2] = ["presence", "game"];

/// Sources recalled only when the talk comes to them, never as the recent
/// context: what she noted in passing, and what she looked up (one search,
/// put before her every turn, became a story she kept retelling).
pub const RECALLED_WHEN_NAMED: [&str; 3] = ["presence", "game", "lookup"];

/// People she keeps memories of from private talk, most remembered first.
pub async fn people_remembered<C: ConnectionTrait>(db: &C, limit: i64) -> Result<Vec<i32>, DbErr> {
    let rows = db
        .query_all_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT user_id FROM agent_memories \
             WHERE user_id IS NOT NULL AND venue = 'private' AND source = 'chat' \
               AND invalid_at IS NULL \
             GROUP BY user_id ORDER BY count(*) DESC LIMIT $1",
            [limit.into()],
        ))
        .await?;
    Ok(rows
        .iter()
        .filter_map(|row| row.try_get::<i32>("", "user_id").ok())
        .collect())
}

/// Rows of these sources, retired ones too (with why they were), oldest
/// first: for looking at how things changed, never for recall.
pub async fn with_history<C: ConnectionTrait>(
    db: &C,
    sources: &[&str],
    limit: u64,
) -> Result<Vec<agent_memories::Model>, DbErr> {
    let mut rows = agent_memories::Entity::find()
        .filter(agent_memories::Column::Source.is_in(sources.iter().copied()))
        .order_by_desc(agent_memories::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await?;
    rows.reverse();
    Ok(rows)
}

/// The latest active rows of `source` about `user_id`, newest first.
pub async fn latest_of<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    source: &str,
    limit: u64,
) -> Result<Vec<agent_memories::Model>, DbErr> {
    agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::Source.eq(source))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .order_by_desc(agent_memories::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await
}

/// Every active row from `source`, whoever it is about.
pub async fn active_from_source<C: ConnectionTrait>(
    db: &C,
    source: &str,
) -> Result<Vec<agent_memories::Model>, DbErr> {
    agent_memories::Entity::find()
        .filter(agent_memories::Column::Source.eq(source))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .all(db)
        .await
}

/// Which of these rows are still kept, faded or not.
pub async fn still_kept<C: ConnectionTrait>(
    db: &C,
    ids: Vec<String>,
) -> Result<std::collections::HashSet<String>, DbErr> {
    if ids.is_empty() {
        return Ok(std::collections::HashSet::new());
    }
    let kept: Vec<String> = agent_memories::Entity::find()
        .select_only()
        .column(agent_memories::Column::Id)
        .filter(agent_memories::Column::Id.is_in(ids))
        .into_tuple()
        .all(db)
        .await?;
    Ok(kept.into_iter().collect())
}

/// A person's active memory from `source` whose text mentions `needle`.
pub async fn find_active<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    source: &str,
    needle: &str,
) -> Result<Option<agent_memories::Model>, DbErr> {
    agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::Source.eq(source))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::Content.contains(needle))
        .order_by_desc(agent_memories::Column::CreatedAt)
        .one(db)
        .await
}

/// It came up again: keep it fresh.
pub async fn refresh<C: ConnectionTrait>(db: &C, user_id: i32, id: &str) -> Result<bool, DbErr> {
    let now = Utc::now().fixed_offset();
    let result = agent_memories::Entity::update_many()
        .set(agent_memories::ActiveModel {
            updated_at: Set(now),
            ..Default::default()
        })
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

/// Rows from `source` that have not come up again for `older_than` fade.
pub async fn fade_source<C: ConnectionTrait>(
    db: &C,
    source: &str,
    older_than: chrono::Duration,
) -> Result<u64, DbErr> {
    let now = Utc::now().fixed_offset();
    Ok(agent_memories::Entity::update_many()
        .set(agent_memories::ActiveModel {
            invalid_at: Set(Some(now)),
            invalid_reason: Set(Some("faded".into())),
            updated_at: Set(now),
            ..Default::default()
        })
        .filter(agent_memories::Column::Source.eq(source))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::UpdatedAt.lt(now - older_than))
        .exec(db)
        .await?
        .rows_affected)
}
