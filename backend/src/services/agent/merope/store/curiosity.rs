//! Durable daily lookup allowance. The site and person limits and query
//! deduplication are committed together, before any outbound search.
use std::collections::BTreeMap;

use chrono::{NaiveDate, Utc};
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement, TransactionTrait,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::services::runtime_registry::{self as registry, RegistryIdentity};

const NAMESPACE: &str = "merope_curiosity_budget";
const PER_PERSON_PER_DAY: usize = 3;
const PER_SITE_PER_DAY: usize = 40;
/// Words and memes she looks up from group talk are the site's, not any
/// one person's: their own share, still within the site's.
pub(in crate::services::agent::merope) const WORDS: i32 = 0;
const WORDS_PER_DAY: usize = 20;

#[derive(Default, Serialize, Deserialize)]
struct Budget {
    // Store hashes, not conversation-derived search text. At most 40 entries/day.
    asked: BTreeMap<i32, Vec<String>>,
}

impl Budget {
    fn available(&self, user_id: i32) -> bool {
        let own = if user_id == WORDS {
            WORDS_PER_DAY
        } else {
            PER_PERSON_PER_DAY
        };
        self.asked.values().map(Vec::len).sum::<usize>() < PER_SITE_PER_DAY
            && self.asked.get(&user_id).map_or(0, Vec::len) < own
    }

    fn claim(&mut self, user_id: i32, query: &str) -> bool {
        if query.trim().is_empty() || !self.available(user_id) {
            return false;
        }
        let key = hex::encode(Sha256::digest(query.trim().to_lowercase().as_bytes()));
        let asked = self.asked.entry(user_id).or_default();
        if asked.contains(&key) {
            return false;
        }
        asked.push(key);
        true
    }
}

/// An optimization before asking what to search. Only `claim` authorizes a lookup.
pub(in crate::services::agent::merope) async fn available(
    db: &DatabaseConnection,
    user_id: i32,
    day: NaiveDate,
) -> Result<bool, DbErr> {
    Ok(registry::get::<Budget>(db, NAMESPACE, &day.to_string())
        .await?
        .unwrap_or_default()
        .available(user_id))
}

pub(in crate::services::agent::merope) async fn claim(
    db: &DatabaseConnection,
    user_id: i32,
    query: &str,
    day: NaiveDate,
) -> Result<bool, DbErr> {
    let txn = db.begin().await?;
    let key = day.to_string();
    // Also serializes the first claim, when no row exists to SELECT FOR UPDATE.
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        [format!("{NAMESPACE}:{key}").into()],
    ))
    .await?;
    let mut budget: Budget = registry::get(&txn, NAMESPACE, &key)
        .await?
        .unwrap_or_default();
    if !budget.claim(user_id, query) {
        txn.rollback().await?;
        return Ok(false);
    }
    registry::put(
        &txn,
        NAMESPACE,
        &key,
        RegistryIdentity {
            subject_id: None,
            owner_id: None,
            tapp_id: None,
            runtime_id: None,
        },
        &budget,
        (Utc::now() + chrono::Duration::days(3)).timestamp(),
    )
    .await?;
    txn.commit().await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn claims_enforce_both_limits_and_normalized_duplicates() {
        let mut budget = Budget::default();
        assert!(!budget.claim(1, " "));
        assert!(budget.claim(1, " Tame Impala "));
        assert!(!budget.claim(1, "tame impala"));
        assert!(budget.claim(1, "second"));
        assert!(budget.claim(1, "third"));
        assert!(!budget.claim(1, "fourth"));
        for user in 2..=38 {
            assert!(budget.claim(user, "one"));
        }
        assert!(!budget.claim(39, "one"));
    }

    #[tokio::test]
    #[ignore = "requires MYRIAD_MEROPE_TEST_DATABASE_URL pointing to myriad_merope_test"]
    async fn concurrent_claims_survive_reconnect_and_roll_over_by_day() {
        let url = std::env::var("MYRIAD_MEROPE_TEST_DATABASE_URL").expect("explicit disposable DB");
        let db = sea_orm::Database::connect(url.clone()).await.unwrap();
        let name: String = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT current_database() AS name",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "name")
            .unwrap();
        assert_eq!(name, "myriad_merope_test");
        db.execute_unprepared("CREATE TABLE IF NOT EXISTS runtime_registry (namespace TEXT NOT NULL, record_id TEXT NOT NULL, subject_id INTEGER, owner_id INTEGER, tapp_id TEXT, runtime_id TEXT, payload JSONB NOT NULL, expires_at BIGINT NOT NULL, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(), PRIMARY KEY(namespace,record_id))").await.unwrap();
        db.execute_unprepared("CREATE TABLE IF NOT EXISTS runtime_mailbox (message_id BIGSERIAL PRIMARY KEY, channel TEXT NOT NULL, runtime_id TEXT NOT NULL, payload JSONB NOT NULL, expires_at BIGINT NOT NULL)").await.unwrap();
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM runtime_registry WHERE namespace = $1",
            [NAMESPACE.into()],
        ))
        .await
        .unwrap();
        let day = chrono::Local::now().date_naive();
        let other = sea_orm::Database::connect(url.clone()).await.unwrap();
        let claims = (0..20).map(|i| {
            let db = if i % 2 == 0 { &db } else { &other };
            async move { claim(db, 1, &format!("query {i}"), day).await.unwrap() }
        });
        assert_eq!(
            futures::future::join_all(claims)
                .await
                .into_iter()
                .filter(|ok| *ok)
                .count(),
            3
        );
        // A fresh connection has no process-local budget to inherit.
        db.close().await.unwrap();
        other.close().await.unwrap();
        let result = tokio::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "services::agent::merope::store::curiosity::tests::restarted_process_observes_exhausted_allowance", "--ignored"])
            .env("MYRIAD_CURIOSITY_PROBE_DAY", day.to_string())
            .output().await.unwrap();
        assert!(
            result.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&result.stdout),
            String::from_utf8_lossy(&result.stderr)
        );
        let db = sea_orm::Database::connect(url).await.unwrap();
        assert!(!available(&db, 1, day).await.unwrap());
        assert!(!claim(&db, 1, "after restart", day).await.unwrap());
        let duplicates = (0..10).map(|_| claim(&db, 2, " SAME query ", day));
        assert_eq!(
            futures::future::join_all(duplicates)
                .await
                .into_iter()
                .filter(|r| *r.as_ref().unwrap())
                .count(),
            1
        );
        assert!(!claim(&db, 2, "same QUERY", day).await.unwrap());
        let claims = (3..90).map(|user| claim(&db, user, "one", day));
        assert_eq!(
            futures::future::join_all(claims)
                .await
                .into_iter()
                .filter(|r| *r.as_ref().unwrap())
                .count(),
            36
        );
        assert!(!claim(&db, 100, "site full", day).await.unwrap());
        assert!(
            claim(&db, 1, "after restart", day.succ_opt().unwrap())
                .await
                .unwrap()
        );
    }
    #[tokio::test]
    #[ignore = "subprocess fixture for the durable curiosity allowance test"]
    async fn restarted_process_observes_exhausted_allowance() {
        let day: NaiveDate = std::env::var("MYRIAD_CURIOSITY_PROBE_DAY")
            .expect("parent test required")
            .parse()
            .unwrap();
        let url = std::env::var("MYRIAD_MEROPE_TEST_DATABASE_URL").expect("explicit disposable DB");
        let db = sea_orm::Database::connect(url).await.unwrap();
        let name: String = db
            .query_one_raw(Statement::from_string(
                DatabaseBackend::Postgres,
                "SELECT current_database() AS name",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "name")
            .unwrap();
        assert_eq!(name, "myriad_merope_test");
        assert!(!available(&db, 1, day).await.unwrap());
        assert!(!claim(&db, 1, "fresh process", day).await.unwrap());
    }
}
