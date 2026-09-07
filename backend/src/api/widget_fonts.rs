//! Optional custom font for the HoYoverse game-presence widget.
//!
//! Admin upload only. Files live under `DATA_DIR/site/widget-fonts/` (not the
//! frontend public tree). GET is unauthenticated so the public homepage can load
//! the face. Filenames are content-addressed; path traversal is rejected.

use axum::extract::Path;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;

use crate::error::HttpError;
use crate::services::data_paths::paths;
use myriad_error::AppError;

const MAX_FONT_BYTES: usize = 2 * 1024 * 1024;
const FONT_URL_PREFIX: &str = "/api/home/widget-fonts/";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FontKind {
    pub ext: &'static str,
    pub mime: &'static str,
}

const WOFF2: FontKind = FontKind {
    ext: "woff2",
    mime: "font/woff2",
};
const WOFF: FontKind = FontKind {
    ext: "woff",
    mime: "font/woff",
};
const TTF: FontKind = FontKind {
    ext: "ttf",
    mime: "font/ttf",
};
const OTF: FontKind = FontKind {
    ext: "otf",
    mime: "font/otf",
};

#[derive(Debug, Deserialize)]
pub struct UploadWidgetFontRequest {
    pub font: String,
}

fn invalid_font(message: &str) -> AppError {
    AppError::bad_request(message).with_code("INVALID_WIDGET_FONT")
}

pub fn detect_font(bytes: &[u8]) -> Option<FontKind> {
    if bytes.len() < 4 {
        return None;
    }
    match &bytes[..4] {
        b"wOF2" => Some(WOFF2),
        b"wOFF" => Some(WOFF),
        b"OTTO" => Some(OTF),
        b"\x00\x01\x00\x00" | b"true" => Some(TTF),
        _ => None,
    }
}

pub fn parse_stored_font_name(file: &str) -> Option<FontKind> {
    let (stem, ext) = file.rsplit_once('.')?;
    if stem.len() != 64 || !stem.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    match ext {
        "woff2" => Some(WOFF2),
        "woff" => Some(WOFF),
        "ttf" => Some(TTF),
        "otf" => Some(OTF),
        _ => None,
    }
}

pub fn stored_font_filename(bytes: &[u8], kind: FontKind) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    format!("{}.{}", hex::encode(hasher.finalize()), kind.ext)
}

pub fn decode_font_data_url(source: &str) -> Result<(Vec<u8>, FontKind), AppError> {
    let (metadata, encoded) = source
        .trim()
        .strip_prefix("data:")
        .and_then(|data| data.split_once(','))
        .ok_or_else(|| invalid_font("font must be a data URL"))?;
    if !metadata.ends_with(";base64") {
        return Err(invalid_font("font must be a base64 data URL"));
    }
    if encoded.len() > MAX_FONT_BYTES.div_ceil(3) * 4 {
        return Err(invalid_font("font is too large"));
    }
    let bytes = BASE64
        .decode(encoded.as_bytes())
        .map_err(|_| invalid_font("font base64 is invalid"))?;
    if bytes.is_empty() || bytes.len() > MAX_FONT_BYTES {
        return Err(invalid_font("font is too large"));
    }
    let kind = detect_font(&bytes).ok_or_else(|| invalid_font("file is not a web font"))?;
    Ok((bytes, kind))
}

async fn persist_font(bytes: &[u8], kind: FontKind) -> Result<String, AppError> {
    let dir = &paths().widget_fonts;
    fs::create_dir_all(dir).await.map_err(|error| {
        tracing::error!(%error, path = %dir.display(), "widget font dir");
        AppError::internal("could not store font").with_code("WIDGET_FONT_STORE_FAILED")
    })?;
    let filename = stored_font_filename(bytes, kind);
    let dest = dir.join(&filename);
    let url = format!("{FONT_URL_PREFIX}{filename}");
    if dest.exists() {
        return Ok(url);
    }
    let tmp = dir.join(format!(".{filename}.{}.tmp", Uuid::new_v4().simple()));
    let write_err = |error: std::io::Error| {
        tracing::error!(%error, path = %tmp.display(), "widget font write");
        AppError::internal("could not store font").with_code("WIDGET_FONT_STORE_FAILED")
    };
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&tmp)
        .await
        .map_err(write_err)?;
    if let Err(error) = file.write_all(bytes).await {
        drop(file);
        let _ = fs::remove_file(&tmp).await;
        return Err(write_err(error));
    }
    if let Err(error) = file.flush().await {
        drop(file);
        let _ = fs::remove_file(&tmp).await;
        return Err(write_err(error));
    }
    drop(file);
    match fs::rename(&tmp, &dest).await {
        Ok(()) => Ok(url),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            let _ = fs::remove_file(&tmp).await;
            Ok(url)
        }
        Err(error) => {
            let _ = fs::remove_file(&tmp).await;
            Err(write_err(error))
        }
    }
}

/// POST /api/home/widget-fonts — admin only.
pub async fn upload_widget_font(
    Json(payload): Json<UploadWidgetFontRequest>,
) -> Result<Json<serde_json::Value>, HttpError> {
    let (bytes, kind) = decode_font_data_url(&payload.font).map_err(HttpError)?;
    let url = persist_font(&bytes, kind).await.map_err(HttpError)?;
    Ok(Json(json!({ "url": url })))
}

/// GET /api/home/widget-fonts/{file} — public (homepage card).
pub async fn get_widget_font(Path(file): Path<String>) -> Result<Response, HttpError> {
    let kind = parse_stored_font_name(&file).ok_or_else(|| {
        HttpError(AppError::not_found("font not found").with_code("WIDGET_FONT_NOT_FOUND"))
    })?;
    let path = paths().widget_fonts.join(&file);
    let bytes = fs::read(&path).await.map_err(|_| {
        HttpError(AppError::not_found("font not found").with_code("WIDGET_FONT_NOT_FOUND"))
    })?;
    let mut response = bytes.into_response();
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(kind.mime));
    headers.insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=604800, immutable"),
    );
    *response.status_mut() = StatusCode::OK;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data_url(bytes: &[u8]) -> String {
        format!(
            "data:application/octet-stream;base64,{}",
            BASE64.encode(bytes)
        )
    }

    #[test]
    fn detect_known_sfnt_headers() {
        assert_eq!(detect_font(b"wOF2xxxx").unwrap().ext, "woff2");
        assert_eq!(detect_font(b"wOFFxxxx").unwrap().ext, "woff");
        assert_eq!(detect_font(b"OTTOxxxx").unwrap().ext, "otf");
        assert_eq!(detect_font(b"\x00\x01\x00\x00rest").unwrap().ext, "ttf");
        assert_eq!(detect_font(b"truexxxx").unwrap().ext, "ttf");
        assert!(detect_font(b"PNG").is_none());
        assert!(detect_font(b"").is_none());
    }

    #[test]
    fn stored_name_is_sha256_and_ext() {
        let bytes = b"wOF2fixture-bytes";
        let name = stored_font_filename(bytes, detect_font(bytes).unwrap());
        assert!(name.ends_with(".woff2"));
        assert_eq!(name.len(), 64 + 6);
        assert!(parse_stored_font_name(&name).is_some());
        assert!(parse_stored_font_name("../etc/passwd").is_none());
        assert!(parse_stored_font_name("abc.woff2").is_none());
        assert!(parse_stored_font_name(&format!("{}.exe", "a".repeat(64))).is_none());
    }

    #[test]
    fn decode_rejects_non_fonts_and_accepts_magic() {
        assert!(decode_font_data_url("").is_err());
        assert!(decode_font_data_url("/api/home/widget-fonts/aa.woff2").is_err());
        assert!(decode_font_data_url(&data_url(b"not-a-font-file!!!!")).is_err());
        let (bytes, kind) = decode_font_data_url(&data_url(b"wOF2hello")).unwrap();
        assert_eq!(bytes, b"wOF2hello");
        assert_eq!(kind.ext, "woff2");
    }
}
