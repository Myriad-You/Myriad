//! What is kept in one venue (a group) rather than with one person: notes on people there from outside the community, heard only there.

use super::*;

/// Something she keeps about no account, in one group (`group:<id>`): a
/// note on someone there from outside the community. Heard only there.
pub async fn remember_in_venue<C: ConnectionTrait>(
    db: &C,
    venue: &str,
    content: &str,
    evidence: &str,
    source: &'static str,
) -> Result<Option<String>, DbErr> {
    let content = normalize_content(content);
    if content.is_empty() || !venue.starts_with("group:") {
        return Ok(None);
    }
    let now = Utc::now().fixed_offset();
    let id = format!("grp_{}", uuid::Uuid::new_v4().simple());
    let row = agent_memories::ActiveModel {
        id: Set(id.clone()),
        user_id: Set(None),
        kind: Set(MemoryKind::Fact.as_str().into()),
        content: Set(content),
        evidence: Set(Some(bounded_evidence(evidence))),
        speaker: Set(Speaker::Agent.as_str().into()),
        source: Set(source.into()),
        venue: Set(venue.chars().take(MAX_VENUE_CHARS).collect()),
        audience: Set(json!([])),
        concepts: Set(json!([])),
        importance: Set(0.4),
        access_count: Set(0),
        last_accessed_at: Set(None),
        valid_from: Set(now),
        invalid_at: Set(None),
        invalid_reason: Set(None),
        created_at: Set(now),
        updated_at: Set(now),
    };
    agent_memories::Entity::insert(row)
        .exec_without_returning(db)
        .await?;
    Ok(Some(id))
}

/// It came up again: keep a row of no account's fresh, in its venue.
pub async fn refresh_unowned<C: ConnectionTrait>(
    db: &C,
    venue: &str,
    id: &str,
) -> Result<bool, DbErr> {
    let now = Utc::now().fixed_offset();
    let result = agent_memories::Entity::update_many()
        .set(agent_memories::ActiveModel {
            updated_at: Set(now),
            ..Default::default()
        })
        .filter(agent_memories::Column::UserId.is_null())
        .filter(agent_memories::Column::Venue.eq(venue))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

/// Retire a row of no account's, in its venue.
pub async fn retire_unowned<C: ConnectionTrait>(
    db: &C,
    venue: &str,
    id: &str,
    reason: &str,
) -> Result<bool, DbErr> {
    let now = Utc::now().fixed_offset();
    let result = agent_memories::Entity::update_many()
        .set(agent_memories::ActiveModel {
            invalid_at: Set(Some(now)),
            invalid_reason: Set(Some(reason.chars().take(16).collect())),
            updated_at: Set(now),
            ..Default::default()
        })
        .filter(agent_memories::Column::UserId.is_null())
        .filter(agent_memories::Column::Venue.eq(venue))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

/// The unowned row from `source` in `venue` whose evidence carries `marker`,
/// most recently touched first: a note kept in a group, found by whom it is on.
pub async fn unowned_with_evidence<C: ConnectionTrait>(
    db: &C,
    venue: &str,
    source: &str,
    marker: &str,
) -> Result<Option<agent_memories::Model>, DbErr> {
    agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.is_null())
        .filter(agent_memories::Column::Venue.eq(venue))
        .filter(agent_memories::Column::Source.eq(source))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::Evidence.contains(marker))
        .order_by_desc(agent_memories::Column::UpdatedAt)
        .one(db)
        .await
}

/// Notes kept on someone from outside (rows of `source` with no owner whose
/// evidence carries `marker`) become `user_id`'s ordinary memories from
/// `as_source` once they pair: still of the group they were said in, so
/// they come up there and nowhere else. How many were taken over.
pub async fn adopt_unowned<C: ConnectionTrait>(
    db: &C,
    source: &str,
    marker: &str,
    user_id: i32,
    as_source: &str,
) -> Result<u64, DbErr> {
    let result = agent_memories::Entity::update_many()
        .col_expr(
            agent_memories::Column::UserId,
            sea_orm::sea_query::Expr::value(user_id),
        )
        .col_expr(
            agent_memories::Column::Source,
            sea_orm::sea_query::Expr::value(as_source),
        )
        .col_expr(
            agent_memories::Column::UpdatedAt,
            sea_orm::sea_query::Expr::value(Utc::now().fixed_offset()),
        )
        .filter(agent_memories::Column::UserId.is_null())
        .filter(agent_memories::Column::Source.eq(source))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::Evidence.contains(marker))
        .exec(db)
        .await?;
    Ok(result.rows_affected)
}

/// The latest active rows of `source` kept in `venue`, newest first.
pub async fn latest_in_venue<C: ConnectionTrait>(
    db: &C,
    venue: &str,
    source: &str,
    limit: u64,
) -> Result<Vec<agent_memories::Model>, DbErr> {
    agent_memories::Entity::find()
        .filter(agent_memories::Column::Venue.eq(venue))
        .filter(agent_memories::Column::Source.eq(source))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .order_by_desc(agent_memories::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await
}

/// Rows of one source kept in one venue (`private`, `group:<id>`), freshest
/// first: one person's, or everyone's there when `user_id` is `None`.
pub async fn venue_source_rows<C: ConnectionTrait>(
    db: &C,
    user_id: Option<i32>,
    venue: &str,
    source: &str,
    limit: u64,
) -> Result<Vec<agent_memories::Model>, DbErr> {
    let mut query = agent_memories::Entity::find()
        .filter(agent_memories::Column::Venue.eq(venue))
        .filter(agent_memories::Column::Source.eq(source))
        .filter(agent_memories::Column::InvalidAt.is_null());
    if let Some(user_id) = user_id {
        query = query.filter(agent_memories::Column::UserId.eq(user_id));
    }
    query
        .order_by_desc(agent_memories::Column::UpdatedAt)
        .limit(limit)
        .all(db)
        .await
}
