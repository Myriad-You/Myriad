//! Moving what she kept before her nodes onto them (step S4 of moving the
//! site's memory onto the shared core's nodes). The old rows are read and
//! never written: every row here goes through the one way in
//! ([`super::write`]), as hers, under each audience's lock, its id made
//! from what it is made from, so a run taken up again, or run twice,
//! writes only what is not there yet.
//!
//! First what needs no model, in this order:
//!
//! 1. her old topics and what she put under them ([`super::migrate_topics`]);
//! 2. her other sources, as each is hung where it is written
//!    ([`super::hanging`]), and what she noted on a child end
//!    ([`super::child`]) ([`super::migrate_sources`]);
//! 3. what is on her mind about someone and what she wants, as her intents
//!    ([`super::home_intents`]), without letting any row go.
//!
//! Then, only when asked for, what does ([`super::migrate_replay`]): what
//! she looked at and put under no topic, what she kept before topics, and
//! a group's facts, sorted through the one sorting path
//! ([`super::sorting::answer`]). It is estimated by count before it starts
//! (written on the run's record and in the log), and stops once it has
//! cost more than twice that, saying why; the next run goes on from there.
//!
//! Then the fold-back check ([`super::migrate_check`]): the moved rows,
//! folded again, hold one entry for each old placement on the node it was
//! moved to; what does not is reported, never mended here. And the count
//! by audience ([`super::migrate_census`]): of what belongs on a node, how
//! much is on one, put nowhere, or not decided; a run that asked the model
//! and still leaves some undecided stops, saying where, to be run again.
//!
//! A dry run only counts and estimates. Every run is a row of
//! `agent_node_migrations` (`migrations/memory_nodes.sql`); one runs at a
//! time. Her rows need her pen: while her mind is elsewhere a run stops
//! before writing, to be run again.

use std::time::Duration;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde::Serialize;
use serde_json::{Value, json};

use super::{
    Error, hanging, migrate_census, migrate_check, migrate_replay, migrate_sources, migrate_topics,
};
use crate::services::her_log::pen::{self, Thought};

/// A run left `running` this long ago is taken to have died with its
/// process: another may start.
const STALE: Duration = Duration::from_secs(12 * 3600);

/// What a run is asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct Options {
    /// Only count and estimate: nothing written but the run's record.
    pub dry_run: bool,
    /// Also the part that asks the model.
    pub replay: bool,
}

/// What a run counted and wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Counts {
    /// Old topics, and how each went: started as a node, onto a node there
    /// already under its key or that goes by its name, or not at all (its
    /// node forgotten since, or a key or name the core does not read).
    pub topics: usize,
    pub topics_started: usize,
    pub topics_kept: usize,
    pub topics_joined: usize,
    pub topics_gone: usize,
    pub topics_bad: usize,
    /// Old placements under a topic of a memory still held: put on its
    /// node, placed before (by her nodes' own paths or a run before), or
    /// with no text to put.
    pub placements: usize,
    pub placed: usize,
    pub placed_before: usize,
    pub with_message: usize,
    pub empty: usize,
    /// Rows of her other sources still held, and those hung now.
    pub sources: usize,
    pub hung: usize,
    /// Her notes on a child end, and those put now.
    pub child_notes: usize,
    pub child_put: usize,
    /// Threads and wants still held; intents written; refused by the
    /// gate (staying rows).
    pub threads: usize,
    pub wants: usize,
    pub intended: usize,
    pub refused: usize,
    /// For the model: facts to sort, sorted, asks made.
    pub to_sort: usize,
    pub sorted: usize,
    pub asks: usize,
    /// Rows of her nodes before and after the run (all accounts).
    pub rows_before: i64,
    pub rows_after: i64,
    /// The fold-back check.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub check: Option<migrate_check::Checked>,
    /// The count by audience, after the run.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub census: Option<migrate_census::Census>,
}

/// The part that asks the model, by count: asks, tokens, dollars.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Estimate {
    pub facts: usize,
    pub audiences: usize,
    pub asks: usize,
    pub input_tokens: u64,
    pub output_tokens: u64,
    /// At the plan's price (input $1, output $5 a million: uncertain).
    pub usd: f64,
}

impl Estimate {
    pub(crate) fn tokens(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }
}

/// What the part that asks the model cost.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Spent {
    pub asks: usize,
    pub tokens: u64,
}

/// How a run ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Running,
    Done,
    Stopped,
    Failed,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Done => "done",
            Self::Stopped => "stopped",
            Self::Failed => "failed",
        }
    }
}

/// One run, as its record holds it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Report {
    pub id: i64,
    pub dry_run: bool,
    pub status: Status,
    pub counts: Counts,
    pub estimate: Estimate,
    pub spent: Spent,
    pub note: Option<String>,
}

/// Why a run stopped before its end.
#[derive(Debug)]
pub(crate) enum Halt {
    /// To be taken up again: said why.
    Stopped(String),
    Failed(Error),
}

impl From<Error> for Halt {
    fn from(error: Error) -> Self {
        Self::Failed(error)
    }
}

impl From<sea_orm::DbErr> for Halt {
    fn from(error: sea_orm::DbErr) -> Self {
        Self::Failed(Error::Db(error))
    }
}

/// Her pen for a write, or a stop: her rows are not written while her
/// mind is elsewhere.
pub(crate) fn her_pen() -> Result<Thought, Halt> {
    pen::think().ok_or_else(|| {
        Halt::Stopped("her mind is not on the site: nothing of hers is written".into())
    })
}

/// Tokens of `text` as the plan counts them: a CJK character one, four
/// other characters one.
pub(crate) fn tokens_of(text: &str) -> u64 {
    let (wide, other) = text.chars().fold((0u64, 0u64), |(wide, other), ch| {
        if ch as u32 >= 0x2E80 {
            (wide + 1, other)
        } else {
            (wide, other + 1)
        }
    });
    wide + other.div_ceil(4)
}

async fn rows_now<C: ConnectionTrait>(db: &C) -> Result<i64, Error> {
    Ok(db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*) AS n FROM agent_node_rows",
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "n"))
        .transpose()?
        .unwrap_or(0))
}

/// Starts a run's record; none while another runs.
async fn begin(db: &DatabaseConnection, dry_run: bool) -> Result<Option<i64>, Error> {
    let txn = db.begin().await?;
    txn.execute_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended('myriad-memory-nodes-migrate', 0))",
    ))
    .await?;
    let now = crate::clock::now();
    let since = now - chrono::Duration::from_std(STALE).unwrap_or_default();
    let running = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id FROM agent_node_migrations WHERE status = 'running' AND started_at > $1",
            [since.fixed_offset().into()],
        ))
        .await?
        .is_some();
    if running {
        txn.commit().await?;
        return Ok(None);
    }
    let row = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_node_migrations (dry_run, status, started_at) \
             VALUES ($1, 'running', $2) RETURNING id",
            [dry_run.into(), now.fixed_offset().into()],
        ))
        .await?
        .ok_or_else(|| Error::Refused("a run's record was not kept".into()))?;
    txn.commit().await?;
    Ok(Some(row.try_get("", "id")?))
}

fn to<T: Serialize>(value: &T) -> Value {
    serde_json::to_value(value).unwrap_or(Value::Null)
}

async fn keep(db: &DatabaseConnection, report: &Report) -> Result<(), Error> {
    let finished = (report.status != Status::Running).then(|| crate::clock::now().fixed_offset());
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_node_migrations SET status = $2, counts = $3, estimate = $4, spent = $5, \
         note = $6, finished_at = $7 WHERE id = $1",
        [
            report.id.into(),
            report.status.as_str().into(),
            to(&report.counts).into(),
            to(&report.estimate).into(),
            to(&report.spent).into(),
            report.note.clone().into(),
            finished.into(),
        ],
    ))
    .await?;
    Ok(())
}

/// The estimate is kept on the run's record before the model is asked.
pub(crate) async fn estimated(db: &DatabaseConnection, report: &Report) {
    tracing::info!(
        facts = report.estimate.facts,
        asks = report.estimate.asks,
        input_tokens = report.estimate.input_tokens,
        output_tokens = report.estimate.output_tokens,
        usd = report.estimate.usd,
        "[memory-nodes] moving her memory: what asking the model is estimated to cost"
    );
    if let Err(error) = keep(db, report).await {
        tracing::warn!(%error, "[memory-nodes] a run's estimate not kept");
    }
}

/// Runs once, now ([module docs](self)); none while another runs.
pub(crate) async fn run(
    db: &DatabaseConnection,
    options: Options,
) -> Result<Option<Report>, Error> {
    let Some(id) = begin(db, options.dry_run).await? else {
        return Ok(None);
    };
    Ok(Some(run_as(db, id, options).await))
}

/// Starts a run in the background; its record's id, none while another
/// runs.
pub(crate) async fn start(db: &DatabaseConnection, options: Options) -> Result<Option<i64>, Error> {
    let Some(id) = begin(db, options.dry_run).await? else {
        return Ok(None);
    };
    let owned = db.clone();
    let spawned =
        crate::services::agent::merope::background::spawn("memory_nodes_migrate", async move {
            run_as(&owned, id, options).await;
        });
    if !spawned {
        let report = Report {
            id,
            dry_run: options.dry_run,
            status: Status::Stopped,
            counts: Counts::default(),
            estimate: Estimate::default(),
            spent: Spent::default(),
            note: Some("no room for background work now".into()),
        };
        keep(db, &report).await?;
    }
    Ok(Some(id))
}

async fn run_as(db: &DatabaseConnection, id: i64, options: Options) -> Report {
    let mut report = Report {
        id,
        dry_run: options.dry_run,
        status: Status::Running,
        counts: Counts::default(),
        estimate: Estimate::default(),
        spent: Spent::default(),
        note: None,
    };
    let ended = steps(db, options, &mut report).await;
    report.counts.rows_after = rows_now(db).await.unwrap_or(report.counts.rows_after);
    match ended {
        Ok(()) => report.status = Status::Done,
        Err(Halt::Stopped(why)) => {
            report.status = Status::Stopped;
            report.note = Some(why);
        }
        Err(Halt::Failed(error)) => {
            report.status = Status::Failed;
            report.note = Some(error.to_string());
        }
    }
    tracing::info!(
        id,
        status = report.status.as_str(),
        dry_run = options.dry_run,
        counts = %json!(report.counts),
        spent = %json!(report.spent),
        note = report.note.as_deref().unwrap_or(""),
        "[memory-nodes] moving her memory onto her nodes"
    );
    if let Err(error) = keep(db, &report).await {
        tracing::warn!(%error, "[memory-nodes] a run's record not kept");
    }
    report
}

async fn steps(db: &DatabaseConnection, options: Options, report: &mut Report) -> Result<(), Halt> {
    report.counts.rows_before = rows_now(db).await?;
    report.counts.rows_after = report.counts.rows_before;
    let site_owner = hanging::site_owner(db).await;
    if options.dry_run {
        migrate_topics::count(db, &mut report.counts).await?;
        migrate_sources::count(db, &mut report.counts).await?;
        report.estimate = migrate_replay::estimate(db, site_owner).await?;
        report.counts.to_sort = report.estimate.facts;
        report.counts.census = Some(migrate_census::census(db).await?);
        return Ok(());
    }
    her_pen()?;
    let moved = migrate_topics::run(db, site_owner, &mut report.counts).await?;
    migrate_sources::run(db, site_owner, &mut report.counts).await?;
    if options.replay {
        report.estimate = migrate_replay::estimate(db, site_owner).await?;
        report.counts.to_sort = report.estimate.facts;
        estimated(db, report).await;
        migrate_replay::run(db, site_owner, report).await?;
    }
    report.counts.check = Some(migrate_check::check(db, &moved).await?);
    let census = migrate_census::census(db).await?;
    let undecided = census.undecided;
    let undecided_where = census.undecided_where();
    report.counts.census = Some(census);
    if options.replay && undecided > 0 {
        return Err(Halt::Stopped(format!(
            "{undecided} of what belongs on a node is on none and not put nowhere \
             ({undecided_where}); the next run asks of it again"
        )));
    }
    Ok(())
}

/// The latest runs' records, newest first, at most `limit`.
pub(crate) async fn runs(db: &DatabaseConnection, limit: u64) -> Result<Vec<Value>, Error> {
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, dry_run, status, counts, estimate, spent, note, started_at, finished_at \
             FROM agent_node_migrations ORDER BY id DESC LIMIT $1",
            [(limit as i64).into()],
        ))
        .await?;
    rows.iter()
        .map(|row| {
            let at = |column: &str| -> Result<Option<i64>, Error> {
                Ok(row
                    .try_get::<Option<chrono::DateTime<chrono::FixedOffset>>>("", column)?
                    .map(|at| at.timestamp_millis()))
            };
            Ok(json!({
                "id": row.try_get::<i64>("", "id")?,
                "dryRun": row.try_get::<bool>("", "dry_run")?,
                "status": row.try_get::<String>("", "status")?,
                "counts": row.try_get::<Value>("", "counts")?,
                "estimate": row.try_get::<Value>("", "estimate")?,
                "spent": row.try_get::<Value>("", "spent")?,
                "note": row.try_get::<Option<String>>("", "note")?,
                "startedAt": at("started_at")?,
                "finishedAt": at("finished_at")?,
            }))
        })
        .collect()
}
