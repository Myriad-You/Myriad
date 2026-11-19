use super::*;

/// What one person left on her mind is theirs to see: another person, a
/// group and what she writes of her own time see only that something about
/// someone else is there. What she says stays takes the place of what did;
/// saying nothing changes nothing.
#[tokio::test]
async fn what_someone_left_on_her_mind_is_only_theirs_to_see() {
    let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
        return;
    };
    let isolated = crate::db::IsolatedSchema::migrated(&url, "her_on_mind").await;
    let db = isolated.db.clone();
    let (jie, cheng) = (7001, 7002);

    assert_eq!(seen_by(&db, Before::Private(jie)).await, None, "nothing on her mind");
    offered(&db, None, Some(jie), FROM_TALK).await;
    assert_eq!(now(&db).await, None, "she said nothing stays");

    offered(&db, Some("阿杰下周的面试".into()), Some(jie), FROM_TALK).await;
    let to_him = seen_by(&db, Before::Private(jie)).await.unwrap();
    assert!(to_him.contains("阿杰下周的面试"), "{to_him}");
    for before in [Before::Private(cheng), Before::Anyone] {
        let seen = seen_by(&db, before).await.unwrap();
        assert!(!seen.contains("阿杰") && !seen.contains("面试"), "{seen}");
        assert!(seen.contains("someone else"), "{seen}");
    }

    offered(&db, Some("那首歌的最后一句".into()), None, "song:netease:1").await;
    let mind = now(&db).await.unwrap();
    assert_eq!((mind.what.as_str(), mind.about), ("那首歌的最后一句", None));
    assert!(seen_by(&db, Before::Private(cheng)).await.unwrap().contains("那首歌的最后一句"));
    // Said again, it stays as it was, with how far she had got.
    let mut further = mind.clone().stepped("也许是因为没唱完", crate::clock::now());
    further.thought_at = mind.thought_at;
    keep(&db, &further).await;
    offered(&db, Some(" 那首歌的最后一句 ".into()), Some(jie), FROM_TALK).await;
    assert_eq!(now(&db).await.unwrap(), further);
    let mind = further;
    assert!(!mind.step_due(crate::clock::now()), "just came to mind");

    // Thinking it further asks no one while it is not yet time.
    step(&db, 1).await;
    assert_eq!(now(&db).await.unwrap(), mind);
    isolated.drop().await;
}

/// Her notes on a song she had are what she has of it when she thinks it
/// further; what an exchange left her with brings nothing more.
#[tokio::test]
async fn she_thinks_it_further_from_her_own_notes_on_it() {
    let Some(url) = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"]) else {
        return;
    };
    let isolated = crate::db::IsolatedSchema::migrated(&url, "her_on_mind_notes").await;
    let db = isolated.db.clone();
    for (id, note) in [("1", "最后一句一直在耳边"), ("2", "别的歌")] {
        let evidence = serde_json::json!({
            "key": format!("song:netease:{id}"),
            "thing": {
                "kind": "song", "id": id, "source": "netease", "name": "晴天", "artist": "周杰伦",
                "album": "", "cover": "", "durationMs": 240000,
            },
            "reaction": "moved",
        });
        crate::services::agent::memory::unified::remember_own(
            &db,
            note,
            &evidence.to_string(),
            Vec::new(),
            crate::services::agent::memory::unified::OWN_EXPERIENCE,
        )
        .await
        .unwrap()
        .unwrap();
    }
    let notes = notes_on(&db, "song:netease:1").await;
    assert_eq!(notes.len(), 1, "{notes:?}");
    assert!(notes[0].contains("最后一句一直在耳边"), "{notes:?}");
    assert!(notes_on(&db, FROM_TALK).await.is_empty());
    isolated.drop().await;
}
