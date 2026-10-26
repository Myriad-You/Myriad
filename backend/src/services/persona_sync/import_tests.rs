use super::*;
use sea_orm::ConnectionTrait;

/// A site with her, for the owner 41: a persona, one memory of the owner's,
/// one private conversation with a line cut off, her affect, her pace and
/// which set she wears.
async fn seed(db: &DatabaseConnection) {
    for statement in [
        "INSERT INTO users (id, username, is_admin, is_owner) VALUES (41, 'owner41', true, true)",
        "INSERT INTO agent_persona (id, name, personality, zone, visual_profile, updated_at) \
         VALUES ('site', 'Merope', '安静', 'Asia/Tokyo', \
         '{\"activeOutfitId\":\"default\",\"wardrobe\":[{\"id\":\"default\",\"clothingStyle\":\"everyday\",\"outfit\":{}}]}'::jsonb, \
         '2026-10-09T00:00:00Z')",
        "INSERT INTO agent_memories (id, user_id, kind, content, evidence, speaker, source, venue, \
         audience, concepts, valid_from, created_at, updated_at) VALUES \
         ('m_owner', 41, 'fact', '她喜欢雨天', NULL, 'user', 'chat', 'private', '[41]', '[]', \
          '2026-10-02Z', '2026-10-02Z', '2026-10-02Z')",
        "INSERT INTO agent_sessions (id, user_id, title, context, message_count, archived, \
         created_at, last_active_at) VALUES \
         ('s_owner', 41, '雨', '{\"mode\":\"chat\"}', 2, false, '2026-10-09Z', '2026-10-09Z')",
        "INSERT INTO agent_messages (session_id, role, content, metadata, created_at) VALUES \
         ('s_owner', 'user', '下雨了', NULL, '2026-10-09T00:00:00Z'), \
         ('s_owner', 'assistant', '带伞了吗', '{\"cutOff\":\"unheard_end\",\"heardChars\":2}', \
          '2026-10-09T00:00:01Z')",
        "INSERT INTO agent_addressee_state (user_id, mood, arousal, emotion, emotion_arousal, \
         activity, mood_settled_at, emotion_settled_at, updated_at) VALUES \
         (41, 40, 50, 60, 50, 'coding', '2026-10-09Z', '2026-10-09Z', '2026-10-09Z')",
        "INSERT INTO runtime_registry (namespace, record_id, payload, expires_at, updated_at) \
         VALUES ('merope_pace', 'lazed:2026-10-09', '12.5', 9999999999, '2026-10-09Z')",
    ] {
        db.execute_unprepared(statement).await.unwrap();
    }
}

/// A site with its owner and no her: the owner was already seen there (a row
/// of the owner's state exists before she comes).
async fn owner_only(db: &DatabaseConnection) {
    db.execute_unprepared(
        "INSERT INTO users (id, username, is_admin, is_owner) VALUES (41, 'owner41', true, true); \
         INSERT INTO agent_addressee_state (user_id, updated_at) VALUES (41, NOW())",
    )
    .await
    .unwrap();
}

async fn one<T: sea_orm::TryGetable>(db: &DatabaseConnection, sql: &str, column: &str) -> T {
    db.query_one_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        sql.to_string(),
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get("", column)
    .unwrap()
}

/// Her, exported from one site and brought into two empty ones (isolated
/// schemas on a real database; skipped without a test database): from the
/// file, uploaded in pieces and opened with the passphrase, and from a
/// device, page by page after the owner confirmed. Each lands whole, keeps
/// her lineage and goes one epoch on; a wrong passphrase, a piece out of
/// turn, a manifest that is not the confirmed one and a site that has her
/// already are refused; an import called off leaves the site as it was.
#[tokio::test]
async fn she_comes_into_an_empty_site_from_a_file_or_a_device() {
    let Some(source) = crate::federation::test_db::SchemaDb::new_or_media().await else {
        return;
    };
    seed(&source.db).await;
    let born = lease::read(&source.db).await.unwrap().unwrap();
    let file = super::super::export::export(&source.db, 41, "雨天的伞".into())
        .await
        .unwrap();
    let opened = archive::open("雨天的伞", &file.bytes).unwrap();
    source.close().await;

    // From the file.
    let Some(target) = crate::federation::test_db::SchemaDb::new_or_media().await else {
        return;
    };
    let db = &target.db;
    owner_only(db).await;
    assert!(not_empty(db, 41).await.unwrap().is_none());
    let session = begin_file(db, 41, file.bytes.len() as u64).await.unwrap();
    let half = file.bytes.len() / 2;
    assert!(matches!(
        take_piece(db, &session.id, 7, file.bytes[..half].to_vec()).await,
        Err(ImportError::Size)
    ));
    take_piece(db, &session.id, 0, file.bytes[..half].to_vec())
        .await
        .unwrap();
    assert!(matches!(
        open_file(db, 41, &session.id, "雨天的伞".into(), false).await,
        Err(ImportError::Phase(_))
    ));
    let whole = take_piece(db, &session.id, half as u64, file.bytes[half..].to_vec())
        .await
        .unwrap();
    assert_eq!(whole.received as usize, file.bytes.len());
    assert!(matches!(
        open_file(db, 41, &session.id, "晴天的伞".into(), false).await,
        Err(ImportError::Unreadable)
    ));
    let landed = open_file(db, 41, &session.id, "雨天的伞".into(), false)
        .await
        .unwrap();
    assert_eq!(landed.counts.get("memory"), Some(&1));
    assert_eq!(landed.counts.get("conversation"), Some(&1));
    assert_eq!(landed.counts.get("persona"), Some(&1));
    let here = lease::read(db).await.unwrap().unwrap();
    assert_eq!(
        (here.holder.as_str(), here.lineage.as_str(), here.epoch),
        ("site", born.lineage.as_str(), born.epoch + 1)
    );
    let name: String = one(db, "SELECT name FROM agent_persona", "name").await;
    assert_eq!(name, "Merope");
    let cut: Value = one(
        db,
        "SELECT metadata::jsonb AS m FROM agent_messages WHERE role = 'assistant'",
        "m",
    )
    .await;
    assert_eq!(cut["cutOff"], "unheard_end");
    let mood: f64 = one(db, "SELECT mood FROM agent_addressee_state", "mood").await;
    assert!(mood.is_finite());
    let lazed: Value = one(
        db,
        "SELECT payload FROM runtime_registry WHERE record_id = 'lazed:2026-10-09'",
        "payload",
    )
    .await;
    assert_eq!(lazed, json!(12.5));
    // She is here now: another import is refused with her name.
    match begin_file(db, 41, 10).await {
        Err(ImportError::NotEmpty(name)) => assert_eq!(name.as_deref(), Some("Merope")),
        other => panic!("{other:?}"),
    }
    target.close().await;

    // From a device.
    let Some(target) = crate::federation::test_db::SchemaDb::new_or_media().await else {
        return;
    };
    let db = &target.db;
    owner_only(db).await;
    let (mac, _) = super::super::devices::add(db, 41, "Mac").await.unwrap();
    let manifest = Manifest::of_envelopes(&opened.envelopes).unwrap();
    let proposal = |manifest: Manifest| Proposal {
        lineage: born.lineage.clone(),
        epoch: 3,
        name: Some("Merope".into()),
        manifest,
        kind: None,
        subject: None,
        preview: Vec::new(),
    };
    // Called off after the confirmation: no her, no lease.
    let asked = propose(db, 41, &mac.id, proposal(manifest.clone()))
        .await
        .unwrap();
    assert!(matches!(
        take_page(db, &asked.id, &mac.id, opened.envelopes[0].clone()).await,
        Err(ImportError::Phase(_))
    ));
    let frozen = confirm(db, 41, &asked.id, None, false).await.unwrap();
    assert_eq!(
        (frozen.holder.as_str(), frozen.state, frozen.epoch),
        (mac.id.as_str(), lease::State::Frozen, 3)
    );
    let called_off = lease::cancel(db, &asked.id, None).await.unwrap();
    assert!(called_off.is_none());
    assert!(lease::read(db).await.unwrap().is_none());
    assert!(current(db).await.unwrap().is_none());

    let asked = propose(db, 41, &mac.id, proposal(manifest.clone()))
        .await
        .unwrap();
    confirm(db, 41, &asked.id, None, false).await.unwrap();
    for page in &opened.envelopes {
        take_page(db, &asked.id, &mac.id, page.clone())
            .await
            .unwrap();
        // A page sent twice is the same page.
        take_page(db, &asked.id, &mac.id, page.clone())
            .await
            .unwrap();
    }
    assert!(matches!(
        commit(db, 41, &asked.id, &mac.id, &"0".repeat(64)).await,
        Err(ImportError::Mismatch(_))
    ));
    assert!(matches!(
        commit(db, 41, &asked.id, "dev_other", &manifest.digest).await,
        Err(ImportError::Unknown)
    ));
    let landed = commit(db, 41, &asked.id, &mac.id, &manifest.digest)
        .await
        .unwrap();
    assert_eq!(landed.counts.get("memory"), Some(&1));
    let here = lease::read(db).await.unwrap().unwrap();
    assert_eq!(
        (
            here.holder.as_str(),
            here.lineage.as_str(),
            here.epoch,
            here.state
        ),
        ("site", born.lineage.as_str(), 4, lease::State::Live)
    );
    assert!(here.moving.is_none());
    // A commit asked again after she landed answers, and changes nothing.
    commit(db, 41, &asked.id, &mac.id, &manifest.digest)
        .await
        .unwrap();
    assert_eq!(lease::read(db).await.unwrap().unwrap().epoch, 4);
    let staged: i64 = one(
        db,
        "SELECT COUNT(*)::BIGINT AS n FROM agent_host_import_staging",
        "n",
    )
    .await;
    assert_eq!(staged, 0);
    target.close().await;
}

/// The Mac's her (`fixtures/merge/other.json`, her pictures left out: a
/// picture that is not sent is not taken), as one page from `dev`.
fn mac_her(lineage: &str, dev: &str) -> Envelope {
    let path = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../core/crates/myriad-persona-sync/fixtures/merge/other.json");
    let mut entities: Vec<EntityVersion> =
        serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    for entity in &mut entities {
        if entity.kind == Kind::Outfit {
            entity.body["portrait"] = Value::Null;
            entity.digest.clear();
        }
        let text = serde_json::to_string(&*entity)
            .unwrap()
            .replace("dev_mac1", dev);
        *entity = serde_json::from_str(&text).unwrap();
        entity.digest.clear();
    }
    myriad_persona_sync::canonicalize_envelope(Envelope {
        format: myriad_persona_sync::FORMAT.into(),
        format_version: myriad_persona_sync::FORMAT_VERSION,
        lineage: Some(lineage.into()),
        origin: dev.into(),
        embedding_model: None,
        entities,
        embeddings: Vec::new(),
        assets: Vec::new(),
    })
    .unwrap()
}

/// A Mac with a her of its own (Luna) meets the site's (Merope), the owner
/// having chosen the Mac's (H3d): nothing moves until the owner confirms
/// with a passphrase, which seals the site's her into `retired/` first; she
/// lives on while the pages arrive; then she is one: Luna's persona with
/// Merope's values in her history (`subject`), both sides' memories and
/// conversations, the Mac's proposal for that Mac, the site's lineage one
/// epoch on. Turned down, the site is as it was and the sealed copy goes.
#[tokio::test]
async fn a_macs_her_is_merged_into_the_sites() {
    let Some(target) = crate::federation::test_db::SchemaDb::new_or_media().await else {
        return;
    };
    let db = &target.db;
    seed(db).await;
    let born = lease::read(db).await.unwrap().unwrap();
    let (mac, _) = super::super::devices::add(db, 41, "书房的 Mac")
        .await
        .unwrap();
    let page = mac_her("0b1e3f6a-mac-born", &mac.id);
    let manifest = Manifest::of_envelopes(std::slice::from_ref(&page)).unwrap();
    let preview: Vec<EntityVersion> = page
        .entities
        .iter()
        .filter(|entity| matches!(entity.kind, Kind::Persona | Kind::Outfit | Kind::Wearing))
        .cloned()
        .collect();
    let proposal = |subject: Option<&str>| Proposal {
        lineage: "0b1e3f6a-mac-born".into(),
        epoch: 1,
        name: Some("Luna".into()),
        manifest: manifest.clone(),
        kind: Some("merge".into()),
        subject: subject.map(str::to_string),
        preview: preview.clone(),
    };

    // Turned down: the site is as it was, the sealed copy gone.
    let asked = propose(db, 41, &mac.id, proposal(Some("device")))
        .await
        .unwrap();
    let shown = shown(db, 41).await.unwrap();
    assert_eq!(shown["kind"], "merge");
    assert_eq!(shown["merge"]["names"]["device"], "Luna");
    assert!(
        shown["merge"]["fields"]
            .as_array()
            .unwrap()
            .iter()
            .any(|field| field["field"] == "personality" && field["toHistory"] == "host"),
        "{shown}"
    );
    assert!(matches!(
        confirm(db, 41, &asked.id, None, false).await,
        Err(ImportError::Invalid("passphrase"))
    ));
    confirm(db, 41, &asked.id, Some("合并前".into()), false)
        .await
        .unwrap();
    let sealed = super::super::merge::retired_dir()
        .join(format!("her-before-merge-{}.her-archive", asked.id));
    assert!(sealed.is_file());
    // She lives on while the pages come.
    assert_eq!(
        lease::read(db).await.unwrap().unwrap().state,
        lease::State::Live
    );
    cancel(db, &asked.id, None).await.unwrap();
    assert!(!sealed.exists());
    let here = lease::read(db).await.unwrap().unwrap();
    assert!(here.moving.is_none());
    assert_eq!(here.epoch, born.epoch);

    // Confirmed and sent: she is one.
    let asked = propose(db, 41, &mac.id, proposal(Some("device")))
        .await
        .unwrap();
    confirm(db, 41, &asked.id, Some("合并前".into()), false)
        .await
        .unwrap();
    take_page(db, &asked.id, &mac.id, page.clone())
        .await
        .unwrap();
    assert_eq!(
        lease::read(db).await.unwrap().unwrap().state,
        lease::State::Live
    );
    let landed = commit(db, 41, &asked.id, &mac.id, &manifest.digest)
        .await
        .unwrap();
    assert_eq!(landed.counts.get("memory"), Some(&2));
    let here = lease::read(db).await.unwrap().unwrap();
    assert_eq!(
        (
            here.holder.as_str(),
            here.lineage.as_str(),
            here.epoch,
            here.state
        ),
        (
            "site",
            born.lineage.as_str(),
            born.epoch + 1,
            lease::State::Live
        )
    );
    assert!(here.moving.is_none());
    let name: String = one(db, "SELECT name FROM agent_persona", "name").await;
    assert_eq!(name, "Luna");
    let reason: String = one(
        db,
        "SELECT reason FROM agent_persona_field_history WHERE field = 'name'",
        "reason",
    )
    .await;
    assert_eq!(reason, "subject");
    let old: Value = one(
        db,
        "SELECT value FROM agent_persona_field_history WHERE field = 'name'",
        "value",
    )
    .await;
    assert_eq!(old, json!("Merope"));
    let sessions: i64 = one(db, "SELECT COUNT(*)::BIGINT AS n FROM agent_sessions", "n").await;
    assert_eq!(sessions, 2);
    let place: Value = one(
        db,
        "SELECT proposal FROM agent_intentions WHERE id = 'int_mac'",
        "proposal",
    )
    .await;
    assert_eq!(place["place"], "mac");
    assert_eq!(place["device_id"], json!(mac.id));
    let wardrobe: Value = one(
        db,
        "SELECT visual_profile FROM agent_persona",
        "visual_profile",
    )
    .await;
    let ids: Vec<&str> = wardrobe["wardrobe"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|item| item["id"].as_str())
        .collect();
    assert_eq!(ids.len(), 3, "{wardrobe}");
    assert!(ids.contains(&"default") && ids.contains(&"mac-hat"));
    assert_eq!(wardrobe["activeOutfitId"], "default");
    let mood: f64 = one(db, "SELECT mood FROM agent_addressee_state", "mood").await;
    assert!(mood.is_finite());
    let sealed = super::super::merge::retired_dir()
        .join(format!("her-before-merge-{}.her-archive", asked.id));
    assert!(sealed.is_file(), "the site's her before the merge is kept");
    let _ = std::fs::remove_file(&sealed);
    target.close().await;
}
