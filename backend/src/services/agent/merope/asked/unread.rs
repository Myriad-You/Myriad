//! Short notes left unread time after time, of someone who asked her not to
//! keep something: kept for good as they are, as of anyone
//! (`store::memory_jobs::settling`), but for what is of it.

use std::time::Instant;

use sea_orm::{DatabaseConnection, TransactionTrait};

use crate::services::agent::memory::unified::{self, Audience};
use crate::services::agent::merope::memory_jobs::Failure;
use crate::services::agent::merope::memory_nodes::take_back_check;
use crate::services::agent::merope::store::memory_jobs::{Own, settling};

type At = chrono::DateTime<chrono::FixedOffset>;

/// The notes of whose job it is at `present` from `since` until `until`:
/// those of what they asked are taken back, those told not to be are kept for
/// good. An `Err`: not all could be told (by `until_told`) or written; a note
/// not told stays one, and, given up on, is let be. Nothing is written once
/// it is not its `own` any more, nor of a note that went or was changed while
/// it was judged.
pub(in crate::services::agent::merope) async fn keep(
    db: &DatabaseConnection,
    own: Own<'_>,
    present: &Audience,
    last: At,
    (since, until): (At, At),
    told_by: Instant,
) -> Result<(), Failure> {
    let user_id = own.owner();
    let notes = settling::notes_left(db, user_id, present, since, until)
        .await
        .map_err(|_| Failure::Storage)?;
    let texts: Vec<String> = notes.iter().map(|note| note.content.clone()).collect();
    let told = super::told_of(db, user_id, None, &texts, told_by).await;
    let written = async {
        let tx = db.begin().await?;
        if !own.still(&tx).await? {
            return Ok(None);
        }
        let (mut gone, mut kept) = (Vec::new(), Vec::new());
        for note in settling::notes_left(&tx, user_id, present, since, until).await? {
            let judged = notes
                .iter()
                .position(|judged| judged.id == note.id && judged.content == note.content);
            match judged {
                Some(index) if told.out.contains(&index) => gone.push(note.id),
                Some(index) if !told.untold.contains(&index) => kept.push(note),
                _ => {}
            }
        }
        if !gone.is_empty() {
            unified::retire(&tx, user_id, &gone, "superseded").await?;
            take_back_check::hold(&tx, user_id, &gone, None).await?;
        }
        let facts = settling::keep_notes(&tx, user_id, present, last, kept).await?;
        tx.commit().await?;
        anyhow::Ok(Some(facts))
    }
    .await;
    let Some(facts) = written.map_err(|_| Failure::Storage)? else {
        return Ok(());
    };
    if !facts.is_empty() {
        let kept = settling::Kept {
            owner: user_id,
            present: present.clone(),
            facts,
        };
        kept.on_nodes(db).await;
    }
    told.let_be(own.gives_up())
}
