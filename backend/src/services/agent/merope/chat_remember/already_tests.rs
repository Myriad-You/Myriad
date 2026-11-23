//! A correction whose new fact she already had, judged on a database of its
//! own (`MYRIAD_MEDIA_TEST_DATABASE_URL`, an isolated schema each), with the
//! judge's answer given by the test (`channel_group::g0`).

use sea_orm::{ConnectionTrait, TransactionTrait};

use super::super::{ChatMemoryUpdate, ChatMemoryUpdates, SaidNew};
use crate::services::agent::memory::unified::{Audience, Concept};
use crate::services::agent::merope::store::{self, Keep};
use crate::services::channel_group::g0;

const THEM: i32 = 7;

fn fact(text: &str, about: &str, supersedes: &[&str]) -> ChatMemoryUpdate {
    ChatMemoryUpdate {
        standing: Default::default(),
        topic: None,
        cues: Vec::new(),
        fact: (!text.is_empty()).then(|| text.to_string()),
        supersedes: supersedes.iter().map(|old| old.to_string()).collect(),
        evidence: Some(
            if text.is_empty() {
                "不是周五"
            } else {
                text
            }
            .into(),
        ),
        concepts: (!about.is_empty())
            .then(|| Concept {
                name: about.into(),
                aliases: Vec::new(),
            })
            .into_iter()
            .collect(),
    }
}

/// 「周五去看牙」 and `had`, each kept for good; then the line 「不是周五」
/// with its new fact `new` (about `about`), judged one she had. What the
/// line comes to.
async fn judged(prefix: &str, had: (&str, &str), new: (&str, &str)) -> Option<ChatMemoryUpdates> {
    let url = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"])?;
    let schema = crate::db::IsolatedSchema::migrated(&url, prefix).await;
    let db = &schema.db;
    db.execute_unprepared("INSERT INTO users (id, username) VALUES (7, 'seven')")
        .await
        .unwrap();
    let present = Audience::private(THEM);
    for kept in [fact("周五去看牙", "看牙", &[]), fact(had.0, had.1, &[])] {
        let transaction = db.begin().await.unwrap();
        assert!(
            store::apply_chat_memory_update_on(
                &transaction,
                THEM,
                crate::clock::now().fixed_offset(),
                &kept,
                &present,
                Keep::Lasting,
            )
            .await
            .unwrap()
        );
        transaction.commit().await.unwrap();
    }
    let mut updates = ChatMemoryUpdates {
        updates: vec![fact("", "", &["周五去看牙"]), fact(new.0, new.1, &[])],
        said_new: SaidNew::Something,
        ..ChatMemoryUpdates::default()
    };
    let mut world = g0::World::new(0.5);
    world
        .models
        .entry("chat_already".to_string())
        .or_default()
        .push_back(Some(r#"{"pairs":[{"i":0,"verdict":"same"}]}"#.to_string()));
    g0::enter(world);
    super::check_against_unshown(db, THEM, &present, &[], &mut updates, super::TIMEOUT).await;
    g0::leave();
    schema.drop().await;
    Some(updates)
}

/// The new fact she already had, about what the line lets go of and kept
/// for good: the correction is to it, and what it lets go of is replaced
/// by it at once.
#[tokio::test]
async fn a_correction_to_what_she_had_for_good_replaces_by_it() {
    let Some(updates) = judged(
        "chat_already_correction",
        ("周六去看牙", "看牙"),
        ("改到周六看牙", "看牙"),
    )
    .await
    else {
        return;
    };
    assert_eq!(updates.said_new, SaidNew::WhatSheHad);
    assert_eq!(updates.updates.len(), 1);
    assert_eq!(updates.updates[0].fact.as_deref(), Some("周六去看牙"));
    assert_eq!(
        updates.updates[0].supersedes,
        vec!["周五去看牙".to_string()]
    );
}

/// Something she had said again beside what the line lets go of, about
/// another thing: no correction to it.
#[tokio::test]
async fn something_she_had_said_again_beside_a_taking_back_corrects_nothing() {
    let Some(updates) = judged(
        "chat_already_said_again",
        ("养了一只猫叫年糕", "猫咪"),
        ("养了一只猫", "猫咪"),
    )
    .await
    else {
        return;
    };
    assert_eq!(updates.said_new, SaidNew::Something);
    assert_eq!(updates.updates.len(), 1);
    assert_eq!(updates.updates[0].fact, None);
    assert_eq!(
        updates.updates[0].supersedes,
        vec!["周五去看牙".to_string()]
    );
}
