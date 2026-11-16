//! Her private chat with someone, nearest by meaning: on each private turn
//! of the site owner's, the few messages of the whole talk nearest what they
//! just said, whole, beside what scrolling back by words found
//! ([`super::remembering::look_back`]), in the one section for both.
//!
//! - **Which lines.** Only what a chat turn of theirs may look back
//!   through ([`super::chat_lines`]), as scrolling back by words reads.
//! - **Forgetting.** Each line's vector is kept beside it
//!   (`agent_message_embeddings`) and goes with it by its foreign key:
//!   whatever deletes a line takes its vector in the same statement, and a
//!   vector is only read joined to a line still there.
//! - **Worked out where she is, from the lines.** Vectors are derived data:
//!   made from the lines on the end she lives on and never synced; an end
//!   that becomes where she is builds them again from the lines it has
//!   ([`rebuild`]). A line forgotten anywhere is deleted here by the log,
//!   and its vector with it.
//! - **No model call while she answers.** What they said is embedded for
//!   recall already (the meaning cache); lines are embedded behind the
//!   reply ([`fill_behind`]), and a line without a vector yet is not found.

use std::collections::{HashMap, HashSet};

use super::chat_lines::{Line, private_lines};

use myriad_merope::remembering::Who;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};

use crate::services::agent::memory::meaning;

/// Messages nearest what they said brought on each turn.
pub(crate) const NEAREST: usize = 3;

/// The newest messages of a talk looked through, as scrolling back does.
const LATEST: i64 = 3000;

/// Lines given a vector in one request behind a reply, and about this many
/// characters at most.
const FILL_PER_ASK: usize = 63;
const FILL_CHARS: usize = 60_000;

/// How long a request for vectors behind the reply may take.
const FILL_WITHIN: std::time::Duration = std::time::Duration::from_secs(60);

/// A line as she is shown it, and as it is embedded: its lines joined, up
/// to what a message of hers read whole may be
/// ([`super::remembering::READ_WHOLE_CHARS`]).
fn shown(content: &str) -> String {
    let whole = content
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join(" / ");
    let most = super::remembering::READ_WHOLE_CHARS;
    let mut read: String = whole.chars().take(most).collect();
    if whole.chars().count() > most {
        read.push_str(" …");
    }
    read
}

/// Vectors read back, by line id, with the model and the text they were
/// made from: a turn reads the vectors of the whole talk, and they do not
/// change. Only ever used for a line read from the database on that turn,
/// so a line deleted is never found by what is held here.
#[cfg(not(test))]
static HELD: std::sync::LazyLock<
    std::sync::Mutex<HashMap<i32, (String, String, std::sync::Arc<Vec<f32>>)>>,
> = std::sync::LazyLock::new(Default::default);

/// Lines whose vectors are held at most (one talk read back is 3,000).
#[cfg(not(test))]
const HELD_AT_MOST: usize = 20_000;

#[cfg(not(test))]
fn held(model: &str, wanted: &HashMap<i32, String>) -> HashMap<i32, std::sync::Arc<Vec<f32>>> {
    let Ok(held) = HELD.lock() else {
        return HashMap::new();
    };
    wanted
        .iter()
        .filter_map(|(id, digest)| {
            let (was_model, was_digest, vector) = held.get(id)?;
            (was_model == model && was_digest == digest).then(|| (*id, vector.clone()))
        })
        .collect()
}

#[cfg(test)]
fn held(_: &str, _: &HashMap<i32, String>) -> HashMap<i32, std::sync::Arc<Vec<f32>>> {
    HashMap::new()
}

#[cfg(not(test))]
fn hold(model: &str, id: i32, digest: &str, vector: std::sync::Arc<Vec<f32>>) {
    if let Ok(mut held) = HELD.lock() {
        if held.len() >= HELD_AT_MOST {
            held.clear();
        }
        held.insert(id, (model.to_string(), digest.to_string(), vector));
    }
}

#[cfg(test)]
fn hold(_: &str, _: i32, _: &str, _: std::sync::Arc<Vec<f32>>) {}

/// The vectors kept under `model` for `lines`, where the text is still the
/// one embedded, by line id.
async fn kept(
    db: &DatabaseConnection,
    model: &str,
    lines: &[Line],
) -> HashMap<i32, std::sync::Arc<Vec<f32>>> {
    if lines.is_empty() {
        return HashMap::new();
    }
    let digests: HashMap<i32, String> = lines
        .iter()
        .map(|line| (line.id, meaning::digest(&shown(&line.content))))
        .collect();
    let mut found = held(model, &digests);
    let ids: Vec<i32> = lines
        .iter()
        .map(|line| line.id)
        .filter(|id| !found.contains_key(id))
        .collect();
    if ids.is_empty() {
        return found;
    }
    let Ok(rows) = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT e.message_id, e.digest, e.vector FROM agent_message_embeddings e \
             JOIN agent_messages m ON m.id = e.message_id \
             WHERE e.model = $1 AND e.message_id = ANY($2)",
            [model.into(), ids.into()],
        ))
        .await
    else {
        return found;
    };
    for row in &rows {
        let (Ok(id), Ok(digest), Ok(bytes)) = (
            row.try_get::<i32>("", "message_id"),
            row.try_get::<String>("", "digest"),
            row.try_get::<Vec<u8>>("", "vector"),
        ) else {
            continue;
        };
        if digests.get(&id) != Some(&digest) {
            continue;
        }
        let Some(vector) = meaning::from_kept(&bytes) else {
            continue;
        };
        let vector = std::sync::Arc::new(vector);
        hold(model, id, &digest, vector.clone());
        found.insert(id, vector);
    }
    found
}

/// The `wanted` lines of `user_id`'s private chat nearest by meaning what
/// they just said (`asked`), oldest first, each as she is shown it; not
/// `asked` itself, nor, with `in_front`, the talk still in front of her
/// (found again, it would read as something said before). None when there
/// is no embedding model, what was asked could not be embedded in time, or
/// no line has a vector yet.
pub(crate) async fn nearest(
    db: &DatabaseConnection,
    user_id: i32,
    asked: &str,
    wanted: usize,
    in_front: bool,
) -> Vec<(String, Who, String)> {
    if wanted == 0 || asked.trim().is_empty() {
        return Vec::new();
    }
    let Some(model) = meaning::model_in_use().await else {
        return Vec::new();
    };
    let front = if in_front {
        super::remembering::in_front(db, user_id).await
    } else {
        HashSet::new()
    };
    let lines: Vec<Line> = private_lines(db, user_id, LATEST)
        .await
        .into_iter()
        .filter(|line| !front.contains(&line.id) && line.content.trim() != asked.trim())
        .collect();
    let vectors = kept(db, &model, &lines).await;
    if vectors.is_empty() {
        return Vec::new();
    }
    let Some(query) = meaning::embed_asked(asked).await else {
        return Vec::new();
    };
    let mut scored: Vec<(f64, usize)> = lines
        .iter()
        .enumerate()
        .filter_map(|(index, line)| Some((meaning::cosine(&query, vectors.get(&line.id)?), index)))
        .collect();
    scored.sort_by(|left, right| right.0.total_cmp(&left.0).then(left.1.cmp(&right.1)));
    let mut chosen: Vec<usize> = scored
        .into_iter()
        .take(wanted)
        .map(|(_, index)| index)
        .collect();
    chosen.sort_unstable();
    chosen
        .into_iter()
        .map(|index| {
            let line = &lines[index];
            let who = if line.hers { Who::You } else { Who::They };
            (
                line.at.format("%Y-%m-%d").to_string(),
                who,
                shown(&line.content),
            )
        })
        .collect()
}

/// Requests for vectors one [`rebuild`] makes at most: a whole talk read
/// back ([`LATEST`]) in requests of [`FILL_PER_ASK`], and a little over.
const REBUILD_ASKS: usize = LATEST as usize / FILL_PER_ASK + 2;

/// Lines (by model, id and the text embedded) the model in use gave no
/// vector, so not asked again while the process runs: a line it cannot
/// embed would otherwise be asked for on every turn.
static NOT_GIVEN: std::sync::LazyLock<std::sync::Mutex<HashSet<(String, i32, String)>>> =
    std::sync::LazyLock::new(Default::default);

fn not_given(model: &str, id: i32, digest: &str) -> bool {
    NOT_GIVEN
        .lock()
        .is_ok_and(|not_given| not_given.contains(&(model.to_string(), id, digest.to_string())))
}

/// What one [`fill`] did: lines given a vector, how many were still
/// without one (and not left as given none) before it asked, and how many
/// it left as given none ([`NOT_GIVEN`]).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Filled {
    pub(crate) given: usize,
    pub(crate) missing: usize,
    pub(crate) not_given: usize,
}

/// Gives a vector to up to [`FILL_PER_ASK`] lines of `user_id`'s private
/// chat still without one under the model in use, newest first, in one
/// request. A line the model gives no vector (an empty one) is left out
/// from then on in this process ([`NOT_GIVEN`]).
pub(crate) async fn fill(db: &DatabaseConnection, user_id: i32) -> Filled {
    let Some(model) = meaning::model_in_use().await else {
        return Filled::default();
    };
    let lines = private_lines(db, user_id, LATEST).await;
    let kept = kept(db, &model, &lines).await;
    let without: Vec<(i32, String)> = lines
        .iter()
        .rev()
        .filter(|line| !kept.contains_key(&line.id))
        .map(|line| (line.id, shown(&line.content)))
        // Nothing to embed is never asked for (`private_lines` leaves out
        // what is empty; this holds whatever it becomes as shown).
        .filter(|(id, text)| {
            !text.trim().is_empty() && !not_given(&model, *id, &meaning::digest(text))
        })
        .collect();
    let mut filled = Filled {
        missing: without.len(),
        ..Filled::default()
    };
    let mut missing: Vec<(i32, String)> = Vec::new();
    let mut chars = 0;
    for (id, text) in without {
        let length = text.chars().count();
        if missing.len() >= FILL_PER_ASK || (!missing.is_empty() && chars + length > FILL_CHARS) {
            break;
        }
        chars += length;
        missing.push((id, text));
    }
    if missing.is_empty() {
        return filled;
    }
    let texts: Vec<String> = missing.iter().map(|(_, text)| text.clone()).collect();
    let Some((model, vectors)) = meaning::embed_to_keep(&texts, FILL_WITHIN).await else {
        return filled;
    };
    for ((id, text), vector) in missing.iter().zip(vectors) {
        // No vector (or one that could not be read back) is not kept, and
        // the line is not asked for again.
        if vector.is_empty() || vector.iter().any(|value| !value.is_finite()) {
            if let Ok(mut not_given) = NOT_GIVEN.lock() {
                not_given.insert((model.clone(), *id, meaning::digest(text)));
            }
            filled.not_given += 1;
            continue;
        }
        // A line deleted meanwhile takes nothing: the row it would rest on
        // is gone.
        let stored = db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO agent_message_embeddings (message_id, model, digest, vector, created_at) \
                 SELECT $1, $2, $3, $4, $5::timestamptz \
                 WHERE EXISTS (SELECT 1 FROM agent_messages WHERE id = $1) \
                 ON CONFLICT (message_id) DO UPDATE SET model = EXCLUDED.model, \
                 digest = EXCLUDED.digest, vector = EXCLUDED.vector, created_at = EXCLUDED.created_at",
                [
                    (*id).into(),
                    model.clone().into(),
                    meaning::digest(text).into(),
                    meaning::kept_bytes(&vector).into(),
                    crate::clock::now().into(),
                ],
            ))
            .await;
        match stored {
            Ok(done) if done.rows_affected() > 0 => filled.given += 1,
            Ok(_) => {}
            Err(error) => tracing::warn!(%error, "[chat-near] a line's vector not kept"),
        }
    }
    filled
}

/// Gives every line of `user_id`'s private chat a vector, as many
/// requests as it takes: what an end that becomes where she is runs, the
/// lines it has being all it needs. How many were given one. It stops when
/// none is left without, at a request that gives none, at one after which
/// no fewer are left without (whatever it says it stored), and after
/// [`REBUILD_ASKS`] requests. Each rebuild leaves one line of counts only
/// ([`told_rebuilt`]).
pub(crate) async fn rebuild(db: &DatabaseConnection, user_id: i32) -> usize {
    let mut done = Rebuilt::default();
    let mut before: Option<usize> = None;
    let mut stopped = Stopped::MostAsks;
    for _ in 0..REBUILD_ASKS {
        let filled = fill(db, user_id).await;
        done.asks += 1;
        done.not_given += filled.not_given;
        done.left = filled.missing;
        if filled.missing == 0 {
            stopped = Stopped::Done;
            break;
        }
        if before.is_some_and(|before| filled.missing >= before) {
            stopped = Stopped::NoFewer;
            break;
        }
        done.given += filled.given;
        done.left = filled
            .missing
            .saturating_sub(filled.given + filled.not_given);
        if filled.given == 0 {
            stopped = Stopped::NoneStored;
            break;
        }
        before = Some(filled.missing);
    }
    told_rebuilt(&done, stopped);
    done.given
}

/// What one [`rebuild`] did, in counts.
#[derive(Debug, Default, Clone, Copy)]
struct Rebuilt {
    asks: usize,
    given: usize,
    left: usize,
    not_given: usize,
}

#[derive(Debug, Clone, Copy)]
enum Stopped {
    Done,
    NoneStored,
    NoFewer,
    MostAsks,
}

/// The one line a rebuild leaves: counts, never a line, a vector or whose.
fn told_rebuilt(done: &Rebuilt, stopped: Stopped) {
    let Rebuilt {
        asks,
        given,
        left,
        not_given,
    } = *done;
    match stopped {
        Stopped::Done => tracing::info!(
            asks,
            given,
            left,
            not_given,
            "[chat-near] rebuilt: none left without a vector"
        ),
        Stopped::NoneStored => tracing::info!(
            asks,
            given,
            left,
            not_given,
            "[chat-near] rebuilt: a request stored none"
        ),
        Stopped::NoFewer => tracing::warn!(
            asks,
            given,
            left,
            not_given,
            "[chat-near] rebuilt: no fewer left without after a request"
        ),
        Stopped::MostAsks => tracing::warn!(
            asks,
            given,
            left,
            not_given,
            "[chat-near] rebuilt: as many requests as one rebuild makes"
        ),
    }
}

/// The one line a private turn leaves about what it brought by meaning:
/// how many lines and characters, never what they say.
pub(crate) fn told_brought(near: &[(String, Who, String)]) {
    let chars: usize = near.iter().map(|(_, _, text)| text.chars().count()).sum();
    tracing::info!(
        lines = near.len(),
        chars,
        "[chat-near] a private turn brought lines by meaning"
    );
}

/// Whether lines are being given vectors behind a reply now.
static FILLING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// [`rebuild`] behind the reply: nothing waits on it. Most turns leave a
/// line or two without one, one request; on an end that has just become
/// where she is, the whole talk. Once at a time: a turn while it runs
/// leaves it to the one running.
pub(crate) fn fill_behind(db: DatabaseConnection, user_id: i32) {
    use std::sync::atomic::Ordering;
    if FILLING.swap(true, Ordering::AcqRel) {
        return;
    }
    /// Lets the next turn fill again however this one ends (a panic too).
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            FILLING.store(false, Ordering::Release);
        }
    }
    // On her managed runner: refused (at capacity, or shutting down), the
    // next turn tries again.
    if !super::background::spawn("chat lines' vectors", async move {
        let _done = Done;
        rebuild(&db, user_id).await;
    }) {
        FILLING.store(false, Ordering::Release);
    }
}

/// What scrolling back found (`found`), with each of `near` added once,
/// oldest first: a line it showed cut is shown whole in its place, one shown
/// whole is not shown again.
pub(crate) fn beside(
    found: Vec<(String, Who, String)>,
    near: Vec<(String, Who, String)>,
) -> Vec<(String, Who, String)> {
    // Letters and digits only: an excerpt joins lines with " / " and marks
    // what it cut with "…".
    let bare = |text: &str| -> String {
        text.chars()
            .filter(|c| c.is_alphanumeric())
            .flat_map(char::to_lowercase)
            .collect()
    };
    let mut all = found;
    for (date, who, text) in near {
        let whole = bare(&text);
        let shown = all.iter().position(|(was_on, was_who, was)| {
            *was_on == date && !matches!(was_who, Who::TheDay) && whole.contains(&bare(was))
        });
        match shown {
            Some(index) => all[index].2 = text,
            None => all.push((date, who, text)),
        }
    }
    all.sort_by(|left, right| left.0.cmp(&right.0));
    all
}

#[cfg(test)]
#[path = "chat_near_tests.rs"]
mod tests;
