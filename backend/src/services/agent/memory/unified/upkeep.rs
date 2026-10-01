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

/// One row carried over from a pre-unified store, keeping its identity and
/// history. An id seen before is skipped, so importing twice is harmless.
pub struct ImportedMemory {
    pub id: String,
    pub user_id: i32,
    pub kind: MemoryKind,
    pub content: String,
    pub importance: f64,
    pub access_count: i32,
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
    pub last_accessed_at: Option<chrono::DateTime<chrono::FixedOffset>>,
}

/// Returns whether the row was new.
pub async fn import<C: ConnectionTrait>(db: &C, memory: ImportedMemory) -> Result<bool, DbErr> {
    let content = normalize_content(&memory.content);
    if memory.user_id <= 0 || content.is_empty() {
        return Ok(false);
    }
    let row = agent_memories::ActiveModel {
        id: Set(memory.id),
        user_id: Set(Some(memory.user_id)),
        kind: Set(memory.kind.as_str().into()),
        content: Set(content),
        evidence: Set(None),
        speaker: Set(Speaker::Import.as_str().into()),
        source: Set("import".into()),
        venue: Set("private".into()),
        audience: Set(json!([memory.user_id])),
        concepts: Set(json!([])),
        importance: Set(memory.importance.clamp(0.0, 1.0)),
        access_count: Set(std::cmp::Ord::max(memory.access_count, 0)),
        last_accessed_at: Set(memory.last_accessed_at),
        valid_from: Set(memory.created_at),
        invalid_at: Set(None),
        invalid_reason: Set(None),
        created_at: Set(memory.created_at),
        updated_at: Set(memory.created_at),
    };
    let inserted = agent_memories::Entity::insert(row)
        .on_conflict(
            sea_orm::sea_query::OnConflict::column(agent_memories::Column::Id)
                .do_nothing()
                .to_owned(),
        )
        .exec_without_returning(db)
        .await?;
    Ok(inserted > 0)
}
