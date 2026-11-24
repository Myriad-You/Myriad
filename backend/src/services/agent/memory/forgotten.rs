//! What she was made to forget, kept so no copy elsewhere brings it back
//! (`agent_forgotten`): a device's her merged into the site's
//! (`persona_sync::merge`) lands only what is not here.
//!
//! - **No words.** A memory is kept by its id; a line by its id here and
//!   by a one-way digest of who said it, the microsecond it was said and what
//!   ([`line_mark`]: SHA-256, its first 128 bits), never the words nor
//!   anything they could be read back from; the moment in it keeps a short
//!   line from being found by trying words.
//! - **Not her log's forgets.** Those are not enough: with syncing off
//!   there is no log, and turned off its rows go after the retention
//!   period; a forgotten row is a stub with nothing of the line left to
//!   match a device's copy against (which comes with no row of the log);
//!   and a line the log never had leaves no forget at all.
//! - **Written where forgetting is.** In the same transaction that deletes
//!   the memory or the line: forgetting a memory (`manage`), a memory
//!   forgotten elsewhere taken in (`her_log::memories`), the words a memory
//!   came from and a day's talk (`origins`), one line (`manage`).
//! - **With the person.** Their account gone, what was forgotten of theirs
//!   goes too (foreign key).

use std::collections::HashSet;

use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement};

/// A forgotten memory, by its id.
pub const MEMORY: &str = "memory";
/// A forgotten line, by [`line_mark`].
pub const LINE: &str = "line";

/// A forgotten line, by its id here (`agent_messages.id`): what an intent
/// that comes after the forgetting may name as its grounds.
pub const MESSAGE: &str = "message";

/// A version of a span or an entry whose words went, while a later version
/// of it stands here: by its row's id, digested (a row's id may be long).
pub const PIECE: &str = "piece";

/// The mark of a line: who said it (`role`), when (`at`, micros) and what,
/// digested; the same for the site's line and a device's copy of it.
pub fn line_mark(role: &str, at_micros: i64, content: &str) -> String {
    super::meaning::digest(&format!("{role}\n{at_micros}\n{content}"))
}

/// Keeps `marks` of `what` as forgotten for `user_id`.
pub async fn keep<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    what: &str,
    marks: &[String],
) -> Result<(), DbErr> {
    if marks.is_empty() {
        return Ok(());
    }
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_forgotten (user_id, what, mark, forgotten_at) \
         SELECT $1, $2, mark, $4::timestamptz FROM unnest($3::text[]) AS mark \
         WHERE EXISTS (SELECT 1 FROM users WHERE id = $1) \
         ON CONFLICT DO NOTHING",
        [
            user_id.into(),
            what.into(),
            marks.to_vec().into(),
            crate::clock::now().into(),
        ],
    ))
    .await?;
    Ok(())
}

/// Of `marks` of `what`, those `user_id` was made to forget.
pub async fn among<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    what: &str,
    marks: &[String],
) -> Result<HashSet<String>, DbErr> {
    if marks.is_empty() {
        return Ok(HashSet::new());
    }
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT mark FROM agent_forgotten WHERE user_id = $1 AND what = $2 AND mark = ANY($3)",
            [user_id.into(), what.into(), marks.to_vec().into()],
        ))
        .await?;
    rows.iter()
        .map(|row| row.try_get::<String>("", "mark"))
        .collect()
}

#[cfg(test)]
mod tests {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

    /// A line's mark is a digest of who said it, when and what: the same
    /// line marks the same, a moment apart or a word apart it does not,
    /// and none of its words are in it.
    #[test]
    fn a_line_is_kept_by_a_digest_with_its_moment() {
        let mark = super::line_mark("user", 1_760_000_000_000_000, "外面下雨了");
        assert_eq!(
            mark,
            super::line_mark("user", 1_760_000_000_000_000, "外面下雨了")
        );
        assert_ne!(
            mark,
            super::line_mark("user", 1_760_000_000_000_001, "外面下雨了")
        );
        assert_ne!(
            mark,
            super::line_mark("assistant", 1_760_000_000_000_000, "外面下雨了")
        );
        assert_ne!(
            mark,
            super::line_mark("user", 1_760_000_000_000_000, "外面下雨")
        );
        assert_eq!(mark.len(), 32);
        assert!(mark.chars().all(|c| c.is_ascii_hexdigit()), "{mark}");
    }

    /// What was forgotten of someone goes with their account.
    #[tokio::test]
    async fn what_was_forgotten_goes_with_the_account() {
        let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
            return;
        };
        let schema = crate::db::IsolatedSchema::migrated(&url, "forgotten_account").await;
        let db = &schema.db;
        db.execute_unprepared("INSERT INTO users (id, username) VALUES (61, 'sixty-one')")
            .await
            .unwrap();
        super::keep(db, 61, super::MEMORY, &["mem_1".to_string()])
            .await
            .unwrap();
        super::keep(db, 61, super::LINE, &[super::line_mark("user", 1, "你好")])
            .await
            .unwrap();
        // Someone with no account here keeps nothing.
        super::keep(db, 62, super::MEMORY, &["mem_2".to_string()])
            .await
            .unwrap();
        let count = |sql: &'static str| async move {
            db.query_one_raw(Statement::from_string(DatabaseBackend::Postgres, sql))
                .await
                .unwrap()
                .unwrap()
                .try_get::<i64>("", "n")
                .unwrap()
        };
        assert_eq!(count("SELECT COUNT(*) AS n FROM agent_forgotten").await, 2);
        db.execute_unprepared("DELETE FROM users WHERE id = 61")
            .await
            .unwrap();
        assert_eq!(count("SELECT COUNT(*) AS n FROM agent_forgotten").await, 0);
        schema.drop().await;
    }
}
