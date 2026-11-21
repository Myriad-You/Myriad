//! What she said about herself in a chat (`myriad_merope::own_said`), kept
//! where the rule says who hears it again (`own_said::kept_for`): said in a
//! group, it is hers anywhere; said in private, it stays in that private
//! chat, under that person, as what she tells one friend is between them.

use super::*;

/// Keep what she said of herself in front of `present`: her own when said in
/// a group, in that private audience when said in private. `None` when it
/// was empty or that person already holds it.
pub async fn remember_said_of_herself<C: ConnectionTrait>(
    db: &C,
    present: &Audience,
    content: &str,
    evidence: &str,
    concepts: Vec<Concept>,
) -> Result<Option<String>, DbErr> {
    use myriad_merope::own_said::{SOURCE, kept_for};
    match kept_for(&present.rules()).venue {
        rules::Venue::Private => {
            let Some(&user_id) = present.members().first() else {
                return Ok(None);
            };
            remember(
                db,
                NewMemory {
                    user_id,
                    kind: MemoryKind::Knowledge,
                    content: content.into(),
                    evidence: Some(evidence.into()),
                    speaker: Speaker::Agent,
                    source: SOURCE,
                    audience: present.clone(),
                    importance: 0.4,
                    concepts,
                },
            )
            .await
        }
        rules::Venue::Group(_) | rules::Venue::Own => {
            remember_own(db, content, evidence, concepts, SOURCE).await
        }
    }
}

/// What she said of herself that `present` may hear, most recent first, at
/// most `limit`: what is her own, and in private what she said in front of
/// these same people.
pub async fn said_of_herself_heard_by<C: ConnectionTrait>(
    db: &C,
    present: &Audience,
    limit: u64,
) -> Result<Vec<agent_memories::Model>, DbErr> {
    let source = myriad_merope::own_said::SOURCE;
    let mut rows = own_rows(db, source, limit).await?;
    if present.is_group() || present.members().is_empty() {
        return Ok(rows);
    }
    let theirs = agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.is_in(present.members().to_vec()))
        .filter(agent_memories::Column::Kind.eq(MemoryKind::Knowledge.as_str()))
        .filter(agent_memories::Column::Source.eq(source))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .order_by_desc(agent_memories::Column::CreatedAt)
        .limit(limit)
        .all(db)
        .await?;
    rows.extend(theirs.into_iter().filter(|row| admits(row, present)));
    rows.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    rows.truncate(limit as usize);
    Ok(rows)
}

/// Where the one-time sorting of old rows keeps when it first ran.
const SORTED_NAMESPACE: &str = "memory.said_of_herself";
const SORTED_KEY: &str = "scoped_from";
/// How long before a row her reply may have been said, at most.
const REPLY_WITHIN_HOURS: i64 = 48;

/// What [`sort_out_old_said_of_herself`] did with the rows it looked at.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SortedOut {
    /// Said in private: moved to that private chat.
    pub to_private: u64,
    /// Said in a group: left hers.
    pub stayed_hers: u64,
    /// No reply of hers says it: no longer brought back (kept, not deleted).
    pub let_go: u64,
}

/// Once, at startup: what she said of herself before it was kept by where
/// she said it was all kept as her own, heard anywhere. Each such row is
/// matched to her reply it came from (its evidence is an excerpt of it):
/// said in private, it goes to that private chat; said in a group, it stays
/// hers; matched to no reply, it is no longer brought back, its words kept.
/// Only rows from before the first run are looked at, so running it again
/// changes nothing.
pub async fn sort_out_old_said_of_herself<C: ConnectionTrait>(db: &C) -> Result<SortedOut, DbErr> {
    use crate::services::runtime_registry as registry;
    let now = crate::clock::now().fixed_offset();
    let identity = registry::RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    };
    // Kept for a century: the bound must not move.
    let kept_until = now.timestamp() + 100 * 365 * 24 * 3600;
    registry::put_if_absent(db, SORTED_NAMESPACE, SORTED_KEY, identity, &now, kept_until).await?;
    let Some(before) =
        registry::get::<chrono::DateTime<chrono::FixedOffset>>(db, SORTED_NAMESPACE, SORTED_KEY)
            .await?
    else {
        return Ok(SortedOut::default());
    };
    let rows = agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.is_null())
        .filter(agent_memories::Column::Venue.eq(OWN_VENUE))
        .filter(agent_memories::Column::Source.eq(myriad_merope::own_said::SOURCE))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::CreatedAt.lt(before))
        .all(db)
        .await?;
    let mut sorted = SortedOut::default();
    for row in rows {
        match said_where(db, &row).await? {
            Some((user_id, None)) if user_id > 0 => {
                agent_memories::ActiveModel {
                    id: Set(row.id.clone()),
                    user_id: Set(Some(user_id)),
                    venue: Set("private".into()),
                    audience: Set(json!([user_id])),
                    updated_at: Set(now),
                    ..Default::default()
                }
                .update(db)
                .await?;
                sorted.to_private += 1;
            }
            Some((_, Some(_))) => sorted.stayed_hers += 1,
            _ => {
                agent_memories::ActiveModel {
                    id: Set(row.id.clone()),
                    invalid_at: Set(Some(now)),
                    invalid_reason: Set(Some("unmatched".into())),
                    updated_at: Set(now),
                    ..Default::default()
                }
                .update(db)
                .await?;
                sorted.let_go += 1;
            }
        }
    }
    if sorted != SortedOut::default() {
        tracing::info!(
            ?sorted,
            "Memory: old rows of what she said of herself sorted by where she said it"
        );
    }
    Ok(sorted)
}

/// Where she said what `row` keeps: the person she was talking to and the
/// group it was in (none in private), by her latest reply before it that
/// holds its evidence.
async fn said_where<C: ConnectionTrait>(
    db: &C,
    row: &agent_memories::Model,
) -> Result<Option<(i32, Option<String>)>, DbErr> {
    let evidence = row
        .evidence
        .as_deref()
        .and_then(|evidence| serde_json::from_str::<serde_json::Value>(evidence).ok())
        .and_then(|evidence| evidence["said"].as_str().map(str::to_string))
        .unwrap_or_default();
    let evidence = evidence.split_whitespace().collect::<Vec<_>>().join(" ");
    if evidence.is_empty() {
        return Ok(None);
    }
    let found = db
        .query_one_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT s.user_id, s.context->>'venue' AS venue FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE m.role = 'assistant' AND s.context->>'mode' = 'chat' \
               AND m.created_at <= $2 + interval '5 minutes' \
               AND m.created_at >= $2 - make_interval(hours => $3) \
               AND position($1 in regexp_replace(m.content, '\\s+', ' ', 'g')) > 0 \
             ORDER BY m.created_at DESC LIMIT 1",
            [
                evidence.into(),
                row.created_at.into(),
                (REPLY_WITHIN_HOURS as i32).into(),
            ],
        ))
        .await?;
    Ok(found.and_then(|found| {
        let user_id = found.try_get::<i32>("", "user_id").ok()?;
        let venue = found.try_get::<Option<String>>("", "venue").ok()?;
        Some((user_id, venue))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn session(db: &sea_orm::DatabaseConnection, id: &str, user_id: i32, context: &str) {
        db.execute_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
             created_at, last_active_at) VALUES ($1, $2, 't', $3::jsonb, 1, false, NOW(), NOW())",
            [id.into(), user_id.into(), context.into()],
        ))
        .await
        .unwrap();
    }

    async fn reply(db: &sea_orm::DatabaseConnection, session_id: &str, content: &str) {
        db.execute_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "INSERT INTO agent_messages (session_id, role, content, created_at) \
             VALUES ($1, 'assistant', $2, NOW() - INTERVAL '10 minutes')",
            [session_id.into(), content.into()],
        ))
        .await
        .unwrap();
    }

    async fn hers(db: &sea_orm::DatabaseConnection, content: &str, said: &str) -> String {
        let evidence = json!({ "said": said }).to_string();
        remember_own(
            db,
            content,
            &evidence,
            Vec::new(),
            myriad_merope::own_said::SOURCE,
        )
        .await
        .unwrap()
        .unwrap()
    }

    async fn row(db: &sea_orm::DatabaseConnection, id: &str) -> agent_memories::Model {
        agent_memories::Entity::find_by_id(id.to_string())
            .one(db)
            .await
            .unwrap()
            .unwrap()
    }

    /// Kept as hers before it was kept by where she said it: said in
    /// private goes to that chat, said in a group stays hers, matched to
    /// nothing is no longer brought back; a second run changes nothing.
    #[tokio::test]
    async fn old_rows_go_where_she_said_them_once() {
        let Some(url) = crate::db::test_url(&["MYRIAD_MEMORY_TEST_DATABASE_URL"]) else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "said_of_herself_old").await;
        let db = schema.db.clone();
        let base = (uuid::Uuid::new_v4().as_u128() % 1_000_000_000) as i32 + 10;
        let (alone, member) = (base, base + 1);
        for user_id in [alone, member] {
            db.execute_unprepared(&format!(
                "INSERT INTO users (id, username) VALUES ({user_id}, 'said_old_{user_id}')"
            ))
            .await
            .unwrap();
        }
        session(&db, "s_private", alone, r#"{"mode":"chat"}"#).await;
        session(&db, "s_group", member, r#"{"mode":"chat","venue":"qq:42"}"#).await;
        reply(&db, "s_private", "嗯我在追《北窗》，刚看到 第五集啦").await;
        reply(&db, "s_group", "我在读《白塔》，读到一半，太慢了").await;
        let in_private = hers(&db, "我在追《北窗》，看到第五集", "刚看到 第五集").await;
        let in_group = hers(&db, "我在读《白塔》，读到一半", "读到一半").await;
        let nowhere = hers(&db, "我养过一只猫", "我养过一只猫").await;

        let sorted = sort_out_old_said_of_herself(&db).await.unwrap();
        assert_eq!(
            sorted,
            SortedOut {
                to_private: 1,
                stayed_hers: 1,
                let_go: 1
            }
        );
        let moved = row(&db, &in_private).await;
        assert_eq!(moved.user_id, Some(alone));
        assert_eq!(moved.venue, "private");
        assert_eq!(moved.audience, json!([alone]));
        assert_eq!(moved.content, "我在追《北窗》，看到第五集");
        let stayed = row(&db, &in_group).await;
        assert_eq!((stayed.user_id, stayed.venue.as_str()), (None, OWN_VENUE));
        let gone = row(&db, &nowhere).await;
        assert!(gone.invalid_at.is_some());
        assert_eq!(gone.content, "我养过一只猫", "its words are kept");

        // Heard where she said it now.
        let heard = |present: Audience| {
            let db = db.clone();
            async move {
                let mut said: Vec<String> = said_of_herself_heard_by(&db, &present, 10)
                    .await
                    .unwrap()
                    .into_iter()
                    .map(|row| row.content)
                    .collect();
                said.sort();
                said
            }
        };
        assert_eq!(
            heard(Audience::private(alone)).await,
            ["我在读《白塔》，读到一半", "我在追《北窗》，看到第五集"]
        );
        assert_eq!(
            heard(Audience::private(member)).await,
            ["我在读《白塔》，读到一半"]
        );

        // What is kept after the first run is not looked at; nothing changes.
        let later = hers(&db, "我在追《北窗》，看到第六集", "刚看到 第五集").await;
        let all = || {
            agent_memories::Entity::find()
                .order_by_asc(agent_memories::Column::Id)
                .all(&db)
        };
        let before = all().await.unwrap();
        let again = sort_out_old_said_of_herself(&db).await.unwrap();
        assert_eq!((again.to_private, again.let_go), (0, 0));
        assert_eq!(before, all().await.unwrap());
        assert_eq!(row(&db, &later).await.venue, OWN_VENUE);
        schema.drop().await;
    }
}
