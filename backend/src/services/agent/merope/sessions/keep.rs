//! What comes of her look, kept: each change's card on its task, the one
//! line she would say, and the look itself (what she was shown and what the
//! model answered, word for word) in the owner's ledger for a week.

use myriad_her_link::now::Session;
use myriad_nodes::sessions;
use sea_orm::{ConnectionTrait, DatabaseConnection};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Picture, audience};
use crate::services::agent::merope::memory_nodes::{
    Error,
    prepare::{Author, RowIn},
    read, write,
};
use crate::services::runtime_registry::{self as registry, RegistryIdentity};
use myriad_nodes::session_look::{Judgment, Material, Verdict, card_text};

/// Her looks, word for word, by owner.
const LEDGER: &str = "agent_session_look";
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
    owner: i32,
    picture: &Picture,
) -> Result<Option<String>, Error> {
    let node = task_node(db, owner, picture).await?;
    Ok(node
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
        .map(|(_, text)| text))
}

fn task_id(owner: i32, picture: &Picture) -> Result<String, Error> {
    let audience = audience::private_talk(owner, Some(owner));
    let session = &picture.session;
    let task_key = sessions::task_key(&picture.device, &session.grant, &session.session);
    Ok(myriad_nodes::node_id(&audience, &task_key)?)
}

async fn task_node(
    db: &DatabaseConnection,
    owner: i32,
    picture: &Picture,
) -> Result<Option<Value>, Error> {
    let audience = audience::private_talk(owner, Some(owner));
    let task = task_id(owner, picture)?;
    read::node(db, owner, &audience, &task).await
}

/// Keeps her card on `picture`'s task: what she made of this change, on the
/// grounds she was shown. False when it was not kept (her mind is not
/// here).
pub(super) async fn card(
    db: &DatabaseConnection,
    owner: i32,
    picture: &Picture,
    judgment: &Judgment,
    material: &Material,
) -> Result<bool, Error> {
    // What she made of it is hers: kept only while her mind is here.
    let Some(pen) = crate::services::her_log::pen::think() else {
        return Ok(false);
    };
    let session = &picture.session;
    let audience = audience::private_talk(owner, Some(owner));
    let task = task_id(owner, picture)?;
    let node = read::node(db, owner, &audience, &task).await?;
    let view = view_id(session);
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
    because.push(grant);
    because.sort();
    because.dedup();
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
            "base": base, "view": view, "text": card_text(judgment), "because": because,
        }),
        author: Author::Her,
    };
    write::write(db, owner, &audience, &[row], Some(&pen)).await?;
    Ok(true)
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
/// and how it read (`good`, `bad_grounds`, `unreadable`, `hollow`; none
/// when no answer came).
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
