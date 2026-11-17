//! What the site keeps of her calls to a device (contract `calls`), as her
//! host: the owner's approval per device and permission group (the
//! approved layer; what is granted at a call is worked out then, never
//! kept), and one row per call she made. A row says what was asked, for
//! whom, how it ended and how long it took; never its args, its result or
//! anything the device said, only the code a failure ended with.

use chrono::{DateTime, Utc};
use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, Statement};

/// How long a call's row is kept.
pub(super) const KEPT_DAYS: i64 = 30;

fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values)
}

/// Whether a permission group with no decision of the owner's is approved:
/// those of `calls.defaultApproved` are, until the owner turns them off.
pub(super) fn approved_by_default(group: &str) -> bool {
    myriad_her_link::calls_default_approved()
        .iter()
        .any(|one| one == group)
}

/// What the owner decided for one device, per permission group: the groups
/// it never decided are not in it ([`approved_by_default`]).
pub(super) async fn decided<C: ConnectionTrait>(
    db: &C,
    device_id: &str,
) -> Result<Vec<(String, bool)>, DbErr> {
    db.query_all_raw(statement(
        "SELECT call_group, approved FROM agent_device_call_groups \
         WHERE device_id = $1 ORDER BY call_group",
        vec![device_id.into()],
    ))
    .await?
    .iter()
    .map(|row| Ok((row.try_get("", "call_group")?, row.try_get("", "approved")?)))
    .collect()
}

/// Whether `group` is approved for the device, by what the owner decided.
pub(super) fn approved(decided: &[(String, bool)], group: &str) -> bool {
    decided
        .iter()
        .find(|(one, _)| one == group)
        .map_or_else(|| approved_by_default(group), |(_, approved)| *approved)
}

/// The owner approves or takes back `group` for a live device of theirs.
/// `false` when the device is not one (not theirs, removed, or none).
pub(super) async fn set_approved<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    device_id: &str,
    group: &str,
    approved: bool,
) -> Result<bool, DbErr> {
    let set = db
        .execute_raw(statement(
            "INSERT INTO agent_device_call_groups \
                 (device_id, call_group, user_id, approved, decided_at) \
             SELECT id, $3, user_id, $4, $5::timestamptz FROM agent_sync_devices \
             WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL \
             ON CONFLICT (device_id, call_group) \
             DO UPDATE SET approved = EXCLUDED.approved, decided_at = EXCLUDED.decided_at",
            vec![
                device_id.into(),
                owner.into(),
                group.into(),
                approved.into(),
                crate::clock::now().into(),
            ],
        ))
        .await?;
    Ok(set.rows_affected() > 0)
}

/// How one call ended (`call.result`'s outcome, or the host's own end of
/// it: refused before it was sent, or timed out).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    Done,
    /// With the code of `calls.failCodes` it ended with.
    Failed(&'static str),
    /// The device lost track midway: it may have happened, and is not done
    /// again by any end.
    Unknown,
}

impl Outcome {
    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Done => "done",
            Self::Failed(_) => "failed",
            Self::Unknown => "unknown",
        }
    }

    pub(crate) fn code(self) -> Option<&'static str> {
        match self {
            Self::Failed(code) => Some(code),
            Self::Done | Self::Unknown => None,
        }
    }
}

/// One call she made, as its row keeps it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct Record {
    pub call_id: String,
    pub owner: i32,
    pub device_id: String,
    pub action: String,
    pub version: u64,
    /// `for`'s kind (`callAudiences`), and its groupId for a group.
    pub for_kind: String,
    pub for_group: Option<String>,
    pub outcome: Outcome,
    pub asked_at: DateTime<Utc>,
    pub ended_at: DateTime<Utc>,
    /// It ended after the host had already counted it timed out.
    pub late: bool,
}

impl Record {
    fn took_ms(&self) -> i64 {
        (self.ended_at - self.asked_at).num_milliseconds().max(0)
    }
}

/// Keeps how a call ended. A call already kept (counted timed out, then
/// its result came after all) is changed to what happened, marked late.
pub(super) async fn record<C: ConnectionTrait>(db: &C, call: &Record) -> Result<(), DbErr> {
    db.execute_raw(statement(
        "INSERT INTO agent_device_calls \
             (call_id, user_id, device_id, action, version, for_kind, for_group, \
              outcome, error_code, asked_at, ended_at, took_ms, late) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::timestamptz, $11::timestamptz, $12, $13) \
         ON CONFLICT (call_id) DO UPDATE SET outcome = EXCLUDED.outcome, \
             error_code = EXCLUDED.error_code, ended_at = EXCLUDED.ended_at, \
             took_ms = EXCLUDED.took_ms, late = TRUE",
        vec![
            call.call_id.clone().into(),
            call.owner.into(),
            call.device_id.clone().into(),
            call.action.clone().into(),
            (call.version.min(i32::MAX as u64) as i32).into(),
            call.for_kind.clone().into(),
            call.for_group.clone().into(),
            call.outcome.name().into(),
            call.outcome.code().map(str::to_owned).into(),
            call.asked_at.into(),
            call.ended_at.into(),
            call.took_ms().into(),
            call.late.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// A kept call, read back: what [`record`] wrote, the outcome by name.
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(not(test), allow(dead_code))]
pub(super) struct Kept {
    pub call_id: String,
    pub device_id: String,
    pub action: String,
    pub version: i32,
    pub for_kind: String,
    pub for_group: Option<String>,
    pub outcome: String,
    pub error_code: Option<String>,
    pub took_ms: i64,
    pub late: bool,
}

/// The owner's latest calls, newest first.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) async fn recent<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    at_most: u64,
) -> Result<Vec<Kept>, DbErr> {
    db.query_all_raw(statement(
        "SELECT call_id, device_id, action, version, for_kind, for_group, outcome, \
                error_code, took_ms, late \
         FROM agent_device_calls WHERE user_id = $1 \
         ORDER BY asked_at DESC, call_id LIMIT $2",
        vec![owner.into(), (at_most.min(i64::MAX as u64) as i64).into()],
    ))
    .await?
    .iter()
    .map(|row| {
        Ok(Kept {
            call_id: row.try_get("", "call_id")?,
            device_id: row.try_get("", "device_id")?,
            action: row.try_get("", "action")?,
            version: row.try_get("", "version")?,
            for_kind: row.try_get("", "for_kind")?,
            for_group: row.try_get("", "for_group")?,
            outcome: row.try_get("", "outcome")?,
            error_code: row.try_get("", "error_code")?,
            took_ms: row.try_get("", "took_ms")?,
            late: row.try_get("", "late")?,
        })
    })
    .collect()
}

/// Lets go of the rows older than [`KEPT_DAYS`]; how many went.
#[cfg_attr(not(test), allow(dead_code))]
pub(super) async fn prune<C: ConnectionTrait>(db: &C, now: DateTime<Utc>) -> Result<u64, DbErr> {
    let before = now - chrono::Duration::days(KEPT_DAYS);
    Ok(db
        .execute_raw(statement(
            "DELETE FROM agent_device_calls WHERE asked_at < $1::timestamptz",
            vec![before.into()],
        ))
        .await?
        .rows_affected())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::persona_sync::devices;

    const OWNER: i32 = 910_201;
    const SOMEONE: i32 = 910_202;

    async fn db() -> Option<crate::federation::test_db::SchemaDb> {
        let fixture = crate::federation::test_db::SchemaDb::new_or_media().await?;
        fixture
            .db
            .execute_unprepared(&format!(
                "INSERT INTO users (id, username) VALUES \
                 ({OWNER}, 'calls_owner'), ({SOMEONE}, 'calls_someone')"
            ))
            .await
            .unwrap();
        Some(fixture)
    }

    fn call(call_id: &str, device_id: &str, outcome: Outcome, late: bool) -> Record {
        let asked_at = crate::clock::now();
        Record {
            call_id: call_id.into(),
            owner: OWNER,
            device_id: device_id.into(),
            action: "screen.look".into(),
            version: 1,
            for_kind: "owner".into(),
            for_group: None,
            outcome,
            asked_at,
            ended_at: asked_at + chrono::Duration::milliseconds(1_250),
            late,
        }
    }

    #[test]
    fn only_the_default_groups_are_approved_until_the_owner_decides() {
        for group in myriad_her_link::calls_default_approved() {
            assert!(approved(&[], group), "{group}");
        }
        assert!(!approved(&[], "work"));
        assert!(!approved(&[], "camera"));
        let decided = vec![("screen".to_string(), false), ("work".to_string(), true)];
        assert!(!approved(&decided, "screen"));
        assert!(approved(&decided, "work"));
        assert!(approved(&decided, "voice"));
    }

    #[tokio::test]
    async fn the_owner_decides_per_device_and_group_and_it_reads_back() {
        let Some(fixture) = db().await else {
            return;
        };
        let db = &fixture.db;
        let (mac, _) = devices::add(db, OWNER, "Mac").await.unwrap();
        let (phone, _) = devices::add(db, OWNER, "Phone").await.unwrap();
        assert!(decided(db, &mac.id).await.unwrap().is_empty());

        assert!(
            set_approved(db, OWNER, &mac.id, "screen", false)
                .await
                .unwrap()
        );
        let read = decided(db, &mac.id).await.unwrap();
        assert_eq!(read, vec![("screen".to_string(), false)]);
        assert!(!approved(&read, "screen"));
        // Another device is not touched.
        assert!(approved(&decided(db, &phone.id).await.unwrap(), "screen"));
        // Turned on again: the same row.
        assert!(
            set_approved(db, OWNER, &mac.id, "screen", true)
                .await
                .unwrap()
        );
        assert_eq!(
            decided(db, &mac.id).await.unwrap(),
            vec![("screen".to_string(), true)]
        );
        // Not someone else's device, nor a removed one.
        assert!(
            !set_approved(db, SOMEONE, &mac.id, "screen", false)
                .await
                .unwrap()
        );
        devices::revoke(db, OWNER, &phone.id, devices::RevokeReason::ByOwner)
            .await
            .unwrap();
        assert!(
            !set_approved(db, OWNER, &phone.id, "screen", false)
                .await
                .unwrap()
        );
        assert!(decided(db, &phone.id).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_call_is_one_row_a_late_result_changes_it_and_a_forgotten_device_takes_its_rows() {
        let Some(fixture) = db().await else {
            return;
        };
        let db = &fixture.db;
        let (mac, _) = devices::add(db, OWNER, "Mac").await.unwrap();
        set_approved(db, OWNER, &mac.id, "screen", true)
            .await
            .unwrap();

        let timed_out = call("c-1", &mac.id, Outcome::Failed("call_timeout"), false);
        record(db, &timed_out).await.unwrap();
        record(db, &call("c-2", &mac.id, Outcome::Unknown, false))
            .await
            .unwrap();
        let kept = recent(db, OWNER, 10).await.unwrap();
        let first = kept.iter().find(|one| one.call_id == "c-1").unwrap();
        assert_eq!(
            first,
            &Kept {
                call_id: "c-1".into(),
                device_id: mac.id.clone(),
                action: "screen.look".into(),
                version: 1,
                for_kind: "owner".into(),
                for_group: None,
                outcome: "failed".into(),
                error_code: Some("call_timeout".into()),
                took_ms: 1_250,
                late: false,
            }
        );
        let unknown = kept.iter().find(|one| one.call_id == "c-2").unwrap();
        assert_eq!(
            (unknown.outcome.as_str(), unknown.error_code.as_deref()),
            ("unknown", None)
        );

        // Its result came after all: the same row says what happened.
        record(db, &call("c-1", &mac.id, Outcome::Done, true))
            .await
            .unwrap();
        let kept = recent(db, OWNER, 10).await.unwrap();
        assert_eq!(kept.len(), 2);
        let first = kept.iter().find(|one| one.call_id == "c-1").unwrap();
        assert_eq!(
            (
                first.outcome.as_str(),
                first.error_code.as_deref(),
                first.late
            ),
            ("done", None, true)
        );

        // Rows past what is kept go.
        assert_eq!(prune(db, crate::clock::now()).await.unwrap(), 0);
        let later = crate::clock::now() + chrono::Duration::days(KEPT_DAYS + 1);
        assert_eq!(prune(db, later).await.unwrap(), 2);

        // A forgotten device takes its decisions and its calls with it.
        record(db, &call("c-3", &mac.id, Outcome::Done, false))
            .await
            .unwrap();
        devices::revoke(db, OWNER, &mac.id, devices::RevokeReason::ByOwner)
            .await
            .unwrap();
        assert!(devices::forget(db, OWNER, &mac.id).await.unwrap());
        assert!(recent(db, OWNER, 10).await.unwrap().is_empty());
        assert!(decided(db, &mac.id).await.unwrap().is_empty());
    }

    #[test]
    fn a_row_holds_no_args_result_or_words_of_the_device() {
        // The table's columns are the whole of what is kept: nothing that
        // could hold what was asked, what came back or what the device said.
        let sql = include_str!("../../../../migrations/persona_sync.sql");
        let table = sql
            .split("CREATE TABLE IF NOT EXISTS agent_device_calls (")
            .nth(1)
            .and_then(|rest| rest.split(");").next())
            .unwrap();
        for word in [
            "args", "result", "summary", "facts", "text", "message", "detail", "body",
        ] {
            assert!(
                !table.contains(word),
                "agent_device_calls has a column like {word}"
            );
        }
    }
}
