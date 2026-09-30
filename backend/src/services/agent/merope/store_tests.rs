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
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();
    assert_eq!(
        super::as_known(&note("chat", "user")),
        format!("[{today}] 养了一只猫叫年糕")
    );
    assert!(super::as_known(&note("said", "agent")).ends_with("(what you told them)"));
    assert!(super::as_known(&note("event", "agent")).ends_with("not from them)"));
    assert!(super::as_known(&note("work", "agent")).contains("doing a task for them"));
    assert!(super::as_known(&note("chat", "import")).contains("kept from before"));
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
    ] {
        statement.if_not_exists();
        db.execute(&statement).await.unwrap();
    }
    let (_, first) = super::update_affect(&db, 7001, true, |_| {}).await.unwrap();
    let input_at = first.last_user_message_at.unwrap();
    super::set_activity(&db, 7001, "talking").await.unwrap();
    let (_, applied) =
        super::update_utterance_appraisal(&db, 7001, input_at, |affect| affect.mood += 1.0)
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
        |affect| affect.mood = 0.0,
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
        let (_, current) = super::update_affect(&db, 7001, true, |_| {}).await.unwrap();
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
    let source = include_str!("store.rs");
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
