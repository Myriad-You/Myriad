//! Trying to remember before answering (see `myriad_merope::remembering`).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use futures::FutureExt;
use futures::future::{BoxFuture, Shared};

use sea_orm::DatabaseConnection;

use super::call::{self, Voice};
use super::store::{self, Recalled};
use crate::services::agent::memory::unified::{Audience, Priming};
use myriad_merope::remembering::{Cues, SCHEMA_NAME, input, merged, parse, schema, system};

/// Past this she answers from what their words alone brought back.
const THINK_WITHIN: Duration = Duration::from_secs(3);
/// What she goes through when answering needs all of it.
pub const THOROUGH: usize = 16;

/// What she would try to remember before answering `message`; billed to
/// `owner`.
async fn think(owner: i32, message: String) -> Option<Cues> {
    let today = chrono::Local::now().format("%Y-%m-%d").to_string();
    let raw = call::Ask::new(Voice::Judge, owner, "remembering")
        .within(THINK_WITHIN)
        .json_raw(
            &system(),
            &input(&message, None, &today),
            SCHEMA_NAME,
            &schema(),
        )
        .await
        .ok()?;
    parse(&raw)
}

type Thinking = Shared<BoxFuture<'static, Option<Cues>>>;

/// Trying to remember that began as a message came, by person: the
/// message, and the thinking.
static THINKING: LazyLock<Mutex<HashMap<i32, (String, Thinking)>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));
const PEOPLE_KEPT: usize = 4096;

/// Start trying to remember as soon as their message comes, alongside how
/// it lands with her, so the reply does not wait for it after.
pub fn begin(owner: i32, message: &str) {
    let thinking: Thinking = think(owner, message.to_string()).boxed().shared();
    let running = thinking.clone();
    super::background::spawn("remembering", async move {
        running.await;
    });
    if let Ok(mut all) = THINKING.lock() {
        if all.len() >= PEOPLE_KEPT {
            all.clear();
        }
        all.insert(owner, (message.to_string(), thinking));
    }
}

/// What she tried to remember before answering `message`: begun when it
/// came, or now if it was not.
pub async fn cues(owner: i32, message: &str) -> Option<Cues> {
    let begun = THINKING
        .lock()
        .ok()
        .and_then(|mut all| all.remove(&owner))
        .filter(|(said, _)| said == message);
    match begun {
        Some((_, thinking)) => tokio::time::timeout(THINK_WITHIN, thinking)
            .await
            .ok()
            .flatten(),
        None => think(owner, message.to_string()).await,
    }
}

/// Recall for `message` as a chat turn does, and for each cue besides,
/// merged; `limit` lines, or `THOROUGH` when answering needs all of it.
/// What their words brought to mind comes from their words only, and only
/// their words move what stays on her mind.
#[allow(clippy::too_many_arguments)]
pub async fn recall_with(
    db: &DatabaseConnection,
    user_id: i32,
    present: &Audience,
    message: &str,
    cues: Option<&Cues>,
    limit: usize,
    priming: &Priming,
    breadth: f64,
) -> Result<(Recalled, Priming), anyhow::Error> {
    let limit = match cues {
        Some(cues) if cues.thorough => THOROUGH.max(limit),
        _ => limit,
    };
    let (first, next) = store::recall_remembered_split(
        db,
        user_id,
        present,
        Some(message),
        limit,
        priming,
        breadth,
    )
    .await?;
    let Some(cues) = cues.filter(|cues| !cues.cues.is_empty()) else {
        return Ok((first, next));
    };
    let mut tries = vec![first.named];
    for cue in &cues.cues {
        let (found, _) = store::recall_remembered_split(
            db,
            user_id,
            present,
            Some(cue),
            limit,
            &Priming::default(),
            0.0,
        )
        .await?;
        tries.push(found.named);
    }
    let room = limit.saturating_sub(first.brought_to_mind.len()).max(1);
    Ok((
        Recalled {
            named: merged(&tries, room),
            brought_to_mind: first.brought_to_mind,
        },
        next,
    ))
}
