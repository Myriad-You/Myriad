//! What people asked her not to keep from now on: kept for good under its
//! own source, so it is there every time she keeps something of them, not
//! only when a line happens to bring it to mind.

use super::*;

/// What `user_id` asked her not to keep, newest first: wherever they said
/// it, private talk or any group, for it holds wherever they say more. Only
/// hold on to what is kept by it: it is heard by no one ([`NO_ONE`]).
pub async fn asked_not_to_keep<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
) -> Result<Vec<MemoryRecord>, DbErr> {
    Ok(agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.eq(user_id))
        .filter(agent_memories::Column::Source.eq(ASKED_SOURCE))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .order_by_desc(agent_memories::Column::CreatedAt)
        .order_by_desc(agent_memories::Column::Id)
        .all(db)
        .await?
        .into_iter()
        .map(MemoryRecord::from)
        .collect())
}
