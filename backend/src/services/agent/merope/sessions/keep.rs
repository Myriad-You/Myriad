//! What comes of her look, kept: each change's card, what she made of it
//! (beside her nodes, for her next look at the session, and in her inbox
//! for her upkeep), the one line she would say, and the look itself (what
//! she was shown and what the model answered, word for word) in the
//! owner's ledger for a week. A card's suggestion is that card's, never a
//! view of hers on a node.

use myriad_her_link::now::{Change, Session};
use myriad_nodes::sessions;
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::{Deserialize, Serialize};

use super::{Picture, audience};
use crate::services::agent::merope::memory_nodes::{Error, inbox};
use crate::services::runtime_registry::{self as registry, RegistryIdentity};
use myriad_nodes::session_look::{Judgment, Material, Verdict, card_text};

/// Her looks, word for word, by owner.
const LEDGER: &str = "agent_session_look";
/// What she made of each session last time she looked, by device, grant
/// and session: shown to her the next time. Kept as long as its picture.
const MADE: &str = "agent_session_made";
const MADE_SECS: i64 = 14 * 24 * 60 * 60;

/// What she made of one session, as she last said it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Made {
    device: String,
    grant: String,
    text: String,
}

fn made_id(picture: &Picture) -> String {
    let session = &picture.session;
    format!("{}/{}/{}", picture.device, session.grant, session.session)
}
/// How long a look is kept in it.
const LEDGER_SECS: i64 = 7 * 24 * 60 * 60;

/// What a change asks of the owner, as one key: what it is, what it said,
/// and how its checks stand.
pub(super) fn asking(session: &Session) -> String {
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

/// The view she keeps on the task for one change: one per question.
fn view_id(session: &Session) -> String {
    let digest = myriad_persona_sync::canonical::sha256_hex(asking(session).as_bytes());
    format!("look-{}", &digest[..16])
}

/// What she made of this session last time, if anything.
pub(super) async fn before(
    db: &DatabaseConnection,
    _owner: i32,
    picture: &Picture,
) -> Result<Option<String>, Error> {
    Ok(registry::get::<Made>(db, MADE, &made_id(picture))
        .await?
        .map(|made| made.text))
}

/// Every record of what she made of a dropped grant's sessions goes.
pub(super) async fn forget_made<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    device: &str,
    grant: &str,
) -> Result<(), Error> {
    for row in registry::list(db, MADE, Some(owner), None).await? {
        if serde_json::from_value::<Made>(row.payload)
            .is_ok_and(|made| made.device == device && made.grant == grant)
        {
            registry::delete(db, MADE, &row.record_id).await?;
        }
    }
    Ok(())
}

pub(super) fn task_id(owner: i32, picture: &Picture) -> Result<String, Error> {
    let audience = audience::private_talk(owner, Some(owner));
    let session = &picture.session;
    let task_key = sessions::task_key(&picture.device, &session.grant, &session.session);
    Ok(myriad_nodes::node_id(&audience, &task_key)?)
}

/// Keeps what she made of this change: for her next look at the session,
/// and in her inbox, where her upkeep judges whether it is worth a node and
/// which. False when it was not kept (her mind is not here). Nothing of it
/// goes on a node now: a card's suggestion is not a view of hers.
pub(super) async fn card(
    db: &DatabaseConnection,
    owner: i32,
    picture: &Picture,
    judgment: &Judgment,
    _material: &Material,
) -> Result<bool, Error> {
    // What she made of it is hers: kept only while her mind is here.
    if crate::services::her_log::pen::think().is_none() {
        return Ok(false);
    }
    made(db, owner, picture, judgment).await?;
    Ok(true)
}

/// What she made of a change, kept for her next look at the session and
/// put in her inbox: of every change she said something of, a card or not.
pub(super) async fn made(
    db: &DatabaseConnection,
    owner: i32,
    picture: &Picture,
    judgment: &Judgment,
) -> Result<(), Error> {
    let session = &picture.session;
    registry::put(
        db,
        MADE,
        &made_id(picture),
        RegistryIdentity {
            subject_id: Some(owner),
            owner_id: Some(owner),
            tapp_id: None,
            runtime_id: Some(&picture.device),
        },
        &Made {
            device: picture.device.clone(),
            grant: session.grant.clone(),
            text: card_text(judgment),
        },
        crate::clock::now().timestamp() + MADE_SECS,
    )
    .await?;
    let Some(item) = sessions::inbox_item(
        &picture.device,
        &session.grant,
        &session.session,
        &judgment.about,
    )?
    else {
        return Ok(());
    };
    let audience = audience::private_talk(owner, Some(owner));
    let day = crate::services::agent::merope::clock::today();
    inbox::put_session(db, owner, &audience, day, &item).await
}

/// Her line on each repository of the look (`repos`, of the changes in
/// `looked`): its progress card put before the owner, the day's counts its
/// facts, while her mind is here. It is the card's: nothing of it goes on
/// a node, so her line said again and again is no heap of versions.
pub(super) async fn progress(
    db: &DatabaseConnection,
    owner: i32,
    _looked: &[Picture],
    material: &Material,
    repos: &[myriad_nodes::session_look::RepoNow],
) {
    if repos.is_empty() || crate::services::her_log::pen::think().is_none() {
        return;
    }
    let audience = audience::private_talk(owner, Some(owner));
    let language = language(db, owner).await;
    let pictures = super::pictures(db, owner).await;
    let told: Vec<_> = pictures.iter().map(told).collect();
    let asking = tasks_asking(db, owner, &pictures).await;
    let since = {
        use crate::services::agent::merope::clock;
        clock::day_start(clock::today()).map_or(0, |start| start.timestamp_millis())
    };
    for said in repos {
        let Some(repo) = said
            .repo
            .checked_sub(1)
            .and_then(|at| material.repos.get(at))
        else {
            continue;
        };
        let made = match myriad_nodes::look_progress::progress(
            myriad_nodes::look_progress::ProgressInput {
                audience: audience.clone(),
                key: repo.key.clone(),
                name: repo.name.clone(),
                text: said.text.clone(),
                base: Vec::new(),
                // Only the card is kept, on the site.
                sync: myriad_nodes::Sync::Local(crate::services::her_log::NODE.to_string()),
                told: told.clone(),
                asking: asking.clone(),
                since,
                at: crate::clock::now().timestamp_millis(),
                language: language.clone(),
            },
        ) {
            Ok(made) => made,
            Err(error) => {
                tracing::warn!(%error, "[Merope] her line on a repository was not made");
                continue;
            }
        };
        if let Err(error) = crate::services::agent::merope::cards::put(db, owner, made.card).await {
            tracing::warn!(%error, "[Merope] a repository's progress card was not put");
        }
    }
}

/// The owner's language as they chose it on the site (`users.locale`),
/// as the program says a card's facts in it; one the core does not know
/// is said in English there.
async fn language(db: &DatabaseConnection, owner: i32) -> String {
    let found = db
        .query_one_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT locale FROM users WHERE id = $1",
            [owner.into()],
        ))
        .await
        .ok()
        .flatten()
        .and_then(|row| row.try_get::<Option<String>>("", "locale").ok().flatten());
    found
        .map(|locale| locale.trim().to_string())
        .filter(|locale| !locale.is_empty())
        .unwrap_or_else(|| "en-US".to_string())
}

fn told(picture: &Picture) -> myriad_nodes::session_look::Told {
    myriad_nodes::session_look::Told {
        device: picture.device.clone(),
        session: picture.session.clone(),
    }
}

/// The tasks of `pictures` her card about still waits on the owner: one she
/// judged needs them (`decide`, `nod`), neither answered nor put away. A
/// session that finished a turn on one still waits on them.
pub(super) async fn tasks_asking(
    db: &DatabaseConnection,
    owner: i32,
    pictures: &[Picture],
) -> Vec<String> {
    let mut tasks = Vec::new();
    for picture in pictures {
        let Ok(task) = task_id(owner, picture) else {
            continue;
        };
        for kind in ["decide", "nod"] {
            let Some(card) = card_of(&task, kind) else {
                continue;
            };
            if let Ok(Some(now)) =
                crate::services::agent::merope::cards::now(db, owner, &card).await
                && now.waiting
            {
                tasks.push(task);
                break;
            }
        }
    }
    tasks
}

/// The card of `kind` on `task`, by its id.
pub(super) fn card_of(task: &str, kind: &str) -> Option<String> {
    myriad_nodes::cards::card_id(
        myriad_nodes::look_cards::SCENE,
        kind,
        &format!("node:{task}"),
    )
    .ok()
}

/// Puts the card of her judgment of change `change` (from 1) before the
/// owner, when it needs them (`decide`, `nod`); the card of the other of
/// the two on the same task, if it still waits, is settled. Its id, when
/// it is before them.
pub(super) async fn put_card(
    db: &DatabaseConnection,
    owner: i32,
    picture: &Picture,
    judgment: &Judgment,
    material: &Material,
    change: usize,
) -> Option<String> {
    let other = match judgment.verdict {
        Verdict::Decide => "nod",
        Verdict::Nod => "decide",
        Verdict::NoNeed => return None,
    };
    let task = task_id(owner, picture).ok()?;
    let language = language(db, owner).await;
    // The moment of the picture it is about, not of her judgment: an end
    // that sees the session sends a choice pressed on it back only while
    // the card is of the session's latest picture (the same `occurredAt`).
    let at = picture.session.occurred_at;
    let card = match myriad_nodes::look_cards::decision(myriad_nodes::look_cards::DecisionInput {
        task: task.clone(),
        told: told(picture),
        judgment: judgment.clone(),
        material: material.clone(),
        change,
        at,
        language: language.clone(),
    }) {
        Ok(card) => card,
        Err(error) => {
            tracing::warn!(%error, "[Merope] her card of a session was not made");
            return None;
        }
    };
    let id = card.id.clone();
    if let Err(error) = crate::services::agent::merope::cards::put(db, owner, card).await {
        tracing::warn!(%error, "[Merope] her card of a session was not put before the owner");
        return None;
    }
    if let Some(other) = card_of(&task, other) {
        settle(db, owner, picture, &other, &language).await;
    }
    Some(id)
}

/// The card `card` settled as its session now stands: one that still
/// waits on the owner, or one that never asked them anything (an open
/// question she had no answer to: only going to the session) whose state
/// or facts the session has moved past. One already settled to this is
/// left as it is.
async fn settle(
    db: &DatabaseConnection,
    owner: i32,
    picture: &Picture,
    card: &str,
    language: &str,
) {
    let Ok(Some(now)) = crate::services::agent::merope::cards::now(db, owner, card).await else {
        return;
    };
    let waiting = now.waiting;
    let before = now.card.clone();
    let settled = myriad_nodes::look_cards::settled(myriad_nodes::look_cards::SettledInput {
        card: now.card,
        told: told(picture),
        at: picture.session.occurred_at,
        language: language.to_string(),
    });
    if let Ok(card) = &settled
        && !waiting
        && card.state == before.state
        && card.facts == before.facts
    {
        return;
    }
    match settled {
        Ok(card) => {
            if let Err(error) = crate::services::agent::merope::cards::put(db, owner, card).await {
                tracing::warn!(%error, "[Merope] a settled card was not put");
            }
        }
        Err(error) => tracing::warn!(%error, "[Merope] a card was not settled"),
    }
}

/// The card of where `picture`'s session came to, when it finished or
/// failed and the owner has that card: her line about that end is about
/// it, so it carries it.
pub(super) async fn status_card(
    db: &DatabaseConnection,
    owner: i32,
    picture: &Picture,
) -> Option<String> {
    if !matches!(picture.session.change, Change::Finished | Change::Failed) {
        return None;
    }
    let card = card_of(&task_id(owner, picture).ok()?, "status")?;
    crate::services::agent::merope::cards::now(db, owner, &card)
        .await
        .ok()
        .flatten()
        .map(|_| card)
}

/// The cards of `picture`'s session as it changed: where it came to when
/// it finished or failed (`status`), and her cards on it settled once it
/// went on or ended without waiting on the owner (they answered it there).
pub(super) async fn session_cards(db: &DatabaseConnection, owner: i32, picture: &Picture) {
    use myriad_her_link::now::WaitingOn;
    let Ok(task) = task_id(owner, picture) else {
        return;
    };
    let language = language(db, owner).await;
    let session = &picture.session;
    if matches!(session.change, Change::Finished | Change::Failed) {
        match myriad_nodes::look_cards::status(myriad_nodes::look_cards::StatusInput {
            task: task.clone(),
            told: told(picture),
            at: session.occurred_at,
            language: language.clone(),
        }) {
            Ok(Some(card)) => {
                if let Err(error) =
                    crate::services::agent::merope::cards::put(db, owner, card).await
                {
                    tracing::warn!(%error, "[Merope] a session's status card was not put");
                }
            }
            Ok(None) => {}
            Err(error) => tracing::warn!(%error, "[Merope] a session's status card was not made"),
        }
    }
    // It went on (or ended) without them: going quiet is not going on,
    // and a session that still waits on them keeps their card.
    let went_on = matches!(session.change, Change::Started | Change::Finished)
        && session.waiting_on != Some(WaitingOn::Owner);
    if went_on {
        for kind in ["decide", "nod"] {
            if let Some(card) = card_of(&task, kind) {
                settle(db, owner, picture, &card, &language).await;
            }
        }
    }
}

/// What she would tell them of `picture`'s change, to be said in person
/// when it fits; under its grant, for a drop to take back what is not said
/// yet.
pub(super) fn say(
    db: &DatabaseConnection,
    owner: i32,
    picture: &Picture,
    line: &str,
    judgment: &Judgment,
    card: Option<String>,
) {
    use crate::services::agent::consciousness::{
        EventUrgency, enqueue_speak_intent, new_speak_intent,
    };
    let session = &picture.session;
    let grant = sessions::grant_ref(&picture.device, &session.grant);
    let task = task_id(owner, picture).unwrap_or_default();
    let mut intent = new_speak_intent(
        owner,
        format!("{grant}/{task}:{}", view_id(session)),
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
    intent.card = card;
    enqueue_speak_intent(intent);
    let db = db.clone();
    crate::services::agent::merope::background::spawn("speak intents", async move {
        crate::services::agent::merope::ingest::tick_speak_intents(db).await;
    });
}

/// One look as it went: the grants of what she was shown, the material,
/// and each answer the model gave, word for word, with how it came.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Looked {
    /// `device/grant` of each change.
    pub grants: Vec<String>,
    pub material: String,
    pub answers: Vec<String>,
    /// One for each of `answers`, in the same order.
    #[serde(default)]
    pub replies: Vec<Reply>,
}

/// How one answer came, as its source said: why the model stopped and how
/// many tokens it wrote (neither when it did not say, or no answer came);
/// and how it read (`good`, `bad_grounds`, `unreadable`, `hollow`,
/// `hollow_kept`; none when no answer came).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct Reply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stop: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub read: Option<String>,
}

impl Reply {
    /// What the answers read inside one ask reported.
    pub fn of(reported: Option<&crate::services::ai_cost_ledger::Reported>) -> Self {
        Self {
            stop: reported.and_then(|reported| reported.stops.last().cloned()),
            output: reported.map(|reported| reported.usage.output),
            read: None,
        }
    }
}

fn grant_name(device: &str, grant: &str) -> String {
    format!("{device}/{grant}")
}

/// Keeps a look in the owner's ledger for a week.
pub(super) async fn ledger(
    db: &DatabaseConnection,
    owner: i32,
    pictures: &[Picture],
    material: &Material,
    answers: Vec<String>,
    replies: Vec<Reply>,
) {
    let Some(first) = pictures.first() else {
        return;
    };
    let mut grants: Vec<String> = pictures
        .iter()
        .map(|picture| grant_name(&picture.device, &picture.session.grant))
        .collect();
    grants.sort();
    grants.dedup();
    let looked = Looked {
        grants,
        material: material.numbered(),
        answers,
        replies,
    };
    let id = format!("{owner}/{}", uuid::Uuid::new_v4().simple());
    if let Err(error) = registry::put(
        db,
        LEDGER,
        &id,
        RegistryIdentity {
            subject_id: Some(owner),
            owner_id: Some(owner),
            tapp_id: None,
            runtime_id: Some(&first.device),
        },
        &looked,
        crate::clock::now().timestamp() + LEDGER_SECS,
    )
    .await
    {
        tracing::warn!(%error, "[Merope] a session look was not kept in the ledger");
    }
}

/// Every look of a dropped grant goes from the ledger.
pub(super) async fn forget_ledger<C: ConnectionTrait>(
    db: &C,
    owner: i32,
    device: &str,
    grant: &str,
) -> Result<(), Error> {
    let name = grant_name(device, grant);
    for row in registry::list(db, LEDGER, Some(owner), None).await? {
        if serde_json::from_value::<Looked>(row.payload)
            .is_ok_and(|looked| looked.grants.contains(&name))
        {
            registry::delete(db, LEDGER, &row.record_id).await?;
        }
    }
    Ok(())
}

/// The looks in the owner's ledger.
#[cfg(test)]
pub(super) async fn ledger_of(db: &DatabaseConnection, owner: i32) -> Vec<Looked> {
    registry::list(db, LEDGER, Some(owner), None)
        .await
        .unwrap()
        .into_iter()
        .filter_map(|row| serde_json::from_value(row.payload).ok())
        .collect()
}
