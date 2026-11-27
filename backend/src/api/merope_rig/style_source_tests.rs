//! Handler-level tests of the portrait style source, entered where the
//! router enters them: `GET`, `PATCH` and `POST .../probe` of
//! `/api/merope/rig/persona-style`, and `POST /api/merope/rig/portrait`
//! with a configured service that fails. Against a database (an isolated
//! schema); skipped without `MYRIAD_MEDIA_TEST_DATABASE_URL`.
//!
//! They change the process-wide `GLOBAL_DYNAMIC_CONFIG`, so they take turns
//! with every test that does (`super::DYNAMIC_CONFIG_TURN`) and set every
//! field they rely on.

use axum::{
    Extension, Json, Router,
    extract::State,
    http::{StatusCode, Uri},
    routing::any,
};
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

use super::{full_body, portrait, style_source};
use crate::{
    db::IsolatedSchema,
    middleware::auth::Claims,
    services::{
        agent::merope,
        persona_style::tests::{
            Fake, TOKEN, closed_url, house_style, png, profile, serve, served_recipe,
        },
    },
};

const OWNER: i32 = 7;
const ADMIN: i32 = 8;

fn claims(user: i32) -> Claims {
    Claims {
        sub: user.to_string(),
        username: format!("user{user}"),
        is_admin: true,
        is_owner: user == OWNER,
        exp: i64::MAX,
        iat: 0,
        tv: 0,
        subject: crate::middleware::auth::AuthSubject::from_test_sub(&user.to_string()),
    }
}

/// The site with its owner, an admin who is not the owner, and her with a
/// drawable design and no portrait yet.
async fn site(prefix: &str) -> Option<IsolatedSchema> {
    let url = crate::db::test_url(&["MYRIAD_MEDIA_TEST_DATABASE_URL"])?;
    let schema = IsolatedSchema::migrated(&url, prefix).await;
    for (id, owner) in [(OWNER, true), (ADMIN, false)] {
        schema
            .db
            .execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO users (id, username, is_admin, is_owner) VALUES ($1, $2, true, $3)",
                [id.into(), format!("user{id}").into(), owner.into()],
            ))
            .await
            .unwrap();
    }
    schema
        .db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_persona (id, name, personality, visual_profile, updated_at) \
             VALUES ('site', 'Nova', '', $1, NOW())",
            [profile().into()],
        ))
        .await
        .unwrap();
    Some(schema)
}

/// Her switch on and the style source as given, nothing else about it.
async fn configure(url: Option<String>, token: Option<&str>) {
    let mut config = crate::GLOBAL_DYNAMIC_CONFIG.write().await;
    config.merope_enabled = true;
    config.pro_enabled = true;
    config.merope_persona_url = url;
    config.merope_persona_token = token.map(str::to_string);
}

async fn stored_style() -> (Option<String>, Option<String>) {
    let config = crate::GLOBAL_DYNAMIC_CONFIG.read().await;
    (
        config.merope_persona_url.clone(),
        config.merope_persona_token.clone(),
    )
}

async fn stored_row(db: &DatabaseConnection, key: &str) -> Option<Value> {
    db.query_one_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT value FROM configurations WHERE key = $1",
        [key.into()],
    ))
    .await
    .unwrap()
    .and_then(|row| row.try_get::<Value>("", "value").ok())
}

async fn get(db: &DatabaseConnection, user: i32) -> Result<Value, (StatusCode, Value)> {
    style_source::get_persona_style(State(db.clone()), Extension(claims(user)))
        .await
        .map(|Json(body)| body)
        .map_err(|(status, Json(body))| (status, body))
}

async fn patch(db: &DatabaseConnection, user: i32, body: Value) -> Result<Value, (StatusCode, Value)> {
    style_source::update_persona_style(
        State(db.clone()),
        Extension(claims(user)),
        Json(serde_json::from_value(body).unwrap()),
    )
    .await
    .map(|Json(body)| body)
    .map_err(|(status, Json(body))| (status, body))
}

#[tokio::test]
async fn only_the_owner_reads_and_sets_the_style_source_and_no_answer_carries_the_token() {
    let _turn = super::DYNAMIC_CONFIG_TURN.lock().await;
    let Some(schema) = site("persona_style_patch").await else {
        return;
    };
    let db = &schema.db;
    configure(None, None).await;
    let service = Fake::answering(
        StatusCode::OK,
        &served_recipe(&house_style(), &[png()], "house-v6"),
    );
    let url = serve(service.clone()).await;

    let shown = get(db, OWNER).await.unwrap();
    assert_eq!(shown["source"], "builtin");
    assert_eq!(get(db, ADMIN).await.unwrap_err().0, StatusCode::FORBIDDEN);
    assert_eq!(
        patch(db, ADMIN, json!({ "url": url, "token": TOKEN }))
            .await
            .unwrap_err()
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(stored_style().await, (None, None));

    let shown = patch(db, OWNER, json!({ "url": format!("{url}/"), "token": TOKEN }))
        .await
        .unwrap();
    assert_eq!(shown["source"], "remote");
    assert_eq!(shown["configured"], true);
    assert_eq!(shown["tokenConfigured"], true);
    assert_eq!(shown["probe"], Value::Null);
    assert!(!shown.to_string().contains(TOKEN));
    assert_eq!(stored_style().await, (Some(url.clone()), Some(TOKEN.into())));
    assert_eq!(
        stored_row(db, "merope_persona_url").await,
        Some(json!(url.clone()))
    );
    // The token is a sealed secret at rest.
    assert_ne!(
        stored_row(db, "merope_persona_token").await,
        Some(json!(TOKEN))
    );
    assert!(service.asked().is_empty(), "saving and reading send nothing");

    // Asked for, a probe goes to the service once and is then shown.
    let probed = style_source::probe_persona_style(State(db.clone()), Extension(claims(OWNER)))
        .await
        .map(|Json(body)| body)
        .unwrap();
    assert_eq!(probed["probe"]["available"], true);
    assert_eq!(service.asked().len(), 1);
    assert_eq!(get(db, OWNER).await.unwrap()["probe"]["available"], true);
    assert_eq!(service.asked().len(), 1);

    // A URL the policy refuses is refused and nothing changes.
    for refused in [
        "http://169.254.169.254",
        "http://[fe80::1]:8080",
        "http://persona.example.com",
    ] {
        let (status, body) = patch(db, OWNER, json!({ "url": refused })).await.unwrap_err();
        assert_eq!(status, StatusCode::BAD_REQUEST, "{refused}");
        assert_eq!(body["code"], "persona_style_url_invalid");
    }
    assert_eq!(stored_style().await, (Some(url.clone()), Some(TOKEN.into())));

    // Another origin without a token: the token goes with the old origin,
    // so nothing is sent until a new one comes.
    let elsewhere = closed_url().await;
    let shown = patch(db, OWNER, json!({ "url": elsewhere })).await.unwrap();
    assert_eq!(stored_style().await, (Some(elsewhere.clone()), None));
    assert_eq!(shown["configured"], false);
    assert_eq!(shown["error"], "persona_style_not_configured");
    let probed = style_source::probe_persona_style(State(db.clone()), Extension(claims(OWNER)))
        .await
        .map(|Json(body)| body)
        .unwrap();
    assert_eq!(probed["error"], "persona_style_not_configured");
    assert_eq!(service.asked().len(), 1);

    // Another origin with its own token in the same request keeps that one.
    let shown = patch(db, OWNER, json!({ "url": url, "token": "second-token-0123456789" }))
        .await
        .unwrap();
    assert_eq!(shown["configured"], true);
    assert_eq!(
        stored_style().await,
        (Some(url.clone()), Some("second-token-0123456789".into()))
    );
    // A probe made under the earlier settings is not shown for these.
    assert_eq!(shown["probe"], Value::Null);

    // Cleared: back to this build's own recipe, and the token goes too.
    let shown = patch(db, OWNER, json!({ "url": null })).await.unwrap();
    assert_eq!(shown["source"], "builtin");
    assert_eq!(stored_style().await, (None, None));

    configure(None, None).await;
    schema.drop().await;
}

/// An image provider that counts what it is asked; its base URL.
async fn provider() -> (String, Arc<AtomicUsize>) {
    let asked = Arc::new(AtomicUsize::new(0));
    let counted = asked.clone();
    let app = Router::new().fallback(any(move |_: Uri| {
        let counted = counted.clone();
        async move {
            counted.fetch_add(1, Ordering::SeqCst);
            (StatusCode::INTERNAL_SERVER_ERROR, "no")
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/api/v3"), asked)
}

async fn configure_provider(base_url: &str) {
    let mut config = crate::GLOBAL_DYNAMIC_CONFIG.write().await;
    config.ai_image_source = "persona-test-images".into();
    config.ai_image_model = "test-image-model".into();
    config.ai_vendor_sources = vec![crate::config::AiVendorSource {
        slug: "persona-test-images".into(),
        kind: "volcengine".into(),
        display_name: "Test images".into(),
        enabled: true,
        credential_mode: "own".into(),
        api_key: Some("provider-test-key".into()),
        base_url: base_url.into(),
        ..crate::config::AiVendorSource::default()
    }];
}

/// A portrait asked for while the configured service is down or refuses the
/// token answers with the service's error before the portrait's lease is
/// taken, before anything is charged, and before the image provider is
/// asked: nothing is drawn in another style.
#[tokio::test]
async fn a_portrait_whose_service_fails_stops_before_its_lease_spend_and_provider() {
    let _turn = super::DYNAMIC_CONFIG_TURN.lock().await;
    let Some(schema) = site("persona_style_portrait").await else {
        return;
    };
    let db = &schema.db;
    let (provider_url, provider_asked) = provider().await;
    let saved = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    configure_provider(&provider_url).await;
    let refusing = serve(Fake::answering(StatusCode::UNAUTHORIZED, &json!({}))).await;
    for (service, status, code) in [
        (
            closed_url().await,
            StatusCode::SERVICE_UNAVAILABLE,
            "persona_style_unavailable",
        ),
        (refusing, StatusCode::BAD_GATEWAY, "persona_style_unauthorized"),
        (
            "http://169.254.169.254".to_string(),
            StatusCode::PRECONDITION_REQUIRED,
            "persona_style_url_invalid",
        ),
    ] {
        configure(Some(service.clone()), Some(TOKEN)).await;
        let request: portrait::GeneratePortraitRequest = serde_json::from_value(json!({})).unwrap();
        let (got, Json(body)) =
            portrait::generate_portrait(State(db.clone()), Extension(claims(OWNER)), Json(request))
                .await
                .unwrap_err();
        assert_eq!((got, body["code"].as_str()), (status, Some(code)), "{body}");
        assert!(!body.to_string().contains(TOKEN));
        let persona = merope::get_persona(db).await.unwrap().unwrap();
        assert_eq!(persona.portrait_generation, None, "no lease was taken");
        assert_eq!(persona.portrait_asset_id, None);
    }
    assert_eq!(provider_asked.load(Ordering::SeqCst), 0);
    let spent: i64 = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*)::BIGINT AS n FROM ai_cost_ledger".to_string(),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap();
    assert_eq!(spent, 0, "nothing was charged");

    {
        let mut config = crate::GLOBAL_DYNAMIC_CONFIG.write().await;
        *config = saved;
    }
    configure(None, None).await;
    schema.drop().await;
}

fn restored(key: &str, value: &str) -> crate::api::config::SettingsBackupEntry {
    crate::api::config::SettingsBackupEntry {
        key: key.into(),
        value: json!(value),
        schema_version: 1,
        description: None,
        category: None,
        is_encrypted: None,
        is_public: None,
    }
}

/// Settings restore treats the two fields as saving them does: a restored
/// URL of another origin, without a token in the same backup, clears the
/// stored token; with one, or for the same origin, the token is as the
/// backup has it (or as stored).
#[tokio::test]
async fn a_restored_url_of_another_origin_clears_the_token_unless_the_backup_has_one() {
    let Some(schema) = site("persona_style_restore").await else {
        return;
    };
    let db = &schema.db;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO configurations (key, value, updated_at) VALUES ('merope_persona_url', $1, NOW())",
        [json!("https://persona.example.com").into()],
    ))
    .await
    .unwrap();
    let token_entry = |entries: &[crate::api::config::SettingsBackupEntry]| {
        entries
            .iter()
            .find(|entry| entry.key == "merope_persona_token")
            .map(|entry| entry.value.clone())
    };
    for (entries, expected) in [
        (
            vec![restored("merope_persona_url", "https://other.example.com")],
            Some(json!("")),
        ),
        (
            vec![restored("merope_persona_url", "https://persona.example.com/v2")],
            None,
        ),
        (
            vec![
                restored("merope_persona_url", "https://other.example.com"),
                restored("merope_persona_token", "restored-token-0123456789"),
            ],
            Some(json!("restored-token-0123456789")),
        ),
        (vec![restored("site_title", "x")], None),
    ] {
        let mut entries = entries;
        crate::api::config::bind_restored_persona_style_token(db, &mut entries)
            .await
            .unwrap();
        assert_eq!(token_entry(&entries), expected);
    }
    schema.drop().await;
}

/// Her design with a full-body set `fb` drawn from the design (no bust it
/// is redrawn from).
fn with_full_body_set() -> Value {
    let mut profile = profile();
    profile["wardrobe"] = json!([{
        "id": "fb",
        "profile": "fullBody",
        "clothingStyle": "everyday",
        "outfit": profile["visualIdentity"]["outfit"].clone(),
    }]);
    profile
}

/// A full figure asked for while the configured service fails answers with
/// the service's error before any drawing begins: the set is not marked as
/// being drawn, nothing is charged, the image provider is not asked, and the
/// set is as it was.
#[tokio::test]
async fn a_full_figure_whose_service_fails_begins_no_drawing_spend_or_provider_call() {
    let _turn = super::DYNAMIC_CONFIG_TURN.lock().await;
    let Some(schema) = site("persona_style_full_body").await else {
        return;
    };
    let db = &schema.db;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE agent_persona SET visual_profile = $1",
        [with_full_body_set().into()],
    ))
    .await
    .unwrap();
    let (provider_url, provider_asked) = provider().await;
    let saved = crate::GLOBAL_DYNAMIC_CONFIG.read().await.clone();
    configure_provider(&provider_url).await;
    let refusing = serve(Fake::answering(StatusCode::UNAUTHORIZED, &json!({}))).await;
    for (service, status, code) in [
        (
            closed_url().await,
            StatusCode::SERVICE_UNAVAILABLE,
            "persona_style_unavailable",
        ),
        (refusing, StatusCode::BAD_GATEWAY, "persona_style_unauthorized"),
    ] {
        configure(Some(service), Some(TOKEN)).await;
        let (got, Json(body)) = full_body::generate_full_body_portrait(
            State(db.clone()),
            Extension(claims(OWNER)),
            axum::extract::Path("fb".to_string()),
        )
        .await
        .unwrap_err();
        assert_eq!((got, body["code"].as_str()), (status, Some(code)), "{body}");
        assert!(!body.to_string().contains(TOKEN));
        let Json(drawing) = full_body::full_body_portrait_drawing(
            State(db.clone()),
            Extension(claims(OWNER)),
            axum::extract::Path("fb".to_string()),
        )
        .await
        .unwrap();
        assert_eq!(drawing["drawing"], false, "no drawing began");
        let persona = merope::get_persona(db).await.unwrap().unwrap();
        assert_eq!(persona.visual_profile, Some(with_full_body_set()));
    }
    assert_eq!(provider_asked.load(Ordering::SeqCst), 0);
    let spent: i64 = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT COUNT(*)::BIGINT AS n FROM ai_cost_ledger".to_string(),
        ))
        .await
        .unwrap()
        .unwrap()
        .try_get("", "n")
        .unwrap();
    assert_eq!(spent, 0, "nothing was charged");
    {
        let mut config = crate::GLOBAL_DYNAMIC_CONFIG.write().await;
        *config = saved;
    }
    configure(None, None).await;
    schema.drop().await;
}

fn backup_of(entries: Vec<crate::api::config::SettingsBackupEntry>) -> crate::api::config::SettingsBackup {
    crate::api::config::SettingsBackup {
        format: crate::api::config::SETTINGS_BACKUP_FORMAT.to_string(),
        version: crate::api::config::SETTINGS_BACKUP_VERSION,
        product_version: None,
        exported_at: "2026-11-27T00:00:00Z".to_string(),
        contains_secrets: true,
        configurations: entries,
        effective_config: Default::default(),
        user_preferences: Default::default(),
    }
}

/// A restore whose portrait style URL fails the policy, through the restore
/// plan and the restore write: the URL and the token that came with it are
/// both skipped, so the stored URL keeps its own token and nothing is bound
/// to an origin it was not given for. A stored URL that fails the policy
/// (written before the policy was this strict) reads back after a restore
/// as a configuration error, not as this build's own recipe, and no token
/// is sent anywhere.
#[tokio::test]
async fn a_restored_style_url_failing_the_policy_keeps_the_token_bound_and_the_error_shown() {
    let Some(schema) = site("persona_style_restore_bad").await else {
        return;
    };
    let db = &schema.db;
    crate::services::config_service::ConfigService::new(db.clone())
        .update_configs(std::collections::HashMap::from([
            (
                "merope_persona_url".to_string(),
                json!("https://persona.example.com"),
            ),
            ("merope_persona_token".to_string(), json!(TOKEN)),
        ]))
        .await
        .unwrap();
    for refused in ["http://169.254.169.254", "http://[fd00:ec2::254]"] {
        let plan = crate::api::config::build_settings_restore_plan(&backup_of(vec![
            restored("merope_persona_url", refused),
            restored("merope_persona_token", "restored-token-0123456789"),
        ]));
        assert!(
            plan.preview
                .invalid_keys
                .iter()
                .any(|key| key == "merope_persona_url")
        );
        assert!(
            plan.preview
                .invalid_keys
                .iter()
                .any(|key| key == "merope_persona_token")
        );
        crate::api::config::write_restored_configurations(db, plan.entries, &[])
            .await
            .unwrap();
        let config = crate::services::config_service::ConfigService::load_config_on(db)
            .await
            .unwrap();
        assert_eq!(
            config.merope_persona_url.as_deref(),
            Some("https://persona.example.com"),
            "{refused}"
        );
        assert_eq!(config.merope_persona_token.as_deref(), Some(TOKEN), "{refused}");
    }

    // A stored URL the policy refuses stays as stored and reads back as a
    // configuration error; a restore without style settings leaves it so.
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "UPDATE configurations SET value = $1 WHERE key = 'merope_persona_url'",
        [json!("http://[fd00:ec2::254]").into()],
    ))
    .await
    .unwrap();
    let plan = crate::api::config::build_settings_restore_plan(&backup_of(vec![restored(
        "site_title",
        "x",
    )]));
    crate::api::config::write_restored_configurations(db, plan.entries, &[])
        .await
        .unwrap();
    let config = crate::services::config_service::ConfigService::load_config_on(db)
        .await
        .unwrap();
    assert_eq!(
        config.merope_persona_url.as_deref(),
        Some("http://[fd00:ec2::254]")
    );
    assert_eq!(
        crate::services::persona_style::configured(&config).unwrap_err(),
        crate::services::persona_style::PersonaStyleError::InvalidUrl
    );
    let shown = crate::services::persona_style::status(&config);
    assert_eq!(shown["source"], "remote");
    assert_eq!(shown["error"], "persona_style_url_invalid");
    let probed = crate::services::persona_style::probe(&config).await;
    assert!(probed["probe"].is_null(), "a refused URL is never asked");
    schema.drop().await;
}
