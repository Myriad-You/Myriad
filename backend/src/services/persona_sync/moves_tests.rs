use super::*;
use myriad_persona_sync::archive;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use super::super::{backups, import, lease};

/// A site with her and with others: the owner 41 and a member 42; her own
/// memory of the owner and the member's private one, a group's memory, how
/// she feels toward each of them, her sticker and a group's, her own Work
/// switched on for the owner, and a message held for her from a chat app.
async fn seed(db: &DatabaseConnection) {
    for statement in [
        "INSERT INTO users (id, username, is_admin, is_owner) VALUES (41, 'owner41', true, true), \
         (42, 'member42', false, false)",
        "INSERT INTO agent_persona (id, name, personality, zone, visual_profile, updated_at) \
         VALUES ('site', 'Merope', '安静', 'Asia/Tokyo', \
         '{\"activeOutfitId\":\"default\",\"wardrobe\":[{\"id\":\"default\",\"clothingStyle\":\"everyday\",\"outfit\":{}}]}'::jsonb, \
         '2026-10-09T00:00:00Z')",
        "INSERT INTO agent_memories (id, user_id, kind, content, evidence, speaker, source, venue, \
         audience, concepts, valid_from, created_at, updated_at) VALUES \
         ('m_owner', 41, 'fact', '她喜欢雨天', NULL, 'user', 'chat', 'private', '[41]', '[]', \
          '2026-10-02Z', '2026-10-02Z', '2026-10-02Z'), \
         ('m_member', 42, 'fact', '他养了一只猫', NULL, 'user', 'chat', 'private', '[42]', '[]', \
          '2026-10-02Z', '2026-10-02Z', '2026-10-02Z'), \
         ('m_group', 42, 'fact', '群里在说新番', NULL, 'user', 'chat', 'telegram:-1', '[42]', '[]', \
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
         (41, 40, 50, 60, 50, 'coding', '2026-10-09Z', '2026-10-09Z', '2026-10-09Z'), \
         (42, 55, 50, 50, 50, 'idle', '2026-10-09Z', '2026-10-09Z', '2026-10-09Z')",
        "INSERT INTO agent_autonomy_grants (user_id, allowed_permissions, revoked, created_at, \
         updated_at) VALUES (41, '[\"web.search\"]', false, NOW(), NOW())",
        "INSERT INTO agent_channel_held (platform, session_key, inbound_id, user_id, text) \
         VALUES ('qq', 'qq:p:1', 'in-1', 42, '在吗')",
    ] {
        db.execute_unprepared(statement).await.unwrap();
    }
}

async fn count(db: &DatabaseConnection, sql: &str) -> i64 {
    db.query_one_raw(Statement::from_string(
        DatabaseBackend::Postgres,
        sql.to_string(),
    ))
    .await
    .unwrap()
    .unwrap()
    .try_get::<i64>("", "n")
    .unwrap()
}

async fn whole_file(db: &DatabaseConnection, id: &str, device: &str) -> Vec<u8> {
    let shown = shown_to_device(db, id, device).await.unwrap();
    let total = shown["archive"]["bytes"].as_u64().unwrap();
    let mut bytes = Vec::new();
    while (bytes.len() as u64) < total {
        let got = piece(db, id, device, bytes.len() as u64, 1000)
            .await
            .unwrap();
        assert!(!got.is_empty());
        bytes.extend(got);
    }
    bytes
}

/// She moves to a Mac and back (isolated schemas on a real database;
/// skipped without one): the device agrees and the owner confirms with a
/// passphrase; she is sealed twice, the device reads its copy in pieces,
/// keeps it apart, pushes its first backup and commits. The site's own
/// parts of her go then and others' stay sealed; held mail stays. Back, the
/// device freezes, reports what it froze and sends her; she lands one epoch
/// on with her lineage, takes up what she left, and her own Work is off on
/// the site she arrived at (H3e-17).
#[tokio::test]
async fn she_moves_to_a_device_and_back_and_finds_others_as_she_left_them() {
    let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
        return;
    };
    let db = &fixture.db;
    seed(db).await;
    let (mac, _) = super::super::devices::add(db, 41, "Mac").await.unwrap();
    let born = lease::read(db).await.unwrap().unwrap();
    assert_eq!(born.epoch, 1);

    let started = lease::start(db, lease::Kind::Move, &mac.id, lease::Starter::Owner)
        .await
        .unwrap();
    let id = started.moving.unwrap().id;
    // Not before the device agreed.
    assert!(matches!(
        confirm_to_device(db, 41, &id, "留在站点的".into()).await,
        Err(MoveError::NotAgreed)
    ));
    assert_eq!(
        lease::read(db).await.unwrap().unwrap().state,
        lease::State::Live
    );
    lease::agree(db, &id, &mac.id).await.unwrap();
    let sealed = confirm_to_device(db, 41, &id, "留在站点的".into())
        .await
        .unwrap();
    assert_eq!(sealed.state, lease::State::Frozen);
    let digest = sealed.moving.as_ref().unwrap().digest.clone().unwrap();

    // The device reads its copy in pieces; the key opens it, nothing else.
    let shown = shown_to_device(db, &id, &mac.id).await.unwrap();
    let key = shown["archive"]["key"].as_str().unwrap().to_string();
    assert!(matches!(
        shown_to_device(db, &id, "dev_other").await,
        Err(MoveError::Unknown)
    ));
    let bytes = whole_file(db, &id, &mac.id).await;
    assert!(archive::open("留在站点的", &bytes, false).is_err());
    let opened = archive::open(&key, &bytes, false).unwrap();
    assert_eq!(opened.manifest.digest, digest);
    // The copy the site keeps opens with the owner's passphrase.
    let kept = tokio::fs::read(super::super::merge::retired_dir().join(retired_name(&id)))
        .await
        .unwrap();
    assert_eq!(
        archive::open("留在站点的", &kept, false)
            .unwrap()
            .manifest
            .digest,
        digest
    );

    assert!(matches!(
        applied(db, &id, &mac.id, &"0".repeat(64)).await,
        Err(MoveError::Mismatch)
    ));
    applied(db, &id, &mac.id, &digest).await.unwrap();
    // No commit before the device's first backup is here.
    assert!(matches!(
        commit_to_device(db, 41, &id, &mac.id, &digest).await,
        Err(MoveError::BackupMissing)
    ));
    backups::take_piece(
        db,
        &mac.id,
        "2026-10-14",
        backups::PieceQuery {
            offset: 0,
            total: bytes.len() as u64,
            lineage: born.lineage.clone(),
            epoch: 2,
            digest: digest.clone(),
        },
        bytes.clone(),
    )
    .await
    .unwrap();
    let committed = commit_to_device(db, 41, &id, &mac.id, &digest)
        .await
        .unwrap();
    assert_eq!(
        (committed.holder.as_str(), committed.epoch, committed.state),
        (mac.id.as_str(), 2, lease::State::Live)
    );
    assert_eq!(committed.lineage, born.lineage);
    // Asked again: the same answer.
    assert_eq!(
        commit_to_device(db, 41, &id, &mac.id, &digest)
            .await
            .unwrap()
            .epoch,
        2
    );

    // Her own parts went; others' stay, and so does what was held for her.
    assert_eq!(
        count(db, "SELECT COUNT(*)::BIGINT AS n FROM agent_persona").await,
        0
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*)::BIGINT AS n FROM agent_memories WHERE id = 'm_owner'"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*)::BIGINT AS n FROM agent_memories WHERE id IN ('m_member', 'm_group')"
        )
        .await,
        2
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*)::BIGINT AS n FROM agent_addressee_state WHERE user_id = 41"
        )
        .await,
        0
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*)::BIGINT AS n FROM agent_addressee_state WHERE user_id = 42"
        )
        .await,
        1
    );
    assert_eq!(
        count(db, "SELECT COUNT(*)::BIGINT AS n FROM agent_sessions").await,
        0
    );
    assert_eq!(
        count(db, "SELECT COUNT(*)::BIGINT AS n FROM agent_channel_held").await,
        1
    );
    let mut backups_listed = backups::listed(db).await.unwrap();
    assert_eq!(backups_listed.len(), 1);
    assert_eq!(backups_listed.remove(0)["deviceId"], mac.id.as_str());

    // Back to the site: the device asks, the owner confirms, it reports
    // what it froze and sends her.
    let asked = lease::start_back(db, &mac.id, lease::Starter::Device)
        .await
        .unwrap();
    let back = asked.moving.unwrap().id;
    lease::confirm(db, &back).await.unwrap();
    let frozen = Frozen {
        lineage: born.lineage.clone(),
        epoch: 2,
        name: Some("Merope".into()),
        manifest: opened.manifest.clone(),
    };
    let session = report_frozen(db, &back, &mac.id, frozen).await.unwrap();
    // Said again: the same import.
    let again = report_frozen(
        db,
        &back,
        &mac.id,
        Frozen {
            lineage: born.lineage.clone(),
            epoch: 2,
            name: None,
            manifest: opened.manifest.clone(),
        },
    )
    .await
    .unwrap();
    assert_eq!(again.id, session.id);
    for page in &opened.envelopes {
        import::take_page(db, &session.id, &mac.id, page.clone())
            .await
            .unwrap();
    }
    import::commit(db, 41, &session.id, &mac.id, &opened.manifest.digest)
        .await
        .unwrap();
    let home = lease::read(db).await.unwrap().unwrap();
    assert_eq!(
        (
            home.holder.as_str(),
            home.epoch,
            home.state,
            home.lineage.as_str()
        ),
        ("site", 3, lease::State::Live, born.lineage.as_str())
    );
    assert_eq!(
        count(db, "SELECT COUNT(*)::BIGINT AS n FROM agent_persona").await,
        1
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*)::BIGINT AS n FROM agent_memories \
             WHERE id IN ('m_member', 'm_group') OR content = '她喜欢雨天'"
        )
        .await,
        3,
        "her own back, others' as she left them"
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*)::BIGINT AS n FROM agent_autonomy_grants"
        )
        .await,
        0,
        "her own Work starts off where she arrived"
    );
    fixture.close().await;
}

/// Taken back from a backup by another device while the one she lived on
/// is away, and a site she left that keeps another her's lineage (isolated
/// schemas; skipped without a database): the reclaim lands one epoch past
/// the lease's; a file of another lineage is refused until the owner lets
/// what was left go, and then lands on a site with none of it.
#[tokio::test]
async fn she_is_taken_back_from_a_backup_and_another_lineage_replaces_what_was_left() {
    let Some(source) = crate::federation::test_db::SchemaDb::new_or_media().await else {
        return;
    };
    seed(&source.db).await;
    let born = lease::read(&source.db).await.unwrap().unwrap();
    let file = super::super::export::export(&source.db, 41, "备份口令".into())
        .await
        .unwrap();
    let opened = archive::open("备份口令", &file.bytes, false).unwrap();
    source.close().await;

    let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
        return;
    };
    let db = &fixture.db;
    seed(db).await;
    let (a, _) = super::super::devices::add(db, 41, "A").await.unwrap();
    let (b, _) = super::super::devices::add(db, 41, "B").await.unwrap();
    // She lives on A at epoch 5 with the source's lineage, and the site let
    // its own copy go.
    db.execute_unprepared(&format!(
        "DELETE FROM agent_host_lease; INSERT INTO agent_host_lease (id, holder, lineage, epoch, \
         state, updated_at) VALUES ('her', '{}', '{}', 5, 'live', NOW())",
        a.id, born.lineage
    ))
    .await
    .unwrap();
    crate::services::agent::merope::retire_her_on(db, 41)
        .await
        .unwrap();
    // B asks to take her back from a backup (epoch 4 in it).
    let proposal = import::Proposal {
        lineage: born.lineage.clone(),
        epoch: 4,
        name: Some("Merope".into()),
        manifest: opened.manifest.clone(),
        kind: Some("recover".into()),
        subject: None,
        preview: Vec::new(),
    };
    let asked = import::propose(db, 41, &b.id, proposal).await.unwrap();
    let held = lease::read(db).await.unwrap().unwrap();
    let moving = held.moving.clone().unwrap();
    assert_eq!(
        (
            moving.kind,
            moving.from.as_str(),
            moving.via.as_deref(),
            held.state
        ),
        (
            lease::Kind::Recover,
            a.id.as_str(),
            Some(b.id.as_str()),
            lease::State::Live
        )
    );
    import::confirm(db, 41, &asked.id, None, false)
        .await
        .unwrap();
    assert_eq!(
        lease::read(db).await.unwrap().unwrap().state,
        lease::State::Frozen
    );
    for page in &opened.envelopes {
        import::take_page(db, &asked.id, &b.id, page.clone())
            .await
            .unwrap();
    }
    import::commit(db, 41, &asked.id, &b.id, &opened.manifest.digest)
        .await
        .unwrap();
    let here = lease::read(db).await.unwrap().unwrap();
    assert_eq!(
        (here.holder.as_str(), here.epoch, here.lineage.as_str()),
        ("site", 6, born.lineage.as_str()),
        "one past the lease's, whatever the backup said"
    );

    // She moves out and is let go: vacant, others' parts left with her lineage.
    lease::move_out(db).await.unwrap();
    {
        use sea_orm::TransactionTrait;
        let txn = db.begin().await.unwrap();
        lease::let_go_on(&txn).await.unwrap();
        crate::services::agent::merope::retire_her_on(&txn, 41)
            .await
            .unwrap();
        txn.commit().await.unwrap();
    }
    // A file of another lineage: the site keeps what was left of this one.
    db.execute_unprepared(
        "DELETE FROM agent_host_lease; INSERT INTO agent_host_lease (id, holder, lineage, epoch, \
         state, updated_at) VALUES ('her', 'site', 'other-lineage', 2, 'vacant', NOW())",
    )
    .await
    .unwrap();
    let session = import::begin_file(db, 41, file.bytes.len() as u64)
        .await
        .unwrap();
    import::take_piece(db, &session.id, 0, file.bytes.clone())
        .await
        .unwrap();
    assert!(matches!(
        import::open_file(db, 41, &session.id, "备份口令".into(), false).await,
        Err(import::ImportError::ReplacesSealed)
    ));
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*)::BIGINT AS n FROM agent_memories WHERE id = 'm_group'"
        )
        .await,
        1
    );
    import::open_file(db, 41, &session.id, "备份口令".into(), true)
        .await
        .unwrap();
    let landed = lease::read(db).await.unwrap().unwrap();
    assert_eq!(
        (landed.holder.as_str(), landed.lineage.as_str()),
        ("site", born.lineage.as_str())
    );
    assert_eq!(
        count(
            db,
            "SELECT COUNT(*)::BIGINT AS n FROM agent_memories WHERE id IN ('m_group', 'm_member')"
        )
        .await,
        0,
        "what was left of the other her went"
    );
    assert_eq!(
        count(db, "SELECT COUNT(*)::BIGINT AS n FROM agent_channel_held").await,
        1
    );
    fixture.close().await;
}

/// The newest seven backups of her lineage are kept (isolated schema).
#[tokio::test]
async fn seven_backups_are_kept() {
    let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
        return;
    };
    let db = &fixture.db;
    db.execute_unprepared(
        "INSERT INTO users (id, username, is_admin, is_owner) VALUES (41, 'o', true, true)",
    )
    .await
    .unwrap();
    let (mac, _) = super::super::devices::add(db, 41, "Mac").await.unwrap();
    db.execute_unprepared(&format!(
        "INSERT INTO agent_host_lease (id, holder, lineage, epoch, state, updated_at) \
         VALUES ('her', '{}', 'l-7', 2, 'live', NOW())",
        mac.id
    ))
    .await
    .unwrap();
    for day in 1..=9 {
        let bytes = [day as u8; 10];
        let query = |offset: u64| backups::PieceQuery {
            offset,
            total: 10,
            lineage: "l-7".into(),
            epoch: 2,
            digest: "a".repeat(64),
        };
        // In two pieces; a piece out of turn is refused.
        assert!(
            backups::take_piece(
                db,
                &mac.id,
                &format!("2026-10-{day:02}"),
                query(4),
                bytes[..4].to_vec()
            )
            .await
            .is_err()
        );
        backups::take_piece(
            db,
            &mac.id,
            &format!("2026-10-{day:02}"),
            query(0),
            bytes[..4].to_vec(),
        )
        .await
        .unwrap();
        let whole = backups::take_piece(
            db,
            &mac.id,
            &format!("2026-10-{day:02}"),
            query(4),
            bytes[4..].to_vec(),
        )
        .await
        .unwrap();
        assert_eq!(whole["complete"], true);
    }
    let listed = backups::listed(db).await.unwrap();
    assert_eq!(listed.len(), 7);
    assert_eq!(listed[0]["day"], "2026-10-09");
    assert_eq!(listed[6]["day"], "2026-10-03");
    let first = listed[0]["id"].as_str().unwrap().to_string();
    assert_eq!(
        backups::piece(db, &first, 2, 100).await.unwrap(),
        vec![9u8; 8]
    );
    // Not from a device she does not live on.
    let (other, _) = super::super::devices::add(db, 41, "Other").await.unwrap();
    assert!(matches!(
        backups::take_piece(
            db,
            &other.id,
            "2026-10-10",
            backups::PieceQuery {
                offset: 0,
                total: 1,
                lineage: "l-7".into(),
                epoch: 2,
                digest: "a".repeat(64)
            },
            vec![1],
        )
        .await,
        Err(backups::BackupError::NotHost)
    ));
    fixture.close().await;
}
