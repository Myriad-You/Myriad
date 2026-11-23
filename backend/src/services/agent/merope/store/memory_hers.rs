//! What she said worth keeping having said, kept and looked up.

use super::*;

/// What she said worth keeping having said, and what they put her onto,
/// of `updates`: kept once each.
pub(super) async fn keep_what_she_said<C: ConnectionTrait + sea_orm::TransactionTrait>(
    db: &C,
    user_id: i32,
    updates: &super::super::super::chat_remember::ChatMemoryUpdates,
    present: &crate::services::agent::memory::unified::Audience,
) -> Result<bool, anyhow::Error> {
    let mut kept = false;
    use crate::services::agent::memory::unified;
    // What they put her onto is hers now, as what she heard in a group is:
    // about the thing, no one named.
    if !updates.put_onto.is_empty() {
        let held: Vec<String> = unified::own_rows(db, super::super::super::heard::SOURCE, 300)
            .await?
            .into_iter()
            .map(|row| row.content)
            .collect();
        for thing in &updates.put_onto {
            if held.iter().any(|held| held == thing) {
                continue;
            }
            let evidence = serde_json::json!({ "heard": "suggested to her in a chat" }).to_string();
            kept |= unified::remember_own(
                db,
                thing,
                &evidence,
                Vec::new(),
                super::super::super::heard::SOURCE,
            )
            .await?
            .is_some();
        }
    }
    // What she said about herself is hers, naming no one: said in a group,
    // heard anywhere; said in private, only there (`own_said::kept_for`).
    // The same words again are kept once.
    let mut held_own: Vec<String> = Vec::new();
    if updates.said.iter().any(|said| said.hers) {
        held_own = unified::said_of_herself_heard_by(db, present, 300)
            .await?
            .into_iter()
            .map(|row| row.content)
            .collect();
    }
    for said in &updates.said {
        if said.hers {
            if held_own.contains(&said.said) {
                continue;
            }
            let evidence = serde_json::json!({ "said": said.evidence }).to_string();
            kept |= unified::remember_said_of_herself(
                db,
                present,
                &said.said,
                &evidence,
                said.concepts.clone(),
            )
            .await?
            .is_some();
            held_own.push(said.said.clone());
            continue;
        }
        let remembered = unified::remember(
            db,
            unified::NewMemory {
                user_id,
                kind: unified::MemoryKind::Fact,
                content: said.said.clone(),
                evidence: Some(said.evidence.clone()),
                speaker: unified::Speaker::Agent,
                source: SAID_SOURCE,
                audience: present.clone(),
                importance: 0.4,
                concepts: Vec::new(),
            },
        )
        .await?;
        if let (Some(id), Some(rests)) = (&remembered, updates.said_from.get(&said.evidence)) {
            unified::said_first_at(db, id, rests.first).await?;
            crate::services::agent::memory::origins::tell(db, &rests.lines, id).await?;
        }
        kept |= remembered.is_some();
    }
    Ok(kept)
}

/// What she said about herself that `talk` touches, by its words or its
/// meaning, best first, at most `limit`: of what `present` may hear (her own,
/// and in private what she said to these same people).
pub(crate) async fn said_of_herself<C: ConnectionTrait>(
    db: &C,
    present: &crate::services::agent::memory::unified::Audience,
    talk: &str,
    limit: usize,
) -> Vec<myriad_merope::own_said::Recalled> {
    use crate::services::agent::memory::unified;
    let rows = unified::said_of_herself_heard_by(db, present, 300)
        .await
        .unwrap_or_default();
    let now = crate::services::agent::memory::utc_now();
    unified::named_among(db, rows, talk, limit)
        .await
        .into_iter()
        .map(|row| myriad_merope::own_said::Recalled {
            said: row.content,
            ago: myriad_merope::doing::ago_text(now - row.created_at.with_timezone(&chrono::Utc)),
        })
        .collect()
}
