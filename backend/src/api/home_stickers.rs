//! Home free-layout stickers: admin AI generation and local image upload.

use axum::Json;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::GLOBAL_DYNAMIC_CONFIG;
use crate::error::HttpError;
use crate::services::ai_task_image::load_image_references;
use crate::services::image_generation::{
    self, ImageBackground, ImageGenerationError, image_generation_failure_code,
};
use myriad_error::AppError;

const MAX_PROMPT_CHARS: usize = 2_000;
const MAX_STICKER_UPLOAD_BYTES: usize = 10 * 1024 * 1024;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateHomeStickerRequest {
    pub prompt: String,
    pub width: Option<u32>,
    pub height: Option<u32>,
    #[serde(default)]
    pub reference_images: Vec<String>,
    /// Named family for wording, e.g. `16:9`.
    #[serde(default)]
    pub aspect: Option<String>,
    /// Drawn cell span; cover-crop math uses this against generate width/height.
    #[serde(default)]
    pub slot_cols: Option<u32>,
    #[serde(default)]
    pub slot_rows: Option<u32>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateHomeStickerResponse {
    pub image_url: String,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UploadHomeStickerRequest {
    pub image: String,
}

#[cfg(test)]
fn sticker_pixels(cols: u32, rows: u32) -> (u32, u32) {
    let px = |n: u32| n.saturating_mul(256).clamp(256, 2048);
    (px(cols.max(1)), px(rows.max(1)))
}

pub fn normalize_sticker_prompt(raw: &str) -> Result<String, AppError> {
    let prompt = raw.trim();
    let chars = prompt.chars().count();
    if chars == 0 || chars > MAX_PROMPT_CHARS {
        return Err(
            AppError::bad_request("sticker prompt must be 1 to 2000 characters")
                .with_code("INVALID_STICKER_PROMPT"),
        );
    }
    Ok(prompt.to_string())
}

fn ratio_label(w: u32, h: u32) -> String {
    let d = gcd_u32(w.max(1), h.max(1));
    format!("{}:{}", w / d, h / d)
}

fn gcd_u32(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    a.max(1)
}

fn pct(value: f64) -> i32 {
    (value * 100.0).round().clamp(1.0, 99.0) as i32
}

/// Cover-crop of generate canvas onto the drawn cell rectangle.
pub fn cover_crop_instruction(gen_w: u32, gen_h: u32, slot_w: u32, slot_h: u32) -> String {
    let gw = gen_w.max(1) as f64;
    let gh = gen_h.max(1) as f64;
    let sw = slot_w.max(1) as f64;
    let sh = slot_h.max(1) as f64;
    let gen_a = gw / gh;
    let slot_a = sw / sh;
    let slot = ratio_label(slot_w.max(1), slot_h.max(1));
    if (gen_a - slot_a).abs() / slot_a < 0.02 {
        return format!("Display is {slot}, same as this canvas; do not leave unused margins.");
    }
    if gen_a > slot_a {
        let visible = (slot_a / gen_a).clamp(0.05, 1.0);
        let side = pct((1.0 - visible) / 2.0);
        let mid = pct(visible);
        format!(
            "Center-crop left and right to {slot}. Discard the left {side}% and right {side}% of the canvas. Keep the entire subject inside the middle {mid}% of the width, vertically centered."
        )
    } else {
        let visible = (gen_a / slot_a).clamp(0.05, 1.0);
        let side = pct((1.0 - visible) / 2.0);
        let mid = pct(visible);
        format!(
            "Center-crop top and bottom to {slot}. Discard the top {side}% and bottom {side}% of the canvas. Keep the entire subject inside the middle {mid}% of the height, horizontally centered."
        )
    }
}

pub fn compose_sticker_prompt(
    prompt: &str,
    gen_w: u32,
    gen_h: u32,
    slot_cols: Option<u32>,
    slot_rows: Option<u32>,
    aspect: Option<&str>,
) -> String {
    let canvas = ratio_label(gen_w.max(1), gen_h.max(1));
    let slot_w = slot_cols.unwrap_or(gen_w).max(1);
    let slot_h = slot_rows.unwrap_or(gen_h).max(1);
    let named = aspect
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| format!(", {value} class"))
        .unwrap_or_default();
    let crop = cover_crop_instruction(gen_w.max(1), gen_h.max(1), slot_w, slot_h);
    format!(
        "{prompt}. Isolated die-cut sticker of that subject only, transparent background, no text, no extra props, no white plate. Draw on a {gen_w}x{gen_h} canvas ({canvas}). Final tile is {slot_w}x{slot_h} cells{named}. {crop}",
    )
}

fn invalid_sticker_image(message: &str) -> AppError {
    AppError::bad_request(message).with_code("INVALID_STICKER_IMAGE")
}

pub fn decode_sticker_upload(source: &str) -> Result<(Vec<u8>, &'static str), AppError> {
    let (metadata, encoded) = source
        .trim()
        .strip_prefix("data:")
        .and_then(|data| data.split_once(','))
        .ok_or_else(|| invalid_sticker_image("sticker image must be a data URL"))?;
    let media_type = metadata
        .strip_suffix(";base64")
        .and_then(|mime| match mime {
            "image/png" => Some("image/png"),
            "image/jpeg" => Some("image/jpeg"),
            "image/webp" => Some("image/webp"),
            _ => None,
        })
        .ok_or_else(|| invalid_sticker_image("sticker image must be base64 PNG, JPEG, or WebP"))?;
    if encoded.len() > MAX_STICKER_UPLOAD_BYTES.div_ceil(3) * 4 {
        return Err(invalid_sticker_image("sticker image is too large"));
    }
    let bytes = BASE64
        .decode(encoded.as_bytes())
        .map_err(|_| invalid_sticker_image("sticker image base64 is invalid"))?;
    if bytes.is_empty() || bytes.len() > MAX_STICKER_UPLOAD_BYTES {
        return Err(invalid_sticker_image("sticker image is too large"));
    }
    Ok((bytes, media_type))
}

fn map_generation_error(error: ImageGenerationError) -> HttpError {
    let code = image_generation_failure_code(&error);
    let app = match error {
        ImageGenerationError::NotConfigured(_) | ImageGenerationError::UnsupportedProvider(_) => {
            AppError::bad_request(error.to_string())
        }
        // A provider 4xx is our request (model/params/input), not a gateway fault;
        // surfacing it as 502 made UI errors misleading (issue #546).
        ImageGenerationError::Provider(_) if code == "image_provider_rejected" => {
            AppError::bad_request(error.to_string())
        }
        ImageGenerationError::Provider(_) | ImageGenerationError::InvalidResponse(_) => {
            AppError::bad_gateway(error.to_string())
        }
    };
    HttpError(app.with_code(code))
}

fn map_sticker_processing_error(
    error: crate::services::sticker_cutout::StickerProcessingError,
    generated: bool,
) -> HttpError {
    use crate::services::sticker_cutout::StickerProcessingError;
    HttpError(match error {
        StickerProcessingError::Busy => AppError::service_unavailable("Sticker processing is busy")
            .with_code("STICKER_PROCESSING_BUSY"),
        StickerProcessingError::WorkerFailed => {
            AppError::internal("Sticker processing failed").with_code("STICKER_PROCESSING_FAILED")
        }
        StickerProcessingError::Invalid(message) if generated => {
            AppError::internal(message).with_code("STICKER_CUTOUT_FAILED")
        }
        StickerProcessingError::Invalid(message) => {
            AppError::bad_request(message).with_code("INVALID_STICKER_IMAGE")
        }
    })
}

/// POST /api/home/stickers/generate — admin only.
pub async fn generate_home_sticker(
    crate::extract::Db(db): crate::extract::Db,
    Json(payload): Json<GenerateHomeStickerRequest>,
) -> Result<Json<GenerateHomeStickerResponse>, HttpError> {
    let prompt = normalize_sticker_prompt(&payload.prompt).map_err(HttpError)?;
    let (width, height) = (
        payload.width.unwrap_or(512).clamp(256, 2048),
        payload.height.unwrap_or(512).clamp(256, 2048),
    );
    let composed = compose_sticker_prompt(
        &prompt,
        width,
        height,
        payload.slot_cols,
        payload.slot_rows,
        payload.aspect.as_deref(),
    );

    let references = if payload.reference_images.is_empty() {
        Vec::new()
    } else {
        load_image_references(&json!({
            "prompt": prompt,
            "referenceImages": payload.reference_images,
        }))
        .await
        .map_err(|error| HttpError(AppError::bad_request(error.message).with_code(error.code)))?
    };

    let dynamic = GLOBAL_DYNAMIC_CONFIG.read().await;
    let config = image_generation::config_from_dynamic(&dynamic).map_err(map_generation_error)?;
    drop(dynamic);

    let generated = image_generation::generate_image_with_references(
        &config,
        &composed,
        width,
        height,
        &references,
        Some(ImageBackground::Transparent),
    )
    .await
    .map_err(map_generation_error)?;
    let (width, height) = (generated.width, generated.height);
    let (bytes, _) = image_generation::load_generated_bytes(generated)
        .await
        .map_err(map_generation_error)?;
    let png = crate::services::sticker_cutout::prepare_sticker_png(bytes)
        .await
        .map_err(|error| map_sticker_processing_error(error, true))?;
    let (asset, _) =
        crate::services::media::MediaService::from_data_paths(crate::services::data_paths::paths())
            .persist_ready_bytes(
                &db,
                crate::services::media::MediaContext::site(
                    crate::services::media::MediaActor::site_operator(None, true),
                    crate::services::media::MediaSource::Generated,
                ),
                crate::services::media::NewMediaBytes {
                    bytes: png.into(),
                    claimed_mime: "image/png".into(),
                    filename: "sticker.png".into(),
                    max_bytes: MAX_STICKER_UPLOAD_BYTES,
                    derived_from_id: None,
                    exposure: crate::services::media::MediaExposure::Private,
                },
            )
            .await
            .map_err(|error| {
                HttpError(AppError::internal(error.to_string()).with_code("STICKER_STORE_FAILED"))
            })?;

    Ok(Json(GenerateHomeStickerResponse {
        image_url: asset.catalog_url(),
        width,
        height,
    }))
}

/// POST /api/home/stickers/upload — admin only. Store as-is; no cutout.
pub async fn upload_home_sticker(
    crate::extract::Db(db): crate::extract::Db,
    Json(payload): Json<UploadHomeStickerRequest>,
) -> Result<Json<GenerateHomeStickerResponse>, HttpError> {
    let (bytes, media_type) = decode_sticker_upload(&payload.image).map_err(HttpError)?;
    let (bytes, (width, height)) = crate::services::sticker_cutout::inspect_sticker_upload(bytes)
        .await
        .map_err(|error| map_sticker_processing_error(error, false))?;
    let (asset, _) =
        crate::services::media::MediaService::from_data_paths(crate::services::data_paths::paths())
            .persist_ready_bytes(
                &db,
                crate::services::media::MediaContext::site(
                    crate::services::media::MediaActor::site_operator(None, true),
                    crate::services::media::MediaSource::Upload,
                ),
                crate::services::media::NewMediaBytes {
                    bytes: bytes.into(),
                    claimed_mime: media_type.to_string(),
                    filename: "sticker".into(),
                    max_bytes: MAX_STICKER_UPLOAD_BYTES,
                    derived_from_id: None,
                    exposure: crate::services::media::MediaExposure::Private,
                },
            )
            .await
            .map_err(|error| {
                HttpError(AppError::internal(error.to_string()).with_code("STICKER_STORE_FAILED"))
            })?;
    Ok(Json(GenerateHomeStickerResponse {
        image_url: asset.catalog_url(),
        width,
        height,
    }))
}

#[cfg(test)]
mod tests {
    use super::{
        compose_sticker_prompt, cover_crop_instruction, decode_sticker_upload,
        normalize_sticker_prompt, sticker_pixels,
    };
    use base64::Engine;

    #[test]
    fn sticker_pixels_scale_from_cells() {
        assert_eq!(sticker_pixels(1, 1), (256, 256));
        assert_eq!(sticker_pixels(2, 2), (512, 512));
        assert_eq!(sticker_pixels(4, 4), (1024, 1024));
        assert_eq!(sticker_pixels(8, 8), (2048, 2048));
    }

    #[test]
    fn compose_sticker_prompt_states_canvas_slot_and_crop() {
        let out = compose_sticker_prompt(
            "a cat with sunglasses",
            1536,
            1024,
            Some(10),
            Some(6),
            Some("16:9"),
        );
        assert!(out.contains("a cat with sunglasses"));
        assert!(out.contains("1536x1024"));
        assert!(out.contains("3:2"));
        assert!(out.contains("10x6"));
        assert!(out.contains("16:9"));
        assert!(out.contains("top and bottom"));
        let square = compose_sticker_prompt("cat", 1024, 1024, Some(2), Some(2), None);
        assert!(square.contains("no crop") || square.contains("same as this canvas"));
    }

    #[test]
    fn cover_crop_instruction_picks_axis() {
        let wide_slot = cover_crop_instruction(1536, 1024, 4, 3);
        assert!(wide_slot.contains("left and right"));
        let tall_slot = cover_crop_instruction(1536, 1024, 10, 6);
        assert!(tall_slot.contains("top and bottom"));
        let same = cover_crop_instruction(1024, 1024, 2, 2);
        assert!(same.contains("same as this canvas"));
    }

    #[test]
    fn sticker_prompt_rejects_empty_and_too_long() {
        assert!(normalize_sticker_prompt("  ").is_err());
        assert!(normalize_sticker_prompt("cat").is_ok());
        let long = "a".repeat(2001);
        assert!(normalize_sticker_prompt(&long).is_err());
    }

    #[test]
    fn decode_sticker_upload_rejects_non_data_urls() {
        assert!(decode_sticker_upload("").is_err());
        assert!(decode_sticker_upload("/api/phantasi/image-cache/aa/bb.png").is_err());
        assert!(decode_sticker_upload("data:text/plain;base64,YQ==").is_err());
    }

    #[test]
    fn decode_sticker_upload_accepts_png_jpeg_and_webp_data_urls() {
        let png = image::RgbaImage::from_pixel(4, 4, image::Rgba([20, 180, 40, 255]));
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(png)
            .write_to(
                &mut std::io::Cursor::new(&mut bytes),
                image::ImageFormat::Png,
            )
            .unwrap();
        let url = format!("data:image/png;base64,{}", super::BASE64.encode(&bytes));
        let (decoded, media) = decode_sticker_upload(&url).unwrap();
        assert_eq!(decoded, bytes);
        assert_eq!(media, "image/png");

        let jpeg = format!(
            "data:image/jpeg;base64,{}",
            super::BASE64.encode(b"\xff\xd8\xff")
        );
        assert_eq!(decode_sticker_upload(&jpeg).unwrap().1, "image/jpeg");

        let webp = format!(
            "data:image/webp;base64,{}",
            super::BASE64.encode(b"RIFF\x08\0\0\0WEBP")
        );
        assert_eq!(decode_sticker_upload(&webp).unwrap().1, "image/webp");
        assert!(decode_sticker_upload("data:image/gif;base64,R0lG").is_err());

        let mut encoded = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            2,
            2,
            image::Rgba([20, 180, 40, 255]),
        ))
        .write_to(
            &mut std::io::Cursor::new(&mut encoded),
            image::ImageFormat::WebP,
        )
        .unwrap();
        let decoded = image::load_from_memory(&encoded).unwrap();
        assert_eq!(decoded.width(), 2);
        assert_eq!(decoded.height(), 2);
    }

    #[test]
    fn generated_and_uploaded_stickers_persist_as_assets() {
        let src = include_str!("home_stickers.rs");
        let prod = src.split("mod tests").next().expect("prod");
        assert!(prod.contains("persist_ready_bytes"));
        assert!(!prod.contains("media_catalog::register"));
        assert!(!prod.contains("store_bytes_with_status"));
    }
}
