//! What belongs to her alone: her days, what she did on her own and the views that grew out of it. Nothing personal is in it, so any audience may hear it.

use super::*;

/// Keep one day of her own life. One entry per day: writing the same day
/// again changes nothing. The text must be built from material that names no
/// one, because every audience may hear it.
pub async fn write_own_day<C: ConnectionTrait>(
    db: &C,
    day: chrono::NaiveDate,
    content: &str,
) -> Result<bool, DbErr> {
    let content = normalize_content(content);
    if content.is_empty() {
        return Ok(false);
    }
    let Some(at) = day
        .and_hms_opt(12, 0, 0)
        .map(|noon| noon.and_utc().fixed_offset())
    else {
        return Ok(false);
    };
    let row = agent_memories::ActiveModel {
        id: Set(format!("day_{day}")),
        user_id: Set(None),
        kind: Set(MemoryKind::Narrative.as_str().into()),
        content: Set(content),
        evidence: Set(None),
        speaker: Set(Speaker::Agent.as_str().into()),
        source: Set("narrative".into()),
        venue: Set(OWN_VENUE.into()),
        audience: Set(json!([])),
        concepts: Set(json!([])),
        importance: Set(0.5),
        access_count: Set(0),
        last_accessed_at: Set(None),
        valid_from: Set(at),
        invalid_at: Set(None),
        invalid_reason: Set(None),
        created_at: Set(at),
        updated_at: Set(Utc::now().fixed_offset()),
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

/// Venue of what belongs to her alone: her days and what she did on her own.
pub const OWN_VENUE: &str = "own";

/// Source of what she did on her own and what stayed with her.
pub const OWN_EXPERIENCE: &str = "doing";

/// Source of a view of her own, grown out of those experiences.
pub const OWN_VIEW: &str = "view";

/// Something of her own: what she did (a song, something she read) and what
/// stayed with her, or a view that grew out of such things, in her words.
/// Belongs to no one and names no one; `evidence` says what it was about.
/// Nothing personal is in it, so any conversation may hear it.
pub async fn remember_own<C: ConnectionTrait>(
    db: &C,
    content: &str,
    evidence: &str,
    concepts: Vec<Concept>,
    source: &'static str,
) -> Result<Option<String>, DbErr> {
    let content = normalize_content(content);
    if content.is_empty() {
        return Ok(None);
    }
    let now = Utc::now().fixed_offset();
    let id = format!("own_{}", uuid::Uuid::new_v4().simple());
    let row = agent_memories::ActiveModel {
        id: Set(id.clone()),
        user_id: Set(None),
        kind: Set(MemoryKind::Knowledge.as_str().into()),
        content: Set(content),
        evidence: Set(Some(bounded_evidence(evidence))),
        speaker: Set(Speaker::Agent.as_str().into()),
        source: Set(source.into()),
        venue: Set(OWN_VENUE.into()),
        audience: Set(json!([])),
        concepts: Set(json!(clean_concepts(concepts))),
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

/// What she did on her own, most recent first.
pub async fn own_experiences<C: ConnectionTrait>(
    db: &C,
    limit: u64,
) -> Result<Vec<agent_memories::Model>, DbErr> {
    own_rows(db, OWN_EXPERIENCE, limit).await
}

/// The views she holds now, most recent first.
pub async fn own_views<C: ConnectionTrait>(
    db: &C,
    limit: u64,
) -> Result<Vec<agent_memories::Model>, DbErr> {
    own_rows(db, OWN_VIEW, limit).await
}

/// Her own rows of one source since `since`, most recent first, at most
/// `limit`.
pub async fn own_rows_since<C: ConnectionTrait>(
    db: &C,
    source: &str,
    since: chrono::DateTime<chrono::FixedOffset>,
    limit: u64,
) -> Result<Vec<agent_memories::Model>, DbErr> {
    agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.is_null())
        .filter(agent_memories::Column::Kind.eq(MemoryKind::Knowledge.as_str()))
        .filter(agent_memories::Column::Venue.eq(OWN_VENUE))
        .filter(agent_memories::Column::Source.eq(source))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::CreatedAt.gte(since))
        .order_by_desc(agent_memories::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await
}

/// Her own rows of one source, most recent first.
pub async fn own_rows<C: ConnectionTrait>(
    db: &C,
    source: &str,
    limit: u64,
) -> Result<Vec<agent_memories::Model>, DbErr> {
    agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.is_null())
        .filter(agent_memories::Column::Kind.eq(MemoryKind::Knowledge.as_str()))
        .filter(agent_memories::Column::Venue.eq(OWN_VENUE))
        .filter(agent_memories::Column::Source.eq(source))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .order_by_desc(agent_memories::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await
}

/// Let what she did on her own longer ago than `older_than` fade: the views
/// it grew into stay. Faded rows are deleted once they are `purge_after` old.
pub async fn fade_own_experiences<C: ConnectionTrait>(
    db: &C,
    older_than: chrono::Duration,
    purge_after: chrono::Duration,
) -> Result<(u64, u64), DbErr> {
    let now = Utc::now().fixed_offset();
    let faded = agent_memories::Entity::update_many()
        .set(agent_memories::ActiveModel {
            invalid_at: Set(Some(now)),
            invalid_reason: Set(Some("faded".into())),
            updated_at: Set(now),
            ..Default::default()
        })
        .filter(agent_memories::Column::UserId.is_null())
        .filter(agent_memories::Column::Venue.eq(OWN_VENUE))
        .filter(agent_memories::Column::Source.eq(OWN_EXPERIENCE))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::CreatedAt.lt(now - older_than))
        .exec(db)
        .await?
        .rows_affected;
    let purged = agent_memories::Entity::delete_many()
        .filter(agent_memories::Column::UserId.is_null())
        .filter(agent_memories::Column::Venue.eq(OWN_VENUE))
        .filter(agent_memories::Column::Source.eq(OWN_EXPERIENCE))
        .filter(agent_memories::Column::InvalidAt.lt(now - purge_after))
        .exec(db)
        .await?
        .rows_affected;
    Ok((faded, purged))
}

/// How many things she did on her own since `since`.
pub async fn own_experiences_since<C: ConnectionTrait>(
    db: &C,
    since: chrono::DateTime<chrono::FixedOffset>,
) -> Result<u64, DbErr> {
    use sea_orm::PaginatorTrait;
    agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.is_null())
        .filter(agent_memories::Column::Venue.eq(OWN_VENUE))
        .filter(agent_memories::Column::Source.eq(OWN_EXPERIENCE))
        .filter(agent_memories::Column::CreatedAt.gte(since))
        .count(db)
        .await
}

/// Retire something of her own (a view she no longer holds). Kept, not
/// deleted: what she used to think is part of her.
pub async fn retire_own<C: ConnectionTrait>(db: &C, id: &str, reason: &str) -> Result<bool, DbErr> {
    let now = Utc::now().fixed_offset();
    let result = agent_memories::Entity::update_many()
        .set(agent_memories::ActiveModel {
            invalid_at: Set(Some(now)),
            invalid_reason: Set(Some(reason.chars().take(16).collect())),
            updated_at: Set(now),
            ..Default::default()
        })
        .filter(agent_memories::Column::UserId.is_null())
        .filter(agent_memories::Column::Venue.eq(OWN_VENUE))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::Id.eq(id))
        .exec(db)
        .await?;
    Ok(result.rows_affected > 0)
}

/// Whether her day `day` is written, and whether any day before it is.
pub async fn own_day_written<C: ConnectionTrait>(
    db: &C,
    day: chrono::NaiveDate,
) -> Result<(bool, bool), DbErr> {
    use sea_orm::PaginatorTrait;
    let written = agent_memories::Entity::find_by_id(format!("day_{day}"))
        .one(db)
        .await?
        .is_some();
    let Some(noon) = day.and_hms_opt(12, 0, 0) else {
        return Ok((written, false));
    };
    let before = agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.is_null())
        .filter(agent_memories::Column::Kind.eq(MemoryKind::Narrative.as_str()))
        .filter(agent_memories::Column::CreatedAt.lt(noon.and_utc().fixed_offset()))
        .count(db)
        .await?
        > 0;
    Ok((written, before))
}

/// Her latest days, most recent first.
pub async fn own_days<C: ConnectionTrait>(db: &C, limit: u64) -> Result<Vec<MemoryRecord>, DbErr> {
    Ok(agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.is_null())
        .filter(agent_memories::Column::Kind.eq(MemoryKind::Narrative.as_str()))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .order_by_desc(agent_memories::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await?
        .into_iter()
        .map(MemoryRecord::from)
        .collect())
}
