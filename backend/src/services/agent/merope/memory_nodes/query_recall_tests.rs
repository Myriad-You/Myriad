//! Their words reach her nodes by name (`myriad_nodes::recall::pick`'s
//! `query`): a node not gone, with something on it, whose name or one of
//! its other names they say comes to mind though the ranking found nothing
//! on it; on a database of its own, as `read_tests`. The ranking here is
//! empty, so whatever is picked was picked by name.

use myriad_nodes::{SaidBy, entries::Placement};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::json;

use super::read_tests::{GROUP, OWNER, SEVEN, owned, put};
use super::recall::{self, Recollection};
use super::shown::Shown;
use super::{Author, RowIn, audience, context, forget, group_context, write};
use crate::services::agent::memory::unified::Audience;
use crate::services::agent::merope::store::Recalled;

/// Nothing ranked: what comes, came by name.
fn nothing_ranked() -> Recalled {
    Recalled {
        named: Vec::new(),
        brought_to_mind: Vec::new(),
        recent: Vec::new(),
        ids: Vec::new(),
        said: Default::default(),
    }
}

fn start(name: &str, aliases: &[&str]) -> Placement {
    Placement::Start {
        name: name.into(),
        aliases: aliases.iter().map(|alias| alias.to_string()).collect(),
    }
}

/// The id of the node named `name` in `shown`.
fn named(shown: &Shown, name: &str) -> String {
    shown
        .nodes
        .iter()
        .find(|node| node.name.as_deref() == Some(name))
        .unwrap_or_else(|| panic!("a node named {name}"))
        .id
        .clone()
}

/// A line under `name` that says `fact`.
fn says(recollection: &Recollection, name: &str, fact: &str) -> bool {
    recollection
        .named
        .iter()
        .any(|line| line.starts_with(&format!("{name}: ")) && line.contains(fact))
}

/// Taken back (let go with nothing in its place), as letting go writes it.
async fn let_go(db: &DatabaseConnection, memory: &str) {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_memories SET invalid_at = now(), invalid_reason = 'superseded', \
         superseded_by = NULL, updated_at = now() WHERE id = $1",
        [memory.into()],
    ))
    .await
    .unwrap();
}

/// A node they name comes, by its name or by another name it goes by, in
/// their words in any case, though nothing ranked found it.
#[tokio::test]
async fn a_node_comes_when_they_say_its_name_or_another_it_goes_by() {
    let Some(schema) = owned("query_recall_by_name").await else {
        return;
    };
    let db = &schema.db;
    let talk = audience::private_talk(SEVEN, Some(OWNER));
    let private = || Audience::private(SEVEN);
    put(
        db,
        SEVEN,
        &talk,
        SEVEN,
        private(),
        SaidBy::Partner,
        start("Houseplants", &["the green ones"]),
        &["keeps a snake plant on the balcony"],
    )
    .await;
    put(
        db,
        SEVEN,
        &talk,
        SEVEN,
        private(),
        SaidBy::Partner,
        start("乐器", &["木吉他"]),
        &["周末总练一小时指弹"],
    )
    .await;
    let now = crate::clock::now();
    let turn = context::load(db, SEVEN, now).await.unwrap();
    // The other names are read with the node.
    let plants = named(&turn.shown, "Houseplants");
    let plants_node = turn.shown.node(&plants).unwrap();
    assert_eq!(plants_node.aliases, vec!["the green ones"]);
    let ask = |query: Option<&str>| {
        recall::recollect_here(
            &turn.shown,
            &turn.own,
            None,
            &nothing_ranked(),
            None,
            now,
            query,
        )
    };
    // Without their words nothing comes: the ranking found nothing.
    assert!(ask(None).picked.is_empty(), "{:?}", ask(None).named);

    let by_name = ask(Some("How are the HOUSEPLANTS doing?"));
    assert_eq!(by_name.picked, vec![plants.clone()], "{:?}", by_name.named);
    assert!(
        says(&by_name, "Houseplants", "keeps a snake plant on the balcony"),
        "{:?}",
        by_name.named
    );
    let by_alias = ask(Some("did you water the green ones?"));
    assert_eq!(by_alias.picked, vec![plants], "{:?}", by_alias.named);
    assert!(
        says(&by_alias, "Houseplants", "keeps a snake plant on the balcony"),
        "{:?}",
        by_alias.named
    );
    let guitar = named(&turn.shown, "乐器");
    let by_cjk_alias = ask(Some("我的木吉他是不是该换弦了"));
    assert_eq!(by_cjk_alias.picked, vec![guitar], "{:?}", by_cjk_alias.named);
    assert!(
        says(&by_cjk_alias, "乐器", "周末总练一小时指弹"),
        "{:?}",
        by_cjk_alias.named
    );
    schema.drop().await;
}

/// A node forgotten, one whose only fact they took back, and one that is a
/// name with nothing on it never come by name; one beside them that has
/// something on it does, in the same words.
#[tokio::test]
async fn gone_taken_back_and_name_only_nodes_do_not_come_by_name() {
    let Some(schema) = owned("query_recall_not_these").await else {
        return;
    };
    let db = &schema.db;
    let talk = audience::private_talk(SEVEN, Some(OWNER));
    let private = || Audience::private(SEVEN);
    put(
        db,
        SEVEN,
        &talk,
        SEVEN,
        private(),
        SaidBy::Partner,
        start("Knitting", &[]),
        &["KNIT-FACT made a scarf last winter"],
    )
    .await;
    put(
        db,
        SEVEN,
        &talk,
        SEVEN,
        private(),
        SaidBy::Partner,
        start("Sourdough", &["bread starter"]),
        &["GONE-FACT feeds the starter every Friday"],
    )
    .await;
    let marathon = put(
        db,
        SEVEN,
        &talk,
        SEVEN,
        private(),
        SaidBy::Partner,
        start("Marathon", &["long run"]),
        &["TAKEN-BACK-FACT runs one every autumn"],
    )
    .await;
    // A name with nothing on it.
    let made = myriad_nodes::entries::fact_rows(
        serde_json::from_value(json!({
            "audience": talk,
            "start": {"name": "Photography", "aliases": ["camera"]},
            "facts": [],
        }))
        .unwrap(),
    )
    .unwrap();
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
    let thought = crate::services::her_log::pen::think().expect("her mind in tests");
    write(db, SEVEN, &talk, &rows, Some(&thought)).await.unwrap();

    let now = crate::clock::now();
    let before = context::load(db, SEVEN, now).await.unwrap();
    let knitting = named(&before.shown, "Knitting");
    let sourdough = named(&before.shown, "Sourdough");
    let marathon_node = named(&before.shown, "Marathon");
    let photography = named(&before.shown, "Photography");
    forget::forget_one(db, SEVEN, &talk, &sourdough, forget::Forgets::Node)
        .await
        .unwrap();
    let_go(db, &marathon[0]).await;

    let after = context::load(db, SEVEN, now).await.unwrap();
    // As set: forgotten, its fact taken back, nothing on it.
    assert!(after.shown.node(&sourdough).is_none_or(|node| node.gone));
    for empty in [&marathon_node, &photography] {
        assert!(
            after
                .shown
                .node(empty)
                .is_none_or(|node| node.gone || node.facts.is_empty()),
            "{empty} still has a fact"
        );
    }
    for words in [
        "about sourdough and the bread starter, and knitting",
        "about the marathon and the long run, and knitting",
        "about photography and my camera, and knitting",
    ] {
        let recollection = recall::recollect_here(
            &after.shown,
            &after.own,
            None,
            &nothing_ranked(),
            None,
            now,
            Some(words),
        );
        assert_eq!(
            recollection.picked,
            vec![knitting.clone()],
            "{words}: {:?}",
            recollection.named
        );
        let shown = recollection.named.join("\n");
        assert!(shown.contains("KNIT-FACT"), "{words}: {shown}");
        for never in ["GONE-FACT", "TAKEN-BACK-FACT", "Photography"] {
            assert!(!shown.contains(never), "{words}: {never} in {shown}");
        }
    }
    schema.drop().await;
}

/// Named in a group, a node of their private talk never comes, nor one of
/// the group's in the private talk: each audience reads its own nodes, and
/// a name only reaches those.
#[tokio::test]
async fn a_name_said_in_a_group_does_not_reach_the_private_talk_or_back() {
    let Some(schema) = owned("query_recall_audiences").await else {
        return;
    };
    let db = &schema.db;
    let talk = audience::private_talk(SEVEN, Some(OWNER));
    put(
        db,
        SEVEN,
        &talk,
        SEVEN,
        Audience::private(SEVEN),
        SaidBy::Partner,
        start("SecretDiary", &["private cafe"]),
        &["PRIVATE-FACT writes in a teahouse"],
    )
    .await;
    put(
        db,
        OWNER,
        &audience::group(GROUP),
        SEVEN,
        Audience::group(GROUP, SEVEN),
        SaidBy::OtherPerson,
        start("Board games", &["Catan night"]),
        &["GROUP-FACT plays every Friday"],
    )
    .await;
    let words = "tell me about SecretDiary at the private cafe, and Catan night";
    let now = crate::clock::now();

    let in_group = group_context::load(db, SEVEN, GROUP)
        .await
        .unwrap()
        .expect("a site owner keeps it");
    let group_node = named(&in_group.shown, "Board games");
    let here = in_group.group.as_ref().map(|group| &group.here);
    let there = recall::recollect_here(
        &in_group.shown,
        &in_group.own,
        here,
        &nothing_ranked(),
        None,
        now,
        Some(words),
    );
    assert_eq!(there.picked, vec![group_node], "{:?}", there.named);
    let shown = there.named.join("\n");
    assert!(shown.contains("GROUP-FACT"), "{shown}");
    assert!(!shown.contains("PRIVATE-FACT"), "{shown}");
    assert!(!shown.contains("SecretDiary"), "{shown}");

    let in_private = context::load(db, SEVEN, now).await.unwrap();
    let private_node = named(&in_private.shown, "SecretDiary");
    let alone = recall::recollect_here(
        &in_private.shown,
        &in_private.own,
        None,
        &nothing_ranked(),
        None,
        now,
        Some(words),
    );
    assert_eq!(alone.picked, vec![private_node], "{:?}", alone.named);
    let shown = alone.named.join("\n");
    assert!(shown.contains("PRIVATE-FACT"), "{shown}");
    assert!(!shown.contains("GROUP-FACT"), "{shown}");
    assert!(!shown.contains("Board games"), "{shown}");
    schema.drop().await;
}
