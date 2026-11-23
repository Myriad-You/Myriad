//! Lines of a talk that are what they asked her not to keep: marked where
//! they are kept (`agent_messages.metadata`, `notKept`), their words left as
//! they are. Looking back leaves a marked line out, with her answer to it
//! (`chat_lines`, `chat_days`); the talk in front of her is not read here.
//!
//! Judged once the stretch they were said in is gone over, so what they ask
//! in it holds for it too, and never while anyone waits: a job written with
//! what the stretch kept (`memory_jobs::Payload::NotKept`), and nothing of
//! the stretch looked back to until it is done ([`waiting`]). What they let her
//! keep again after that holds from then on: a line marked stays so.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use crate::services::agent::memory::unified::Audience;
use crate::services::agent::merope::memory_jobs::Failure;

pub(super) static MARKED: AtomicU64 = AtomicU64::new(0);

/// A little before a stretch began, as going over it reads
/// (`chat_remember::settle`).
const BEFORE: chrono::Duration = chrono::Duration::minutes(2);

/// A stretch of a talk still to be judged: whose, where (none: in private),
/// and when it was said. None of it is looked back to until it is.
pub(in crate::services::agent::merope) struct Waiting {
    pub(in crate::services::agent::merope) user_id: i32,
    from: chrono::DateTime<chrono::FixedOffset>,
    last: chrono::DateTime<chrono::FixedOffset>,
}

impl Waiting {
    /// Whether what `user_id` (or she, to them) said `at` is of it.
    pub(in crate::services::agent::merope) fn holds(
        &self,
        user_id: i32,
        at: chrono::DateTime<chrono::FixedOffset>,
    ) -> bool {
        self.user_id == user_id && (self.from - BEFORE..=self.last + BEFORE).contains(&at)
    }
}

/// The stretches still to be judged at `venue` (none: in private), of
/// `user_id` or, with none, of everyone there.
pub(in crate::services::agent::merope) async fn waiting<C: ConnectionTrait>(
    db: &C,
    user_id: Option<i32>,
    venue: Option<&str>,
) -> Result<Vec<Waiting>, sea_orm::DbErr> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT (payload->>'owner')::int AS user_id, payload->'data'->'NotKept' AS stretch \
             FROM runtime_registry WHERE namespace = $1 \
               AND payload->'data'->'NotKept' IS NOT NULL \
               AND ($2::int IS NULL OR (payload->>'owner')::int = $2)",
            [
                super::super::store::memory_jobs::ASKED_NAMESPACE.into(),
                user_id.into(),
            ],
        ))
        .await?;
    #[derive(serde::Deserialize)]
    struct Stretch {
        present: Audience,
        from: chrono::DateTime<chrono::FixedOffset>,
        last: chrono::DateTime<chrono::FixedOffset>,
    }
    let mut waiting = Vec::new();
    for row in rows {
        let Ok(Stretch {
            present,
            from,
            last,
        }) = serde_json::from_value(row.try_get("", "stretch")?)
        else {
            // Another build's, or broken: it holds back nothing else.
            tracing::warn!("[Merope] a stretch to be judged could not be read");
            continue;
        };
        if present.group_id() == venue {
            waiting.push(Waiting {
                user_id: row.try_get("", "user_id")?,
                from,
                last,
            });
        }
    }
    Ok(waiting)
}

/// The stretch of `user_id`'s talk at `present` said from `from` to `last`,
/// and her answers then, up to their next line: each line judged and noted
/// as it is, so a stretch tried again goes on where it stopped. What is not
/// told (by `until`) is not marked: an `Err`, to be judged again, or, with
/// `gives_up`, let be (`notTold`: not looked back to either, though never
/// said to be of it). What is marked is hidden on her nodes too.
pub(in crate::services::agent::merope) async fn mark(
    db: &DatabaseConnection,
    user_id: i32,
    present: &Audience,
    from: chrono::DateTime<chrono::FixedOffset>,
    last: chrono::DateTime<chrono::FixedOffset>,
    until: Instant,
    gives_up: bool,
) -> Result<(), Failure> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT m.id, m.role, m.content, m.created_at, \
               (m.metadata->>'notKept') IS NOT NULL AS judged FROM agent_messages m \
             JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 AND COALESCE(s.context->>'mode', 'chat') = 'chat' \
               AND (s.context->>'venue') IS NOT DISTINCT FROM $2 \
               AND m.role IN ('user', 'assistant') AND (m.metadata->>'piece') IS NULL \
               AND m.created_at >= $3 AND m.created_at <= $4 \
             ORDER BY m.created_at, m.id",
            [
                user_id.into(),
                present.group_id().map(str::to_string).into(),
                (from - BEFORE).into(),
                (last + BEFORE).into(),
            ],
        ))
        .await
        .map_err(|_| Failure::Storage)?;
    let mut ids: Vec<i32> = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    for row in rows {
        let content: String = row.try_get("", "content").map_err(|_| Failure::Storage)?;
        let at: chrono::DateTime<chrono::FixedOffset> = row
            .try_get("", "created_at")
            .map_err(|_| Failure::Storage)?;
        let role: String = row.try_get("", "role").map_err(|_| Failure::Storage)?;
        // Their next line, and what came after, is the next stretch's; one
        // too short to be one of a stretch begins none.
        if at > last
            && role == "user"
            && crate::services::agent::merope::chat_remember::settle_raw::words_of(&content)
                .is_some()
        {
            break;
        }
        if row.try_get("", "judged").map_err(|_| Failure::Storage)? {
            continue;
        }
        ids.push(row.try_get("", "id").map_err(|_| Failure::Storage)?);
        texts.push(crate::services::agent::chat_prompt::chat_safe_content(
            &content,
        ));
    }
    let mut hidden: Vec<i32> = Vec::new();
    let mut not_told: Vec<i32> = Vec::new();
    let mut untold = super::Told::default();
    for part in myriad_merope::not_to_keep::parts(&texts) {
        let ids = &ids[part.clone()];
        let told = super::told_of(db, user_id, None, &texts[part], until).await;
        MARKED.fetch_add(told.out.len() as u64, Ordering::Relaxed);
        let marked: Vec<i32> = told.out.iter().map(|index| ids[*index]).collect();
        let judged: Vec<i32> = (ids.iter().enumerate())
            .filter(|(index, _)| !told.untold.contains(index))
            .map(|(_, id)| *id)
            .collect();
        hidden.extend(&marked);
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_messages SET metadata = (CASE WHEN jsonb_typeof(metadata::jsonb) = 'object' \
               THEN metadata::jsonb ELSE '{}'::jsonb END \
               - 'notTold' || jsonb_build_object('notKept', id = ANY($2)))::json \
             WHERE id = ANY($1)",
            [judged.into(), marked.into()],
        ))
        .await
        .map_err(|_| Failure::Storage)?;
        not_told.extend(told.untold.iter().map(|index| ids[*index]));
        untold.untold.extend(told.untold);
        if let Some(failure) = told.failure {
            // Not to be reached, or no time left: the rest is asked later.
            let stop = !failure.untold();
            untold.failure = Some(failure);
            if stop {
                break;
            }
        }
    }
    // Never told of: not looked back to all the same, and nothing waits for
    // them any more. The mark is no judgment: without it they are as they were.
    if gives_up && untold.failure.as_ref().is_some_and(Failure::untold) {
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_messages SET metadata = (CASE WHEN jsonb_typeof(metadata::jsonb) = 'object' \
               THEN metadata::jsonb ELSE '{}'::jsonb END || '{\"notTold\":true}'::jsonb)::json \
             WHERE id = ANY($1)",
            [not_told.clone().into()],
        ))
        .await
        .map_err(|_| Failure::Storage)?;
        hidden.extend(not_told);
    }
    if !hidden.is_empty() && !present.is_group() {
        crate::services::agent::merope::memory_nodes::forget::hide_lines_let_go(
            db, user_id, &hidden,
        )
        .await;
    }
    untold.let_be(gives_up)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::agent::merope::asked::tests::asks;
    use crate::services::agent::merope::chat_days;
    use crate::services::agent::merope::chat_lines::{
        private_lines,
        tests::{OTHER, line, schema, session},
    };
    use crate::services::channel_group::g0;

    /// How often the judge was asked, and whether the stretch was told of.
    async fn marking(
        db: &DatabaseConnection,
        present: &Audience,
        answer: Option<&str>,
    ) -> (usize, bool) {
        let mut world = g0::World::new(0.5);
        world
            .models
            .entry("not_to_keep".into())
            .or_default()
            // The same each time it is asked; with none, no answer that reads.
            .extend(vec![Some(answer.unwrap_or("嗯").to_string()); 3]);
        g0::enter(world);
        let now = crate::clock::now().fixed_offset();
        let told = mark(
            db,
            OTHER,
            present,
            now - chrono::Duration::minutes(25),
            now - chrono::Duration::minutes(10),
            Instant::now() + std::time::Duration::from_secs(60),
            false,
        )
        .await;
        let world = g0::leave().unwrap();
        let asked = world
            .log
            .iter()
            .filter(|entry| entry["kind"] == "model")
            .count();
        (asked, told.is_ok())
    }

    /// In private: a marked line and her answer to it are not looked back
    /// to, its words stay, and nothing before the stretch is touched.
    #[tokio::test]
    async fn a_line_of_what_they_asked_not_to_keep_is_not_brought_up_again() {
        let Some(schema) = schema("asked_lines").await else {
            return;
        };
        let db = &schema.db;
        session(db, "mine", OTHER, Some(r#"{"mode":"chat"}"#), false).await;
        line(db, "mine", "user", "上个月房东也吵过", 90, None).await;
        line(db, "mine", "user", "房东又来吵", 24, None).await;
        line(db, "mine", "assistant", "别气", 23, None).await;
        line(
            db,
            "mine",
            "user",
            "周末去爬香山",
            12,
            Some(r#"{"kept":1}"#),
        )
        .await;
        let alone = Audience::private(OTHER);
        // No one who asked nothing is asked about.
        assert_eq!(marking(db, &alone, None).await, (0, true));
        db.execute_unprepared(
            "UPDATE agent_messages SET metadata = (metadata::jsonb - 'notKept')::json",
        )
        .await
        .unwrap();
        asks(db, OTHER, "不要记和房东吵架的事").await;
        assert_eq!(
            marking(db, &alone, Some(r#"{"drop":[0]}"#)).await,
            (1, true)
        );
        let said = async || -> Vec<String> {
            private_lines(db, OTHER, 100)
                .await
                .into_iter()
                .map(|line| line.content)
                .collect()
        };
        assert_eq!(said().await, ["上个月房东也吵过", "周末去爬香山"]);
        let words = db
            .query_all_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT content FROM agent_messages WHERE content = '房东又来吵'",
            ))
            .await
            .unwrap();
        assert_eq!(words.len(), 1);
        // One that cannot be told is not marked, and is asked about again.
        line(db, "mine", "user", "晚上吃了火锅", 11, None).await;
        assert_eq!(marking(db, &alone, None).await, (3, false));
        assert_eq!(judged_as(db, "晚上吃了火锅").await, None);
        assert_eq!(marking(db, &alone, Some(r#"{"drop":[]}"#)).await, (1, true));
        assert_eq!(judged_as(db, "晚上吃了火锅").await, Some(false));
        schema.drop().await;
    }

    /// In a group, what one of them asked not to keep, in their own talk
    /// with her there: not turned back to, nor her answer to it; another's is.
    #[tokio::test]
    async fn a_groups_days_leave_out_what_one_of_them_asked_not_to_keep() {
        let Some(schema) = schema("asked_lines_group").await else {
            return;
        };
        let db = &schema.db;
        let group = Some(r#"{"mode":"chat","venue":"qq:42"}"#);
        session(db, "his", OTHER, group, false).await;
        session(db, "hers", 1, group, false).await;
        line(db, "his", "user", "房东又来吵", 24, None).await;
        line(db, "hers", "user", "周末爬山吗", 23, None).await;
        line(db, "his", "assistant", "别气", 22, None).await;
        line(db, "hers", "assistant", "去", 21, None).await;
        asks(db, OTHER, "不要记和房东吵架的事").await;
        let there = Audience::group("qq:42", OTHER);
        assert_eq!(
            marking(db, &there, Some(r#"{"drop":[0]}"#)).await,
            (1, true)
        );
        let said = chat_days::said_in(db, "qq:42").await;
        assert_eq!(said.len(), 2, "{said:?}");
        assert!(said[0].contains("周末爬山吗") && said[1] == "去");
        schema.drop().await;
    }

    /// A stretch still to be judged is not looked back to, in private or in
    /// the group it was said in, nor her answer to it; what was said before
    /// or after it, and by others there, is. Judged, only what was of it is
    /// left out, and one tried again goes on where it stopped.
    #[tokio::test]
    async fn a_stretch_not_judged_yet_is_not_looked_back_to() {
        let Some(schema) = schema("asked_lines_waiting").await else {
            return;
        };
        let db = &schema.db;
        asks(db, OTHER, "不要记和房东吵架的事").await;
        session(db, "mine", OTHER, Some(r#"{"mode":"chat"}"#), false).await;
        let group = Some(r#"{"mode":"chat","venue":"qq:42"}"#);
        session(db, "his", OTHER, group, false).await;
        session(db, "hers", 1, group, false).await;
        line(db, "mine", "user", "上个月的事", 90, None).await;
        line(db, "mine", "user", "房东又来吵", 24, None).await;
        line(db, "mine", "assistant", "别气", 23, None).await;
        line(db, "mine", "user", "今天下雨", 22, None).await;
        line(db, "his", "user", "群里也说房东", 24, None).await;
        line(db, "his", "assistant", "嗯", 23, None).await;
        line(db, "hers", "user", "周末爬山吗", 23, None).await;
        let now = crate::clock::now().fixed_offset();
        let (from, last) = (
            now - chrono::Duration::minutes(25),
            now - chrono::Duration::minutes(21),
        );
        for (id, present) in [
            ("private", Audience::private(OTHER)),
            ("group", Audience::group("qq:42", OTHER)),
        ] {
            let data = crate::services::agent::merope::memory_jobs::Payload::NotKept {
                present,
                from,
                last,
            };
            crate::services::agent::merope::store::memory_jobs::enqueue(db, id, OTHER, data, None)
                .await
                .unwrap();
        }
        let said = async || -> Vec<String> {
            private_lines(db, OTHER, 100)
                .await
                .into_iter()
                .map(|line| line.content)
                .collect()
        };
        assert_eq!(said().await, ["上个月的事"]);
        let there = chat_days::said_in(db, "qq:42").await;
        assert!(
            there.len() == 1 && there[0].contains("周末爬山吗"),
            "{there:?}"
        );
        // The judge cannot be reached: still to be judged, nothing marked.
        let mut world = g0::World::new(0.5);
        world
            .models
            .insert("not_to_keep".into(), [None, None, None].into());
        g0::enter(world);
        let alone = Audience::private(OTHER);
        let soon = Instant::now() + std::time::Duration::from_secs(60);
        assert!(
            mark(db, OTHER, &alone, from, last, soon, false)
                .await
                .is_err()
        );
        g0::leave();
        assert_eq!(said().await, ["上个月的事"]);
        // Judged and done with: what was of it is left out, the rest is back.
        assert_eq!(
            marking(db, &alone, Some(r#"{"drop":[0]}"#)).await,
            (1, true)
        );
        db.execute_unprepared("DELETE FROM runtime_registry WHERE record_id = 'private'")
            .await
            .unwrap();
        assert_eq!(said().await, ["上个月的事", "今天下雨"]);
        // Nothing of it is asked about again.
        assert_eq!(marking(db, &alone, None).await, (0, true));
        schema.drop().await;
    }

    async fn judged_as(db: &DatabaseConnection, text: &str) -> Option<bool> {
        db.query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT (metadata->>'notKept')::boolean AS judged FROM agent_messages \
             WHERE content = $1",
            [text.into()],
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "judged")
        .unwrap()
    }

    /// Their next line, however soon after, is the next stretch's: what they
    /// ask by then holds for it. One too short to be one, and her answers,
    /// are of this; a long one in a group is judged as any.
    #[tokio::test]
    async fn the_next_stretchs_line_is_not_judged_with_this_one() {
        let Some(schema) = schema("asked_lines_next").await else {
            return;
        };
        let db = &schema.db;
        asks(db, OTHER, "不要记和房东吵架的事").await;
        session(db, "mine", OTHER, Some(r#"{"mode":"chat"}"#), false).await;
        line(db, "mine", "user", "房东又来吵", 20, None).await;
        line(db, "mine", "assistant", "别气", 20, None).await;
        line(db, "mine", "user", "嗯", 20, None).await;
        line(db, "mine", "assistant", "摸摸", 19, None).await;
        line(db, "mine", "user", "明天去看牙医", 19, None).await;
        line(db, "mine", "assistant", "几点去", 19, None).await;
        let now = crate::clock::now().fixed_offset();
        let judging = async |present: &Audience, answers: &[Option<&str>], gives_up, late| {
            let mut world = g0::World::new(0.5);
            world
                .models
                .entry("not_to_keep".into())
                .or_default()
                .extend(answers.iter().map(|answer| answer.map(str::to_string)));
            if late {
                // Longer than it may take, however long that is.
                world
                    .delays
                    .insert("not_to_keep".into(), [1_000.0; 3].into());
            }
            g0::enter(world);
            let within = std::time::Duration::from_secs(if late { 3 } else { 60 });
            let done = mark(
                db,
                OTHER,
                present,
                now - chrono::Duration::minutes(21),
                now - chrono::Duration::minutes(20) + chrono::Duration::seconds(5),
                Instant::now() + within,
                gives_up,
            )
            .await;
            g0::leave();
            done
        };
        let alone = Audience::private(OTHER);
        judging(&alone, &[Some(r#"{"drop":[0]}"#)], false, false)
            .await
            .unwrap();
        assert_eq!(judged_as(db, "房东又来吵").await, Some(true));
        assert_eq!(judged_as(db, "嗯").await, Some(false));
        assert_eq!(judged_as(db, "摸摸").await, Some(false));
        assert_eq!(judged_as(db, "明天去看牙医").await, None);
        assert_eq!(judged_as(db, "几点去").await, None);
        // Long, in a group.
        let group = Some(r#"{"mode":"chat","venue":"qq:42"}"#);
        session(db, "his", OTHER, group, false).await;
        let long = "房东".repeat(1_001);
        line(db, "his", "user", &long, 20, None).await;
        let there = Audience::group("qq:42", OTHER);
        // Not to be reached, the account refused, its quota gone, too many
        // asks, an answer that is not one, or none in time: not marked, and
        // judged again; however long it is tried, marked by no failure.
        for answer in [
            None,
            Some("!rejected"),
            Some("!quota"),
            Some("!rateLimited"),
        ] {
            for gives_up in [false, true] {
                assert!(
                    judging(&there, &[answer; 3], gives_up, false)
                        .await
                        .is_err()
                );
                assert_eq!(judged_as(db, &long).await, None);
            }
        }
        assert!(
            judging(&there, &[Some("嗯"); 3], false, false)
                .await
                .is_err()
        );
        for gives_up in [false, true] {
            assert!(judging(&there, &[], gives_up, true).await.is_err());
            assert_eq!(judged_as(db, &long).await, None);
        }
        let before = super::super::LET_BE.load(Ordering::Relaxed);
        judging(&there, &[Some("嗯"); 3], true, false)
            .await
            .unwrap();
        assert_eq!(judged_as(db, &long).await, None, "let be");
        assert!(super::super::LET_BE.load(Ordering::Relaxed) > before);
        schema.drop().await;
    }

    /// A line whose words are refused each time, and one whose metadata is
    /// JSON's nothing: the rest is told of and looked back to as soon as the
    /// refused one is given up on, with nothing closed; that one is not
    /// looked back to, never said to be of it, and comes back with its mark
    /// gone. The job goes with it.
    #[tokio::test]
    async fn a_line_refused_each_time_is_let_be_and_holds_up_nothing() {
        use crate::services::agent::merope::memory_jobs;
        use crate::services::agent::merope::store::memory_jobs::enqueue;
        let Some(schema) = schema("asked_lines_refused").await else {
            return;
        };
        let db = &schema.db;
        session(db, "mine", OTHER, Some(r#"{"mode":"chat"}"#), false).await;
        line(db, "mine", "user", "今天下雨了", 24, None).await;
        line(db, "mine", "user", "不能说的那句", 23, None).await;
        line(db, "mine", "user", "房东又来吵", 22, None).await;
        line(db, "mine", "user", "周末去爬香山", 2, None).await;
        db.execute_unprepared(
            "UPDATE agent_messages SET metadata = 'null'::json WHERE content <> '今天下雨了'",
        )
        .await
        .unwrap();
        asks(db, OTHER, "不要记和房东吵架的事").await;
        let now = crate::clock::now().fixed_offset();
        let data = memory_jobs::Payload::NotKept {
            present: Audience::private(OTHER),
            from: now - chrono::Duration::minutes(25),
            last: now - chrono::Duration::minutes(21),
        };
        enqueue(db, "judging", OTHER, data, None).await.unwrap();
        let said = async || -> Vec<String> {
            private_lines(db, OTHER, 100)
                .await
                .into_iter()
                .map(|line| line.content)
                .collect()
        };
        let tick = async |answers: &[&str]| {
            db.execute_unprepared(
                "UPDATE runtime_registry SET payload = \
                   jsonb_set(jsonb_set(payload, '{ready}', '0'), '{lease}', '0')",
            )
            .await
            .unwrap();
            let mut world = g0::World::new(0.5);
            world.models.insert(
                "not_to_keep".into(),
                answers.iter().map(|one| Some(one.to_string())).collect(),
            );
            g0::enter(world);
            memory_jobs::tick(db.clone()).await;
            g0::leave();
        };
        let waits = async || !waiting(db, Some(OTHER), None).await.unwrap().is_empty();
        // All together refused, then each on its own.
        tick(&[
            "!refused",
            r#"{"drop":[]}"#,
            "!refused",
            r#"{"drop":[0]}"#,
            r#"{"drop":[0]}"#,
        ])
        .await;
        assert!(waits().await);
        assert_eq!(said().await, ["周末去爬香山"]);
        assert_eq!(judged_as(db, "今天下雨了").await, Some(false));
        assert_eq!(judged_as(db, "房东又来吵").await, Some(true));
        assert_eq!(judged_as(db, "不能说的那句").await, None);
        // Away then, for days: not given up on.
        let since = now.timestamp() - memory_jobs::GIVES_UP_AFTER_SECS;
        db.execute_unprepared(&format!(
            "UPDATE runtime_registry SET payload = jsonb_set(payload, '{{untold_since}}', '{since}')"
        ))
        .await
        .unwrap();
        tick(&["!rejected"]).await;
        assert!(waits().await);
        tick(&["!refused"]).await;
        assert!(!waits().await);
        assert_eq!(said().await, ["今天下雨了", "周末去爬香山"]);
        assert_eq!(judged_as(db, "不能说的那句").await, None);
        db.execute_unprepared(
            "UPDATE agent_messages SET metadata = (metadata::jsonb - 'notTold')::json \
             WHERE content = '不能说的那句'",
        )
        .await
        .unwrap();
        assert_eq!(said().await, ["今天下雨了", "不能说的那句", "周末去爬香山"]);
        schema.drop().await;
    }

    /// A stretch to be judged that cannot be read holds back no other.
    #[tokio::test]
    async fn a_stretch_that_cannot_be_read_holds_back_no_other() {
        use crate::services::agent::merope::memory_jobs::Payload;
        use crate::services::agent::merope::store::memory_jobs::enqueue;
        let Some(schema) = schema("asked_lines_unreadable").await else {
            return;
        };
        let db = &schema.db;
        session(db, "mine", OTHER, Some(r#"{"mode":"chat"}"#), false).await;
        line(db, "mine", "user", "上个月的事", 90, None).await;
        line(db, "mine", "user", "房东又来吵", 24, None).await;
        let now = crate::clock::now().fixed_offset();
        for id in ["good", "odd"] {
            let data = Payload::NotKept {
                present: Audience::private(OTHER),
                from: now - chrono::Duration::minutes(25),
                last: now - chrono::Duration::minutes(21),
            };
            enqueue(db, id, OTHER, data, None).await.unwrap();
        }
        db.execute_unprepared(
            "UPDATE runtime_registry SET payload = \
               jsonb_set(payload, '{data,NotKept,from}', '\"soon\"') WHERE record_id = 'odd'",
        )
        .await
        .unwrap();
        assert_eq!(waiting(db, Some(OTHER), None).await.unwrap().len(), 1);
        let said: Vec<String> = private_lines(db, OTHER, 100)
            .await
            .into_iter()
            .map(|line| line.content)
            .collect();
        assert_eq!(said, ["上个月的事"]);
        schema.drop().await;
    }

    /// What one of them took back of what they said in a group (「当我没说」)
    /// is not turned back to there, nor her answer to it; what another said
    /// in the same words is.
    #[tokio::test]
    async fn a_groups_days_leave_out_what_one_of_them_took_back() {
        use crate::services::agent::memory::unified;
        let Some(schema) = schema("asked_lines_group_taken").await else {
            return;
        };
        let db = &schema.db;
        let group = Some(r#"{"mode":"chat","venue":"qq:42"}"#);
        session(db, "his", OTHER, group, false).await;
        session(db, "hers", 1, group, false).await;
        line(db, "his", "user", "我下周去面试新工作", 24, None).await;
        line(db, "his", "assistant", "加油", 23, None).await;
        line(db, "hers", "user", "我下周去面试新工作", 22, None).await;
        let taken = unified::remember(
            db,
            unified::NewMemory {
                user_id: OTHER,
                kind: unified::MemoryKind::Fact,
                content: "下周面试".into(),
                evidence: Some("下周去面试新工作".into()),
                speaker: unified::Speaker::User,
                source: "chat",
                audience: Audience::group("qq:42", OTHER),
                importance: 0.5,
                concepts: Vec::new(),
            },
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(chat_days::said_in(db, "qq:42").await.len(), 3);
        db.execute_unprepared(&format!(
            "UPDATE agent_memories SET invalid_at = now(), invalid_reason = 'superseded' \
             WHERE id = '{taken}'"
        ))
        .await
        .unwrap();
        let said = chat_days::said_in(db, "qq:42").await;
        assert_eq!(said.len(), 1, "{said:?}");
        assert!(said[0].contains("下周去面试新工作"), "{said:?}");
        schema.drop().await;
    }
}
