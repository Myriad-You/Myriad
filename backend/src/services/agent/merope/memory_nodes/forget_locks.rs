//! The node locks a forgetting takes before anything of it is written
//! ([`super::forget`]).

use std::collections::BTreeSet;

use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use super::forget::{OF_A_MEMORY, citing, refs_of};
use super::{Error, audience, intent_grounds, write};
use crate::services::agent::memory::{origins::Gone as Line, unified::OWN_VENUE};

/// Takes the locks [`super::forget::forget`] of `memories` and `lines` needs,
/// in one order, for a caller that writes forgets of its own into her log
/// before it: a node's lock goes before the log's wherever both are taken
/// ([`super::logged`]).
///
/// Every audience they may be named in, whether a row names one yet or not:
/// one being written as they go waits here, and reads that they are gone
/// once this commits (`forget::resting_on_what_went`).
pub(crate) async fn lock_for<C: ConnectionTrait>(
    db: &C,
    site_owner: Option<i32>,
    memories: &[String],
    lines: &[Line],
) -> Result<(), Error> {
    let ids: Vec<i32> = lines.iter().map(|line| line.id).collect();
    let refs = refs_of(memories, &ids);
    let mut wanted = named_in(db, site_owner, memories, lines).await?;
    wanted.extend(
        citing(db, &refs, OF_A_MEMORY)
            .await?
            .into_iter()
            .map(|(user_id, audience, _, _)| (user_id, audience)),
    );
    wanted.extend(intent_grounds::citing(db, &refs).await?);
    for (user_id, audience) in &wanted {
        write::lock(db, *user_id, audience).await?;
    }
    Ok(())
}

/// The audiences `memories` and `lines` may be named in: the private talk of
/// whose they are, the group they were said in, her own when they are the
/// site owner's or hers.
async fn named_in<C: ConnectionTrait>(
    db: &C,
    site_owner: Option<i32>,
    memories: &[String],
    lines: &[Line],
) -> Result<BTreeSet<(i32, String)>, Error> {
    let mut rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT user_id, venue FROM agent_memories WHERE id = ANY($1)",
            [memories.to_vec().into()],
        ))
        .await?;
    // One that is not here is the site owner's, from another end.
    let mut hers = rows.len() < memories.len();
    let sessions: Vec<String> = lines.iter().map(|line| line.session_id.clone()).collect();
    rows.extend(
        db.query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT user_id, 'group:' || (context->>'venue') AS venue \
             FROM agent_sessions WHERE id = ANY($1)",
            [sessions.into()],
        ))
        .await?,
    );
    let mut audiences = BTreeSet::new();
    for row in &rows {
        let user_id: Option<i32> = row.try_get("", "user_id")?;
        let venue: Option<String> = row.try_get("", "venue")?;
        if let Some(user_id) = user_id.filter(|id| *id > 0) {
            audiences.insert((user_id, audience::private_talk(user_id, site_owner)));
            hers |= site_owner == Some(user_id);
        }
        match venue.as_deref() {
            Some(OWN_VENUE) => hers = true,
            Some(venue) => {
                if let (Some(owner), Some(group)) = (site_owner, venue.strip_prefix("group:")) {
                    audiences.insert((owner, audience::group(group)));
                }
            }
            None => {}
        }
    }
    if let Some(owner) = site_owner.filter(|_| hers) {
        audiences.insert((owner, audience::owners_talk()));
        audiences.insert((owner, audience::OWN.to_string()));
    }
    Ok(audiences)
}
