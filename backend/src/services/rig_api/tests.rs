//! Tenants, keys and quotas against a database: keys that are taken and
//! keys that are not (malformed, unknown, revoked, expired, a disabled
//! tenant's), limits and the site's defaults, and the quotas held under
//! requests and admissions made at once, from one pool and from two (two
//! processes), admissions that do not commit, lapsed leases, and a key
//! hashed under another data key.

use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection, TransactionTrait};

use super::Period;
use super::admission::{Admission, AdmitError, Refusal, admit_job_in, refund_job_in};
use super::keys::{self, HashKey, Refused};
use super::quota::{self, Clock, Limits, RateCount};
use super::tenants::{self, Overrides, TenantError};

async fn schema(prefix: &str) -> Option<(crate::db::IsolatedSchema, String)> {
    let url = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"])?;
    let schema = crate::db::IsolatedSchema::migrated(&url, prefix).await;
    schema
        .db
        .execute_unprepared("INSERT INTO users (id, username) VALUES (7, 'owner'), (8, 'other')")
        .await
        .unwrap();
    Some((schema, url))
}

/// Another pool on the same schema: what a second backend process holds.
async fn second_process(url: &str, schema: &crate::db::IsolatedSchema) -> DatabaseConnection {
    crate::db::IsolatedSchema::existing(url, schema.name())
        .await
        .db
}

fn at(hour: u32, minute: u32, second: u32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(2026, 11, 28, hour, minute, second)
        .unwrap()
}

fn own(concurrent: i32, per_period: i32, per_minute: i32) -> Overrides {
    Overrides {
        concurrent_jobs: Some(concurrent),
        jobs_per_period: Some(per_period),
        job_period: Some(Period::Day),
        requests_per_minute: Some(per_minute),
    }
}

/// A job of `caller`'s in `rig_jobs`, its lease just renewed.
async fn insert_job<C: ConnectionTrait>(db: &C, id: &str, caller: &str, state: &str) {
    db.execute_unprepared(&format!(
        "INSERT INTO rig_jobs (id, kind, caller, input, state, ledger_task_id, heartbeat_at) \
         VALUES ('{id}', 'bust', '{caller}', '{{}}', '{state}', 'rig-job:{id}', NOW())"
    ))
    .await
    .unwrap();
}

async fn end_job(db: &DatabaseConnection, id: &str) {
    db.execute_unprepared(&format!(
        "UPDATE rig_jobs SET state = 'succeeded', ended_at = NOW() WHERE id = '{id}'"
    ))
    .await
    .unwrap();
}

/// What the job route does: admit and insert the job in one transaction.
async fn start_job(
    db: &DatabaseConnection,
    tenant_id: &str,
    job_id: &str,
    clock: Clock,
) -> Result<Admission, AdmitError> {
    let txn = db.begin().await?;
    let admission = admit_job_in(&txn, tenant_id, clock).await?;
    insert_job(&txn, job_id, &admission.caller, "running").await;
    txn.commit().await?;
    Ok(admission)
}

#[tokio::test]
async fn a_key_is_shown_once_kept_as_a_keyed_hash_and_taken_until_revoked() {
    let Some((schema, _)) = schema("rig_api_keys").await else {
        return;
    };
    let db = &schema.db;
    let now = at(10, 0, 0);
    let tenant = tenants::create(db, 7, "Studio", Overrides::default(), now)
        .await
        .unwrap();
    let (key, secret) = keys::issue(db, &tenant.id, "ci", None, Some(7), now)
        .await
        .unwrap();
    assert!(secret.starts_with(&key.prefix));
    assert_eq!(key.created_by_user_id, Some(7));

    let stored = db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT prefix, secret_hash FROM rig_api_keys",
        ))
        .await
        .unwrap()
        .unwrap();
    let hash: String = stored.try_get("", "secret_hash").unwrap();
    assert_eq!(hash.len(), 64);
    assert!(!hash.contains(&secret[key.prefix.len() + 1..]));
    assert_ne!(hash, secret);

    let found = keys::authenticate(db, &secret, now).await.unwrap();
    assert_eq!(found.tenant.id, tenant.id);
    assert_eq!(found.key.id, key.id);

    // Listed with its prefix, never its secret.
    let listed = keys::list(db, &tenant.id).await.unwrap();
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].prefix, key.prefix);

    let revoked = keys::revoke(db, &tenant.id, &key.id, now).await.unwrap();
    assert_eq!(revoked.unwrap().revoked_at, Some(now));
    assert!(matches!(
        keys::authenticate(db, &secret, now).await,
        Err(Refused::Revoked)
    ));
    // Revoking again keeps when it was revoked.
    let again = keys::revoke(db, &tenant.id, &key.id, now + TimeDelta::hours(1))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(again.revoked_at, Some(now));
    // Another tenant's key id is not this tenant's to revoke.
    let other = tenants::create(db, 8, "Other", Overrides::default(), now)
        .await
        .unwrap();
    assert!(
        keys::revoke(db, &other.id, &key.id, now)
            .await
            .unwrap()
            .is_none()
    );
    schema.drop().await;
}

#[tokio::test]
async fn malformed_unknown_expired_and_disabled_keys_are_refused() {
    let Some((schema, _)) = schema("rig_api_refused").await else {
        return;
    };
    let db = &schema.db;
    let now = at(10, 0, 0);
    let tenant = tenants::create(db, 7, "Studio", Overrides::default(), now)
        .await
        .unwrap();
    let (_, secret) = keys::issue(db, &tenant.id, "", None, None, now)
        .await
        .unwrap();

    for malformed in ["", "Bearer", "mrk_short", "eyJhbGciOiJIUzI1NiJ9.e30.c2ln"] {
        assert!(
            matches!(
                keys::authenticate(db, malformed, now).await,
                Err(Refused::Malformed)
            ),
            "{malformed:?}"
        );
    }
    // The right prefix with another secret is no key, and says no more.
    let mut forged = secret.clone();
    let last = forged.pop().unwrap();
    forged.push(if last == 'a' { 'b' } else { 'a' });
    assert!(matches!(
        keys::authenticate(db, &forged, now).await,
        Err(Refused::Unknown)
    ));
    // A well-formed key nobody issued.
    let unknown = format!("mrk_{}_{}", "A".repeat(12), "b".repeat(43));
    assert!(matches!(
        keys::authenticate(db, &unknown, now).await,
        Err(Refused::Unknown)
    ));

    // A key expiring is taken until then, and not at or after it.
    let expires = now + TimeDelta::minutes(30);
    let (_, expiring) = keys::issue(db, &tenant.id, "", Some(expires), None, now)
        .await
        .unwrap();
    assert!(
        keys::authenticate(db, &expiring, expires - TimeDelta::seconds(1))
            .await
            .is_ok()
    );
    assert!(matches!(
        keys::authenticate(db, &expiring, expires).await,
        Err(Refused::Expired)
    ));
    // An expiry not after now is not issued.
    assert!(matches!(
        keys::issue(db, &tenant.id, "", Some(now), None, now).await,
        Err(keys::IssueError::ExpiryPast)
    ));

    // A disabled tenant's good key is refused, and taken again once enabled.
    tenants::update(db, &tenant.id, None, Some(true), now)
        .await
        .unwrap();
    assert!(matches!(
        keys::authenticate(db, &secret, now).await,
        Err(Refused::TenantDisabled)
    ));
    tenants::update(db, &tenant.id, None, Some(false), now)
        .await
        .unwrap();
    assert!(keys::authenticate(db, &secret, now).await.is_ok());
    schema.drop().await;
}

#[tokio::test]
async fn last_use_is_recorded_at_most_once_a_minute() {
    let Some((schema, _)) = schema("rig_api_touch").await else {
        return;
    };
    let db = &schema.db;
    let now = at(10, 0, 0);
    let tenant = tenants::create(db, 7, "Studio", Overrides::default(), now)
        .await
        .unwrap();
    let (key, _) = keys::issue(db, &tenant.id, "", None, None, now)
        .await
        .unwrap();
    let last_used = || async { keys::list(db, &tenant.id).await.unwrap()[0].last_used_at };
    keys::touch(db, &key.id, now).await.unwrap();
    assert_eq!(last_used().await, Some(now));
    keys::touch(db, &key.id, now + TimeDelta::seconds(30))
        .await
        .unwrap();
    assert_eq!(last_used().await, Some(now));
    keys::touch(db, &key.id, now + TimeDelta::seconds(61))
        .await
        .unwrap();
    assert_eq!(last_used().await, Some(now + TimeDelta::seconds(61)));
    schema.drop().await;
}

#[tokio::test]
async fn tenants_need_an_owner_a_name_and_limits_in_range() {
    let Some((schema, _)) = schema("rig_api_tenants").await else {
        return;
    };
    let db = &schema.db;
    let now = at(10, 0, 0);
    assert!(matches!(
        tenants::create(db, 99, "Studio", Overrides::default(), now).await,
        Err(TenantError::OwnerMissing)
    ));
    assert!(matches!(
        tenants::create(db, 7, "  ", Overrides::default(), now).await,
        Err(TenantError::InvalidName)
    ));
    let negative = Overrides {
        requests_per_minute: Some(-1),
        ..Overrides::default()
    };
    assert!(matches!(
        tenants::create(db, 7, "Studio", negative, now).await,
        Err(TenantError::InvalidLimit)
    ));
    let tenant = tenants::create(db, 7, " Studio ", own(1, 2, 3), now)
        .await
        .unwrap();
    assert_eq!(tenant.name, "Studio");
    assert_eq!(tenant.overrides, own(1, 2, 3));
    let reset = tenants::set_overrides(db, &tenant.id, Overrides::default(), now)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(reset.overrides, Overrides::default());

    // Deleting the owner's account takes the tenant and its keys with it.
    keys::issue(db, &tenant.id, "", None, Some(7), now)
        .await
        .unwrap();
    db.execute_unprepared("DELETE FROM users WHERE id = 7")
        .await
        .unwrap();
    assert!(tenants::get(db, &tenant.id).await.unwrap().is_none());
    assert!(keys::list(db, &tenant.id).await.unwrap().is_empty());
    schema.drop().await;
}

#[tokio::test]
async fn a_tenant_without_limits_of_its_own_has_the_sites_as_stored_now() {
    let Some((schema, _)) = schema("rig_api_defaults").await else {
        return;
    };
    let db = &schema.db;
    let now = at(10, 0, 0);
    let tenant = tenants::create(
        db,
        7,
        "Studio",
        Overrides {
            concurrent_jobs: Some(5),
            ..Overrides::default()
        },
        now,
    )
    .await
    .unwrap();
    let builtin = Limits::from_config(&crate::config::DynamicConfig::default());
    assert_eq!(quota::site_defaults(db).await.unwrap(), builtin);

    db.execute_unprepared(
        "INSERT INTO configurations (key, value) VALUES \
         ('rig_api_jobs_per_period', '7'), ('rig_api_job_period', '\"day\"'), \
         ('rig_api_requests_per_minute', '9') \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    )
    .await
    .unwrap();
    let limits = quota::limits_for(db, tenant.overrides).await.unwrap();
    assert_eq!(
        limits,
        Limits {
            concurrent_jobs: 5,
            jobs_per_period: 7,
            job_period: Period::Day,
            requests_per_minute: 9,
        }
    );
    schema.drop().await;
}

#[tokio::test]
async fn a_key_hashed_under_another_data_key_is_refused_as_unknown() {
    let Some((schema, _)) = schema("rig_api_rehash").await else {
        return;
    };
    let db = &schema.db;
    let now = at(10, 0, 0);
    let tenant = tenants::create(db, 7, "Studio", Overrides::default(), now)
        .await
        .unwrap();
    let before = HashKey::derive(&myriad_data_key::DataKey::from_key_bytes([1; 32]));
    let after = HashKey::derive(&myriad_data_key::DataKey::from_key_bytes([2; 32]));
    let (_, secret) = keys::issue_under(db, &before, &tenant.id, "", None, None, now)
        .await
        .unwrap();
    assert!(
        keys::authenticate_under(db, &before, &secret, now)
            .await
            .is_ok()
    );
    assert!(matches!(
        keys::authenticate_under(db, &after, &secret, now).await,
        Err(Refused::HashKeyChanged)
    ));
    let stored: String = db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT hash_key_id FROM rig_api_keys",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "hash_key_id")
        .unwrap();
    assert_eq!(stored, before.id());
    schema.drop().await;
}

#[tokio::test]
async fn requests_over_the_minutes_limit_are_refused_until_the_next_minute() {
    let Some((schema, _)) = schema("rig_api_rate").await else {
        return;
    };
    let db = &schema.db;
    let now = at(10, 0, 50);
    let tenant = tenants::create(db, 7, "Studio", own(1, 1, 2), now)
        .await
        .unwrap();
    let clock = Clock::At(now);
    for used in 1..=2 {
        assert!(matches!(
            quota::count_request(db, &tenant.id, 2, clock).await.unwrap(),
            RateCount::Counted { used: u, retry_after_secs: 10, .. } if u == used
        ));
    }
    assert_eq!(
        quota::count_request(db, &tenant.id, 2, clock)
            .await
            .unwrap(),
        RateCount::Limited {
            limit: 2,
            resets_at: Some(at(10, 1, 0)),
            retry_after_secs: Some(10),
        }
    );
    // The next minute starts again from one, and the minute before goes.
    assert!(matches!(
        quota::count_request(db, &tenant.id, 2, Clock::At(at(10, 1, 0)))
            .await
            .unwrap(),
        RateCount::Counted { used: 1, .. }
    ));
    let windows = db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT COUNT(*)::BIGINT AS n FROM rig_api_request_windows",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<i64>("", "n")
        .unwrap();
    assert_eq!(windows, 1);
    // A limit of none refuses everything, with no time to come back at.
    assert_eq!(
        quota::count_request(db, &tenant.id, 0, clock)
            .await
            .unwrap(),
        RateCount::Limited {
            limit: 0,
            resets_at: None,
            retry_after_secs: None,
        }
    );
    // Without a clock of its own the minute is the database's.
    assert!(matches!(
        quota::count_request(db, &tenant.id, 1000, Clock::Database)
            .await
            .unwrap(),
        RateCount::Counted {
            retry_after_secs: 1..=60,
            ..
        }
    ));
    schema.drop().await;
}

#[tokio::test]
async fn requests_made_at_once_from_two_processes_never_pass_the_limit() {
    let Some((schema, url)) = schema("rig_api_rate_race").await else {
        return;
    };
    let other = second_process(&url, &schema).await;
    let now = at(10, 0, 0);
    let tenant = tenants::create(&schema.db, 7, "Studio", own(1, 1, 10), now)
        .await
        .unwrap();
    let requests = (0..40).map(|n| {
        let db = if n % 2 == 0 {
            schema.db.clone()
        } else {
            other.clone()
        };
        let id = tenant.id.clone();
        tokio::spawn(async move {
            quota::count_request(&db, &id, 10, Clock::At(now))
                .await
                .unwrap()
        })
    });
    let counted = futures::future::join_all(requests)
        .await
        .into_iter()
        .map(Result::unwrap)
        .filter(|count| matches!(count, RateCount::Counted { .. }))
        .count();
    assert_eq!(counted, 10);
    other.close().await.unwrap();
    schema.drop().await;
}

#[tokio::test]
async fn a_job_is_admitted_with_its_insert_and_ending_frees_its_place() {
    let Some((schema, _)) = schema("rig_api_admit").await else {
        return;
    };
    let db = &schema.db;
    let now = at(10, 0, 0);
    let clock = Clock::At(now);
    let tenant = tenants::create(db, 7, "Studio", own(1, 2, 100), now)
        .await
        .unwrap();
    let limits = quota::limits_for(db, tenant.overrides).await.unwrap();

    let first = start_job(db, &tenant.id, "job-a", clock).await.unwrap();
    assert_eq!(first.caller, quota::tenant_caller(&tenant.id));
    assert_eq!(first.period_start, at(0, 0, 0));
    // Another tenant's running job is not this tenant's.
    insert_job(db, "job-x", "tenant:ten_other", "running").await;
    // One place: a second job waits for it.
    assert!(matches!(
        start_job(db, &tenant.id, "job-b", clock).await,
        Err(AdmitError::Refused(Refusal::Concurrency {
            limit: 1,
            running: 1
        }))
    ));
    let usage = quota::usage(db, &tenant, &limits, clock).await.unwrap();
    assert_eq!((usage.concurrent_jobs, usage.jobs_this_period), (1, 1));
    end_job(db, "job-a").await;
    let usage = quota::usage(db, &tenant, &limits, clock).await.unwrap();
    assert_eq!((usage.concurrent_jobs, usage.jobs_this_period), (0, 1));

    start_job(db, &tenant.id, "job-b", clock).await.unwrap();
    end_job(db, "job-b").await;
    // Two jobs started today: the third waits for tomorrow.
    match start_job(db, &tenant.id, "job-c", clock).await {
        Err(AdmitError::Refused(Refusal::Period {
            limit,
            used,
            resets_at,
            retry_after_secs,
        })) => {
            assert_eq!((limit, used), (2, 2));
            assert_eq!(resets_at, Some(at(0, 0, 0) + TimeDelta::days(1)));
            assert_eq!(retry_after_secs, Some(14 * 3600));
        }
        other => panic!("third job of the day: {other:?}"),
    }
    let tomorrow = Clock::At(now + TimeDelta::days(1));
    assert!(start_job(db, &tenant.id, "job-c", tomorrow).await.is_ok());

    // A disabled tenant starts nothing; a missing one neither.
    tenants::update(db, &tenant.id, None, Some(true), now)
        .await
        .unwrap();
    assert!(matches!(
        start_job(db, &tenant.id, "job-d", tomorrow).await,
        Err(AdmitError::Refused(Refusal::TenantDisabled))
    ));
    assert!(matches!(
        start_job(db, "ten_missing", "job-e", clock).await,
        Err(AdmitError::Refused(Refusal::TenantMissing))
    ));
    schema.drop().await;
}

#[tokio::test]
async fn an_admission_whose_transaction_does_not_commit_leaves_nothing_behind() {
    let Some((schema, _)) = schema("rig_api_rollback").await else {
        return;
    };
    let db = &schema.db;
    let now = at(10, 0, 0);
    let clock = Clock::At(now);
    let tenant = tenants::create(db, 7, "Studio", own(1, 1, 100), now)
        .await
        .unwrap();
    // The claim after the admission fails: the transaction rolls back.
    let txn = db.begin().await.unwrap();
    admit_job_in(&txn, &tenant.id, clock).await.unwrap();
    txn.rollback().await.unwrap();
    // A process dying between the two is the same: its transaction never commits.
    let txn = db.begin().await.unwrap();
    admit_job_in(&txn, &tenant.id, clock).await.unwrap();
    drop(txn);
    let limits = quota::limits_for(db, tenant.overrides).await.unwrap();
    let usage = quota::usage(db, &tenant, &limits, clock).await.unwrap();
    assert_eq!((usage.concurrent_jobs, usage.jobs_this_period), (0, 0));
    assert!(start_job(db, &tenant.id, "job-a", clock).await.is_ok());
    schema.drop().await;
}

#[tokio::test]
async fn a_refunded_job_no_longer_counts_against_its_period() {
    let Some((schema, _)) = schema("rig_api_refund").await else {
        return;
    };
    let db = &schema.db;
    let now = at(10, 0, 0);
    let clock = Clock::At(now);
    let tenant = tenants::create(db, 7, "Studio", own(10, 1, 100), now)
        .await
        .unwrap();
    let admission = start_job(db, &tenant.id, "job-a", clock).await.unwrap();
    assert_eq!(admission.counted_on, now.date_naive());
    end_job(db, "job-a").await;
    assert!(matches!(
        start_job(db, &tenant.id, "job-b", clock).await,
        Err(AdmitError::Refused(Refusal::Period { used: 1, .. }))
    ));
    assert!(
        refund_job_in(db, &tenant.id, admission.counted_on)
            .await
            .unwrap()
    );
    assert!(
        !refund_job_in(db, &tenant.id, admission.counted_on)
            .await
            .unwrap()
    );
    assert!(start_job(db, &tenant.id, "job-b", clock).await.is_ok());
    schema.drop().await;
}

#[tokio::test]
async fn a_job_whose_lease_lapsed_gives_its_place_to_the_next_admission() {
    let Some((schema, _)) = schema("rig_api_lapse").await else {
        return;
    };
    let db = &schema.db;
    let now = at(10, 0, 0);
    let clock = Clock::At(now);
    let tenant = tenants::create(db, 7, "Studio", own(1, 10, 100), now)
        .await
        .unwrap();
    start_job(db, &tenant.id, "job-a", clock).await.unwrap();
    assert!(matches!(
        start_job(db, &tenant.id, "job-b", clock).await,
        Err(AdmitError::Refused(Refusal::Concurrency { .. }))
    ));
    // Its process went away: the lease is not renewed past its length.
    db.execute_unprepared(&format!(
        "UPDATE rig_jobs SET heartbeat_at = NOW() - INTERVAL '{} seconds' WHERE id = 'job-a'",
        crate::services::rig_jobs::LEASE.as_secs() + 1
    ))
    .await
    .unwrap();
    start_job(db, &tenant.id, "job-b", clock).await.unwrap();
    let state: String = db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT state FROM rig_jobs WHERE id = 'job-a'",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "state")
        .unwrap();
    assert_eq!(state, "failed");
    schema.drop().await;
}

#[tokio::test]
async fn a_tenant_moved_from_days_to_months_counts_the_months_earlier_days() {
    let Some((schema, _)) = schema("rig_api_period").await else {
        return;
    };
    let db = &schema.db;
    let first_day = Utc.with_ymd_and_hms(2026, 11, 3, 9, 0, 0).unwrap();
    let tenant = tenants::create(db, 7, "Studio", own(100, 3, 100), first_day)
        .await
        .unwrap();
    for n in 0..2 {
        start_job(db, &tenant.id, &format!("d1-{n}"), Clock::At(first_day))
            .await
            .unwrap();
    }
    // Last month's jobs are not this month's.
    start_job(
        db,
        &tenant.id,
        "oct",
        Clock::At(first_day - TimeDelta::days(10)),
    )
    .await
    .unwrap();
    tenants::set_overrides(
        db,
        &tenant.id,
        Overrides {
            job_period: Some(Period::Month),
            ..own(100, 3, 100)
        },
        first_day,
    )
    .await
    .unwrap();
    let later = Clock::At(first_day + TimeDelta::days(5));
    start_job(db, &tenant.id, "d6", later).await.unwrap();
    assert!(matches!(
        start_job(db, &tenant.id, "d6-b", later).await,
        Err(AdmitError::Refused(Refusal::Period { used: 3, .. }))
    ));
    schema.drop().await;
}

#[tokio::test]
async fn admissions_at_once_from_two_processes_hold_both_job_limits() {
    let Some((schema, url)) = schema("rig_api_admit_race").await else {
        return;
    };
    let other = second_process(&url, &schema).await;
    let now = at(10, 0, 0);
    let concurrent = tenants::create(&schema.db, 7, "Concurrent", own(3, 1000, 1000), now)
        .await
        .unwrap();
    let per_day = tenants::create(&schema.db, 8, "Per day", own(1000, 5, 1000), now)
        .await
        .unwrap();

    let race = |tenant: String, tag: &'static str, jobs: usize| {
        let pools = [schema.db.clone(), other.clone()];
        let attempts = (0..jobs).map(move |n| {
            let db = pools[n % 2].clone();
            let tenant = tenant.clone();
            tokio::spawn(async move {
                match start_job(&db, &tenant, &format!("{tag}-{n}"), Clock::At(now)).await {
                    Ok(_) => true,
                    Err(AdmitError::Refused(_)) => false,
                    Err(AdmitError::Db(error)) => panic!("{error}"),
                }
            })
        });
        async move {
            futures::future::join_all(attempts)
                .await
                .into_iter()
                .map(Result::unwrap)
                .filter(|admitted| *admitted)
                .count()
        }
    };
    let (concurrent_admitted, per_day_admitted) = tokio::join!(
        race(concurrent.id.clone(), "c", 24),
        race(per_day.id.clone(), "p", 24)
    );
    assert_eq!(concurrent_admitted, 3);
    assert_eq!(per_day_admitted, 5);

    let limits = quota::limits_for(&schema.db, per_day.overrides)
        .await
        .unwrap();
    let usage = quota::usage(&schema.db, &per_day, &limits, Clock::At(now))
        .await
        .unwrap();
    assert_eq!(usage.jobs_this_period, 5);
    let limits = quota::limits_for(&schema.db, concurrent.overrides)
        .await
        .unwrap();
    let usage = quota::usage(&schema.db, &concurrent, &limits, Clock::At(now))
        .await
        .unwrap();
    assert_eq!(usage.concurrent_jobs, 3);
    other.close().await.unwrap();
    schema.drop().await;
}
