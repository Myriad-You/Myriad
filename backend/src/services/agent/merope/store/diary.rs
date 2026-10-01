//! Her diary, and who she talked with or spoke up to when.

use super::*;

/// One diary table, source-scoped reads. Superseded persona facts remain as
/// history but never participate in active recall.
///
/// The rows are the same shape, are created and deleted together, and are read
/// together (`list_diary_from_sources`), so a discriminator is the right split
/// and three tables would only buy a three-way union. What the sources do not
/// share is meaning: `remember` is a fact the user stated, while `event` and
/// `chat` are summaries the platform generated about them. Reads are therefore
/// always source-scoped — there is no "latest row of any kind" — so a stated
/// fact can never arrive somewhere expecting a generated summary.
/// Events she let pass were once written here too; nothing read them, so no
/// more are. Rows from then remain, and must never leak into what she
/// recalls (the store tests seed some).
#[cfg(test)]
pub const DIARY_SOURCE_EVENT: &str = "event";

pub const DIARY_SOURCE_CHAT: &str = "chat";

pub async fn insert_diary<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    content: &str,
    source: &str,
) -> Result<agent_diary::Model, anyhow::Error> {
    let active = agent_diary::ActiveModel {
        id: Set(Uuid::new_v4().simple().to_string()),
        user_id: Set(user_id),
        content: Set(content.to_string()),
        source: Set(source.to_string()),
        created_at: Set(Utc::now().into()),
    };
    Ok(active.insert(db).await?)
}

pub async fn latest_diary(
    db: &DatabaseConnection,
    user_id: i32,
    source: &str,
) -> Result<Option<agent_diary::Model>, anyhow::Error> {
    Ok(agent_diary::Entity::find()
        .filter(agent_diary::Column::UserId.eq(user_id))
        .filter(agent_diary::Column::Source.eq(source))
        .order_by_desc(agent_diary::Column::CreatedAt)
        .one(db)
        .await?)
}

pub async fn list_diary_from_sources(
    db: &DatabaseConnection,
    user_id: i32,
    sources: &[&str],
    limit: u64,
) -> Result<Vec<agent_diary::Model>, anyhow::Error> {
    if sources.is_empty() || limit == 0 {
        return Ok(Vec::new());
    }
    // As of now as this turn is put together: a turn answered again later
    // does not see what came after it.
    Ok(agent_diary::Entity::find()
        .filter(agent_diary::Column::UserId.eq(user_id))
        .filter(agent_diary::Column::Source.is_in(sources.iter().copied()))
        .filter(agent_diary::Column::CreatedAt.lte(super::super::clock::now().fixed_offset()))
        .order_by_desc(agent_diary::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await?)
}

/// How many people last spoke to her in `[start, end)`: whoever came back
/// later counts on the later day, so this undercounts and never names.
pub(crate) async fn people_last_talked_between(
    db: &DatabaseConnection,
    start: chrono::DateTime<chrono::FixedOffset>,
    end: chrono::DateTime<chrono::FixedOffset>,
) -> Result<u64, sea_orm::DbErr> {
    use sea_orm::PaginatorTrait;
    agent_addressee_state::Entity::find()
        .filter(agent_addressee_state::Column::LastUserMessageAt.gte(start))
        .filter(agent_addressee_state::Column::LastUserMessageAt.lt(end))
        .count(db)
        .await
}

/// When each person who spoke to her after `since` last did.
pub(crate) async fn last_talked_since(
    db: &DatabaseConnection,
    since: chrono::DateTime<chrono::FixedOffset>,
) -> Result<Vec<chrono::DateTime<chrono::FixedOffset>>, sea_orm::DbErr> {
    let spoken: Vec<Option<chrono::DateTime<chrono::FixedOffset>>> =
        agent_addressee_state::Entity::find()
            .select_only()
            .column(agent_addressee_state::Column::LastUserMessageAt)
            .filter(agent_addressee_state::Column::LastUserMessageAt.gt(since))
            .into_tuple()
            .all(db)
            .await?;
    Ok(spoken.into_iter().flatten().collect())
}

/// Work that ended with `status` (`completed`, `failed`) in `[start, end)`.
pub(crate) async fn work_ended_between(
    db: &DatabaseConnection,
    status: &str,
    start: chrono::DateTime<chrono::FixedOffset>,
    end: chrono::DateTime<chrono::FixedOffset>,
) -> Result<u64, sea_orm::DbErr> {
    use crate::models::entities::agent_tasks;
    use sea_orm::PaginatorTrait;
    agent_tasks::Entity::find()
        .filter(agent_tasks::Column::Status.eq(status))
        .filter(agent_tasks::Column::CompletedAt.gte(start))
        .filter(agent_tasks::Column::CompletedAt.lt(end))
        .count(db)
        .await
}

/// How many times she spoke up unprompted in `[start, end)`.
pub(crate) async fn spoke_up_between(
    db: &DatabaseConnection,
    start: chrono::DateTime<chrono::FixedOffset>,
    end: chrono::DateTime<chrono::FixedOffset>,
) -> Result<u64, sea_orm::DbErr> {
    use sea_orm::PaginatorTrait;
    agent_proactive_messages::Entity::find()
        .filter(agent_proactive_messages::Column::CreatedAt.gte(start))
        .filter(agent_proactive_messages::Column::CreatedAt.lt(end))
        .count(db)
        .await
}

/// Event persona-memory insert. Dedup and the retraction check run under the
/// persona-memory lock, against the unified memory table.
pub(crate) async fn insert_remembered_if_new(
    db: &DatabaseConnection,
    user_id: i32,
    candidate: &str,
    evidence: Option<&str>,
) -> Result<bool, anyhow::Error> {
    use crate::services::agent::memory::unified;
    let fact = super::super::ingest::compact_summary(candidate);
    if user_id <= 0 || fact.is_empty() {
        return Ok(false);
    }
    let transaction = db.begin().await?;
    lock_persona_memory(&transaction, user_id).await?;
    // Events may add facts, but cannot resurrect a fact the person retracted.
    // Only a new user assertion may re-establish it.
    if unified::retracted_by_person(&transaction, user_id, &fact).await? {
        transaction.commit().await?;
        return Ok(false);
    }
    let inserted = unified::remember(
        &transaction,
        unified::NewMemory {
            user_id,
            kind: unified::MemoryKind::Fact,
            content: fact,
            // The event she gathered it from.
            evidence: evidence
                .map(super::super::ingest::compact_summary)
                .filter(|evidence| !evidence.is_empty()),
            speaker: unified::Speaker::Agent,
            source: "event",
            audience: unified::Audience::private(user_id),
            importance: 0.5,
            concepts: Vec::new(),
        },
    )
    .await?
    .is_some();
    transaction.commit().await?;
    Ok(inserted)
}

pub(super) async fn lock_persona_memory<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
) -> Result<(), sea_orm::DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock($1, $2)",
        vec![1296388173_i32.into(), user_id.into()],
    ))
    .await?;
    Ok(())
}
