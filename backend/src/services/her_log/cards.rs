//! Her cards in her log (core 「卡片」): each version of a card the site
//! puts before the owner is a `card.version` row, and each answer of theirs
//! a `card.answered` row, in the owner's private audience
//! (`personal:<partner id>`), while the log is on, the owner syncs it, and
//! every side of theirs reads `log4` ([`READS`]).
//!
//! - A version's row goes under the site's own name for that version, so
//!   an answer another side writes to it (`version`: that row's id) finds it
//!   here by the same name. A version written while a side still read less
//!   stays on the site only, and so do the answers to it: those sides get
//!   cards through `conversations`, and answer through `card.answer`.
//! - A card with her view on it is hers (her pen: written only while her
//!   mind is here); one with none is the program's (`rule`).
//! - Her line that carries a card is not written as a `segment.line` of
//!   hers: her words are already her `speech.said`, and the run of versions
//!   says where a card was brought up.
//! - The other way ([`fold`]): the owner's `card.answered` rows another side
//!   wrote are answers as any, kept once however often they come and told
//!   to her once (`merope::cards::answer`), never written back.

use myriad_persona_log::{
    Author,
    payloads::{CardAnswered, CardVersion},
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};

use super::{Error, NODE, Owner, keys, store};

/// The core ABI that reads card rows.
pub(crate) const READS: u32 = 4;

/// `user`'s account as a side of her log, when their card rows go into it
/// now.
pub(crate) async fn into_log(db: &DatabaseConnection, user: i32) -> Result<Option<Owner>, Error> {
    if !super::currently_enabled()
        || !store::account(db, user)
            .await?
            .is_some_and(|account| account.on())
        || !super::relay::every_side_reads(db, user, READS).await?
    {
        return Ok(None);
    }
    Ok(Some(super::owner(db, user).await?))
}

/// Writes `card`'s version `version` as its row, under that name.
pub(crate) async fn version(
    db: &DatabaseConnection,
    owner: &Owner,
    card: &CardVersion,
    version: &str,
) -> Result<(), Error> {
    let hers = card.view.is_some();
    let author = if hers {
        let grant = super::lease::current(db).await?;
        if !grant.held_by(NODE) {
            return Err(Error::Refused("her mind is not on the server".into()));
        }
        (Author::Her, Some(grant.epoch))
    } else {
        (Author::Rule, None)
    };
    let payload =
        serde_json::to_value(card).map_err(|_| Error::Refused("a card does not write".into()))?;
    write(
        db,
        owner,
        &card.id,
        "card.version",
        &payload,
        author,
        version,
    )
    .await
}

/// Writes the owner's answer to `card`'s version `version` (its row's id).
pub(crate) async fn answered(
    db: &DatabaseConnection,
    owner: &Owner,
    card: &str,
    answer: &CardAnswered,
) -> Result<(), Error> {
    let payload = serde_json::to_value(answer)
        .map_err(|_| Error::Refused("an answer does not write".into()))?;
    let id = format!("site_{}", uuid::Uuid::new_v4().simple());
    write(
        db,
        owner,
        card,
        "card.answered",
        &payload,
        (Author::Owner, None),
        &id,
    )
    .await
}

async fn write(
    db: &DatabaseConnection,
    owner: &Owner,
    card: &str,
    kind: &str,
    payload: &serde_json::Value,
    author: (Author, Option<u64>),
    id: &str,
) -> Result<(), Error> {
    let audience = owner.personal();
    let object = format!("card:{card}");
    let address = store::Address {
        audience: &audience,
        subject: None,
        object: &object,
        kind,
    };
    let txn = db.begin().await?;
    store::write_row_as(
        &txn,
        keys::root(),
        owner,
        address,
        payload,
        author,
        id.to_string(),
    )
    .await?;
    txn.commit().await?;
    Ok(())
}

/// One answer another side wrote, as it came.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Theirs {
    pub card: String,
    pub answer: CardAnswered,
}

/// The owner's `card.answered` rows other sides wrote, oldest first, with
/// their bodies (a row whose body has not come yet waits for it).
pub(crate) async fn theirs(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    user: i32,
) -> Result<Vec<Theirs>, Error> {
    let owner = super::owner(db, user).await?;
    let audience = owner.personal();
    let mut rows = store::rows_of(
        db.query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT envelope FROM agent_log_rows \
             WHERE user_id = $1 AND audience = $2 AND origin <> $3 AND NOT forgotten \
               AND kind = 'card.answered'",
            [user.into(), audience.clone().into(), NODE.into()],
        ))
        .await?,
    )?;
    rows.retain(|row| row.author == Author::Owner);
    rows.sort_by(|a, b| a.hlc.cmp(&b.hlc).then_with(|| a.id.cmp(&b.id)));
    let held: Vec<_> = rows.iter().collect();
    let bodies = store::bodies(db, root, user, &audience, &held).await?;
    Ok(rows
        .iter()
        .filter_map(|row| {
            let card = row.object.strip_prefix("card:")?.to_string();
            let answer = serde_json::from_value(bodies.get(&row.id)?.clone()).ok()?;
            Some(Theirs { card, answer })
        })
        .collect())
}

/// Forgets `card` in her log: the owner's `forget` of it, so every side
/// takes it away too. Nothing when none of its rows went in.
pub(crate) async fn forget<C: ConnectionTrait + TransactionTrait>(
    db: &C,
    user: i32,
    card: &str,
) -> Result<(), Error> {
    if !store::account(db, user)
        .await?
        .is_some_and(|account| account.on())
    {
        return Ok(());
    }
    let owner = super::owner(db, user).await?;
    let audience = owner.personal();
    let object = format!("card:{card}");
    let any = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT 1 AS one FROM agent_log_rows \
             WHERE user_id = $1 AND audience = $2 AND object = $3 AND NOT forgotten LIMIT 1",
            [user.into(), audience.clone().into(), object.clone().into()],
        ))
        .await?
        .is_some();
    if !any {
        return Ok(());
    }
    let txn = db.begin().await?;
    store::forget_on(&txn, &owner, &audience, &object).await?;
    sea_orm::TransactionSession::commit(txn).await?;
    Ok(())
}

/// Takes in the owner's answers to her cards that other sides wrote into
/// her log: each an answer as any, kept once and told to her once, never
/// written back. How many were new here.
pub(crate) async fn fold(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    user: i32,
) -> Result<usize, Error> {
    let mut new = 0;
    for theirs in theirs(db, root, user).await? {
        let answer = &theirs.answer;
        let before = crate::services::agent::merope::cards::now(db, user, &theirs.card).await?;
        let answered = crate::services::agent::merope::cards::answer_as(
            db,
            user,
            &theirs.card,
            &answer.version,
            &answer.action,
            false,
        )
        .await?;
        if answered.kept && answered.now != before {
            new += 1;
        }
    }
    Ok(new)
}
