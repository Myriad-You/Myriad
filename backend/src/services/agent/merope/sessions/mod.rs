//! The owner's coding-agent sessions on their devices, told as they change
//! (her link's `now` family): kept on her nodes by the core's rules, with
//! no model (`myriad_nodes::sessions`). The repository is a whole, each
//! session a task under it; what a session said and did is written by the
//! rule, never as hers.
//!
//! Only two changes are thought over: a session that asks the owner, and
//! one that failed. She looks at it once, carefully ([`judge`]); a finished
//! one waits a little with the others of its repository and they are
//! looked at together ([`finished`]). Starting and going quiet are only
//! kept.
//!
//! The latest picture of each session (what it said, up to 2000
//! characters) is kept beside the nodes for that look ([`Picture`]), a
//! while after its last change. A grant the owner ends (`now.drop`), and
//! every grant of a device that is removed, is forgotten for good: the
//! tasks it made, every row that names it, the pictures. A repository's
//! node stays a bare name for the sessions of later grants (a node
//! forgotten whole never comes back under its key), except a device's own
//! folder, which goes with the device.

mod finished;
mod judge;
mod look;

use myriad_her_link::now::{Change, Session};
use myriad_nodes::sessions::{self, SessionInput};
use std::collections::BTreeSet;

use sea_orm::{ConnectionTrait, DatabaseConnection, TransactionSession, TransactionTrait};
use serde::{Deserialize, Serialize};

use super::memory_nodes::{
    Error, audience, forget,
    prepare::{Author, RowIn},
    read, write,
};
use crate::services::runtime_registry::{self as registry, RegistryIdentity};

/// The latest picture of each session, by device, grant and session.
const PICTURES: &str = "agent_session_picture";
/// How long a picture is kept after its last change.
const PICTURE_SECS: i64 = 14 * 24 * 60 * 60;
/// The grants each device was given, so a removed device's are found
/// after its pictures are gone. Holds no words, and is kept as long as
/// what it finds may be.
const GRANTS: &str = "agent_session_grant";
const GRANT_SECS: i64 = 100 * 365 * 24 * 60 * 60;
/// The repository nodes of each device's folders with no remote, which
/// go with the device. Holds no words.
const FOLDERS: &str = "agent_session_folder";

/// One session as it last changed, and the device that told it.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Picture {
    pub device: String,
    pub session: Session,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Grant {
    device: String,
    grant: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Folder {
    device: String,
    node: String,
}

fn picture_id(device: &str, grant: &str, session: &str) -> String {
    format!("{device}/{grant}/{session}")
}

fn identity(owner: i32, device: &str) -> RegistryIdentity<'_> {
    RegistryIdentity {
        subject_id: Some(owner),
        owner_id: Some(owner),
        tapp_id: None,
        runtime_id: Some(device),
    }
}

/// Takes one change of a session the owner's `device` told: kept on her
/// nodes by the rules, then looked at when it asks them or failed. A change
/// older than the one kept changes nothing. While her mind is on another
/// end the site keeps none of it: that end does.
pub(crate) async fn take(
    db: &DatabaseConnection,
    owner: i32,
    device: &str,
    session: Session,
) -> Result<(), Error> {
    if crate::services::persona_sync::lease::paused("a session's change") {
        return Ok(());
    }
    // What a removed device left, if forgetting it failed then: again.
    if let Err(error) = forget_removed(db, owner).await {
        tracing::warn!(%error, "[Merope] a removed device's sessions are not forgotten yet");
    }
    // A device removed, or a grant it dropped: nothing more is kept of it.
    if !active(db, owner, device).await? || look::grant_dropped(owner, device, &session.grant) {
        return Ok(());
    }
    let id = picture_id(device, &session.grant, &session.session);
    if let Some(kept) = registry::get::<Picture>(db, PICTURES, &id).await?
        && kept.session.occurred_at >= session.occurred_at
    {
        return Ok(());
    }
    let audience = audience::private_talk(owner, Some(owner));
    let repo_key = match (&session.remote, &session.folder) {
        (Some(remote), _) => myriad_nodes::repo_key(remote)?,
        (_, Some(folder)) => sessions::folder_key(device, folder),
        (None, None) => return Ok(()),
    };
    let repo = myriad_nodes::node_id(&audience, &repo_key)?;
    let task_key = sessions::task_key(device, &session.grant, &session.session);
    let task = myriad_nodes::node_id(&audience, &task_key)?;
    let repo_node = read::node(db, owner, &audience, &repo).await?;
    let repo_known = repo_node.is_some();
    let repo_sync = repo_node.as_ref().and_then(sessions::created_sync);
    let task_known = read::node(db, owner, &audience, &task)
        .await?
        .as_ref()
        .and_then(sessions::task_known);
    let rows = sessions::session_rows(SessionInput {
        audience: audience.clone(),
        device: device.to_string(),
        session: session.clone(),
        repo_known,
        repo_sync,
        task: task_known,
        // Kept on the site's side until every side it syncs with reads
        // rows a rule wrote (log3).
        sync: myriad_nodes::Sync::Local(crate::services::her_log::NODE.to_string()),
    })?;
    let rows: Vec<RowIn> = rows
        .rows
        .into_iter()
        .map(|placed| RowIn {
            id: placed.row.id,
            node: placed.node,
            kind: placed.row.kind,
            body: placed.row.body,
            author: Author::Rule,
        })
        .collect();
    // Found before anything is kept: a removed device's grants are found
    // by these.
    if session.remote.is_none() {
        registry::put(
            db,
            FOLDERS,
            &format!("{device}/{repo}"),
            identity(owner, device),
            &Folder {
                device: device.to_string(),
                node: repo.clone(),
            },
            crate::clock::now().timestamp() + GRANT_SECS,
        )
        .await?;
    }
    registry::put(
        db,
        GRANTS,
        &format!("{device}/{}", session.grant),
        identity(owner, device),
        &Grant {
            device: device.to_string(),
            grant: session.grant.clone(),
        },
        crate::clock::now().timestamp() + GRANT_SECS,
    )
    .await?;
    write::write(db, owner, &audience, &rows, None).await?;
    let picture = Picture {
        device: device.to_string(),
        session,
    };
    registry::put(
        db,
        PICTURES,
        &id,
        identity(owner, device),
        &picture,
        crate::clock::now().timestamp() + PICTURE_SECS,
    )
    .await?;
    // Removed, or the grant dropped, while it was kept: it goes with the
    // rest, this grant by name (the removal may have read the grants before
    // this one was found).
    if !active(db, owner, device).await? {
        forget_grant(db, owner, device, &picture.session.grant).await?;
        return forget_device(db, owner, device).await;
    }
    if look::dropped(owner, &picture) {
        return forget_grant(db, owner, device, &picture.session.grant).await;
    }
    match picture.session.change {
        Change::Asked | Change::Failed => look::spawn(db.clone(), owner, picture),
        Change::Finished => {
            look::moved_on(
                owner,
                device,
                &picture.session.grant,
                &picture.session.session,
            );
            finished::add(db.clone(), owner, picture)
        }
        Change::Started | Change::Quiet => look::moved_on(
            owner,
            device,
            &picture.session.grant,
            &picture.session.session,
        ),
    }
    Ok(())
}

/// Whether `device` is one of the owner's, not removed.
async fn active<C: ConnectionTrait>(db: &C, owner: i32, device: &str) -> Result<bool, Error> {
    Ok(db
        .query_one_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT 1 AS one FROM agent_sync_devices \
             WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL",
            [device.into(), owner.into()],
        ))
        .await?
        .is_some())
}

/// Forgets what every device of the owner's that is no longer one left.
pub(crate) async fn forget_removed<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    owner: i32,
) -> Result<(), Error> {
    let mut devices = BTreeSet::new();
    for row in registry::list(db, GRANTS, Some(owner), None).await? {
        if let Ok(grant) = serde_json::from_value::<Grant>(row.payload) {
            devices.insert(grant.device);
        }
    }
    for row in registry::list(db, FOLDERS, Some(owner), None).await? {
        if let Ok(folder) = serde_json::from_value::<Folder>(row.payload) {
            devices.insert(folder.device);
        }
    }
    for device in devices {
        if !active(db, owner, &device).await? {
            forget_device(db, owner, &device).await?;
        }
    }
    Ok(())
}

/// The pictures the owner's devices keep now, of every session.
pub(crate) async fn pictures(db: &DatabaseConnection, owner: i32) -> Vec<Picture> {
    match registry::list(db, PICTURES, Some(owner), None).await {
        Ok(rows) => rows
            .into_iter()
            .filter_map(|row| serde_json::from_value::<Picture>(row.payload).ok())
            .collect(),
        Err(error) => {
            tracing::warn!(%error, "[Merope] could not read the sessions' pictures");
            Vec::new()
        }
    }
}

/// Forgets for good what the owner's `device` told under `grant`: the
/// tasks it made, every row that names the grant, a repository node only
/// it held, the pictures and anything still waiting to be looked at.
pub(crate) async fn forget_grant<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    owner: i32,
    device: &str,
    grant: &str,
) -> Result<(), Error> {
    look::cancel(owner, device, grant);
    finished::cancel(owner, device, grant);
    // What was to be said of it and is not yet.
    let grant_ref = sessions::grant_ref(device, grant);
    crate::services::agent::consciousness::withdraw_speak_intents(owner, &grant_ref);
    crate::services::agent::consciousness::withdraw_speak_intents(owner, finished::FINISHED);
    let audience = audience::private_talk(owner, Some(owner));
    // Under the audience's lock, as every forget.
    let txn = db.begin().await?;
    forget::forget_named(&txn, owner, &audience, &grant_ref, "minted:s").await?;
    txn.commit().await?;
    for row in registry::list(db, PICTURES, Some(owner), None).await? {
        if serde_json::from_value::<Picture>(row.payload)
            .is_ok_and(|picture| picture.device == device && picture.session.grant == grant)
        {
            registry::delete(db, PICTURES, &row.record_id).await?;
        }
    }
    registry::delete(db, GRANTS, &format!("{device}/{grant}")).await?;
    Ok(())
}

/// [`forget_grant`] for every grant `device` was given: the device was
/// removed, the site has another owner, or the owner disconnected it.
pub(crate) async fn forget_device<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    owner: i32,
    device: &str,
) -> Result<(), Error> {
    for row in registry::list(db, GRANTS, Some(owner), None).await? {
        if let Ok(grant) = serde_json::from_value::<Grant>(row.payload)
            && grant.device == device
        {
            forget_grant(db, owner, device, &grant.grant).await?;
        }
    }
    // Its folders: their keys name the device, which never comes again.
    let mut nodes = BTreeSet::new();
    let mut records = Vec::new();
    for row in registry::list(db, FOLDERS, Some(owner), None).await? {
        if let Ok(folder) = serde_json::from_value::<Folder>(row.payload)
            && folder.device == device
        {
            nodes.insert(folder.node);
            records.push(row.record_id);
        }
    }
    let audience = audience::private_talk(owner, Some(owner));
    let txn = db.begin().await?;
    forget::forget_nodes(&txn, owner, &audience, &nodes).await?;
    txn.commit().await?;
    for record in records {
        registry::delete(db, FOLDERS, &record).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests;
