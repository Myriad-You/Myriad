//! Web-process public media reads. Independent of the federation worker and gate.
//!
//! Privacy stage is not done: unmigrated historical federation/cache files stay
//! publicly readable. New `/media/assets/{id}` only serves ready+public assets.
//! A deleted/private alias never falls back to the old disk.

use axum::{
    Router,
    body::Body,
    extract::{Path, Request},
    http::{HeaderValue, StatusCode, header},
    response::{IntoResponse, Response},
    routing::get,
};
use tower::ServiceExt;
use tower_http::services::ServeFile;
use uuid::Uuid;

use crate::extract::Db;
use crate::services::data_paths::paths;
use crate::services::media::{
    FileServe, LegacyPaths, MediaStore, NO_STORE, ServeOutcome, resolve_alias_or_legacy,
    resolve_public_asset,
};

/// `/api/media/...` mirrors the canonical paths for the SPA's display reads.
/// The proxy only updates on explicit request, and before 0.5.2 it sent
/// `/media/assets` to the SPA and `/media/federation` to the federation worker
/// (absent where the egress gate is closed); every proxy forwards `/api/`.
/// Stored and published URLs stay canonical.
pub fn public_media_routes() -> Router<crate::state::AppState> {
    Router::new()
        .route(
            "/media/assets/{public_id}/{filename}",
            get(serve_public_asset).head(serve_public_asset),
        )
        .route(
            "/api/media/assets/{public_id}/{filename}",
            get(serve_public_asset).head(serve_public_asset),
        )
        .route(
            "/media/federation/{user}/{file}",
            get(serve_federation_media).head(serve_federation_media),
        )
        .route(
            "/api/media/federation/{user}/{file}",
            get(serve_federation_media).head(serve_federation_media),
        )
}

pub fn image_cache_routes() -> Router<crate::state::AppState> {
    Router::new().route(
        "/image-cache/{subdir}/{file}",
        get(serve_phantasi_cache).head(serve_phantasi_cache),
    )
}

pub fn brew_image_cache_routes() -> Router<crate::state::AppState> {
    Router::new().route(
        "/image-cache/{subdir}/{file}",
        get(serve_brew_cache).head(serve_brew_cache),
    )
}

async fn serve_public_asset(
    Db(db): Db,
    Path((public_id, filename)): Path<(String, String)>,
    req: Request,
) -> Response {
    let Ok(public_id) = Uuid::parse_str(&public_id) else {
        return hide();
    };
    let store = MediaStore::new(paths().media.clone());
    match resolve_public_asset(&db, &store, public_id, &filename).await {
        Ok(outcome) => send(req, outcome).await,
        Err(_) => hide(),
    }
}

async fn serve_federation_media(
    Db(db): Db,
    Path((user, file)): Path<(String, String)>,
    req: Request,
) -> Response {
    let local_path = format!("/media/federation/{user}/{file}");
    serve_alias(db, local_path, true, req).await
}

async fn serve_phantasi_cache(
    Db(db): Db,
    Path((subdir, file)): Path<(String, String)>,
    req: Request,
) -> Response {
    let local_path = format!("/api/phantasi/image-cache/{subdir}/{file}");
    serve_alias(db, local_path, true, req).await
}

async fn serve_brew_cache(
    Db(db): Db,
    Path((subdir, file)): Path<(String, String)>,
    req: Request,
) -> Response {
    let local_path = format!("/api/brew/image-cache/{subdir}/{file}");
    serve_alias(db, local_path, true, req).await
}

async fn serve_alias(
    db: sea_orm::DatabaseConnection,
    local_path: String,
    allow_unmigrated: bool,
    req: Request,
) -> Response {
    let data = paths();
    let store = MediaStore::new(data.media.clone());
    let legacy = LegacyPaths::from_data_paths(data);
    match resolve_alias_or_legacy(&db, &store, &legacy, &local_path, allow_unmigrated).await {
        Ok(outcome) => send(req, outcome).await,
        Err(_) => hide(),
    }
}

pub(crate) async fn send_media_outcome(req: Request, outcome: ServeOutcome) -> Response {
    send(req, outcome).await
}

async fn send(req: Request, outcome: ServeOutcome) -> Response {
    match outcome {
        ServeOutcome::NotFound { no_store } => {
            if no_store {
                hide()
            } else {
                StatusCode::NOT_FOUND.into_response()
            }
        }
        ServeOutcome::File(file) => send_file(req, file).await,
    }
}

async fn send_file(req: Request, file: FileServe) -> Response {
    if !matches!(
        req.method(),
        &axum::http::Method::GET | &axum::http::Method::HEAD
    ) {
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    if let Some(etag) = file.etag.as_deref() {
        if if_none_match(req.headers().get(header::IF_NONE_MATCH), etag) {
            let mut res = StatusCode::NOT_MODIFIED.into_response();
            apply_headers(&mut res, &file);
            return res;
        }
    }
    if tokio::fs::metadata(&file.path).await.is_err() {
        return hide();
    }
    match ServeFile::new(&file.path).oneshot(req).await {
        Ok(response) => {
            let mut response = response.map(Body::new);
            apply_headers(&mut response, &file);
            response
        }
        Err(_) => hide(),
    }
}

fn apply_headers(res: &mut Response, file: &FileServe) {
    let headers = res.headers_mut();
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    if let Ok(value) = HeaderValue::from_str(file.cache_control) {
        headers.insert(header::CACHE_CONTROL, value);
    }
    if let Ok(value) = HeaderValue::from_str(&file.mime) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    if let Some(etag) = file.etag.as_deref() {
        if let Ok(value) = HeaderValue::from_str(&format!("\"{etag}\"")) {
            headers.insert(header::ETAG, value);
        }
    }
}

fn if_none_match(header: Option<&header::HeaderValue>, etag: &str) -> bool {
    let Some(raw) = header.and_then(|value| value.to_str().ok()) else {
        return false;
    };
    raw.split(',')
        .map(str::trim)
        .any(|candidate| candidate == "*" || candidate.trim_matches('"') == etag)
}

fn hide() -> Response {
    let mut res = StatusCode::NOT_FOUND.into_response();
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(NO_STORE));
    res
}

#[cfg(test)]
mod tests {
    #[test]
    fn public_reads_do_not_use_federation_storage() {
        let src = include_str!("media_public.rs");
        assert!(!src.contains(concat!("store_federation", "_media")));
        assert!(!src.contains(concat!("use crate", "::", "federation")));
        assert!(src.contains("resolve_alias_or_legacy"));
    }
}
