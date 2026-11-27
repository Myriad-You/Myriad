//! Unit tests for `persona_style.rs`: the builtin recipe builds the plain
//! builders' prompts, a recipe service's answer is used only when every part
//! of it checks out, and the configuration never falls back to the builtin
//! recipe on its own.

use std::{
    net::IpAddr,
    sync::{Arc, Mutex},
};

use axum::{
    Router,
    body::Bytes,
    extract::State,
    http::{HeaderMap, Method, StatusCode},
    response::IntoResponse,
    routing::{any, get},
};
use base64::Engine as _;
use myriad_merope::{
    MEROPE_STYLE_REFERENCE_SHA256, build_character_visual_edit_prompt,
    build_character_visual_prompt, build_full_body_design_prompt, build_full_body_portrait_prompt,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::remote::{Route, route};
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

/// Style words other than the builtin ones, with a reference role.
pub(crate) fn house_style() -> Value {
    json!({
        "school": "Clean flat-colour anime illustration with crisp line art",
        "referenceRole": "Use the attached image for rendering technique only.",
        "priority": "Priority order: framing first, then the confirmed identity.",
        "editFinish": "the same flat-colour finish",
        "redrawFinish": "the same flat-colour finish and lighting",
    })
}

/// A schema 1 recipe as a service sends it: `style` words, the references
/// and a provenance that matches them.
pub(crate) fn served_recipe(style: &Value, references: &[Vec<u8>], version: &str) -> Value {
    json!({
        "schema": 1,
        "recipe": {
            "id": "merope-persona",
            "visualSchoolVersion": version,
            "fullBodyInstructionVersion": "full-body-standing-v1",
            "style": style,
        },
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

/// `GET /v1/info` for [`served_recipe`].
pub(crate) fn served_info(version: &str, reference: Option<&[u8]>) -> Value {
    json!({
        "schema": 1,
        "recipe": {
            "id": "merope-persona",
            "visualSchoolVersion": version,
            "fullBodyInstructionVersion": "full-body-standing-v1",
        },
        "rendering": {
            "recipe": "merope-persona",
            "visualSchoolVersion": version,
            "styleReferenceSha256": reference.map(sha),
            "fullBodyInstructionVersion": "full-body-standing-v1",
        },
    })
}

/// One request a fake service received.
#[derive(Clone, Debug)]
pub(crate) struct Asked {
    pub(crate) method: Method,
    pub(crate) path: String,
    pub(crate) headers: HeaderMap,
    pub(crate) body: Bytes,
}

/// What a fake recipe service answers, and what it was asked.
#[derive(Clone)]
pub(crate) struct Fake {
    pub(crate) status: StatusCode,
    pub(crate) recipe: Arc<Mutex<Vec<u8>>>,
    pub(crate) info: Arc<Mutex<Vec<u8>>>,
    pub(crate) asked: Arc<Mutex<Vec<Asked>>>,
}

impl Fake {
    /// Answers `/v1/recipe` with `recipe` and `/v1/info` with the matching
    /// info (or `recipe` itself when it is not a recipe).
    pub(crate) fn answering(status: StatusCode, recipe: &Value) -> Self {
        let info = match recipe.get("recipe") {
            Some(head) => {
                let mut head = head.clone();
                if let Some(head) = head.as_object_mut() {
                    head.remove("style");
                }
                json!({ "schema": recipe["schema"], "recipe": head, "rendering": recipe["rendering"] })
            }
            None => recipe.clone(),
        };
        Self {
            status,
            recipe: Arc::new(Mutex::new(serde_json::to_vec(recipe).unwrap())),
            info: Arc::new(Mutex::new(serde_json::to_vec(&info).unwrap())),
            asked: Arc::default(),
        }
    }

    pub(crate) fn raw(status: StatusCode, body: Vec<u8>) -> Self {
        Self {
            status,
            recipe: Arc::new(Mutex::new(body.clone())),
            info: Arc::new(Mutex::new(body)),
            asked: Arc::default(),
        }
    }

    pub(crate) fn asked(&self) -> Vec<Asked> {
        self.asked.lock().unwrap().clone()
    }
}

async fn answer(
    State(fake): State<Fake>,
    method: Method,
    uri: axum::http::Uri,
    headers: HeaderMap,
    body: Bytes,
) -> impl IntoResponse {
    let path = uri.path().to_string();
    fake.asked.lock().unwrap().push(Asked {
        method,
        path: path.clone(),
        headers,
        body,
    });
    let answer = if path.ends_with("/v1/info") {
        fake.info.lock().unwrap().clone()
    } else {
        fake.recipe.lock().unwrap().clone()
    };
    (
        fake.status,
        [("content-type", "application/json")],
        answer,
    )
}

/// A fake recipe service on loopback; its base URL.
pub(crate) async fn serve(fake: Fake) -> String {
    let app = Router::new()
        .route("/v1/recipe", any(answer))
        .route("/v1/info", any(answer))
        .with_state(fake);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    format!("http://{addr}")
}

/// A loopback URL nothing listens on.
pub(crate) async fn closed_url() -> String {
    let closed = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", closed.local_addr().unwrap());
    drop(closed);
    url
}

pub(crate) fn remote(url: &str) -> StyleSource {
    StyleSource::Remote(RemoteRecipe::new(url, TOKEN).unwrap())
}

fn drawings(profile: &Value) -> [Drawing<'_>; 4] {
    [
        Drawing::Bust {
            name: "星见",
            visual_profile: profile,
            notes: Some("softer rim light"),
        },
        Drawing::BustEdit {
            notes: "softer rim light",
        },
        Drawing::FullBodyDesign {
            name: "星见",
            visual_profile: profile,
        },
        Drawing::FullBodyFromBust,
    ]
}

#[test]
fn the_builtin_recipe_builds_the_plain_builders_prompts_and_attaches_the_bundled_reference() {
    let profile = profile();
    let plain = [
        build_character_visual_prompt("星见", &profile, Some("softer rim light")),
        build_character_visual_edit_prompt("softer rim light"),
        build_full_body_design_prompt("星见", &profile),
        build_full_body_portrait_prompt(),
    ];
    let recipe = builtin();
    assert_eq!(recipe.rendering, RenderingProvenance::builtin());
    assert_eq!(recipe.source, SourceKind::Builtin);
    for (drawing, plain) in drawings(&profile).into_iter().zip(plain) {
        assert_eq!(recipe.prompt(drawing), plain, "{drawing:?}");
        let references = recipe.style_references(drawing);
        if drawing.uses_style_reference() {
            assert_eq!(references, vec![builtin_style_reference()], "{drawing:?}");
        } else {
            assert!(references.is_empty(), "{drawing:?}");
        }
    }
}

#[test]
fn the_bundled_style_reference_hash_is_the_builtin_provenance_hash() {
    assert_eq!(
        sha(&builtin_style_reference().bytes),
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
fn no_url_is_the_builtin_recipe_and_a_url_without_a_token_is_an_error() {
    let mut dynamic = DynamicConfig::default();
    assert!(matches!(configured(&dynamic), Ok(StyleSource::Builtin)));
    dynamic.merope_persona_url = Some("http://merope-persona:8080".into());
    assert_eq!(
        configured(&dynamic).unwrap_err(),
        PersonaStyleError::NotConfigured
    );
    dynamic.merope_persona_token = Some(TOKEN.into());
    assert!(matches!(configured(&dynamic), Ok(StyleSource::Remote(_))));
    dynamic.merope_persona_url = Some("  ".into());
    assert!(matches!(configured(&dynamic), Ok(StyleSource::Builtin)));
}

#[test]
fn a_stored_url_failing_the_policy_reads_back_as_a_configuration_error() {
    let mut dynamic = DynamicConfig {
        merope_persona_url: Some("http://169.254.169.254".into()),
        merope_persona_token: Some(TOKEN.into()),
        ..DynamicConfig::default()
    };
    assert_eq!(
        configured(&dynamic).unwrap_err(),
        PersonaStyleError::InvalidUrl
    );
    let shown = status(&dynamic);
    assert_eq!(shown["source"], "remote");
    assert_eq!(shown["configured"], false);
    assert_eq!(shown["error"], "persona_style_url_invalid");
    assert!(!shown.to_string().contains("169.254"));
    // The same without a token is still the URL's error, not "builtin".
    dynamic.merope_persona_token = None;
    assert_eq!(
        configured(&dynamic).unwrap_err(),
        PersonaStyleError::InvalidUrl
    );
}

#[test]
fn the_url_policy_refuses_link_local_even_for_http_and_keeps_services_beside_the_site() {
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
        ("http://192.168.1.20:8080", Some("http://192.168.1.20:8080")),
        ("http://[::1]:8080", Some("http://[::1]:8080")),
        ("http://[fd00::5]:8080", Some("http://[fd00::5]:8080")),
        ("http://localhost:8080", Some("http://localhost:8080")),
        ("http://169.254.169.254", None),
        ("https://169.254.169.254", None),
        ("http://169.254.1.1:8080", None),
        ("http://[fe80::1]:8080", None),
        ("https://[fe80::1]", None),
        ("http://[::ffff:169.254.169.254]", None),
        ("http://[fd00:ec2::254]", None),
        ("https://[fd00:ec2::254]:8443", None),
        ("https://100.100.100.200", None),
        ("http://0.0.0.0:8080", None),
        ("http://224.0.0.1", None),
        ("http://persona.example.com", None),
        ("http://8.8.8.8", None),
        ("ftp://merope-persona", None),
        ("https://user:pass@persona.example.com", None),
        ("https://persona.example.com/?token=x", None),
        ("https://persona.example.com/#x", None),
        ("not a url", None),
    ] {
        assert_eq!(sanitize_service_url(raw).as_deref(), expected, "{raw}");
    }
}

#[test]
fn public_hosts_go_through_the_proxy_and_hosts_beside_the_site_go_direct() {
    let ip = |text: &str| text.parse::<IpAddr>().unwrap();
    let public = [ip("93.184.216.34")];
    let private = [ip("10.0.0.5"), ip("127.0.0.1")];
    assert_eq!(route("https", false, &public, true), Ok(Route::Proxy));
    assert_eq!(route("https", false, &public, false), Ok(Route::Direct));
    assert_eq!(route("https", false, &private, true), Ok(Route::Direct));
    assert_eq!(route("http", false, &private, true), Ok(Route::Direct));
    // A single-label name (a container beside the site) goes direct; plain
    // http to it only when it resolves to loopback or private addresses.
    assert_eq!(route("http", true, &private, true), Ok(Route::Direct));
    assert_eq!(route("https", true, &public, true), Ok(Route::Direct));
    assert_eq!(
        route("http", true, &public, false),
        Err(PersonaStyleError::InvalidUrl),
        "a single-label name resolving to a public address needs https"
    );
    // Plain http to a public address is refused, as is any link-local one.
    assert_eq!(
        route("http", false, &public, true),
        Err(PersonaStyleError::InvalidUrl)
    );
    for refused in [
        "169.254.169.254",
        "fe80::1",
        "fd00:ec2::254",
        "100.100.100.200",
        "0.0.0.0",
        "::",
    ] {
        assert_eq!(
            route("https", true, &[ip("10.0.0.5"), ip(refused)], false),
            Err(PersonaStyleError::InvalidUrl),
            "{refused}"
        );
    }
    assert_eq!(
        route("https", false, &[], false),
        Err(PersonaStyleError::InvalidUrl)
    );
}

#[tokio::test]
async fn a_served_recipe_is_used_as_sent_and_nothing_about_the_design_is_sent() {
    let fake = Fake::answering(
        StatusCode::OK,
        &served_recipe(&house_style(), &[png()], "house-v6"),
    );
    let url = serve(fake.clone()).await;
    let got = recipe(&remote(&url)).await.unwrap();
    assert_eq!(got.style, serde_json::from_value(house_style()).unwrap());
    assert_eq!(got.source, SourceKind::Remote);
    assert_eq!(got.reference.as_ref().unwrap().bytes, png());
    assert_eq!(got.rendering.recipe.as_deref(), Some("merope-persona"));
    assert_eq!(got.rendering.visual_school_version, "house-v6");
    assert_eq!(got.rendering.style_reference_sha256, Some(sha(&png())));

    let asked = fake.asked();
    assert_eq!(asked.len(), 1);
    assert_eq!(asked[0].method, Method::GET);
    assert_eq!(asked[0].path, "/v1/recipe");
    assert!(asked[0].body.is_empty());
    assert_eq!(
        asked[0].headers.get("authorization").unwrap(),
        &format!("Bearer {TOKEN}")
    );

    // The site builds the prompt itself from the service's words: the
    // identity sections are the builtin prompt's, the style words the
    // service's.
    let profile = profile();
    let drawing = Drawing::Bust {
        name: "星见",
        visual_profile: &profile,
        notes: None,
    };
    let prompt = got.prompt(drawing);
    assert!(prompt.starts_with(
        "Render one confirmed original character in this visual school: Clean flat-colour anime illustration with crisp line art."
    ));
    assert!(prompt.ends_with("Priority order: framing first, then the confirmed identity."));
    assert!(!prompt.contains(myriad_merope::MEROPE_VISUAL_SCHOOL));
}

/// An edit or a redraw attaches no style reference, so it asks only
/// `/v1/info` and reuses the recipe last checked from that origin while the
/// provenance is the same; a new provenance fetches and checks the recipe
/// again, and its words are the ones used.
#[tokio::test]
async fn a_drawing_without_a_reference_reuses_the_checked_recipe_while_the_provenance_holds() {
    let fake = Fake::answering(
        StatusCode::OK,
        &served_recipe(&house_style(), &[png()], "house-v6"),
    );
    let url = serve(fake.clone()).await;
    let source = remote(&url);
    let profile = profile();
    let paths = |fake: &Fake| {
        fake.asked()
            .iter()
            .map(|asked| asked.path.clone())
            .collect::<Vec<_>>()
    };
    let edit = Drawing::BustEdit { notes: "light" };
    // Nothing checked yet from this origin: info, then the full recipe.
    let first = recipe_for(&source, edit).await.unwrap();
    assert_eq!(paths(&fake), ["/v1/info", "/v1/recipe"]);
    // Same provenance: info only.
    let again = recipe_for(&source, Drawing::FullBodyFromBust).await.unwrap();
    assert_eq!(paths(&fake), ["/v1/info", "/v1/recipe", "/v1/info"]);
    assert_eq!(again.style, first.style);
    assert_eq!(again.rendering, first.rendering);
    // A drawing with a reference always fetches the recipe.
    recipe_for(
        &source,
        Drawing::Bust {
            name: "星见",
            visual_profile: &profile,
            notes: None,
        },
    )
    .await
    .unwrap();
    assert_eq!(paths(&fake).last().map(String::as_str), Some("/v1/recipe"));

    // The service moves to new words under a new version.
    let mut moved = house_style();
    moved["editFinish"] = json!("the same watercolour finish");
    let next = Fake::answering(StatusCode::OK, &served_recipe(&moved, &[png()], "house-v7"));
    *fake.recipe.lock().unwrap() = next.recipe.lock().unwrap().clone();
    *fake.info.lock().unwrap() = next.info.lock().unwrap().clone();
    let before = fake.asked().len();
    let after = recipe_for(&source, edit).await.unwrap();
    assert_eq!(
        paths(&fake)[before..],
        ["/v1/info".to_string(), "/v1/recipe".to_string()]
    );
    assert_eq!(after.rendering.visual_school_version, "house-v7");
    assert!(after.prompt(edit).contains("the same watercolour finish"));
}

#[tokio::test]
async fn a_recipe_without_a_reference_records_none() {
    let mut style = house_style();
    style.as_object_mut().unwrap().remove("referenceRole");
    let url = serve(Fake::answering(
        StatusCode::OK,
        &served_recipe(&style, &[], "house-v6"),
    ))
    .await;
    let got = recipe(&remote(&url)).await.unwrap();
    assert!(got.reference.is_none());
    assert_eq!(got.rendering.style_reference_sha256, None);
    let profile = profile();
    for drawing in drawings(&profile) {
        assert!(got.style_references(drawing).is_empty());
    }
}

#[tokio::test]
async fn a_failing_service_is_an_error_and_never_the_builtin_recipe() {
    for (status, expected) in [
        (StatusCode::UNAUTHORIZED, PersonaStyleError::Unauthorized),
        (StatusCode::FORBIDDEN, PersonaStyleError::Unauthorized),
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
        let url = serve(Fake::answering(status, &json!({ "error": "refused" }))).await;
        assert_eq!(
            recipe(&remote(&url)).await.unwrap_err(),
            expected,
            "{status}"
        );
    }
    assert_eq!(
        recipe(&remote(&closed_url().await)).await.unwrap_err(),
        PersonaStyleError::Unavailable
    );
}

#[tokio::test]
async fn a_redirect_is_not_followed_so_the_token_goes_nowhere_else() {
    let target = Fake::answering(
        StatusCode::OK,
        &served_recipe(&house_style(), &[png()], "v"),
    );
    let target_url = serve(target.clone()).await;
    let redirect = Router::new().route(
        "/v1/recipe",
        get(move || {
            let location = format!("{target_url}/v1/recipe");
            async move { (StatusCode::TEMPORARY_REDIRECT, [("location", location)]) }
        }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, redirect).await.unwrap() });
    assert_eq!(
        recipe(&remote(&format!("http://{addr}")))
            .await
            .unwrap_err(),
        PersonaStyleError::InvalidResponse("unexpected status")
    );
    assert!(target.asked().is_empty());
}

#[tokio::test]
async fn an_unusable_answer_is_refused_part_by_part() {
    let good = served_recipe(&house_style(), &[png()], "house-v6");
    let mut cases: Vec<(&str, Value)> = Vec::new();
    let mut v = good.clone();
    v["schema"] = json!(2);
    cases.push(("unsupported schema", v));
    let mut v = good.clone();
    v["prompt"] = json!("a prompt of its own");
    cases.push(("not a schema 1 recipe", v));
    let mut v = good.clone();
    v["recipe"]["style"]["extra"] = json!("x");
    cases.push(("not a schema 1 recipe", v));
    for (part, bad) in [
        ("school", json!("Plain\u{2028}illustration")),
        ("school", json!("Plain \u{202E}illustration")),
        ("school", json!("Anime key art in the style of Studio Ghibli")),
        ("school", json!("s".repeat(6_001))),
        ("priority", json!("")),
        ("editFinish", json!("the same\nfinish")),
        ("redrawFinish", json!(" padded")),
    ] {
        let mut v = good.clone();
        v["recipe"]["style"][part] = bad;
        cases.push(("style out of bounds", v));
    }
    // Words that leave the identity too little room.
    let mut v = good.clone();
    v["recipe"]["style"]["school"] = json!("s".repeat(5_000));
    v["recipe"]["style"]["referenceRole"] = json!("r".repeat(2_000));
    v["recipe"]["style"]["priority"] = json!("p".repeat(2_000));
    cases.push(("style out of bounds", v));
    let mut v = good.clone();
    v["rendering"].as_object_mut().unwrap().remove("recipe");
    cases.push(("rendering does not name its recipe", v));
    let mut v = good.clone();
    v["rendering"]["recipe"] = json!("builtin");
    v["recipe"]["id"] = json!("builtin");
    cases.push(("rendering out of bounds", v));
    let mut v = good.clone();
    v["rendering"]["visualSchoolVersion"] = json!("v6\nx");
    cases.push(("rendering out of bounds", v));
    let mut v = good.clone();
    v["rendering"]["visualSchoolVersion"] = json!("house-v7");
    cases.push(("rendering does not match the recipe and its reference", v));
    let mut v = good.clone();
    v["rendering"]["styleReferenceSha256"] = json!("c".repeat(64));
    cases.push(("rendering does not match the recipe and its reference", v));
    let mut v = good.clone();
    v["references"][0]["sha256"] = json!("d".repeat(64));
    cases.push(("reference hash does not match", v));
    let mut v = good.clone();
    v["references"][0]["data"] = json!("@@not base64@@");
    cases.push(("reference is not base64", v));
    let mut v = served_recipe(&house_style(), &[b"GIF89a-not-allowed".to_vec()], "v");
    v["references"][0]["mimeType"] = json!("image/gif");
    cases.push(("reference is not an image", v));
    cases.push((
        "more than one reference",
        served_recipe(&house_style(), &[png(), png()], "v"),
    ));
    let mut big = png();
    big.resize(remote::MAX_REFERENCE_BYTES + 1, 0);
    cases.push((
        "reference too large",
        served_recipe(&house_style(), &[big], "v"),
    ));
    let mut no_role = house_style();
    no_role.as_object_mut().unwrap().remove("referenceRole");
    cases.push((
        "reference role without a reference, or a reference without a role",
        served_recipe(&no_role, &[png()], "v"),
    ));
    cases.push((
        "reference role without a reference, or a reference without a role",
        served_recipe(&house_style(), &[], "v"),
    ));
    for (expected, body) in cases {
        let url = serve(Fake::answering(StatusCode::OK, &body)).await;
        assert_eq!(
            recipe(&remote(&url)).await.unwrap_err(),
            PersonaStyleError::InvalidResponse(expected),
            "{body:.200}"
        );
    }
    let url = serve(Fake::raw(
        StatusCode::OK,
        vec![b' '; remote::MAX_RESPONSE_BYTES + 1],
    ))
    .await;
    assert_eq!(
        recipe(&remote(&url)).await.unwrap_err(),
        PersonaStyleError::InvalidResponse("answer too large or cut off")
    );
}

#[test]
fn errors_and_the_source_never_show_the_token_or_the_url() {
    let source = RemoteRecipe::new("http://merope-persona:8080", TOKEN).unwrap();
    assert!(!format!("{source:?}").contains(TOKEN));
    for error in [
        PersonaStyleError::NotConfigured,
        PersonaStyleError::InvalidUrl,
        PersonaStyleError::Unavailable,
        PersonaStyleError::Unauthorized,
        PersonaStyleError::InvalidResponse("style out of bounds"),
    ] {
        assert!(!error.to_string().contains(TOKEN));
        assert!(!error.to_string().contains("merope-persona:8080"));
        assert!(error.code().starts_with("persona_style_"));
    }
    assert!(RemoteRecipe::new("http://merope-persona:8080", "has space").is_err());
    assert!(RemoteRecipe::new("http://merope-persona:8080", "").is_err());
}

#[tokio::test]
async fn status_makes_no_outbound_call_and_shows_the_probe_only_for_its_origin() {
    let builtin = status(&DynamicConfig::default());
    assert_eq!(builtin["source"], "builtin");
    assert_eq!(builtin["available"], true);
    assert_eq!(
        builtin["visualSchoolVersion"],
        myriad_merope::MEROPE_VISUAL_SCHOOL_VERSION
    );

    let fake = Fake::answering(
        StatusCode::OK,
        &served_recipe(&house_style(), &[png()], "house-v6"),
    );
    let url = serve(fake.clone()).await;
    let dynamic = DynamicConfig {
        merope_persona_url: Some(url.clone()),
        merope_persona_token: Some(TOKEN.into()),
        ..DynamicConfig::default()
    };
    for _ in 0..3 {
        let shown = status(&dynamic);
        assert_eq!(shown["source"], "remote");
        assert_eq!(shown["configured"], true);
        assert_eq!(shown["probe"], Value::Null);
    }
    assert!(fake.asked().is_empty(), "reading the status sends nothing");

    let probed = probe(&dynamic).await;
    assert_eq!(fake.asked().len(), 1);
    assert_eq!(fake.asked()[0].path, "/v1/info");
    assert_eq!(probed["probe"]["available"], true);
    assert_eq!(probed["probe"]["recipe"], "merope-persona");
    assert_eq!(probed["probe"]["visualSchoolVersion"], "house-v6");
    assert_eq!(status(&dynamic)["probe"], probed["probe"]);
    assert_eq!(fake.asked().len(), 1);
    for shown in [&probed, &status(&dynamic)] {
        assert!(!shown.to_string().contains(TOKEN));
        assert!(!shown.to_string().contains(&url));
    }

    // Another origin has no probe yet.
    let elsewhere = DynamicConfig {
        merope_persona_url: Some(closed_url().await),
        ..dynamic.clone()
    };
    assert_eq!(status(&elsewhere)["probe"], Value::Null);
    let down = probe(&elsewhere).await;
    assert_eq!(down["probe"]["available"], false);
    assert_eq!(down["probe"]["error"], "persona_style_unavailable");
    assert_eq!(down["probe"].as_object().unwrap().len(), 3);
}

#[tokio::test]
async fn a_probe_with_no_usable_configuration_sends_nothing() {
    let fake = Fake::answering(StatusCode::OK, &served_info("v", None));
    let url = serve(fake.clone()).await;
    let shown = probe(&DynamicConfig {
        merope_persona_url: Some(url),
        merope_persona_token: None,
        ..DynamicConfig::default()
    })
    .await;
    assert_eq!(shown["error"], "persona_style_not_configured");
    assert!(fake.asked().is_empty());
}

#[test]
fn changing_the_origin_clears_the_token_unless_the_same_change_supplies_one() {
    let a = Some("https://persona.example.com");
    for (current, url, token, expected) in [
        // Unchanged URL: the token stays.
        (a, None, None, None),
        // A new token is stored as given.
        (a, None, Some("t2"), Some("t2")),
        // Same origin, another path: the token stays.
        (a, Some("https://persona.example.com/v2"), None, None),
        // Another origin, no token with it: cleared.
        (a, Some("https://other.example.com"), None, Some("")),
        (a, Some("http://persona.example.com"), None, Some("")),
        (a, Some("https://persona.example.com:8443"), None, Some("")),
        // Another origin with a token: that token.
        (a, Some("https://other.example.com"), Some("t3"), Some("t3")),
        // Cleared URL: cleared token.
        (a, Some(""), None, Some("")),
        // First URL with no token alongside: whatever was stored goes.
        (None, Some("https://persona.example.com"), None, Some("")),
    ] {
        assert_eq!(
            token_after_url_change(current, url, token).as_deref(),
            expected,
            "{current:?} -> {url:?} with {token:?}"
        );
    }
}
