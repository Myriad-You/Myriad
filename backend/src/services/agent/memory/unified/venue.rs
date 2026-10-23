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
    #[cfg(test)]
    crate::services::channel_group::g0::note(
        "remember",
        serde_json::json!({ "venue": venue, "content": content, "evidence": evidence, "source": source }),
    );
    let now = super::super::utc_now().fixed_offset();
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
        superseded_by: Set(None),
        origin: Set(super::ORIGIN_SITE.into()),
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
    let now = crate::clock::now().fixed_offset();
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
    retire_unowned_in_favor_of(db, venue, id, reason, None).await
}

/// [`retire_unowned`], naming the row that took its place (`by`) in the same
/// update.
pub async fn retire_unowned_in_favor_of<C: ConnectionTrait>(
    db: &C,
    venue: &str,
    id: &str,
    reason: &str,
    by: Option<&str>,
) -> Result<bool, DbErr> {
    let now = crate::clock::now().fixed_offset();
    let result = agent_memories::Entity::update_many()
        .set(agent_memories::ActiveModel {
            invalid_at: Set(Some(now)),
            invalid_reason: Set(Some(reason.chars().take(16).collect())),
            superseded_by: Set(by.map(str::to_string)),
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

/// The unowned rows from `source` whose evidence carries `marker`, kept in
/// groups other than `here` (stored form), most recently touched first: her
/// notes on the same account in the other groups she knows them from.
pub async fn unowned_elsewhere<C: ConnectionTrait>(
    db: &C,
    here: &str,
    source: &str,
    marker: &str,
    limit: u64,
) -> Result<Vec<agent_memories::Model>, DbErr> {
    agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.is_null())
        .filter(agent_memories::Column::Venue.starts_with("group:"))
        .filter(agent_memories::Column::Venue.ne(here))
        .filter(agent_memories::Column::Source.eq(source))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::Evidence.contains(marker))
        .order_by_desc(agent_memories::Column::UpdatedAt)
        .limit(limit)
        .all(db)
        .await
}

/// Rows of `sources` kept in groups other than `here` (stored form): one
/// person's when `user_id` is given, else everyone's; freshest first, short
/// notes that have faded left out.
pub async fn in_other_groups<C: ConnectionTrait>(
    db: &C,
    user_id: Option<i32>,
    here: &str,
    sources: &[&str],
    limit: u64,
) -> Vec<agent_memories::Model> {
    let mut query = agent_memories::Entity::find()
        .filter(agent_memories::Column::Venue.starts_with("group:"))
        .filter(agent_memories::Column::Venue.ne(here))
        .filter(agent_memories::Column::Source.is_in(sources.iter().copied()))
        .filter(agent_memories::Column::InvalidAt.is_null());
    if let Some(user_id) = user_id {
        query = query.filter(agent_memories::Column::UserId.eq(user_id));
    }
    let now = crate::clock::now().fixed_offset();
    query
        .order_by_desc(agent_memories::Column::UpdatedAt)
        .limit(limit)
        .all(db)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|row| !faded_note(row, now))
        .collect()
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
            sea_orm::sea_query::Expr::value(crate::clock::now().fixed_offset()),
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

/// How many of one source in one venue the talk can bring back from.
const LOOKED_OVER: u64 = 500;

/// Of one source kept in one venue, the `fresh` freshest, then at most
/// `brought` older ones that `words` bring back (see [`named_among`]).
pub async fn fresh_and_brought<C: ConnectionTrait>(
    db: &C,
    venue: &str,
    source: &str,
    fresh: usize,
    words: Option<&str>,
    brought: usize,
) -> Vec<agent_memories::Model> {
    let mut rows = venue_source_rows(db, None, venue, source, LOOKED_OVER)
        .await
        .unwrap_or_default();
    let older = rows.split_off(fresh.min(rows.len()));
    if let Some(words) = words {
        rows.extend(named_among(db, older, words, brought).await);
    }
    rows
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
