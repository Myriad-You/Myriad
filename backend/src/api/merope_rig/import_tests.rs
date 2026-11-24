//! Unit tests for `import.rs`.

use std::io::Write;

use axum::{body::Body, extract::FromRequest, http::Request};
use flate2::{Compression, write::ZlibEncoder};

use super::*;

const BOUNDARY: &str = "rig-import-test";
const PROFILES: [CharacterAssetProfile; 2] =
    [CharacterAssetProfile::Bust, CharacterAssetProfile::FullBody];

/// One upload is parsed at a time: the PNG validator turns a check away while
/// two others run, and tests run side by side.
static ONE_AT_A_TIME: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn push_png_chunk(bytes: &mut Vec<u8>, name: &[u8; 4], data: &[u8]) {
    bytes.extend_from_slice(&(data.len() as u32).to_be_bytes());
    bytes.extend_from_slice(name);
    bytes.extend_from_slice(data);
    bytes.extend_from_slice(&0u32.to_be_bytes());
}

/// A blank RGBA8 PNG of this size, the kind the page packs.
fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = vec![0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    push_png_chunk(&mut bytes, b"IHDR", &ihdr);
    let row = vec![0; width as usize * 4 + 1];
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    for _ in 0..height {
        encoder.write_all(&row).unwrap();
    }
    push_png_chunk(&mut bytes, b"IDAT", &encoder.finish().unwrap());
    push_png_chunk(&mut bytes, b"IEND", &[]);
    bytes
}

/// Source metadata that declares an atlas of this size and nothing to compile.
fn source(profile: CharacterAssetProfile, width: u32, height: u32) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "characterAssetContractVersion": 1,
        "profile": profile,
        "sourceMasterAssetId": "master",
        "canvas": { "width": 1024.0, "height": 1024.0 },
        "atlas": { "id": "atlas", "width": width, "height": height },
        "bones": [],
        "layers": [],
    }))
    .unwrap()
}

/// A field of an upload: its name, its content type when it is a file, its bytes.
type Field<'a> = (&'a str, Option<&'a str>, &'a [u8]);

fn text<'a>(name: &'a str, bytes: &'a [u8]) -> Field<'a> {
    (name, None, bytes)
}

fn file<'a>(name: &'a str, bytes: &'a [u8]) -> Field<'a> {
    (name, Some("image/png"), bytes)
}

/// Parses an upload of these fields.
async fn parse(
    profile: CharacterAssetProfile,
    atlas: AtlasUpload,
    fields: &[Field<'_>],
) -> ApiResult<ParsedRigImport> {
    let mut body = Vec::new();
    for (name, content_type, bytes) in fields {
        body.extend_from_slice(
            format!("--{BOUNDARY}\r\nContent-Disposition: form-data; name=\"{name}\"").as_bytes(),
        );
        if let Some(content_type) = content_type {
            body.extend_from_slice(
                format!("; filename=\"{name}.png\"\r\nContent-Type: {content_type}").as_bytes(),
            );
        }
        body.extend_from_slice(b"\r\n\r\n");
        body.extend_from_slice(bytes);
        body.extend_from_slice(b"\r\n");
    }
    body.extend_from_slice(format!("--{BOUNDARY}--\r\n").as_bytes());
    let request = Request::builder()
        .method("POST")
        .header(
            "content-type",
            format!("multipart/form-data; boundary={BOUNDARY}"),
        )
        .body(Body::from(body))
        .unwrap();
    let multipart = Multipart::from_request(request, &()).await.unwrap();
    let _one = ONE_AT_A_TIME.lock().await;
    parse_rig_import(multipart, profile, atlas).await
}

/// The status and message an upload was turned away with.
fn refusal(parsed: ApiResult<ParsedRigImport>) -> (StatusCode, String) {
    let Err((status, Json(body))) = parsed else {
        panic!("the upload should be turned away");
    };
    (
        status,
        body["error"].as_str().unwrap_or_default().to_owned(),
    )
}

#[tokio::test]
async fn a_preview_compiles_without_the_atlas_being_sent() {
    let reference = png(256, 256);
    for profile in PROFILES {
        let parsed = parse(
            profile,
            AtlasUpload::Optional,
            &[
                text("source", &source(profile, 512, 256)),
                file("analysisReference", &reference),
            ],
        )
        .await
        .unwrap();
        assert!(parsed.atlas_bytes.is_none());
        assert_eq!(
            (parsed.source.atlas.width, parsed.source.atlas.height),
            (512, 256)
        );
        assert_eq!(parsed.analysis_reference_bytes, Some(reference.clone()));
    }
}

#[tokio::test]
async fn a_preview_without_the_atlas_still_holds_its_declared_size_to_the_limits() {
    for profile in PROFILES {
        for (width, height) in [(255, 256), (256, 8_193), (0, 0)] {
            let refused = refusal(
                parse(
                    profile,
                    AtlasUpload::Optional,
                    &[text("source", &source(profile, width, height))],
                )
                .await,
            );
            assert_eq!(
                refused,
                (
                    StatusCode::BAD_REQUEST,
                    "Rig atlas must be between 256 and 8192 pixels on each side".to_owned()
                ),
                "{width}x{height}"
            );
        }
        // The slot it is for is checked from the source alone as well.
        let other = PROFILES[usize::from(profile == PROFILES[0])];
        let refused = refusal(
            parse(
                profile,
                AtlasUpload::Optional,
                &[text("source", &source(other, 256, 256))],
            )
            .await,
        );
        assert_eq!(refused.1, "Rig import is for another asset profile");
    }
}

#[tokio::test]
async fn a_preview_sent_with_its_atlas_has_it_checked() {
    let atlas = png(256, 256);
    for profile in PROFILES {
        let parsed = parse(
            profile,
            AtlasUpload::Optional,
            &[
                text("source", &source(profile, 256, 256)),
                file("atlas", &atlas),
            ],
        )
        .await
        .unwrap();
        assert_eq!(parsed.atlas_bytes, Some(atlas.clone()));

        // Bytes that are no PNG.
        let (status, message) = refusal(
            parse(
                profile,
                AtlasUpload::Optional,
                &[
                    text("source", &source(profile, 256, 256)),
                    file("atlas", b"not-a-png"),
                ],
            )
            .await,
        );
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(message, "Rig atlas must be a PNG image");

        // A PNG cut short.
        let (status, _) = refusal(
            parse(
                profile,
                AtlasUpload::Optional,
                &[
                    text("source", &source(profile, 256, 256)),
                    file("atlas", &atlas[..atlas.len() - 8]),
                ],
            )
            .await,
        );
        assert_eq!(status, StatusCode::BAD_REQUEST);

        // A sound PNG of another size than the source declares.
        let (status, message) = refusal(
            parse(
                profile,
                AtlasUpload::Optional,
                &[
                    text("source", &source(profile, 512, 256)),
                    file("atlas", &atlas),
                ],
            )
            .await,
        );
        assert_eq!(status, StatusCode::BAD_REQUEST);
        assert_eq!(message, "Rig atlas dimensions do not match source metadata");
    }
}

#[tokio::test]
async fn an_import_is_refused_without_its_atlas_and_kept_with_it() {
    let atlas = png(256, 256);
    for profile in PROFILES {
        let refused = refusal(
            parse(
                profile,
                AtlasUpload::Required,
                &[
                    text("source", &source(profile, 256, 256)),
                    file("analysisReference", &atlas),
                ],
            )
            .await,
        );
        assert_eq!(
            refused,
            (
                StatusCode::BAD_REQUEST,
                "Rig import is missing atlas PNG".to_owned()
            )
        );

        let parsed = parse(
            profile,
            AtlasUpload::Required,
            &[
                text("source", &source(profile, 256, 256)),
                file("atlas", &atlas),
            ],
        )
        .await
        .unwrap();
        assert_eq!(parsed.atlas_bytes, Some(atlas.clone()));

        let (status, _) = refusal(
            parse(
                profile,
                AtlasUpload::Required,
                &[
                    text("source", &source(profile, 256, 256)),
                    file("atlas", b"not-a-png"),
                ],
            )
            .await,
        );
        assert_eq!(status, StatusCode::BAD_REQUEST);
    }
}
