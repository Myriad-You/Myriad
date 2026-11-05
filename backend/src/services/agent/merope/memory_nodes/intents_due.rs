//! Her intents come due (decision D5, N3): every half minute, while she
//! reads her nodes ([`super::Mode::reads`]; `shadow` writes them and acts
//! on none) and lives on the site, the waiting intents of each private
//! audience are put to the core (`myriad_nodes::intents::due`): a time come,
//! or the node one waits on changed or done.
//!
//! Each one due is settled first (`said`), so it is acted on once whatever
//! happens next: a row already there is someone else's acting on it. Then:
//!
//! - the partner's instruction to do something is checked again by the core
//!   (`intents::check`), as it stands now: the same person and her, on the
//!   site, every runtime grant it was given under still granted. It runs as
//!   one unattended Work run of that one thing ([`super::intents_run`]),
//!   under those grants at most; what came of it is kept, the intent is
//!   done, and she tells them how it went;
//! - anything else (hers, the partner's to be said, an instruction the
//!   check stops) raises her hand: she says it to them now
//!   (`reach::say_now`), and for a stopped instruction, why it was not done.

use chrono::{DateTime, Utc};
use myriad_nodes::{
    body::{IntentStatus, Origin},
    intents::{self, CheckInput, DueInput, Verdict, Why},
};
use sea_orm::DatabaseConnection;

use super::{
    Error, heads,
    intent_store::{self, Held, Instruction, Outcome},
    intents_run::{self, Ran},
    write,
};
use crate::services::agent::merope::reach;
use crate::services::agent::speech::Source;

/// One of the partner's instructions the check let run, to run.
#[derive(Debug, Clone)]
pub(crate) struct ToRun {
    pub user_id: i32,
    pub audience: String,
    pub held: Held,
    pub given: Instruction,
    /// What it may use: the grants it was given under that are still granted.
    pub cap: Vec<String>,
}

/// The driver's tick: what is due now acted on; each run in the
/// background, so one long run holds up nothing else.
pub(crate) async fn tick(db: DatabaseConnection) {
    let now = crate::services::agent::merope::clock::now();
    for to_run in tick_at(&db, now).await {
        let db = db.clone();
        crate::services::agent::merope::background::spawn("her intent's work", async move {
            finish(&db, to_run).await;
        });
    }
}

/// What is due at `now`, settled and raised; the instructions to run,
/// returned to be run.
pub(crate) async fn tick_at(db: &DatabaseConnection, now: DateTime<Utc>) -> Vec<ToRun> {
    if !super::mode().reads() || !crate::services::agent::merope::is_enabled().await {
        return Vec::new();
    }
    let audiences = match intent_store::waiting_audiences(db).await {
        Ok(audiences) => audiences,
        Err(error) => {
            tracing::warn!(%error, "[memory-nodes] waiting intents could not be read");
            return Vec::new();
        }
    };
    let mut runs = Vec::new();
    for (user_id, audience) in audiences {
        match due_in(db, user_id, &audience, now).await {
            Ok(mut more) => runs.append(&mut more),
            Err(error) => tracing::warn!(%error, "[memory-nodes] intents not acted on"),
        }
    }
    runs
}

async fn due_in(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    now: DateTime<Utc>,
) -> Result<Vec<ToRun>, Error> {
    let rows = heads::rows_of(db, user_id, audience).await?;
    let due = intents::due(DueInput {
        bodies: heads::bodies(&rows)?,
        rows: rows.into_iter().map(|kept| kept.row).collect(),
        ledger: Vec::new(),
        now: now.timestamp_micros(),
    })?;
    if due.due.is_empty() {
        return Ok(Vec::new());
    }
    let held = intent_store::held(db, user_id, audience).await?;
    let standing = intent_store::standing_now(db, user_id).await;
    let mut runs = Vec::new();
    for due in &due.due {
        let Some(held) = held
            .iter()
            .find(|held| held.id == due.intent && held.node == due.node && held.waiting())
        else {
            continue;
        };
        // Settled first: acted on once, whatever comes after.
        if !settle(db, user_id, audience, held, IntentStatus::Said).await {
            continue;
        }
        let given = match held.origin {
            Some(Origin::Partner) => intent_store::instruction(db, user_id, &held.id).await?,
            _ => None,
        };
        let checked = intents::check(CheckInput {
            origin: held.origin,
            action: held.action,
            instruction: given.as_ref().map(Instruction::standing),
            now: standing.clone(),
        });
        match (checked.verdict, given) {
            (Verdict::Run, Some(given)) => {
                let cap = given
                    .given
                    .granted
                    .intersection(&standing.granted)
                    .cloned()
                    .collect();
                runs.push(ToRun {
                    user_id,
                    audience: audience.to_string(),
                    held: held.clone(),
                    given,
                    cap,
                });
            }
            (_, given) => {
                let why = checked.why.unwrap_or(Why::NotAnInstruction);
                raise(db, user_id, held, given.is_some(), Raised::Stopped(why)).await;
            }
        }
    }
    Ok(runs)
}

/// Writes `held` at `status`, as hers; whether it was newly written now.
async fn settle(
    db: &DatabaseConnection,
    user_id: i32,
    audience: &str,
    held: &Held,
    status: IntentStatus,
) -> bool {
    let Some(pen) = crate::services::her_log::pen::think() else {
        return false;
    };
    let row = intent_store::settle_row(held, status);
    match write::write(db, user_id, audience, &[row], Some(&pen)).await {
        Ok(written) => !written.rows.is_empty(),
        Err(error) => {
            tracing::warn!(%error, "[memory-nodes] an intent not settled");
            false
        }
    }
}

/// Why her hand goes up.
enum Raised<'a> {
    /// The check: hers or theirs to say, or an instruction it stopped.
    Stopped(Why),
    /// The site closed the agent to them before it ran.
    Closed,
    /// It ran: whether it went through, and its answer.
    Ran { succeeded: bool, line: &'a str },
}

/// What she is writing to them about, and why now.
fn about(held: &Held, raised: &Raised) -> String {
    let text = &held.text;
    let theirs = held.origin == Some(Origin::Partner);
    match raised {
        Raised::Stopped(Why::NotAnInstruction) if theirs => {
            format!("{text} (they asked you earlier to bring this up now, and now is the time)")
        }
        Raised::Stopped(Why::NotAnInstruction) => {
            format!("{text} (you meant to bring this up about now)")
        }
        Raised::Stopped(Why::Revoked) | Raised::Closed => format!(
            "{text} (they asked you earlier to do this now; a permission it needs has changed \
             since, so you did not do it: tell them, and that they can ask again)"
        ),
        Raised::Stopped(Why::Identity) => format!(
            "{text} (they asked you earlier to do this now; who asked, or who you are, has \
             changed since, so you did not do it: tell them)"
        ),
        Raised::Stopped(Why::OtherDevice) => format!(
            "{text} (they asked you earlier to do this now, on another device, where alone it \
             runs; you did not do it here: tell them)"
        ),
        Raised::Ran {
            succeeded: true,
            line,
        } => format!("{text} (they asked you earlier to do this now; you did it: {line})"),
        Raised::Ran { line, .. } => format!(
            "{text} (they asked you earlier to do this now; you tried and it did not work: \
             {line})"
        ),
    }
}

fn why_word(raised: &Raised) -> &'static str {
    match raised {
        Raised::Stopped(Why::NotAnInstruction) => "not_an_instruction",
        Raised::Stopped(Why::OtherDevice) => "other_device",
        Raised::Stopped(Why::Identity) => "identity",
        Raised::Stopped(Why::Revoked) => "revoked",
        Raised::Closed => "closed",
        Raised::Ran { .. } => "ran",
    }
}

/// Her hand up about `held`, now. An instruction's outcome is kept unless
/// it ran (its own outcome is kept then).
async fn raise(
    db: &DatabaseConnection,
    user_id: i32,
    held: &Held,
    instructed: bool,
    raised: Raised<'_>,
) {
    let source = match raised {
        Raised::Ran { .. } => Source::WorkDone,
        _ => Source::Reach,
    };
    let thing = match raised {
        Raised::Ran { .. } => format!("intent|{}|done", held.id),
        _ => format!("intent|{}", held.id),
    };
    let went = reach::say_now(db, user_id, source, &thing, &about(held, &raised)).await;
    if went.is_none() {
        tracing::info!("[memory-nodes] what came due was not said");
    }
    if instructed && !matches!(raised, Raised::Ran { .. }) {
        let outcome = Outcome {
            way: "raised",
            line: why_word(&raised).to_string(),
            succeeded: None,
            session: None,
        };
        if let Err(error) = intent_store::record(db, user_id, &held.id, &outcome).await {
            tracing::warn!(%error, "[memory-nodes] an intent's outcome not kept");
        }
    }
}

/// Runs one instruction the check let run, keeps what came of it, marks
/// it done, and tells them.
pub(crate) async fn finish(db: &DatabaseConnection, to_run: ToRun) {
    let ToRun {
        user_id,
        audience,
        held,
        given,
        cap,
    } = to_run;
    let quote = intent_store::message_text(db, &given.message_ref)
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| held.text.clone());
    let ran = intents_run::run(
        db,
        user_id,
        &intents_run::work_input(&held.text, &quote),
        cap,
    )
    .await;
    // Forgotten while it ran (what it rested on went): nothing of it is
    // kept or told.
    if !matches!(
        intent_store::instruction(db, user_id, &held.id).await,
        Ok(Some(_))
    ) {
        return;
    }
    let Ran::Done {
        succeeded,
        line,
        session,
    } = ran
    else {
        raise(db, user_id, &held, true, Raised::Closed).await;
        return;
    };
    let outcome = Outcome {
        way: "ran",
        line: line.clone(),
        succeeded: Some(succeeded),
        session,
    };
    if let Err(error) = intent_store::record(db, user_id, &held.id, &outcome).await {
        tracing::warn!(%error, "[memory-nodes] an intent's outcome not kept");
    }
    // Done whatever came of it: she did the one thing she was told, or tried.
    let said = intent_store::held(db, user_id, &audience)
        .await
        .ok()
        .and_then(|all| all.into_iter().find(|kept| kept.id == held.id));
    if let Some(said) = said {
        settle(db, user_id, &audience, &said, IntentStatus::Done).await;
    }
    raise(
        db,
        user_id,
        &held,
        true,
        Raised::Ran {
            succeeded,
            line: &line,
        },
    )
    .await;
}
