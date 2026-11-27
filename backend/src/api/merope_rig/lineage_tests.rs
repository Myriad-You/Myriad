//! What the entries that write the persona do with `portrait_generation`,
//! the stored generation contract the worn bust's fallback lineage is read
//! from: a `PUT /api/agent/persona` carrying a forged one leaves the stored
//! one as it was, and an upload clears it. Each checks the stored column
//! and the fingerprint the rig endpoints read. Against a database (an
//! isolated schema); skipped without `MYRIAD_MEDIA_TEST_DATABASE_URL`.
//! Persona-sync import is checked in `services::persona_sync::import_tests`.

use axum::{Extension, Json, extract::State};
use myriad_merope::{
    RenderingProvenance, build_character_asset_contract_with, character_asset_contract_fingerprint,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};

use crate::{db::IsolatedSchema, middleware::auth::Claims, services::agent::merope};

const OWNER: i32 = 7;

fn claims() -> Claims {
    Claims {
        sub: OWNER.to_string(),
        username: "owner".into(),
        is_admin: true,
        is_owner: true,
        exp: i64::MAX,
        iat: 0,
        tv: 0,
        subject: crate::middleware::auth::AuthSubject::from_test_sub(&OWNER.to_string()),
    }
}

/// The worn outfit stores no `generationFingerprint`, so the worn bust's
/// lineage comes from `portrait_generation`.
fn profile() -> Value {
    json!({
        "activeOutfitId": "a",
        "gender": "female",
        "wardrobe": [{ "id": "a", "portraitAssetId": "/master.png" }],
    })
}

fn document(notes: Option<&str>, rendering: &RenderingProvenance) -> Value {
    let contract = build_character_asset_contract_with("Nova", &profile(), notes, rendering);
    json!({
        "fingerprint": character_asset_contract_fingerprint(&contract),
        "contract": contract,
    })
}

/// A self-consistent contract a client might send: its own fingerprint
/// matches, it names a recipe, and it would keep lineage if it were stored.
fn forged() -> Value {
    let rendering: RenderingProvenance = serde_json::from_value(json!({
        "recipe": "forged-recipe",
        "visualSchoolVersion": "forged-v1",
        "styleReferenceSha256": "f".repeat(64),
        "fullBodyInstructionVersion": "full-body-standing-v1",
    }))
    .unwrap();
    document(Some("forged notes"), &rendering)
}

/// The site with its owner and her, holding the generation contract the
/// generation path wrote for her master portrait.
async fn site(prefix: &str) -> Option<(IsolatedSchema, Value)> {
    let url = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"])?;
    let schema = IsolatedSchema::migrated(&url, prefix).await;
    let generated = document(None, &RenderingProvenance::builtin());
    schema
        .db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO users (id, username, is_admin, is_owner) VALUES ($1, 'owner', true, true)",
            [OWNER.into()],
        ))
        .await
        .unwrap();
    schema
        .db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_persona (id, name, personality, visual_profile, \
             portrait_asset_id, portrait_generation, updated_at) \
             VALUES ('site', 'Nova', '', $1, '/master.png', $2, NOW())",
            [profile().into(), generated.clone().into()],
        ))
        .await
        .unwrap();
    Some((schema, generated))
}

async fn stored(db: &DatabaseConnection) -> Option<Value> {
    merope::get_persona(db)
        .await
        .unwrap()
        .unwrap()
        .portrait_generation
}

async fn fingerprint(db: &DatabaseConnection) -> Option<String> {
    let persona = merope::get_persona(db).await.unwrap().unwrap();
    super::worn_bust_generation_fingerprint(&persona)
}

/// `put_persona` reads the switch the site's settings hold.
async fn her_switch_on() {
    let mut config = crate::GLOBAL_DYNAMIC_CONFIG.write().await;
    config.merope_enabled = true;
    config.pro_enabled = true;
}

async fn put(db: &DatabaseConnection, body: Value) {
    let request: crate::api::agent::persona::PutPersonaRequest =
        serde_json::from_value(body).unwrap();
    let _saved = crate::api::agent::persona::put_persona(
        State(db.clone()),
        Extension(claims()),
        Json(request),
    )
    .await
    .unwrap_or_else(|error| {
        panic!(
            "put persona: {}",
            axum::response::IntoResponse::into_response(error).status()
        )
    });
}

#[tokio::test]
async fn a_forged_portrait_generation_in_put_persona_is_not_stored() {
    let Some((schema, generated)) = site("lineage_put").await else {
        return;
    };
    let db = &schema.db;
    her_switch_on().await;
    let genuine = generated["fingerprint"].as_str().unwrap().to_string();
    assert_eq!(fingerprint(db).await.as_deref(), Some(genuine.as_str()));

    put(
        db,
        json!({ "name": "Nova", "personality": "", "portraitGeneration": forged() }),
    )
    .await;
    assert_eq!(stored(db).await, Some(generated));
    assert_eq!(fingerprint(db).await.as_deref(), Some(genuine.as_str()));

    // A changed design clears it; the forged one still does not land.
    let mut changed = profile();
    changed["gender"] = json!("male");
    put(
        db,
        json!({
            "name": "Nova",
            "personality": "",
            "visualProfile": changed,
            "portraitGeneration": forged(),
        }),
    )
    .await;
    assert_eq!(stored(db).await, None);
    assert_eq!(fingerprint(db).await, None);
    schema.drop().await;
}

#[tokio::test]
async fn an_uploaded_master_portrait_clears_the_generation_contract() {
    let Some((schema, _)) = site("lineage_upload").await else {
        return;
    };
    let db = &schema.db;
    let url = super::portrait_upload::commit_uploaded_portrait(
        db,
        OWNER,
        "https://images.example/uploaded.png",
    )
    .await
    .unwrap_or_else(|error| panic!("commit upload: {:?}", error.1));
    let persona = merope::get_persona(db).await.unwrap().unwrap();
    assert_eq!(persona.portrait_asset_id.as_deref(), Some(url.as_str()));
    assert_eq!(persona.portrait_generation, None);
    assert_eq!(fingerprint(db).await, None);
    schema.drop().await;
}
