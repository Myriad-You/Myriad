//! The order of the locks her moves and her parts are changed under. A
//! transaction that takes more than one of them takes them in this order;
//! taken the other way round, two can wait on each other for good, and
//! Postgres fails one of them:
//!
//! 1. the imports ([`one_at_a_time`](crate::services::persona_sync::import)),
//!    then their rows (`agent_host_import`);
//! 2. queued memory work (`merope::store::memory_jobs::lock`, which every
//!    write of her persona takes through `lock_persona_on`);
//! 3. her lease (`her_log::lease::lock`);
//! 4. the other rows: her copy, her moves, her persona, her memories.
//!
//! Her lease is last of the three: it is held often and for a moment (her
//! deputies, the relay, where she is), queued memory work for as long as it
//! writes.
//!
//! A test build panics when one of them is asked for too late ([`check`]).
//! The rows are not checked: what takes one and then her lease has its own
//! test.

/// A lock whose place is checked as it is asked for.
#[cfg(test)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Lock {
    Imports,
    MemoryWork,
}

/// What this connection holds of the three, and nobody else's. `pg_locks`
/// does not say whether a lock is a transaction's or its session's; each of
/// these is only ever taken for a transaction (the tests hold every place in
/// the source that names one to that).
#[cfg(test)]
pub(crate) async fn check<C: sea_orm::ConnectionTrait>(
    db: &C,
    asked: Lock,
) -> Result<(), sea_orm::DbErr> {
    let row = db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "WITH mine AS ( \
               SELECT classid::bigint AS class, objid::bigint AS object, objsubid AS sub \
               FROM pg_locks WHERE locktype = 'advisory' AND granted AND pid = pg_backend_pid() \
               AND database = (SELECT oid FROM pg_database WHERE datname = current_database()) \
             ), keys AS ( \
               SELECT hashtext('agent_host_import')::bigint AS imports, \
                      hashtextextended('myriad-her-lease', 0) AS lease \
             ) \
             SELECT EXISTS (SELECT 1 FROM mine, keys WHERE sub = 1 \
                      AND class = ((imports >> 32) & 4294967295) \
                      AND object = (imports & 4294967295)) AS imports, \
                    EXISTS (SELECT 1 FROM mine WHERE sub = 2 \
                      AND class = 1296388170 AND object = 1) AS memory, \
                    EXISTS (SELECT 1 FROM mine, keys WHERE sub = 1 \
                      AND class = ((lease >> 32) & 4294967295) \
                      AND object = (lease & 4294967295)) AS lease",
        ))
        .await?
        .ok_or(sea_orm::DbErr::RecordNotFound("pg_locks".into()))?;
    let held = |name: &str| row.try_get::<bool>("", name);
    let late = match asked {
        Lock::Imports => !held("imports")? && (held("memory")? || held("lease")?),
        Lock::MemoryWork => !held("memory")? && held("lease")?,
    };
    assert!(
        !late,
        "{asked:?} is asked for after a lock it goes before (persona_sync::lease::order)"
    );
    Ok(())
}

#[cfg(test)]
#[path = "order_tests.rs"]
mod tests;
