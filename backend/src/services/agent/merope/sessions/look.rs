//! Her careful look at one session that asks the owner, or failed: the
//! material gathered and numbered, asked of the model the chat uses with
//! time to think, her answer held to the rules ([`super::judge`]), and what
//! comes of it kept on the task and, when she would, said.
//!
//! One look per question: the same session asking the same again is not
//! looked at twice. A look fails over once (the model was slow or erred,
//! or her grounds named what she was not shown); a second failure keeps
//! only the card, unsaid.

use std::collections::{HashMap, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use myriad_her_link::now::{AskedBy, Change, CheckKind, CheckState, Session, WaitingOn};
use myriad_nodes::sessions;
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use super::judge::{self, Judgment, Material, Read, Verdict};
use super::{Picture, audience};
use crate::services::agent::merope::call::{Ask, Failure, Voice};
use crate::services::agent::merope::memory_nodes::{
    prepare::{Author, RowIn},
    read, write,
};

/// The look, as the model call and its bill are named.
const LOOK: &str = "session_look";
/// How long the model has for one look.
const LOOK_TIMEOUT: Duration = Duration::from_secs(30);
/// How long before the one try again.
const RETRY_AFTER: Duration = Duration::from_secs(5);
/// What she recalls of how the owner decided before.
const MEMORIES: usize = 8;
/// How many of the repository's other sessions she is shown.
const OTHERS: usize = 8;

/// The looks under way or kept, by owner, device, grant, session and what
/// it asked (a digest): one each. A look that kept nothing is let go, and a
/// session that moved on lets go of its own.
static LOOKED: LazyLock<Mutex<HashSet<(i32, String, String, String, String)>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

/// Looks a dropped grant must not finish, by owner, device and grant.
static DROPPED: LazyLock<Mutex<HashMap<(i32, String, String), ()>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// What this change asks of the owner, as one key: the question, or the
/// failure.
fn asking(session: &Session) -> String {
    let words = session.from_agent.as_deref().unwrap_or_default();
    let checks: Vec<String> = session
        .checks
        .iter()
        .map(|check| format!("{:?}{:?}", check.kind, check.state))
        .collect();
    let asking = format!(
        "{:?}\u{1f}{words}\u{1f}{}",
        session.change,
        checks.join(",")
    );
    myriad_persona_sync::canonical::sha256_hex(asking.as_bytes())[..32].to_string()
}

/// Whether what this change waits on is the owner: it asks them, or says
/// its next step is theirs. Such a one is never let be.
fn waits_on_owner(session: &Session) -> bool {
    session.change == Change::Asked || session.waiting_on == Some(WaitingOn::Owner)
}

/// Looks at `picture` in the background, once per question.
pub(super) fn spawn(db: DatabaseConnection, owner: i32, picture: Picture) {
    let key = (
        owner,
        picture.device.clone(),
        picture.session.grant.clone(),
        picture.session.session.clone(),
        asking(&picture.session),
    );
    if !LOOKED
        .lock()
        .map(|mut looked| looked.insert(key.clone()))
        .unwrap_or(false)
    {
        return;
    }
    crate::services::agent::merope::background::spawn("a session's look", async move {
        let kept = match look(&db, owner, &picture).await {
            Ok(kept) => kept,
            Err(error) => {
                tracing::warn!(%error, "[Merope] a look at a session failed");
                false
            }
        };
        // Nothing kept: the same question may be looked at when it comes again.
        if !kept && let Ok(mut looked) = LOOKED.lock() {
            looked.remove(&key);
        }
    });
}

/// The session moved on from what it asked: what it asks next is looked at
/// afresh, even the same again.
pub(super) fn moved_on(owner: i32, device: &str, grant: &str, session: &str) {
    if let Ok(mut looked) = LOOKED.lock() {
        looked.retain(|(who, from, under, which, _)| {
            !(*who == owner && from == device && under == grant && which == session)
        });
    }
}

/// Nothing more of a dropped grant is looked at or kept.
pub(super) fn cancel(owner: i32, device: &str, grant: &str) {
    if let Ok(mut looked) = LOOKED.lock() {
        looked.retain(|(who, from, under, _, _)| {
            !(*who == owner && from == device && under == grant)
        });
    }
    if let Ok(mut dropped) = DROPPED.lock() {
        dropped.insert((owner, device.to_string(), grant.to_string()), ());
    }
}

/// Whether the owner dropped `picture`'s grant.
pub(super) fn dropped(owner: i32, picture: &Picture) -> bool {
    grant_dropped(owner, &picture.device, &picture.session.grant)
}

/// Whether the owner dropped `device`'s `grant` (in this process).
pub(super) fn grant_dropped(owner: i32, device: &str, grant: &str) -> bool {
    DROPPED
        .lock()
        .is_ok_and(|dropped| dropped.contains_key(&(owner, device.to_string(), grant.to_string())))
}

fn check_word(kind: CheckKind) -> &'static str {
    match kind {
        CheckKind::Test => "tests",
        CheckKind::Build => "build",
        CheckKind::Other => "check",
    }
}

fn checks_line(session: &Session) -> String {
    let mut seen = Vec::new();
    let mut parts = Vec::new();
    for check in &session.checks {
        if seen.contains(&check.kind) {
            continue;
        }
        seen.push(check.kind);
        let state = match check.state {
            CheckState::Running => "running",
            CheckState::Passed => "passed",
            CheckState::Failed => "failed",
            CheckState::Ended => "ended, outcome not known",
        };
        parts.push(format!("{} {state}", check_word(check.kind)));
    }
    parts.join("; ")
}

fn now_line(session: &Session) -> String {
    let change = match session.change {
        Change::Started => "started",
        Change::Asked => "stopped and asks the owner",
        Change::Finished => "finished a turn, nothing to answer",
        Change::Failed => "failed (kept erring, or its checks failed)",
        Change::Quiet => "quiet for long",
    };
    let waiting = match session.waiting_on {
        Some(WaitingOn::Owner) => "; waits on the owner",
        Some(WaitingOn::Session) => "; waits to report back to the session that sent it on",
        Some(WaitingOn::Checks) => "; waits on its tests or builds",
        Some(WaitingOn::Program) => "; waits on another program",
        Some(WaitingOn::Unknown) | None => "",
    };
    format!("{change}{waiting}")
}

/// Where a session runs: its repository's name, or its folder.
pub(super) fn place(session: &Session) -> &str {
    match (&session.remote, &session.folder) {
        (Some(remote), _) => remote.rsplit('/').next().unwrap_or(remote),
        (None, Some(folder)) => folder,
        (None, None) => "",
    }
}

fn title(session: &Session) -> String {
    let repo = place(session);
    match &session.title {
        Some(title) => format!("{title} ({} in {repo})", session.tool),
        None => format!("a {} session in {repo}", session.tool),
    }
}

/// What she is shown of `picture`, numbered: the session itself, the
/// others of its repository, how the owner decided before, and what she
/// made of it last time.
pub(super) fn material(
    picture: &Picture,
    others: &[Picture],
    memories: &[(String, String)],
    before: Option<&str>,
) -> Material {
    let session = &picture.session;
    let reference = sessions::session_ref(&picture.device, &session.grant, &session.session);
    let mut material = Material::default();
    material.add("the session", title(session), &reference);
    let given = match (session.asked_by, &session.asked_by_title) {
        (Some(AskedBy::Owner), _) => "the owner, in person".to_string(),
        (Some(AskedBy::Session), Some(title)) => {
            format!("another of the owner's sessions, «{title}», sent it on; it reports back there")
        }
        (Some(AskedBy::Session), None) => {
            "another of the owner's sessions sent it on; it reports back there".to_string()
        }
        (Some(AskedBy::None) | None, _) => "not known".to_string(),
    };
    material.add("who gave it this work", given, &reference);
    if let Some(words) = &session.from_peer {
        material.add(
            "what the session that sent it on told it",
            words.clone(),
            &reference,
        );
    }
    if let Some(words) = &session.from_owner {
        material.add("what the owner last told it", words.clone(), &reference);
    }
    if let Some(words) = &session.from_agent {
        material.add("what it last said or asked", words.clone(), &reference);
    }
    if !session.touched.is_empty() {
        material.add("files it changed", session.touched.join(", "), &reference);
    }
    material.add("its tests and builds", checks_line(session), &reference);
    material.add("where it stands", now_line(session), &reference);
    for other in others
        .iter()
        .filter(|other| {
            other.session.remote == session.remote
                && (session.remote.is_some()
                    || (other.device == picture.device && other.session.folder == session.folder))
                && !(other.device == picture.device
                    && other.session.grant == session.grant
                    && other.session.session == session.session)
        })
        .take(OTHERS)
    {
        let shared: Vec<&String> = other
            .session
            .touched
            .iter()
            .filter(|path| session.touched.contains(path))
            .collect();
        let mut text = format!("{}: {}", title(&other.session), now_line(&other.session));
        if !shared.is_empty() {
            text.push_str(&format!(
                "; also changed {}",
                shared
                    .iter()
                    .map(|path| path.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        material.add(
            "another session in this repository",
            text,
            &sessions::session_ref(&other.device, &other.session.grant, &other.session.session),
        );
    }
    for (id, text) in memories {
        material.add(
            "what you know of the owner (how they decided before)",
            text.clone(),
            &format!("memory:{id}"),
        );
    }
    if let Some(before) = before {
        material.add(
            "what you made of this session last time",
            before,
            &reference,
        );
    }
    material
}

/// What she recalls of the owner for this session: id and text.
async fn memories(db: &DatabaseConnection, owner: i32, session: &Session) -> Vec<(String, String)> {
    let query = format!(
        "{} {}",
        session.title.as_deref().unwrap_or_default(),
        session.from_agent.as_deref().unwrap_or_default()
    );
    let present = crate::services::agent::memory::unified::Audience::private(owner);
    match crate::services::agent::memory::unified::recall_primed(
        db,
        owner,
        &present,
        Some(query.trim()).filter(|query| !query.is_empty()),
        &crate::services::agent::memory::unified::MemoryKind::ABOUT_PERSON,
        MEMORIES,
        &Default::default(),
        1.0,
    )
    .await
    {
        Ok((recalled, _)) => recalled
            .iter()
            .map(|record| {
                (
                    record.id.clone(),
                    crate::services::agent::merope::store::as_known(record),
                )
            })
            .filter(|(_, text)| !text.is_empty())
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "[Merope] no memories for a session's look");
            Vec::new()
        }
    }
}

/// The view she keeps on the task for what it asked: one per question.
fn view_id(session: &Session) -> String {
    let digest = myriad_persona_sync::canonical::sha256_hex(asking(session).as_bytes());
    format!("look-{}", &digest[..16])
}

/// Her answer, asked once more when the first did not come or did not
/// hold; none when neither did.
async fn ask(owner: i32, soul: &str, material: &Material) -> Option<Judgment> {
    let call = Ask::new(Voice::Chat, owner, LOOK)
        .within(LOOK_TIMEOUT)
        .billed_as("sessions");
    let system = judge::system(soul);
    let schema = judge::schema();
    let mut prompt = judge::prompt(material);
    for attempt in 0..2 {
        match call.json_raw(&system, &prompt, LOOK, &schema).await {
            Ok(raw) => match judge::read(&raw, material) {
                Read::Good(judgment) => return Some(judgment),
                Read::BadGrounds => prompt = judge::again(material),
                Read::Unreadable => {}
            },
            // Not while her mind is elsewhere, nor with no model: no retry.
            Err(Failure::Unavailable) => return None,
            Err(error) => {
                tracing::info!(%error, attempt, "[Merope] a session's look did not come");
                if attempt == 0 {
                    tokio::time::sleep(RETRY_AFTER).await;
                }
            }
        }
    }
    None
}

/// Whether what she made of it was kept.
async fn look(
    db: &DatabaseConnection,
    owner: i32,
    picture: &Picture,
) -> Result<bool, anyhow::Error> {
    let session = &picture.session;
    let asked = waits_on_owner(session);
    let audience = audience::private_talk(owner, Some(owner));
    let task_key = sessions::task_key(&picture.device, &session.grant, &session.session);
    let task = myriad_nodes::node_id(&audience, &task_key)?;
    let view = view_id(session);
    let node = read::node(db, owner, &audience, &task).await?;
    let before = node
        .as_ref()
        .and_then(|node| node.get("views"))
        .and_then(Value::as_object)
        .and_then(|views| {
            views
                .values()
                .filter_map(|held| {
                    Some((
                        held["at"].as_i64()?,
                        held["value"]["text"].as_str()?.to_string(),
                    ))
                })
                .max()
        })
        .map(|(_, text)| text);
    let others = super::pictures(db, owner).await;
    let memories = memories(db, owner, session).await;
    let material = material(picture, &others, &memories, before.as_deref());
    let soul = crate::services::agent::merope::resolve_speaking_soul()
        .await
        .unwrap_or_default();
    let judgment = match ask(owner, &soul, &material).await {
        Some(judgment) => judge::settle(judgment, asked),
        // Nothing with grounds: what waits on them still has its card.
        None if asked => judge::card_only(&now_line(session)),
        None => return Ok(false),
    };
    if dropped(owner, picture) {
        return Ok(false);
    }
    // What she made of it is hers: kept only while her mind is here.
    let Some(pen) = crate::services::her_log::pen::think() else {
        return Ok(false);
    };
    let base: Vec<String> = node
        .as_ref()
        .and_then(|node| node["views"].get(&view))
        .and_then(|held| held["base"].as_array())
        .map(|ids| {
            ids.iter()
                .filter_map(|id| id.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default();
    let grant = sessions::grant_ref(&picture.device, &session.grant);
    let mut because = material.evidence(&judgment.because);
    if because.is_empty() {
        because = vec![sessions::session_ref(
            &picture.device,
            &session.grant,
            &session.session,
        )];
    }
    // The grant it came under, so a drop finds it as every row of it.
    because.push(grant.clone());
    because.sort();
    because.dedup();
    let text = judge::card_text(&judgment);
    // On the task as it was made: kept on the site's side while the task is.
    let sync = node
        .as_ref()
        .and_then(sessions::created_sync)
        .unwrap_or_else(|| myriad_nodes::Sync::Local(crate::services::her_log::NODE.to_string()));
    let row = RowIn {
        id: myriad_nodes::entries::row_id_from(
            "node.view_changed",
            &task,
            &view,
            &session.occurred_at.to_string(),
        ),
        node: task.clone(),
        kind: "node.view_changed".into(),
        body: json!({
            "version": myriad_nodes::BODY_VERSION, "sync": sync, "saidBy": ["her"],
            "base": base, "view": view, "text": text, "because": because,
        }),
        author: Author::Her,
    };
    write::write(db, owner, &audience, &[row], Some(&pen)).await?;
    // Dropped while it was written: it goes with the rest of the grant.
    if dropped(owner, picture) {
        drop(pen);
        super::forget_grant(db, owner, &picture.device, &session.grant).await?;
        return Ok(true);
    }
    if let Some(line) = judgment
        .line
        .as_ref()
        .filter(|_| judge::has_card(&judgment))
    {
        say(db, owner, &grant, &task, &view, line, &judgment);
        // Dropped as it was made: taken back, wherever it is by now.
        if dropped(owner, picture) {
            crate::services::agent::consciousness::withdraw_speak_intents(owner, &grant);
        }
    }
    Ok(true)
}

/// What she would tell them of it, to be said in person when it fits.
fn say(
    db: &DatabaseConnection,
    owner: i32,
    grant: &str,
    task: &str,
    view: &str,
    line: &str,
    judgment: &Judgment,
) {
    use crate::services::agent::consciousness::{
        EventUrgency, enqueue_speak_intent, new_speak_intent,
    };
    let mut intent = new_speak_intent(
        owner,
        // Under its grant, for a drop to take back what is not said yet.
        format!("{grant}/{task}:{view}"),
        crate::services::agent::merope::gates::PEER_AGENT_EVENT.to_string(),
        line.to_string(),
        if judgment.verdict == Verdict::Decide {
            EventUrgency::Soon
        } else {
            EventUrgency::Normal
        },
        None,
    );
    let mut happened = format!("{}：{}", judgment.verdict.label(), judgment.about);
    if !judgment.suggestion.is_empty() {
        happened.push_str(&format!("\n{}", judgment.suggestion));
    }
    intent.happened = Some(happened);
    enqueue_speak_intent(intent);
    let db = db.clone();
    crate::services::agent::merope::background::spawn("speak intents", async move {
        crate::services::agent::merope::ingest::tick_speak_intents(db).await;
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn picture(session: &str, change: &str, touched: &[&str]) -> Picture {
        Picture {
            device: "dev_mac".into(),
            session: myriad_her_link::now::read_session(&json!({
                "grant": "g_0123456789",
                "session": session,
                "change": change,
                "occurredAt": 1_760_000_000_000_i64,
                "tool": "coding-agent",
                "remote": "github.com/me/repo",
                "title": "Split the store",
                "fromOwner": "split the store and commit it",
                "fromAgent": "Committed locally. Push now? (push / wait)",
                "touched": touched,
                "checks": [{"kind": "test", "state": "passed", "at": 1}],
            }))
            .unwrap(),
        }
    }

    #[test]
    fn she_is_shown_the_whole_session_and_what_is_around_it() {
        let asking = picture("s-1", "asked", &["src/a.rs", "src/b.rs"]);
        let other = picture("s-2", "quiet", &["src/b.rs"]);
        let material = material(
            &asking,
            &[asking.clone(), other],
            &[("m1".into(), "pushes need their own word".into())],
            Some("你点头就行：上次也问过"),
        );
        let numbered = material.numbered();
        for line in [
            "[1] the session: Split the store (coding-agent in repo)",
            "[2] who gave it this work: not known",
            "[3] what the owner last told it: split the store and commit it",
            "[4] what it last said or asked: Committed locally. Push now? (push / wait)",
            "[5] files it changed: src/a.rs, src/b.rs",
            "[6] its tests and builds: tests passed",
            "[7] where it stands: stopped and asks the owner",
            "[8] another session in this repository: Split the store (coding-agent in repo): quiet for long; also changed src/b.rs",
            "[9] what you know of the owner (how they decided before): pushes need their own word",
            "[10] what you made of this session last time: 你点头就行：上次也问过",
        ] {
            assert!(numbered.contains(line), "{line}\n{numbered}");
        }
        assert_eq!(material.pieces.len(), 10, "itself is not another session");
        let evidence = material.evidence(&[4, 8, 9]);
        assert_eq!(evidence.len(), 3);
        assert!(evidence.contains(&"memory:m1".to_string()));
    }

    #[test]
    fn one_question_is_one_look() {
        let one = picture("s-1", "asked", &[]);
        let mut again = one.clone();
        again.session.occurred_at += 1;
        assert_eq!(asking(&one.session), asking(&again.session));
        assert_eq!(view_id(&one.session), view_id(&again.session));
        let mut other = one.clone();
        other.session.from_agent = Some("Deploy now?".into());
        assert_ne!(view_id(&one.session), view_id(&other.session));
    }
}
