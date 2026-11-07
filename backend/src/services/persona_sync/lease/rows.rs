//! Where she lives, as stored: the site's copy of her (`agent_her_copy`),
//! her moves (`agent_her_moves`) and her lease (`agent_lease_grants`, kept
//! by `her_log::lease`). [`Lease`] is the three read together; a step of a
//! move writes the copy and the move, and signs the grant that goes with it
//! in the same transaction.

use chrono::{DateTime, Utc};
use myriad_persona_log::{How, lease::Request};
use sea_orm::{ConnectionTrait, DatabaseBackend, DbErr, QueryResult, Statement};
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;

use super::{Done, Lease, Move, SITE, State};
use crate::services::her_log::{self, lease as grants};

fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values)
}

fn grant_failed(error: her_log::Error) -> DbErr {
    match error {
        her_log::Error::Db(error) => error,
        other => DbErr::Custom(format!("her lease: {other}")),
    }
}

/// The name serde gives a value of one of the move's small enums.
fn name<T: Serialize>(value: &T) -> String {
    serde_json::to_value(value)
        .ok()
        .and_then(|value| value.as_str().map(str::to_string))
        .unwrap_or_default()
}

fn named<T: DeserializeOwned>(raw: &str) -> Result<T, DbErr> {
    serde_json::from_value(Value::String(raw.to_string()))
        .map_err(|_| DbErr::Custom(format!("a kept move has {raw:?}")))
}

/// The site's copy of her.
#[derive(Debug, Clone)]
pub(super) struct Copy {
    pub lineage: String,
    pub state: State,
    pub state_at: DateTime<Utc>,
}

fn copy_of(row: &QueryResult) -> Result<Copy, DbErr> {
    let state: String = row.try_get("", "state")?;
    Ok(Copy {
        lineage: row.try_get("", "lineage")?,
        state: State::parse(&state)
            .ok_or_else(|| DbErr::Custom(format!("unknown state of her copy {state}")))?,
        state_at: row
            .try_get::<DateTime<chrono::FixedOffset>>("", "state_at")?
            .with_timezone(&Utc),
    })
}

pub(super) async fn copy<C: ConnectionTrait>(db: &C, locked: bool) -> Result<Option<Copy>, DbErr> {
    let lock = if locked { " FOR UPDATE" } else { "" };
    db.query_one_raw(statement(
        &format!("SELECT lineage, state, state_at FROM agent_her_copy WHERE id = 'her'{lock}"),
        vec![],
    ))
    .await?
    .as_ref()
    .map(copy_of)
    .transpose()
}

/// A copy of her with a new lineage, live on the site, when her persona is
/// here and there is none: a birth. Whether it was made.
pub(super) async fn born<C: ConnectionTrait>(db: &C) -> Result<Option<Copy>, DbErr> {
    db.query_one_raw(statement(
        "INSERT INTO agent_her_copy (id, lineage, state, state_at) \
         SELECT 'her', gen_random_uuid()::text, 'live', $1::timestamptz \
         WHERE EXISTS (SELECT 1 FROM agent_persona WHERE id = 'site') \
         ON CONFLICT (id) DO NOTHING RETURNING lineage, state, state_at",
        vec![crate::clock::now().into()],
    ))
    .await?
    .as_ref()
    .map(copy_of)
    .transpose()
}

/// A copy of her of `lineage`, as `state`: one coming in.
pub(super) async fn put_copy<C: ConnectionTrait>(
    db: &C,
    lineage: &str,
    state: State,
) -> Result<(), DbErr> {
    db.execute_raw(statement(
        "INSERT INTO agent_her_copy (id, lineage, state, state_at) VALUES ('her', $1, $2, $3::timestamptz)",
        vec![
            lineage.into(),
            state.as_str().into(),
            crate::clock::now().into(),
        ],
    ))
    .await?;
    Ok(())
}

pub(super) async fn set_copy<C: ConnectionTrait>(db: &C, state: State) -> Result<(), DbErr> {
    db.execute_raw(statement(
        "UPDATE agent_her_copy SET state = $1, \
         state_at = CASE WHEN state = $1 THEN state_at ELSE $2::timestamptz END WHERE id = 'her'",
        vec![state.as_str().into(), crate::clock::now().into()],
    ))
    .await?;
    Ok(())
}

/// The copy and every move of it go: the next her is another birth.
pub(super) async fn forget_copy<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    db.execute_unprepared("DELETE FROM agent_her_moves; DELETE FROM agent_her_copy")
        .await?;
    Ok(())
}

const MOVE_COLUMNS: &str = "id, kind, from_holder, to_holder, phase, started_by, digest, \
     started_at, confirmed_at, agreed, import_id, via, was_state, updated_at";

fn move_of(row: &QueryResult) -> Result<(Move, DateTime<Utc>), DbErr> {
    let at = |column: &str| -> Result<Option<DateTime<Utc>>, DbErr> {
        Ok(row
            .try_get::<Option<DateTime<chrono::FixedOffset>>>("", column)?
            .map(|at| at.with_timezone(&Utc)))
    };
    let was: Option<String> = row.try_get("", "was_state")?;
    let moving = Move {
        id: row.try_get("", "id")?,
        kind: named(&row.try_get::<String>("", "kind")?)?,
        from: row.try_get("", "from_holder")?,
        to: row.try_get("", "to_holder")?,
        phase: named(&row.try_get::<String>("", "phase")?)?,
        by: named(&row.try_get::<String>("", "started_by")?)?,
        digest: row.try_get("", "digest")?,
        started_at: at("started_at")?.unwrap_or_else(crate::clock::now),
        confirmed_at: at("confirmed_at")?,
        agreed: row.try_get("", "agreed")?,
        import: row.try_get("", "import_id")?,
        via: row.try_get("", "via")?,
        was: was.as_deref().and_then(State::parse),
    };
    Ok((moving, at("updated_at")?.unwrap_or_else(crate::clock::now)))
}

/// The move under way, and when it last took a step.
async fn under_way<C: ConnectionTrait>(db: &C) -> Result<Option<(Move, DateTime<Utc>)>, DbErr> {
    db.query_one_raw(statement(
        &format!("SELECT {MOVE_COLUMNS} FROM agent_her_moves WHERE done_at IS NULL"),
        vec![],
    ))
    .await?
    .as_ref()
    .map(move_of)
    .transpose()
}

/// The move that finished last.
async fn last_done<C: ConnectionTrait>(db: &C) -> Result<Option<Done>, DbErr> {
    db.query_one_raw(statement(
        "SELECT id, kind, done_epoch, import_id FROM agent_her_moves \
         WHERE done_at IS NOT NULL ORDER BY done_at DESC, id LIMIT 1",
        vec![],
    ))
    .await?
    .map(|row| {
        Ok(Done {
            id: row.try_get("", "id")?,
            kind: named(&row.try_get::<String>("", "kind")?)?,
            epoch: row.try_get::<Option<i64>>("", "done_epoch")?.unwrap_or(0),
            import: row.try_get("", "import_id")?,
        })
    })
    .transpose()
}

/// Writes the move under way as it stands now.
pub(super) async fn put_move<C: ConnectionTrait>(db: &C, moving: &Move) -> Result<(), DbErr> {
    db.execute_raw(statement(
        &format!(
            "INSERT INTO agent_her_moves ({MOVE_COLUMNS}) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8::timestamptz, $9::timestamptz, $10, $11, $12, \
                     $13, $14::timestamptz) \
             ON CONFLICT (id) DO UPDATE SET phase = EXCLUDED.phase, digest = EXCLUDED.digest, \
               confirmed_at = EXCLUDED.confirmed_at, agreed = EXCLUDED.agreed, \
               import_id = EXCLUDED.import_id, via = EXCLUDED.via, was_state = EXCLUDED.was_state, \
               updated_at = EXCLUDED.updated_at"
        ),
        vec![
            moving.id.clone().into(),
            name(&moving.kind).into(),
            moving.from.clone().into(),
            moving.to.clone().into(),
            name(&moving.phase).into(),
            name(&moving.by).into(),
            moving.digest.clone().into(),
            moving.started_at.into(),
            moving.confirmed_at.into(),
            moving.agreed.into(),
            moving.import.clone().into(),
            moving.via.clone().into(),
            moving.was.map(|was| was.as_str().to_string()).into(),
            crate::clock::now().into(),
        ],
    ))
    .await?;
    Ok(())
}

/// The move under way is called off: it goes.
pub(super) async fn drop_move<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    db.execute_unprepared("DELETE FROM agent_her_moves WHERE done_at IS NULL")
        .await?;
    Ok(())
}

/// The move under way finished, leaving her at `epoch`: kept as done.
pub(super) async fn finish<C: ConnectionTrait>(db: &C, epoch: i64) -> Result<(), DbErr> {
    db.execute_raw(statement(
        "UPDATE agent_her_moves SET phase = 'committed', done_at = $1::timestamptz, \
         done_epoch = $2, updated_at = $1::timestamptz WHERE done_at IS NULL",
        vec![crate::clock::now().into(), epoch.into()],
    ))
    .await?;
    Ok(())
}

/// The grant that stands, if one was ever issued.
pub(super) async fn standing<C: ConnectionTrait>(
    db: &C,
) -> Result<Option<myriad_persona_log::Grant>, DbErr> {
    grants::standing(db).await.map_err(grant_failed)
}

/// Signs the grant a step of a move asks for (`her_log::lease::for_move`).
pub(super) async fn sign<C: ConnectionTrait>(
    db: &C,
    request: Request,
    at_least: Option<i64>,
) -> Result<myriad_persona_log::Grant, DbErr> {
    grants::for_move(db, request, at_least.map(|epoch| epoch.max(0) as u64))
        .await
        .map_err(grant_failed)
}

/// Holds her lease for the transaction (grants move one at a time).
pub(super) async fn lock<C: ConnectionTrait>(db: &C) -> Result<(), DbErr> {
    grants::lock(db).await.map_err(grant_failed)
}

/// The first grant, to `holder` at `epoch`, when none was ever issued.
pub(super) async fn begin_with<C: ConnectionTrait>(
    db: &C,
    holder: &str,
    epoch: i64,
) -> Result<(), DbErr> {
    grants::begin_with(db, holder, epoch.max(1) as u64)
        .await
        .map_err(grant_failed)?;
    Ok(())
}

/// She stops for the move `moving` (no end holds her) or goes on as she
/// was: a freeze when she was not frozen, a thaw when she was and is not.
pub(super) async fn hold_still<C: ConnectionTrait>(
    db: &C,
    frozen: bool,
    moving: Option<&str>,
) -> Result<(), DbErr> {
    let standing = standing(db).await?;
    let is_frozen = standing
        .as_ref()
        .is_some_and(|grant| grant.how == How::Freeze);
    if frozen && !is_frozen {
        let Some(moving) = moving else {
            return Err(DbErr::Custom("a freeze needs its move".into()));
        };
        let holder = standing
            .and_then(|grant| grant.holder)
            .unwrap_or_else(|| SITE.to_string());
        let through = grants::through_of(db, &holder)
            .await
            .map_err(grant_failed)?;
        sign(
            db,
            Request::Freeze {
                moving: moving.to_string(),
                through: (!through.is_empty()).then_some(through),
            },
            None,
        )
        .await?;
    } else if !frozen && is_frozen {
        sign(db, Request::Thaw, None).await?;
    }
    Ok(())
}

/// Her lease comes to the site, at `at_least` or later: the move she is
/// frozen for commits to it (thaws, if she was frozen on the site), or the
/// end that held her gives way. Nothing when the site holds her already.
pub(super) async fn to_site<C: ConnectionTrait>(
    db: &C,
    at_least: Option<i64>,
) -> Result<(), DbErr> {
    let Some(standing) = standing(db).await? else {
        return begin_with(db, SITE, at_least.unwrap_or(1)).await;
    };
    let request = match (standing.how, standing.holder.as_deref()) {
        (How::Freeze, _) if standing.from.as_deref() == Some(SITE) => Request::Thaw,
        (How::Freeze, _) => Request::Commit { to: SITE.into() },
        (_, Some(SITE)) => return Ok(()),
        _ => Request::Force { to: SITE.into() },
    };
    sign(db, request, at_least).await?;
    Ok(())
}

/// The first epoch of the holding that stands: the grant that gave `holder`
/// her, past every freeze and thaw since (none moves her to another end).
async fn since<C: ConnectionTrait>(db: &C, holder: &str) -> Result<i64, DbErr> {
    Ok(db
        .query_one_raw(statement(
            "SELECT COALESCE(MIN(epoch), 1) AS since FROM agent_lease_grants WHERE epoch > \
             COALESCE((SELECT MAX(epoch) FROM agent_lease_grants \
                       WHERE holder IS NOT NULL AND holder <> $1), 0)",
            vec![holder.into()],
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "since"))
        .transpose()?
        .unwrap_or(1))
}

/// Where she lives, from her copy, her moves and the grant that stands.
/// A site that never issued a grant holds her at epoch 1 (what the first
/// grant will say).
pub(super) async fn assemble<C: ConnectionTrait>(db: &C, copy: Copy) -> Result<Lease, DbErr> {
    let grant = standing(db).await?;
    let (holder, epoch, frozen) = match &grant {
        Some(grant) => (
            grant
                .holder
                .clone()
                .or_else(|| grant.from.clone())
                .unwrap_or_else(|| SITE.to_string()),
            grant.epoch as i64,
            grant.how == How::Freeze,
        ),
        None => (SITE.to_string(), 1, false),
    };
    let since = if grant.is_some() {
        since(db, &holder).await?
    } else {
        1
    };
    let under_way = under_way(db).await?;
    let updated_at = under_way
        .as_ref()
        .map_or(copy.state_at, |(_, at)| (*at).max(copy.state_at));
    Ok(Lease {
        holder,
        lineage: copy.lineage,
        epoch,
        since: since.min(epoch),
        state: copy.state,
        frozen,
        moving: under_way.map(|(moving, _)| moving),
        updated_at,
        done: last_done(db).await?,
    })
}
