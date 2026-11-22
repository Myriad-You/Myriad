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

/// Where the one-time sorting of old rows marks that it ran.
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
    /// No reply of hers says it, or replies in more than one place do: no
    /// longer brought back (kept, not deleted).
    pub let_go: u64,
}

/// Once, at the first start that has it: what she said of herself before it
/// was kept by where she said it was all kept as her own, heard anywhere.
/// Each such row the site wrote is matched to her reply it came from (its
/// evidence is an excerpt of it): said in private, it goes to that private
/// chat; said in a group, it stays hers; matched to no reply, or to replies
/// in more than one place, it is no longer brought back, its words kept.
/// Rows another end wrote are that end's to sort. It runs in one
/// transaction with its mark, so a later start does nothing.
///
/// Her log (`her_log::memories`) follows what it does without being told:
/// a row moved into someone else's private chat leaves what her log keeps
/// and goes out retired; one moved into the site owner's own private chat
/// keeps its digest (which does not hold where it was learned), so the
/// other ends keep it as hers. On the site it ran with no such row at all,
/// so this is not carried further.
pub async fn sort_out_old_said_of_herself(
    db: &sea_orm::DatabaseConnection,
) -> Result<SortedOut, DbErr> {
    use crate::services::runtime_registry as registry;
    use sea_orm::TransactionTrait;
    let now = crate::clock::now().fixed_offset();
    let identity = registry::RegistryIdentity {
        subject_id: None,
        owner_id: None,
        tapp_id: None,
        runtime_id: None,
    };
    // Kept for a century: it marks that this ran.
    let kept_until = now.timestamp() + 100 * 365 * 24 * 3600;
    let txn = db.begin().await?;
    if !registry::put_if_absent(
        &txn,
        SORTED_NAMESPACE,
        SORTED_KEY,
        identity,
        &now,
        kept_until,
    )
    .await?
    {
        txn.commit().await?;
        return Ok(SortedOut::default());
    }
    let rows = agent_memories::Entity::find()
        .filter(agent_memories::Column::UserId.is_null())
        .filter(agent_memories::Column::Venue.eq(OWN_VENUE))
        .filter(agent_memories::Column::Source.eq(myriad_merope::own_said::SOURCE))
        .filter(agent_memories::Column::Origin.eq(ORIGIN_SITE))
        .filter(agent_memories::Column::InvalidAt.is_null())
        .filter(agent_memories::Column::CreatedAt.lt(now))
        .all(&txn)
        .await?;
    let mut sorted = SortedOut::default();
    for row in rows {
        match said_where(&txn, &row).await? {
            Some(Said::Private(user_id)) => {
                agent_memories::ActiveModel {
                    id: Set(row.id.clone()),
                    user_id: Set(Some(user_id)),
                    venue: Set("private".into()),
                    audience: Set(json!([user_id])),
                    updated_at: Set(now),
                    ..Default::default()
                }
                .update(&txn)
                .await?;
                sorted.to_private += 1;
            }
            Some(Said::InGroup) => sorted.stayed_hers += 1,
            None => {
                agent_memories::ActiveModel {
                    id: Set(row.id.clone()),
                    invalid_at: Set(Some(now)),
                    invalid_reason: Set(Some("unmatched".into())),
                    updated_at: Set(now),
                    ..Default::default()
                }
                .update(&txn)
                .await?;
                sorted.let_go += 1;
            }
        }
    }
    txn.commit().await?;
    if sorted != SortedOut::default() {
        tracing::info!(
            ?sorted,
            "Memory: old rows of what she said of herself sorted by where she said it"
        );
    }
    Ok(sorted)
}

/// Where she said something.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Said {
    /// In a private chat with this account.
    Private(i32),
    InGroup,
}

/// Where she said what `row` keeps: the one place whose replies of hers
/// shortly before it hold its evidence. None when no reply does, or replies
/// in more than one place do (a short excerpt can be said to two people;
/// guessing would put one friend's words in front of another).
async fn said_where<C: ConnectionTrait>(
    db: &C,
    row: &agent_memories::Model,
) -> Result<Option<Said>, DbErr> {
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
    let places = db
        .query_all_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT DISTINCT s.context->>'venue' AS venue, \
               CASE WHEN s.context->>'venue' IS NULL THEN s.user_id END AS user_id \
             FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE m.role = 'assistant' AND s.context->>'mode' = 'chat' \
               AND m.created_at <= $2 + interval '5 minutes' \
               AND m.created_at >= $2 - make_interval(hours => $3) \
               AND position($1 in regexp_replace(m.content, '\\s+', ' ', 'g')) > 0 \
             LIMIT 2",
            [
                evidence.into(),
                row.created_at.into(),
                (REPLY_WITHIN_HOURS as i32).into(),
            ],
        ))
        .await?;
    let [place] = places.as_slice() else {
        return Ok(None);
    };
    let venue = place.try_get::<Option<String>>("", "venue")?;
    let user_id = place.try_get::<Option<i32>>("", "user_id")?;
    Ok(match (venue, user_id) {
        (Some(_), _) => Some(Said::InGroup),
        (None, Some(user_id)) if user_id > 0 => Some(Said::Private(user_id)),
        _ => None,
    })
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
    /// nothing or to two places is no longer brought back, another end's is
    /// left to it; a second start does nothing.
    #[tokio::test]
    async fn old_rows_go_where_she_said_them_once() {
        let Some(url) = crate::db::test_url(&["MYRIAD_MEMORY_TEST_DATABASE_URL"]) else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "said_of_herself_old").await;
        let db = schema.db.clone();
        let base = (uuid::Uuid::new_v4().as_u128() % 1_000_000_000) as i32 + 10;
        let (alone, member, friend) = (base, base + 1, base + 2);
        for user_id in [alone, member, friend] {
            db.execute_unprepared(&format!(
                "INSERT INTO users (id, username) VALUES ({user_id}, 'said_old_{user_id}')"
            ))
            .await
            .unwrap();
        }
        session(&db, "s_private", alone, r#"{"mode":"chat"}"#).await;
        session(&db, "s_group", member, r#"{"mode":"chat","venue":"qq:42"}"#).await;
        session(&db, "s_friend", friend, r#"{"mode":"chat"}"#).await;
        reply(&db, "s_private", "嗯我在追《北窗》，刚看到 第五集啦").await;
        reply(&db, "s_group", "我在读《白塔》，读到一半，太慢了").await;
        // The same few words to two people: whose it was cannot be told.
        reply(&db, "s_private", "我也喜欢下雨天").await;
        reply(&db, "s_friend", "哈哈我也喜欢下雨天").await;
        let in_private = hers(&db, "我在追《北窗》，看到第五集", "刚看到 第五集").await;
        let in_group = hers(&db, "我在读《白塔》，读到一半", "读到一半").await;
        let nowhere = hers(&db, "我养过一只猫", "我养过一只猫").await;
        let to_both = hers(&db, "我喜欢下雨天", "我也喜欢下雨天").await;
        // Another end's row of the same: that end sorts its own.
        let theirs = hers(&db, "我在看《北窗》", "刚看到 第五集").await;
        db.execute_unprepared(&format!(
            "UPDATE agent_memories SET origin = 'dev_mac', evidence = NULL WHERE id = '{theirs}'"
        ))
        .await
        .unwrap();

        let sorted = sort_out_old_said_of_herself(&db).await.unwrap();
        assert_eq!(
            sorted,
            SortedOut {
                to_private: 1,
                stayed_hers: 1,
                let_go: 2
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
        assert!(row(&db, &to_both).await.invalid_at.is_some());
        let left = row(&db, &theirs).await;
        assert_eq!((left.venue.as_str(), left.invalid_at), (OWN_VENUE, None));

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
            [
                "我在看《北窗》",
                "我在读《白塔》，读到一半",
                "我在追《北窗》，看到第五集"
            ]
        );
        assert_eq!(
            heard(Audience::private(member)).await,
            ["我在看《北窗》", "我在读《白塔》，读到一半"]
        );

        // A later start does nothing, not even with a row it would move.
        let later = hers(&db, "我在追《北窗》，看到第六集", "刚看到 第五集").await;
        let all = || {
            agent_memories::Entity::find()
                .order_by_asc(agent_memories::Column::Id)
                .all(&db)
        };
        let before = all().await.unwrap();
        let again = sort_out_old_said_of_herself(&db).await.unwrap();
        assert_eq!(again, SortedOut::default());
        assert_eq!(before, all().await.unwrap());
        assert_eq!(row(&db, &later).await.venue, OWN_VENUE);
        schema.drop().await;
    }
}
