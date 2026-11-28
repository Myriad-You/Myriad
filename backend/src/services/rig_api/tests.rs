//! Tenants, keys and quotas against a database: keys that are taken and
//! keys that are not (malformed, unknown, revoked, expired, a disabled
//! tenant's), limits and the site's defaults, and the quotas held under
//! requests and admissions made at once, from one pool and from two (two
//! processes).

use chrono::{DateTime, TimeDelta, TimeZone, Utc};
use sea_orm::{ConnectionTrait, DatabaseConnection};

use super::Period;
use super::admission::{AdmitError, Refusal, admit_job, release_job};
use super::keys::{self, Refused};
use super::quota::{self, CLAIM_GRACE, Limits, RateCount};
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

async fn insert_job(db: &DatabaseConnection, id: &str, state: &str) {
    db.execute_unprepared(&format!(
        "INSERT INTO rig_jobs (id, kind, caller, input, state, ledger_task_id) \
         VALUES ('{id}', 'bust', 'tenant:t', '{{}}', '{state}', 'rig-job:{id}')"
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
async fn requests_over_the_minutes_limit_are_refused_until_the_next_minute() {
    let Some((schema, _)) = schema("rig_api_rate").await else {
        return;
    };
    let db = &schema.db;
    let now = at(10, 0, 50);
    let tenant = tenants::create(db, 7, "Studio", own(1, 1, 2), now)
        .await
        .unwrap();
    for used in 1..=2 {
        assert!(matches!(
            quota::count_request(db, &tenant.id, 2, now).await.unwrap(),
            RateCount::Counted { used: u, .. } if u == used
        ));
    }
    match quota::count_request(db, &tenant.id, 2, now).await.unwrap() {
        RateCount::Limited {
            retry_after_secs,
            resets_at,
            ..
        } => {
            assert_eq!(retry_after_secs, 10);
            assert_eq!(resets_at, at(10, 1, 0));
        }
        counted => panic!("over the limit, counted: {counted:?}"),
    }
    // The next minute starts again from one, and the minute before goes.
    assert!(matches!(
        quota::count_request(db, &tenant.id, 2, at(10, 1, 0))
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
    // A limit of none refuses everything.
    assert!(matches!(
        quota::count_request(db, &tenant.id, 0, at(10, 1, 0))
            .await
            .unwrap(),
        RateCount::Limited { .. }
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
        tokio::spawn(async move { quota::count_request(&db, &id, 10, now).await.unwrap() })
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
async fn a_job_takes_a_place_and_a_count_and_ending_frees_the_place() {
    let Some((schema, _)) = schema("rig_api_admit").await else {
        return;
    };
    let db = &schema.db;
    let now = at(10, 0, 0);
    let tenant = tenants::create(db, 7, "Studio", own(1, 2, 100), now)
        .await
        .unwrap();

    let first = admit_job(db, &tenant.id, "job-a", now).await.unwrap();
    assert_eq!(first.period_start, at(0, 0, 0));
    // The same job admitted again takes nothing more.
    assert_eq!(
        admit_job(db, &tenant.id, "job-a", now).await.unwrap(),
        first
    );
    // One place: a second job waits for it.
    assert!(matches!(
        admit_job(db, &tenant.id, "job-b", now).await,
        Err(AdmitError::Refused(Refusal::Concurrency {
            limit: 1,
            running: 1
        }))
    ));
    // The job runs, then ends: its place is free, its count stays.
    insert_job(db, "job-a", "running").await;
    let limits = quota::limits_for(db, tenant.overrides).await.unwrap();
    let usage = quota::usage(db, &tenant, &limits, now).await.unwrap();
    assert_eq!((usage.concurrent_jobs, usage.jobs_this_period), (1, 1));
    end_job(db, "job-a").await;
    let usage = quota::usage(db, &tenant, &limits, now).await.unwrap();
    assert_eq!((usage.concurrent_jobs, usage.jobs_this_period), (0, 1));

    admit_job(db, &tenant.id, "job-b", now).await.unwrap();
    insert_job(db, "job-b", "queued").await;
    end_job(db, "job-b").await;
    // Two jobs started today: the third waits for tomorrow.
    match admit_job(db, &tenant.id, "job-c", now).await {
        Err(AdmitError::Refused(Refusal::Period {
            limit,
            used,
            resets_at,
        })) => assert_eq!(
            (limit, used, resets_at),
            (2, 2, at(0, 0, 0) + TimeDelta::days(1))
        ),
        other => panic!("third job of the day: {other:?}"),
    }
    let tomorrow = now + TimeDelta::days(1);
    assert!(admit_job(db, &tenant.id, "job-c", tomorrow).await.is_ok());

    // A disabled tenant starts nothing; a missing one neither.
    tenants::update(db, &tenant.id, None, Some(true), now)
        .await
        .unwrap();
    assert!(matches!(
        admit_job(db, &tenant.id, "job-d", tomorrow).await,
        Err(AdmitError::Refused(Refusal::TenantDisabled))
    ));
    assert!(matches!(
        admit_job(db, "ten_missing", "job-e", now).await,
        Err(AdmitError::Refused(Refusal::TenantMissing))
    ));
    schema.drop().await;
}

#[tokio::test]
async fn a_job_never_claimed_gives_back_its_place_on_release_or_after_the_grace() {
    let Some((schema, _)) = schema("rig_api_release").await else {
        return;
    };
    let db = &schema.db;
    let now = at(10, 0, 0);
    let tenant = tenants::create(db, 7, "Studio", own(1, 5, 100), now)
        .await
        .unwrap();
    let limits = quota::limits_for(db, tenant.overrides).await.unwrap();

    let refused_claim = admit_job(db, &tenant.id, "job-a", now).await.unwrap();
    assert!(release_job(db, &refused_claim).await.unwrap());
    let usage = quota::usage(db, &tenant, &limits, now).await.unwrap();
    assert_eq!((usage.concurrent_jobs, usage.jobs_this_period), (0, 0));

    // A job that is in `rig_jobs` is not given back: it ends by itself.
    let claimed = admit_job(db, &tenant.id, "job-b", now).await.unwrap();
    insert_job(db, "job-b", "running").await;
    assert!(!release_job(db, &claimed).await.unwrap());
    end_job(db, "job-b").await;

    // A place whose job never came holds through the grace, then frees.
    admit_job(db, &tenant.id, "job-c", now).await.unwrap();
    let grace = TimeDelta::from_std(CLAIM_GRACE).unwrap();
    assert!(matches!(
        admit_job(db, &tenant.id, "job-d", now + grace - TimeDelta::seconds(1)).await,
        Err(AdmitError::Refused(Refusal::Concurrency { .. }))
    ));
    assert!(
        admit_job(db, &tenant.id, "job-d", now + grace + TimeDelta::seconds(1))
            .await
            .is_ok()
    );
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
                match admit_job(&db, &tenant, &format!("{tag}-{n}"), now).await {
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
    let usage = quota::usage(&schema.db, &per_day, &limits, now)
        .await
        .unwrap();
    assert_eq!(usage.jobs_this_period, 5);
    other.close().await.unwrap();
    schema.drop().await;
}
