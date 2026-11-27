//! Unit tests for `persona_style.rs`: the builtin recipe is today's, and a
//! recipe service's answer is used only when every part of it checks out.

use std::sync::{Arc, Mutex};

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    routing::{get, post},
};
use base64::Engine as _;
use myriad_merope::{
    MEROPE_STYLE_REFERENCE_SHA256, build_character_visual_edit_prompt,
    build_character_visual_prompt, build_full_body_design_prompt, build_full_body_portrait_prompt,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::*;

pub(crate) const TOKEN: &str = "persona-test-token-0123456789abcdef";

pub(crate) fn profile() -> Value {
    json!({
        "gender": "female",
        "language": "en-US",
        "clothingStyle": "everyday",
        "visualIdentity": {
            "character": {
                "faceDesign": "feminine young-adult read, elongated oval face with a calm gaze",
                "eyeDesign": "violet eyes with layered irises",
                "hairShape": "long silver hair in broad ribbon masses",
                "hairLayerPlan": "a few broad front locks over a long back mass"
            },
            "outfit": {
                "upperBodySilhouette": "fitted jacket over a wrapped layer",
                "outfitConstruction": "asymmetric closure with an open neckline",
                "sleeveArmDesign": "split sleeves with ribbon cuffs",
                "heroAccessory": "a crescent brooch",
                "materialPlan": "matte wool and polished silver",
                "paletteHint": "silver, violet and white",
                "motif": "crescent moons",
                "clothingStyle": "everyday"
            }
        }
    })
}

/// A tiny valid PNG, as a recipe service's style reference.
pub(crate) fn png() -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n".to_vec();
    bytes.extend_from_slice(b"house-style-reference");
    bytes
}

pub(crate) fn sha(bytes: &[u8]) -> String {
    hex::encode(Sha256::digest(bytes))
}

pub(crate) fn served_recipe(prompt: &str, references: &[Vec<u8>], version: &str) -> Value {
    json!({
        "schema": 1,
        "prompt": prompt,
        "references": references.iter().map(|bytes| json!({
            "mimeType": "image/png",
            "sha256": sha(bytes),
            "data": base64::engine::general_purpose::STANDARD.encode(bytes),
        })).collect::<Vec<_>>(),
        "rendering": {
            "recipe": "merope-persona",
            "visualSchoolVersion": version,
            "styleReferenceSha256": references.first().map(|bytes| sha(bytes)),
            "fullBodyInstructionVersion": "full-body-standing-v1",
        },
    })
}

/// What a fake recipe service answers, and what it was asked.
#[derive(Clone)]
pub(crate) struct Fake {
    pub(crate) status: StatusCode,
    pub(crate) body: Arc<Mutex<Vec<u8>>>,
    pub(crate) asked: Arc<Mutex<Vec<(HeaderMap, Value)>>>,
}

impl Fake {
    pub(crate) fn answering(status: StatusCode, body: &Value) -> Self {
        Self {
            status,
            body: Arc::new(Mutex::new(serde_json::to_vec(body).unwrap())),
            asked: Arc::default(),
        }
    }

    pub(crate) fn raw(status: StatusCode, body: Vec<u8>) -> Self {
        Self {
            status,
            body: Arc::new(Mutex::new(body)),
            asked: Arc::default(),
        }
    }

    pub(crate) fn asked(&self) -> Vec<(HeaderMap, Value)> {
        self.asked.lock().unwrap().clone()
    }
}

async fn answer(State(fake): State<Fake>, headers: HeaderMap, body: Bytes) -> impl IntoResponse {
    let value = serde_json::from_slice(&body).unwrap_or(Value::Null);
    fake.asked.lock().unwrap().push((headers, value));
    (
        fake.status,
        [("content-type", "application/json")],
        fake.body.lock().unwrap().clone(),
    )
}

/// A fake recipe service on loopback; its base URL.
pub(crate) async fn serve(fake: Fake) -> String {
    let app = Router::new()
        .route("/v1/recipe", post(answer))
        .route("/v1/info", get(answer))
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

pub(crate) fn remote(url: &str) -> StyleSource {
    StyleSource::Remote(RemoteRecipe::new(url, TOKEN).unwrap())
}

fn bust<'a>(visual_profile: &'a Value) -> RecipeRequest<'a> {
    RecipeRequest::Bust {
        name: "星见",
        visual_profile,
        notes: Some("softer rim light"),
    }
}

#[test]
fn the_builtin_recipe_is_todays_prompts_and_reference() {
    let profile = profile();
    let cases = [
        (
            bust(&profile),
            build_character_visual_prompt("星见", &profile, Some("softer rim light")),
            true,
        ),
        (
            RecipeRequest::BustEdit {
                notes: "softer rim light",
            },
            build_character_visual_edit_prompt("softer rim light"),
            false,
        ),
        (
            RecipeRequest::FullBodyDesign {
                name: "星见",
                visual_profile: &profile,
            },
            build_full_body_design_prompt("星见", &profile),
            true,
        ),
        (
            RecipeRequest::FullBodyFromBust,
            build_full_body_portrait_prompt(),
            false,
        ),
    ];
    for (request, prompt, with_reference) in cases {
        let recipe = builtin(request).unwrap();
        assert_eq!(recipe.prompt, prompt, "{request:?}");
        assert_eq!(recipe.rendering, RenderingProvenance::builtin());
        assert_eq!(recipe.source, SourceKind::Builtin);
        assert_eq!(recipe.references.len(), usize::from(with_reference));
        if with_reference {
            let reference = &recipe.references[0];
            assert_eq!(reference.media_type, "image/png");
            assert_eq!(reference.bytes, BUILTIN_STYLE_REFERENCE_BYTES);
        }
    }
}

#[test]
fn bundled_style_reference_matches_the_versioned_contract() {
    assert_eq!(
        sha(BUILTIN_STYLE_REFERENCE_BYTES),
        MEROPE_STYLE_REFERENCE_SHA256
    );
    assert_eq!(
        RenderingProvenance::builtin()
            .style_reference_sha256
            .as_deref(),
        Some(MEROPE_STYLE_REFERENCE_SHA256)
    );
}

#[test]
fn the_configuration_names_the_source_and_never_falls_back() {
    let mut dynamic = DynamicConfig::default();
    assert!(matches!(configured(&dynamic), Ok(StyleSource::Builtin)));
    dynamic.merope_persona_url = Some("http://merope-persona:8080".into());
    assert_eq!(
        configured(&dynamic).unwrap_err(),
        PersonaStyleError::NotConfigured,
        "a URL without a token is an error, not the builtin recipe"
    );
    dynamic.merope_persona_token = Some(TOKEN.into());
    assert!(matches!(configured(&dynamic), Ok(StyleSource::Remote(_))));
    dynamic.merope_persona_url = Some("http://persona.example.com".into());
    assert_eq!(
        configured(&dynamic).unwrap_err(),
        PersonaStyleError::NotConfigured
    );
    dynamic.merope_persona_url = Some("  ".into());
    assert!(matches!(configured(&dynamic), Ok(StyleSource::Builtin)));
}

#[test]
fn the_service_url_policy() {
    for (raw, expected) in [
        ("", Some("")),
        (
            "https://persona.example.com/",
            Some("https://persona.example.com"),
        ),
        (
            "https://persona.example.com/base/",
            Some("https://persona.example.com/base"),
        ),
        (
            "http://merope-persona:8080",
            Some("http://merope-persona:8080"),
        ),
        ("http://127.0.0.1:9000/", Some("http://127.0.0.1:9000")),
        ("http://10.0.0.5", Some("http://10.0.0.5")),
        ("http://[::1]:8080", Some("http://[::1]:8080")),
        ("http://localhost:8080", Some("http://localhost:8080")),
        ("http://persona.example.com", None),
        ("http://8.8.8.8", None),
        ("ftp://merope-persona", None),
        ("https://user:pass@persona.example.com", None),
        ("https://persona.example.com/?token=x", None),
        ("https://persona.example.com/#x", None),
        ("not a url", None),
    ] {
        assert_eq!(
            remote::sanitize_service_url(raw).as_deref(),
            expected,
            "{raw}"
        );
    }
}

#[tokio::test]
async fn a_recipe_service_answer_is_used_as_sent() {
    let fake = Fake::answering(
        StatusCode::OK,
        &served_recipe("House prompt.\nSecond line.", &[png()], "house-v6"),
    );
    let url = serve(fake.clone()).await;
    let profile = profile();
    let recipe = recipe(&remote(&url), bust(&profile)).await.unwrap();
    assert_eq!(recipe.prompt, "House prompt.\nSecond line.");
    assert_eq!(recipe.source, SourceKind::Remote);
    assert_eq!(recipe.references.len(), 1);
    assert_eq!(recipe.references[0].bytes, png());
    assert_eq!(recipe.rendering.recipe.as_deref(), Some("merope-persona"));
    assert_eq!(recipe.rendering.visual_school_version, "house-v6");
    assert_eq!(recipe.rendering.style_reference_sha256, Some(sha(&png())));

    let asked = fake.asked();
    assert_eq!(asked.len(), 1);
    let (headers, body) = &asked[0];
    assert_eq!(
        headers.get("authorization").unwrap(),
        &format!("Bearer {TOKEN}")
    );
    assert_eq!(
        body,
        &json!({
            "schema": 1,
            "kind": "bust",
            "name": "星见",
            "visualProfile": profile,
            "notes": "softer rim light",
        })
    );
}

#[tokio::test]
async fn each_kind_goes_over_the_wire_as_schema_1() {
    let fake = Fake::answering(StatusCode::OK, &served_recipe("p", &[], "house-v6"));
    let url = serve(fake.clone()).await;
    let source = remote(&url);
    let profile = profile();
    recipe(&source, RecipeRequest::BustEdit { notes: "light" })
        .await
        .unwrap();
    recipe(&source, RecipeRequest::FullBodyFromBust)
        .await
        .unwrap();
    let asked: Vec<Value> = fake.asked().into_iter().map(|(_, body)| body).collect();
    assert_eq!(
        asked,
        [
            json!({ "schema": 1, "kind": "bustEdit", "notes": "light" }),
            json!({ "schema": 1, "kind": "fullBodyFromBust" }),
        ]
    );
    // A design-drawn picture with no reference records none.
    let no_reference = recipe(
        &source,
        RecipeRequest::FullBodyDesign {
            name: "星见",
            visual_profile: &profile,
        },
    )
    .await
    .unwrap();
    assert!(no_reference.references.is_empty());
    assert_eq!(no_reference.rendering.style_reference_sha256, None);
}

#[tokio::test]
async fn a_failing_service_is_an_error_and_never_the_builtin_recipe() {
    let profile = profile();
    for (status, expected) in [
        (StatusCode::UNAUTHORIZED, PersonaStyleError::Unauthorized),
        (StatusCode::FORBIDDEN, PersonaStyleError::Unauthorized),
        (
            StatusCode::UNPROCESSABLE_ENTITY,
            PersonaStyleError::Rejected,
        ),
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            PersonaStyleError::Unavailable,
        ),
        (
            StatusCode::SERVICE_UNAVAILABLE,
            PersonaStyleError::Unavailable,
        ),
        (
            StatusCode::TOO_MANY_REQUESTS,
            PersonaStyleError::Unavailable,
        ),
        (
            StatusCode::NOT_FOUND,
            PersonaStyleError::InvalidResponse("unexpected status"),
        ),
    ] {
        let url = serve(Fake::answering(
            status,
            &json!({ "error": { "code": "x" } }),
        ))
        .await;
        assert_eq!(
            recipe(&remote(&url), bust(&profile)).await.unwrap_err(),
            expected,
            "{status}"
        );
    }
    // Nothing listening.
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    assert_eq!(
        recipe(&remote(&url), bust(&profile)).await.unwrap_err(),
        PersonaStyleError::Unavailable
    );
}

#[tokio::test]
async fn a_redirect_is_not_followed() {
    let target = Fake::answering(StatusCode::OK, &served_recipe("p", &[png()], "v"));
    let target_url = serve(target.clone()).await;
    let redirect = Router::new().route(
        "/v1/recipe",
        post(move || {
            let location = format!("{target_url}/v1/recipe");
            async move { (StatusCode::TEMPORARY_REDIRECT, [("location", location)]) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, redirect).await.unwrap() });
    let profile = profile();
    assert_eq!(
        recipe(&remote(&format!("http://{addr}")), bust(&profile))
            .await
            .unwrap_err(),
        PersonaStyleError::InvalidResponse("unexpected status")
    );
    assert!(target.asked().is_empty(), "the token went nowhere else");
}

#[tokio::test]
async fn an_unusable_answer_is_refused_part_by_part() {
    let profile = profile();
    let good = served_recipe("p", &[png()], "house-v6");
    let mut cases: Vec<(&str, Value)> = Vec::new();
    let mut v = good.clone();
    v["schema"] = json!(2);
    cases.push(("unsupported schema", v));
    let mut v = good.clone();
    v["extra"] = json!(1);
    cases.push(("not a schema 1 recipe", v));
    let mut v = good.clone();
    v["prompt"] = json!("  ");
    cases.push(("prompt out of bounds", v));
    let mut v = good.clone();
    v["prompt"] = json!("a\u{0}b");
    cases.push(("prompt out of bounds", v));
    let mut v = good.clone();
    v["prompt"] = json!("p".repeat(remote::MAX_PROMPT_CHARS + 1));
    cases.push(("prompt out of bounds", v));
    let mut v = good.clone();
    v["rendering"].as_object_mut().unwrap().remove("recipe");
    cases.push(("rendering does not name its recipe", v));
    let mut v = good.clone();
    v["rendering"]["visualSchoolVersion"] = json!("v6\nx");
    cases.push(("rendering out of bounds", v));
    let mut v = good.clone();
    v["rendering"]["styleReferenceSha256"] = json!("c".repeat(64));
    cases.push(("rendering does not record the reference sent", v));
    let mut v = good.clone();
    v["references"][0]["sha256"] = json!("d".repeat(64));
    cases.push(("reference hash does not match", v));
    let mut v = good.clone();
    v["references"][0]["data"] = json!("@@not base64@@");
    cases.push(("reference is not base64", v));
    let mut v = served_recipe("p", &[b"GIF89a-not-allowed".to_vec()], "v");
    v["references"][0]["mimeType"] = json!("image/gif");
    cases.push(("reference is not an image", v));
    cases.push((
        "too many references",
        served_recipe("p", &[png(), png(), png()], "v"),
    ));
    let mut big = png();
    big.resize(remote::MAX_REFERENCE_BYTES + 1, 0);
    cases.push(("reference too large", served_recipe("p", &[big], "v")));
    for (expected, body) in cases {
        let url = serve(Fake::answering(StatusCode::OK, &body)).await;
        assert_eq!(
            recipe(&remote(&url), bust(&profile)).await.unwrap_err(),
            PersonaStyleError::InvalidResponse(expected)
        );
    }
    // An edit keeps its own portrait: a reference sent with it is refused.
    let url = serve(Fake::answering(StatusCode::OK, &good)).await;
    assert_eq!(
        recipe(&remote(&url), RecipeRequest::BustEdit { notes: "light" })
            .await
            .unwrap_err(),
        PersonaStyleError::InvalidResponse("references for a picture that keeps its own")
    );
    // Larger than the whole-answer cap.
    let url = serve(Fake::raw(
        StatusCode::OK,
        vec![b' '; remote::MAX_RESPONSE_BYTES + 1],
    ))
    .await;
    assert_eq!(
        recipe(&remote(&url), bust(&profile)).await.unwrap_err(),
        PersonaStyleError::InvalidResponse("answer too large or cut off")
    );
}

#[test]
fn errors_and_the_source_never_show_the_token() {
    let source = RemoteRecipe::new("http://merope-persona:8080", TOKEN).unwrap();
    assert!(!format!("{source:?}").contains(TOKEN));
    for error in [
        PersonaStyleError::NotConfigured,
        PersonaStyleError::Unavailable,
        PersonaStyleError::Unauthorized,
        PersonaStyleError::Rejected,
        PersonaStyleError::InvalidResponse("prompt out of bounds"),
    ] {
        assert!(!error.to_string().contains(TOKEN));
        assert!(!error.to_string().contains("merope-persona:8080"));
        assert!(error.code().starts_with("persona_style_"));
    }
    assert!(RemoteRecipe::new("http://merope-persona:8080", "has space").is_err());
    assert!(RemoteRecipe::new("http://merope-persona:8080", "").is_err());
}

#[tokio::test]
async fn status_says_which_source_and_whether_it_answers() {
    let mut dynamic = DynamicConfig::default();
    let builtin = status(&dynamic).await;
    assert_eq!(builtin["source"], "builtin");
    assert_eq!(builtin["available"], true);
    assert_eq!(
        builtin["visualSchoolVersion"],
        myriad_merope::MEROPE_VISUAL_SCHOOL_VERSION
    );

    let info = json!({
        "schema": 1,
        "rendering": {
            "recipe": "merope-persona",
            "visualSchoolVersion": "house-v6",
            "styleReferenceSha256": sha(&png()),
            "fullBodyInstructionVersion": "full-body-standing-v1",
        }
    });
    let url = serve(Fake::answering(StatusCode::OK, &info)).await;
    dynamic.merope_persona_url = Some(url);
    dynamic.merope_persona_token = Some(TOKEN.into());
    let up = status(&dynamic).await;
    assert_eq!(up["source"], "remote");
    assert_eq!(up["available"], true);
    assert_eq!(up["recipe"], "merope-persona");
    assert_eq!(up["visualSchoolVersion"], "house-v6");
    assert!(!up.to_string().contains(TOKEN));

    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    dynamic.merope_persona_url = Some(format!("http://{}", closed.local_addr().unwrap()));
    drop(closed);
    let down = status(&dynamic).await;
    assert_eq!(down["available"], false);
    assert_eq!(down["error"], "persona_style_unavailable");

    dynamic.merope_persona_token = None;
    let incomplete = status(&dynamic).await;
    assert_eq!(incomplete["configured"], false);
    assert_eq!(incomplete["tokenConfigured"], false);
    assert_eq!(incomplete["error"], "persona_style_not_configured");
}
