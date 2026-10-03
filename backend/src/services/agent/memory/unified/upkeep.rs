//! Upkeep over existing rows: concepts filled in for rows written before concepts existed, and memories imported from elsewhere.

use super::*;

/// A person's active memories that no concept was ever written for (kept
/// before concepts existed), oldest first, so association can reach them.
pub async fn without_concepts<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    limit: u64,
) -> Result<Vec<MemoryRecord>, DbErr> {
    Ok(agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(sea_orm::sea_query::Expr::cust("concepts = '[]'::jsonb"))
        // Kept apart, never recalled by association: written without concepts.
        .filter(agent_memories::Column::Source.is_not_in(KEPT_APART))
        .order_by_asc(agent_memories::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await?
        .into_iter()
        .map(MemoryRecord::from)
        .collect())
}

/// People who have memories still lacking concepts.
pub async fn people_without_concepts<C: ConnectionTrait>(
    db: &C,
    limit: u64,
) -> Result<Vec<i32>, DbErr> {
    let people: Vec<Option<i32>> = agent_memories::Entity::find()
        .select_only()
        .column(agent_memories::Column::UserId)
        .distinct()
        .filter(agent_memories::Column::UserId.is_not_null())
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(sea_orm::sea_query::Expr::cust("concepts = '[]'::jsonb"))
        .filter(agent_memories::Column::Source.is_not_in(KEPT_APART))
        .limit(limit)
        .into_tuple()
        .all(db)
        .await?;
    Ok(people.into_iter().flatten().collect())
}

/// Give a memory the concepts it was never written with. Only fills an empty
/// list, so a concurrent writer's concepts are never overwritten.
pub async fn fill_concepts<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    id: &str,
    concepts: Vec<Concept>,
) -> Result<bool, DbErr> {
    let concepts = clean_concepts(concepts);
    if concepts.is_empty() {
        return Ok(false);
    }
    let result = agent_memories::Entity::update_many()
        .col_expr(
            agent_memories::Column::Concepts,
            sea_orm::sea_query::Expr::value(json!(concepts)),
        )
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::Id.eq(id))
        .filter(sea_orm::sea_query::Expr::cust("concepts = '[]'::jsonb"))
        .exec(db)
        .await?;
    Ok(result.rows_affected == 1)
}
