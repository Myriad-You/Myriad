//! Unit tests for `store.rs`, kept beside it so the module stays readable.

#[test]
fn what_they_said_is_plain_and_the_rest_says_how_she_knows() {
    let note =
        |source: &str, speaker: &str| crate::services::agent::memory::unified::MemoryRecord {
            id: "m".into(),
            user_id: Some(1),
            kind: "fact".into(),
            content: "养了一只猫叫年糕".into(),
            evidence: None,
            source: source.into(),
            speaker: speaker.into(),
            importance: 0.5,
            access_count: 0,
            created_at: chrono::Utc::now().fixed_offset(),
            brought_to_mind: false,
        };
    // This year's are without the year.
    let today = chrono::Utc::now().format("%m-%d").to_string();
    assert_eq!(
        super::as_known(&note("chat", "user")),
        format!("[{today}] 养了一只猫叫年糕")
    );
    assert!(super::as_known(&note("said", "agent")).ends_with("(what you told them)"));
    assert!(super::as_known(&note("event", "agent")).ends_with("not from them)"));
    assert!(super::as_known(&note("work", "agent")).contains("doing a task for them"));
    assert!(super::as_known(&note("chat", "import")).ends_with("(older)"));
    // What she let go, and what she noted of them in a group, say so.
    let with = |evidence: &str| crate::services::agent::memory::unified::MemoryRecord {
        evidence: Some(evidence.into()),
        ..note("chat", "agent")
    };
    assert!(
        super::as_known(&with(r#"{"since":"x","letGo":"y"}"#))
            .ends_with("(it happened between you)")
    );
    assert!(super::as_known(&with(r#"{"who":"telegram:42"}"#)).contains("in a group"));
}

#[test]
fn delayed_appraisal_requires_the_same_unexpired_persisted_input() {
    let input = chrono::Utc::now();
    let next = input + chrono::Duration::milliseconds(1);
    assert!(super::appraisal_is_current(
        Some(input.into()),
        input.into(),
        next
    ));
    assert!(!super::appraisal_is_current(
        Some(next.into()),
        input.into(),
        next
    ));
    assert!(!super::appraisal_is_current(None, input.into(), next));
    assert!(!super::appraisal_is_current(
        Some(input.into()),
        input.into(),
        input + chrono::Duration::seconds(13)
    ));
}

#[test]
fn persona_lookup_error_is_not_default_baseline() {
    let err = super::affect_baseline_from_persona_lookup(Err(anyhow::anyhow!("db down")));
    assert!(
        err.is_err(),
        "DB failure must not become the default personality"
    );
    let missing = super::affect_baseline_from_persona_lookup(Ok(None)).unwrap();
    assert_eq!(missing, super::AffectBaseline::default());
}

#[tokio::test]
#[ignore = "requires a disposable MEROPE_APPRAISAL_TEST_DATABASE_URL"]
async fn appraisal_commit_rechecks_input_after_waiting_for_the_database_lock() {
    use sea_orm::{ConnectionTrait, Database, Schema, TransactionTrait};
    let url = std::env::var("MEROPE_APPRAISAL_TEST_DATABASE_URL").expect("disposable DB URL");
    let db = Database::connect(url).await.unwrap();
    let name = db
        .query_one_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT current_database() AS name",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get::<String>("", "name")
        .unwrap();
    assert_eq!(
        name, "merope_appraisal_test",
        "refuse to create test tables in any other database"
    );
    let schema = Schema::new(sea_orm::DatabaseBackend::Postgres);
    for mut statement in [
        schema.create_table_from_entity(super::agent_persona::Entity),
        schema.create_table_from_entity(super::agent_addressee_state::Entity),
        // What still stings with them weighs on her mood toward them.
        schema.create_table_from_entity(crate::models::entities::agent_memories::Entity),
    ] {
        statement.if_not_exists();
        db.execute(&statement).await.unwrap();
    }
    // Each change of her affect is recorded beside it.
    db.execute_unprepared(&affect_log_ddl()).await.unwrap();
    let (_, first) = super::update_affect(
        &db,
        7001,
        true,
        super::AffectChange::utterance(0, false, false),
    )
    .await
    .unwrap();
    let input_at = first.last_user_message_at.unwrap();
    super::set_activity(&db, 7001, "talking").await.unwrap();
    let (_, applied) = super::update_utterance_appraisal(
        &db,
        7001,
        input_at,
        super::AffectChange::appraisal(myriad_merope::affect::lite_appraisal(2, 0)),
    )
    .await
    .unwrap()
    .expect("activity is not a new input");
    assert!(applied.updated_at.timestamp_millis() > first.updated_at.timestamp_millis());
    assert_eq!(applied.last_user_message_at, Some(input_at));

    let transaction = db.begin().await.unwrap();
    super::lock_addressee(&transaction, 7001).await.unwrap();
    let mut late = Box::pin(super::update_utterance_appraisal(
        &db,
        7001,
        input_at,
        super::AffectChange::task(false),
    ));
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(30), &mut late)
            .await
            .is_err()
    );
    let locked = super::get_or_create_state(&transaction, 7001)
        .await
        .unwrap();
    let second = super::save_affect_on(
        &transaction,
        locked,
        super::affect_from_state(&applied),
        true,
        false,
    )
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    assert!(
        late.await.unwrap().is_none(),
        "the old result must be checked after acquiring the lock"
    );
    let current = super::get_or_create_state(&db, 7001).await.unwrap();
    assert!(current.mood > 60.0);
    assert_eq!(current.last_user_message_at, second.last_user_message_at);

    let mut previous = second;
    for _ in 0..16 {
        let (_, current) = super::update_affect(
            &db,
            7001,
            true,
            super::AffectChange::utterance(0, false, false),
        )
        .await
        .unwrap();
        assert!(current.updated_at.timestamp_millis() > previous.updated_at.timestamp_millis());
        assert!(current.last_user_message_at > previous.last_user_message_at);
        previous = current;
    }
}

use super::*;
use sea_orm::{Database, TransactionTrait};
use serde_json::json;

#[test]
fn postgres_duplicate_key_is_unique_conflict() {
    assert!(is_unique_conflict(
        &"error returned from database: 23505 duplicate key value violates unique constraint \"agent_addressee_state_pkey\""
    ));
    assert!(!is_unique_conflict(&"connection reset"));
    assert!(!is_unique_conflict(&"null value in column unique_id"));
}

#[test]
fn concurrent_activity_only_moves_toward_the_busier_phase() {
    assert!(activity_rank("working") > activity_rank("thinking"));
    assert!(activity_rank("thinking") > activity_rank("talking"));
    assert!(activity_rank("talking") > activity_rank("idle"));
}

#[test]
fn changing_generation_inputs_invalidates_portrait_contract() {
    let mut changed_visual_profile = json!({
        "gender": "female",
        "visualIdentity": {
            "faceDesign": "女性化读取，紧凑圆润鹅蛋脸",
            "eyeDesign": "中等偏大的紫色宝石眼，视线坚定",
            "hairShape": "银灰齐颌短发与偏分刘海",
            "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓",
            "upperBodySilhouette": "紧凑肩线、清楚领口与胸前焦点",
            "outfitConstruction": "敞开领口内搭叠短外套并止于高腰",
            "sleeveArmDesign": "左右袖片携局部前臂进入画面",
            "materialPlan": "哑光布料",
            "heroAccessory": "左胸星轨扣饰",
            "paletteHint": "雾蓝为主、银白为辅、金色点缀",
            "motif": "单一星轨弧线集中在胸前"
        }
    });
    let existing_visual_profile = changed_visual_profile.clone();
    changed_visual_profile["visualIdentity"]["hairShape"] = json!("银灰高马尾与偏分刘海");
    let existing = agent_persona::Model {
        id: PERSONA_ROW_ID.to_string(),
        name: "Arael".to_string(),
        personality: "quiet".to_string(),
        persona_json: Some(json!({ "summary": "quiet" })),
        visual_profile: Some(existing_visual_profile),
        portrait_asset_id: Some("/master.png".to_string()),
        portrait_generation: Some(json!({ "fingerprint": "a".repeat(64) })),
        avatar_asset_id: None,
        avatar_generation: None,
        updated_by: Some(1),
        updated_at: Utc::now().into(),
        zone: String::new(),
        field_revs: serde_json::json!({}),
    };
    let active = apply_persona_update(
        existing,
        "Arael".to_string(),
        "quiet".to_string(),
        &PortraitUpdate::Keep,
        &PersonaContractUpdate {
            visual_profile: JsonDocumentUpdate::Set(changed_visual_profile),
            ..PersonaContractUpdate::default()
        },
        1,
    );
    assert_eq!(active.portrait_generation, Set(None));
    assert_eq!(active.portrait_asset_id, Set(None));
}

#[test]
fn generation_inputs_changed_without_a_portrait() {
    let mut next = json!({
        "gender": "female",
        "visualIdentity": {
            "faceDesign": "女性化读取，紧凑圆润鹅蛋脸",
            "eyeDesign": "中等偏大的紫色宝石眼，视线坚定",
            "hairShape": "银灰齐颌短发与偏分刘海",
            "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓",
            "upperBodySilhouette": "紧凑肩线、清楚领口与胸前焦点",
            "outfitConstruction": "敞开领口内搭叠短外套并止于高腰",
            "sleeveArmDesign": "左右袖片携局部前臂进入画面",
            "materialPlan": "哑光布料",
            "heroAccessory": "左胸星轨扣饰",
            "paletteHint": "雾蓝为主、银白为辅、金色点缀",
            "motif": "单一星轨弧线集中在胸前"
        }
    });
    let existing = next.clone();
    next["visualIdentity"]["hairShape"] = json!("银灰高马尾与偏分刘海");
    assert!(generation_inputs_changed(
        "Arael",
        Some(&existing),
        "Arael",
        &JsonDocumentUpdate::Set(next),
    ));
    assert!(!generation_inputs_changed(
        "Arael",
        Some(&existing),
        "Arael",
        &JsonDocumentUpdate::Keep,
    ));
}

#[test]
fn changing_spoken_persona_keeps_confirmed_visual_assets() {
    let existing = agent_persona::Model {
        id: PERSONA_ROW_ID.to_string(),
        name: "Arael".to_string(),
        personality: "quiet".to_string(),
        persona_json: Some(json!({ "summary": "quiet" })),
        visual_profile: Some(json!({ "gender": "unspecified" })),
        portrait_asset_id: Some("/master.png".to_string()),
        portrait_generation: Some(json!({ "fingerprint": "a".repeat(64) })),
        avatar_asset_id: None,
        avatar_generation: None,
        updated_by: Some(1),
        updated_at: Utc::now().into(),
        zone: String::new(),
        field_revs: serde_json::json!({}),
    };
    let active = apply_persona_update(
        existing,
        "Arael".to_string(),
        "more curious".to_string(),
        &PortraitUpdate::Keep,
        &PersonaContractUpdate {
            persona: JsonDocumentUpdate::Set(json!({
                "summary": "more curious"
            })),
            ..PersonaContractUpdate::default()
        },
        1,
    );
    assert_eq!(
        active.portrait_asset_id,
        sea_orm::ActiveValue::Unchanged(Some("/master.png".to_string()))
    );
    assert!(matches!(
        active.portrait_generation,
        sea_orm::ActiveValue::Unchanged(Some(_))
    ));
}

/// 一份完整到能通过 `appearance_visual_profile` 归一化的外观。
/// 字段不全时归一化会整块丢掉 `visualIdentity`，改它就等于没改，
/// 「改外观」这条断言会假绿。
fn complete_visual_profile() -> Value {
    json!({
        "gender": "female",
        "visualIdentity": {
            "faceDesign": "女性化读取，紧凑圆润鹅蛋脸",
            "eyeDesign": "中等偏大的紫色宝石眼，视线坚定",
            "hairShape": "银灰齐颌短发与偏分刘海",
            "hairLayerPlan": "后发、刘海和左右侧发形成独立轮廓",
            "upperBodySilhouette": "紧凑肩线、清楚领口与胸前焦点",
            "outfitConstruction": "敞开领口内搭叠短外套并止于高腰",
            "sleeveArmDesign": "左右袖片携局部前臂进入画面",
            "materialPlan": "哑光布料",
            "heroAccessory": "左胸星轨扣饰",
            "paletteHint": "雾蓝为主、银白为辅、金色点缀",
            "motif": "单一星轨弧线集中在胸前"
        }
    })
}

fn persona_with_avatar() -> agent_persona::Model {
    agent_persona::Model {
        id: PERSONA_ROW_ID.to_string(),
        name: "Arael".to_string(),
        personality: "quiet".to_string(),
        persona_json: Some(json!({ "summary": "quiet" })),
        visual_profile: Some(complete_visual_profile()),
        portrait_asset_id: Some("/master.png".to_string()),
        portrait_generation: Some(json!({ "fingerprint": "a".repeat(64) })),
        avatar_asset_id: Some("/sticker.png".to_string()),
        avatar_generation: Some(json!({ "fingerprint": "b".repeat(64) })),
        updated_by: Some(1),
        updated_at: Utc::now().into(),
        zone: String::new(),
        field_revs: serde_json::json!({}),
    }
}

/// 贴纸头像画的是主立绘上那个人。换主立绘还留着旧头像，站点上就会同时挂着
/// 两张脸——和留着旧 Rig 是同一类错，必须在同一次写入里清掉。
#[test]
fn replacing_the_portrait_drops_the_sticker_avatar() {
    let active = apply_persona_update(
        persona_with_avatar(),
        "Arael".to_string(),
        "quiet".to_string(),
        &PortraitUpdate::Set("/uploaded.png".to_string()),
        &PersonaContractUpdate::default(),
        1,
    );
    assert_eq!(active.avatar_asset_id, Set(None));
    assert_eq!(active.avatar_generation, Set(None));
}

#[test]
fn clearing_the_portrait_drops_the_sticker_avatar() {
    let active = apply_persona_update(
        persona_with_avatar(),
        "Arael".to_string(),
        "quiet".to_string(),
        &PortraitUpdate::Clear,
        &PersonaContractUpdate::default(),
        1,
    );
    assert_eq!(active.avatar_asset_id, Set(None));
    assert_eq!(active.avatar_generation, Set(None));
}

/// 外观变了主立绘会被作废，头像是从主立绘派生的，一起走。
#[test]
fn changing_the_appearance_drops_the_sticker_avatar() {
    let active = apply_persona_update(
        persona_with_avatar(),
        "Arael".to_string(),
        "quiet".to_string(),
        &PortraitUpdate::Keep,
        &PersonaContractUpdate {
            visual_profile: JsonDocumentUpdate::Set({
                let mut next = complete_visual_profile();
                next["visualIdentity"]["hairShape"] = json!("银灰高马尾与偏分刘海");
                next
            }),
            ..PersonaContractUpdate::default()
        },
        1,
    );
    assert_eq!(active.portrait_asset_id, Set(None));
    assert_eq!(active.avatar_asset_id, Set(None));
}

/// 只改说话人格不动脸。头像跟着一起清掉的话，每次改性格都要重新烧一次图。
#[test]
fn changing_the_spoken_persona_keeps_the_sticker_avatar() {
    let active = apply_persona_update(
        persona_with_avatar(),
        "Arael".to_string(),
        "more curious".to_string(),
        &PortraitUpdate::Keep,
        &PersonaContractUpdate {
            persona: JsonDocumentUpdate::Set(json!({ "summary": "more curious" })),
            ..PersonaContractUpdate::default()
        },
        1,
    );
    assert_eq!(
        active.avatar_asset_id,
        sea_orm::ActiveValue::Unchanged(Some("/sticker.png".to_string()))
    );
}

/// 主立绘落盘的那条 SQL 也得清。它绕开 `apply_persona_update` 直接写库，
/// 上面那几条断言管不到它。
#[test]
fn completing_a_portrait_generation_drops_the_sticker_avatar_in_the_same_write() {
    let source = include_str!("store/generation.rs");
    let at = source
        .find("pub async fn complete_portrait_generation")
        .expect("complete_portrait_generation exists");
    let rest = &source[at..];
    // 切到下一个顶层函数为止，别按字节数硬截——中文注释会把切点落在字符中间。
    let end = rest[1..]
        .find("\npub ")
        .map(|offset| offset + 1)
        .unwrap_or(rest.len());
    let body = &rest[..end];
    assert!(
        body.contains("avatar_asset_id = NULL"),
        "落新主立绘却留着旧贴纸头像"
    );
    assert!(body.contains("avatar_generation = NULL"));
}

#[test]
fn onboarding_seeds_do_not_invalidate_portrait() {
    let existing = agent_persona::Model {
        id: PERSONA_ROW_ID.to_string(),
        name: "Arael".to_string(),
        personality: "quiet".to_string(),
        persona_json: Some(json!({ "summary": "quiet" })),
        visual_profile: Some(json!({
            "gender": "unspecified",
            "visualIdentity": { "hairShape": "short bob" }
        })),
        portrait_asset_id: Some("/master.png".to_string()),
        portrait_generation: Some(json!({ "fingerprint": "a".repeat(64) })),
        avatar_asset_id: None,
        avatar_generation: None,
        updated_by: Some(1),
        updated_at: Utc::now().into(),
        zone: String::new(),
        field_revs: serde_json::json!({}),
    };
    let active = apply_persona_update(
        existing,
        "Arael".to_string(),
        "quiet".to_string(),
        &PortraitUpdate::Keep,
        &PersonaContractUpdate {
            visual_profile: JsonDocumentUpdate::Set(json!({
                "gender": "unspecified",
                "language": "zh-CN",
                "visualIdentity": { "hairShape": "short bob" },
                "sourceTags": ["慢热"],
                "personaExtraRequirements": "话少"
            })),
            ..PersonaContractUpdate::default()
        },
        1,
    );
    assert_eq!(
        active.portrait_asset_id,
        sea_orm::ActiveValue::Unchanged(Some("/master.png".to_string()))
    );
    assert!(matches!(
        active.portrait_generation,
        sea_orm::ActiveValue::Unchanged(Some(_))
    ));
}

#[tokio::test]
async fn portrait_generation_lease_preserves_concurrent_persona_and_rejects_visual_change() {
    let Ok(database_url) = std::env::var("PORTRAIT_TEST_DATABASE_URL") else {
        return;
    };
    let db = Database::connect(database_url).await.unwrap();
    let transaction = db.begin().await.unwrap();
    agent_persona::Entity::delete_by_id(PERSONA_ROW_ID)
        .exec(&transaction)
        .await
        .unwrap();
    let profile = json!({
        "gender": "unspecified",
        "visualIdentity": { "hairShape": "short bob" }
    });
    agent_persona::ActiveModel {
        id: Set(PERSONA_ROW_ID.to_string()),
        name: Set("Nova".to_string()),
        personality: Set("quiet".to_string()),
        persona_json: Set(Some(json!({ "summary": "quiet" }))),
        visual_profile: Set(Some(profile.clone())),
        portrait_asset_id: Set(None),
        portrait_generation: Set(None),
        avatar_asset_id: Set(None),
        avatar_generation: Set(None),
        updated_by: Set(None),
        updated_at: Set(Utc::now().into()),
        zone: Set(String::new()),
        field_revs: Set(serde_json::json!({})),
    }
    .insert(&transaction)
    .await
    .unwrap();

    let first_pending = json!({ "token": "first" });
    assert!(
        acquire_portrait_generation(&transaction, "Nova", &profile, &first_pending)
            .await
            .unwrap()
    );
    assert!(portrait_generation_is_pending(
        get_persona_on(&transaction)
            .await
            .unwrap()
            .unwrap()
            .portrait_generation
            .as_ref()
    ));
    assert!(
        !acquire_portrait_generation(
            &transaction,
            "Nova",
            &profile,
            &json!({ "token": "second" }),
        )
        .await
        .unwrap()
    );

    upsert_persona_on(
        &transaction,
        "Nova".to_string(),
        "more curious".to_string(),
        PortraitUpdate::Keep,
        PersonaContractUpdate::default(),
        1,
    )
    .await
    .unwrap();
    assert!(
        complete_portrait_generation(
            &transaction,
            "Nova",
            &profile,
            "first",
            "/portrait.png",
            &json!({ "fingerprint": "a".repeat(64), "contract": {} }),
            1,
        )
        .await
        .unwrap()
    );
    let saved = get_persona_on(&transaction).await.unwrap().unwrap();
    assert_eq!(saved.personality, "more curious");
    assert_eq!(saved.portrait_asset_id.as_deref(), Some("/portrait.png"));

    assert!(
        acquire_portrait_generation(&transaction, "Nova", &profile, &json!({ "token": "third" }),)
            .await
            .unwrap()
    );
    let changed_profile = json!({
        "gender": "unspecified",
        "visualIdentity": { "hairShape": "long ponytail" }
    });
    upsert_persona_on(
        &transaction,
        "Nova".to_string(),
        "more curious".to_string(),
        PortraitUpdate::Keep,
        PersonaContractUpdate {
            visual_profile: JsonDocumentUpdate::Set(changed_profile),
            ..PersonaContractUpdate::default()
        },
        1,
    )
    .await
    .unwrap();
    assert!(
        !complete_portrait_generation(
            &transaction,
            "Nova",
            &profile,
            "third",
            "/stale.png",
            &json!({ "fingerprint": "b".repeat(64), "contract": {} }),
            1,
        )
        .await
        .unwrap()
    );
    transaction.rollback().await.unwrap();
}

#[test]
fn in_a_group_what_is_about_someone_else_says_whose_it_is() {
    assert_eq!(
        super::about_someone_else("[2026-09-28] 周五要加班", "老周"),
        "[2026-09-28] 周五要加班 (this is about 老周, not the one talking to you)"
    );
}

/// Her zone is stored as named, kept by a save that does not name one, and
/// empty again (the site's own) when cleared. On a real database (an isolated
/// schema; skipped without a test database).
#[tokio::test]
async fn her_zone_is_kept_until_named_otherwise() {
    let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
        return;
    };
    let db = &fixture.db;
    sea_orm::ConnectionTrait::execute_unprepared(
        db,
        "INSERT INTO users (id, username) VALUES (1, 'owner1')",
    )
    .await
    .unwrap();
    let save = |zone: Option<&str>| {
        upsert_persona_on(
            db,
            "Nova".to_string(),
            "curious".to_string(),
            PortraitUpdate::Keep,
            PersonaContractUpdate {
                zone: zone.map(str::to_string),
                ..PersonaContractUpdate::default()
            },
            1,
        )
    };
    assert_eq!(save(None).await.unwrap().zone, "");
    // The process zone in tests is Asia/Shanghai, so naming it changes
    // nothing else running alongside.
    assert_eq!(
        save(Some(" Asia/Shanghai ")).await.unwrap().zone,
        "Asia/Shanghai"
    );
    assert_eq!(save(None).await.unwrap().zone, "Asia/Shanghai");
    let read = get_persona_on(db).await.unwrap().unwrap();
    assert_eq!(
        super::super::clock::Zone::named(&read.zone),
        Some(super::super::clock::Zone::Named(chrono_tz::Asia::Shanghai))
    );
    assert_eq!(save(Some("")).await.unwrap().zone, "");
    fixture.close().await;
}

/// The tables her affect's changes are recorded in, without the users table
/// they refer to (for databases that have only the tables a test makes).
pub(super) fn affect_log_ddl() -> String {
    include_str!("../../../../migrations/persona_sync.sql")
        .replace("REFERENCES users(id) ON DELETE CASCADE", "")
        .replace("REFERENCES agent_sync_devices(id) ON DELETE CASCADE", "")
}

/// Her affect toward someone, replayed from the anchor and the changes
/// recorded with each write (`myriad_persona_sync::affect::replay`), is to the
/// bit what the site reads from the row, through every kind of change; and
/// still is after old changes are folded into the anchor and dropped. On a
/// real database (an isolated schema; skipped without a test database).
#[tokio::test]
async fn her_affect_replayed_from_its_changes_is_what_she_feels() {
    use super::super::clock::as_of;
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
        return;
    };
    let isolated = crate::db::IsolatedSchema::migrated(&url, "affect_replay").await;
    let db = isolated.db.clone();
    let user_id: i32 = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "INSERT INTO users (username) VALUES ('affect-replay-test') RETURNING id",
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "id")
        .unwrap();
    // A persona whose usual mood is not the default, so the baseline counts.
    db.execute_unprepared(
        "INSERT INTO agent_persona (id, name, personality, updated_at) \
         VALUES ('site', 'Nova', '安静，容易低落', NOW())",
    )
    .await
    .unwrap();
    let t0 = Utc::now() - chrono::Duration::days(60);
    let at = |secs: i64, micros: i64| {
        t0 + chrono::Duration::seconds(secs) + chrono::Duration::microseconds(micros)
    };
    let feels = |now: chrono::DateTime<Utc>| {
        let db = db.clone();
        async move {
            as_of(now, async {
                affect_from_state(&get_or_create_state(&db, user_id).await.unwrap())
            })
            .await
        }
    };
    let replayed = |now: chrono::DateTime<Utc>| {
        let db = db.clone();
        async move {
            let (anchor, changes) = affect_log_on(&db, user_id, None).await.unwrap().unwrap();
            let base = load_affect_baseline(&db).await.unwrap();
            let affect = myriad_persona_sync::affect::replay(
                &anchor,
                &changes,
                now.timestamp_micros(),
                myriad_persona_sync::entity::Baseline {
                    mood: base.mood,
                    arousal: base.arousal,
                },
            )
            .unwrap();
            Affect {
                mood: affect.mood,
                arousal: affect.arousal,
                emotion: affect.emotion,
                emotion_arousal: affect.emotion_arousal,
            }
        }
    };

    // Praised on the first turn (the row is made), then turns that only let
    // her settle, a failed task, music, and an appraisal of their words, at
    // uneven moments (part seconds included).
    as_of(
        at(0, 0),
        update_affect(&db, user_id, true, AffectChange::utterance(0, true, false)),
    )
    .await
    .unwrap();
    as_of(
        at(90, 250_000),
        update_affect(&db, user_id, true, AffectChange::utterance(1, false, false)),
    )
    .await
    .unwrap();
    as_of(
        at(7_200, 999_999),
        update_affect(&db, user_id, false, AffectChange::task(false)),
    )
    .await
    .unwrap();
    let credit = as_of(at(10_800, 1), credit_music_listening(&db, user_id, 1_500))
        .await
        .unwrap();
    assert!(credit.credited);
    // Within the cooldown: nothing is written, nothing recorded.
    let again = as_of(at(10_900, 0), credit_music_listening(&db, user_id, 1_500))
        .await
        .unwrap();
    assert!(!again.credited);
    let (_, turn) = as_of(
        at(18_000, 500_000),
        update_affect(&db, user_id, true, AffectChange::utterance(2, false, true)),
    )
    .await
    .unwrap();
    let input_at = turn.last_user_message_at.unwrap();
    let appraised = as_of(
        at(18_003, 0),
        update_utterance_appraisal(
            &db,
            user_id,
            input_at,
            AffectChange::appraisal(myriad_merope::affect::lite_appraisal(-2, 1)),
        ),
    )
    .await
    .unwrap();
    assert!(appraised.is_some());
    let recorded: i64 = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            format!("SELECT count(*) AS n FROM agent_affect_events WHERE user_id = {user_id}"),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap();
    assert_eq!(recorded, 6);

    for now in [at(18_003, 0), at(18_004, 999_999), at(32_400, 123_456)] {
        assert_eq!(replayed(now).await, feels(now).await, "at {now}");
    }

    // Weeks later the old changes fold into the anchor and go; replaying from
    // there is still what she feels, and what she feels did not change.
    let later = at(40 * 86_400, 0);
    let before_fold = feels(later).await;
    let folded = as_of(later, fold_old_affect_changes(&db, Some(user_id)))
        .await
        .unwrap();
    assert_eq!(folded, 6);
    let (anchor, changes) = affect_log_on(&db, user_id, None).await.unwrap().unwrap();
    assert!(changes.is_empty());
    assert!(anchor.through_event.is_some());
    assert_eq!(replayed(later).await, before_fold);
    assert_eq!(feels(later).await, before_fold);
    // And a change after the fold replays on top of the moved anchor.
    as_of(
        later,
        update_affect(&db, user_id, false, AffectChange::task(true)),
    )
    .await
    .unwrap();
    let after = at(40 * 86_400 + 3_600, 0);
    assert_eq!(replayed(after).await, feels(after).await);
    isolated.drop().await;
}

/// Each write of her persona gives the fields it changed a new revision and
/// keeps the values they held; a field it did not touch keeps its revision;
/// putting an old value back is one more edit. On a real database (an
/// isolated schema; skipped without a test database).
#[tokio::test]
async fn her_persona_edits_keep_each_fields_revision_and_what_it_replaced() {
    use crate::services::persona_sync::fields;
    let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
        return;
    };
    let db = &fixture.db;
    sea_orm::ConnectionTrait::execute_unprepared(
        db,
        "INSERT INTO users (id, username) VALUES (1, 'owner1')",
    )
    .await
    .unwrap();
    let save = |summary: &str| {
        upsert_persona_on(
            db,
            "Nova".to_string(),
            "curious".to_string(),
            PortraitUpdate::Keep,
            PersonaContractUpdate {
                persona: JsonDocumentUpdate::Set(json!({
                    "summary": summary,
                    "likes": ["雨天"],
                })),
                ..PersonaContractUpdate::default()
            },
            1,
        )
    };
    let first = save("住在站点上").await.unwrap();
    let revs = fields::revs(&first);
    assert!(revs.contains_key("summary") && revs.contains_key("name"));
    assert!(fields::history(db, "summary", 10).await.unwrap().is_empty());

    let second = save("搬到了海边").await.unwrap();
    let after = fields::revs(&second);
    assert!(after["summary"].rev > revs["summary"].rev);
    assert_eq!(
        after["likes"], revs["likes"],
        "untouched fields keep theirs"
    );
    assert_eq!(after["name"], revs["name"]);
    let kept = fields::history(db, "summary", 10).await.unwrap();
    assert_eq!(kept.len(), 1);
    assert_eq!(kept[0].value, json!("住在站点上"));
    assert_eq!(kept[0].rev, revs["summary"].rev.to_string());
    assert_eq!(kept[0].reason, "edit");
    // Read back as stored, too.
    assert_eq!(
        fields::revs(&get_persona_on(db).await.unwrap().unwrap()),
        after
    );

    // Putting the old value back is an edit like any other.
    let third = save("住在站点上").await.unwrap();
    assert!(fields::revs(&third)["summary"].rev > after["summary"].rev);
    let kept = fields::history(db, "summary", 10).await.unwrap();
    assert_eq!(kept.len(), 2);
    assert_eq!(kept[0].value, json!("搬到了海边"));

    // The same write again changes nothing.
    let same = save("住在站点上").await.unwrap();
    assert_eq!(fields::revs(&same), fields::revs(&third));
    assert_eq!(fields::history(db, "summary", 10).await.unwrap().len(), 2);
    fixture.close().await;
}
