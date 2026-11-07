//! A person's own words about themselves in a group, on 「关于你」 in their
//! own private talk as well (decision N5, D4).
//!
//! What going over a stretch in a group keeps is the group's: it goes on
//! the group's nodes, what is only about a member on that member's node
//! there ([`super::facts::settled`]), and in a group everyone is
//! `other_person`. One thing may also be theirs in private: what they said
//! about themselves. Kept on their own 「关于你」 in their private talk, it
//! is said by them there (`partner`) and keeps where it was said (the
//! group's audience), as the core lets only the person's own words cross
//! (`trust::usable_in_private`, `tags::audience_fits`). Nothing anyone else
//! said there, and nothing she made of the group, ever goes.
//!
//! Its placement is the group's (one per memory); forgetting the memory
//! takes this entry too, as it cites the same row.

use myriad_nodes::{
    SaidBy,
    entries::{self, FactRowsInput, Placement, Start},
    trust::usable_in_private,
};
use sea_orm::DatabaseConnection;

use super::{Author, Error, RowIn, audience, facts::Fact, read, write};
use crate::services::her_log::pen::Thought;

/// Of `facts` going over a stretch of `member`'s talk in the group
/// `group` kept (an audience, `group:<id>`), those only about them that
/// they said, put on 「关于你」 in their private talk too. Never fails the
/// group's: what cannot be written is said in the log.
pub(crate) async fn to_their_talk(
    db: &DatabaseConnection,
    member: i32,
    site_owner: Option<i32>,
    group: &str,
    facts: &[Fact],
    pen: &Thought,
) {
    if let Err(error) = copy(db, member, site_owner, group, facts, pen).await {
        tracing::warn!(%error, "[memory-nodes] what they said of themselves in a group not kept in private");
    }
}

/// Whether `fact`, kept in `group`, is theirs to keep in `private`.
pub(crate) fn theirs(fact: &Fact, group: &str, private: &str) -> bool {
    fact.placement == Placement::AboutThem
        && fact.said_by == SaidBy::OtherPerson
        && usable_in_private(&[SaidBy::Partner], group, private, None)
}

async fn copy(
    db: &DatabaseConnection,
    member: i32,
    site_owner: Option<i32>,
    group: &str,
    facts: &[Fact],
    pen: &Thought,
) -> Result<usize, Error> {
    let private = audience::private_talk(member, site_owner);
    let theirs: Vec<&Fact> = facts
        .iter()
        .filter(|fact| theirs(fact, group, &private))
        .collect();
    if theirs.is_empty() || member <= 0 {
        return Ok(0);
    }
    let key = super::audience::person_key(&private, member);
    let node = myriad_nodes::node_id(&private, &key)?;
    let there = read::node(db, member, &private, &node).await?.is_some();
    let start = if there {
        None
    } else {
        Some(Start {
            key: Some(key.clone()),
            name: super::facts::person_name(db, member).await?,
            aliases: Vec::new(),
        })
    };
    let made = entries::fact_rows(FactRowsInput {
        audience: private.clone(),
        sync: myriad_nodes::Sync::Shared,
        start,
        key: there.then_some(key),
        facts: theirs
            .iter()
            .map(|fact| entries::Fact {
                memory: fact.memory.clone(),
                text: fact.text.clone(),
                said_by: vec![SaidBy::Partner],
                refs: Vec::new(),
                revision: String::new(),
                audience: Some(group.to_string()),
                happened: fact.standing.happened.clone(),
                state: fact.standing.state.clone(),
                open: fact.standing.open,
            })
            .collect(),
    })?;
    let rows: Vec<RowIn> = made
        .rows
        .into_iter()
        .map(|row| RowIn {
            id: row.id,
            node: made.node.clone(),
            kind: row.kind,
            body: row.body,
            author: Author::Her,
        })
        .collect();
    let written = write::write(db, member, &private, &rows, Some(pen)).await?;
    Ok(written.rows.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fact(said_by: SaidBy, placement: Placement) -> Fact {
        Fact {
            memory: "mem_1".into(),
            text: "works nights".into(),
            said_by,
            placement,
            standing: Default::default(),
        }
    }

    #[test]
    fn only_their_own_words_about_themselves_go_to_their_talk() {
        let group = "group:qq:5";
        assert!(theirs(
            &fact(SaidBy::OtherPerson, Placement::AboutThem),
            group,
            "personal:7"
        ));
        // Hers, or of a kind, or nowhere: the group's alone.
        assert!(!theirs(
            &fact(SaidBy::Her, Placement::AboutThem),
            group,
            "personal:7"
        ));
        assert!(!theirs(
            &fact(SaidBy::OtherPerson, Placement::Nowhere),
            group,
            "personal:7"
        ));
        assert!(!theirs(
            &fact(
                SaidBy::OtherPerson,
                Placement::Start {
                    name: "work".into(),
                    aliases: Vec::new()
                }
            ),
            group,
            "personal:7"
        ));
        // Never into anything but one person's private talk.
        assert!(!theirs(
            &fact(SaidBy::OtherPerson, Placement::AboutThem),
            group,
            "own:"
        ));
        assert!(!theirs(
            &fact(SaidBy::OtherPerson, Placement::AboutThem),
            group,
            "group:qq:6"
        ));
    }
}
