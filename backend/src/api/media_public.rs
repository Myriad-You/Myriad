//! Web-process public media reads. Independent of the federation worker and gate.
//!
//! `/media/assets/{id}` serves ready public assets (and private ones to a
//! reader allowed to see them). The image cache serves its own files, or the
//! asset a cited cache file became.

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
    FileServe, MediaStore, NO_STORE, ServeOutcome, resolve_cached_image, resolve_private_asset,
    resolve_public_asset,
};

pub fn public_media_routes() -> Router<crate::state::AppState> {
    Router::new().route(
        "/media/assets/{public_id}/{filename}",
        get(serve_public_asset).head(serve_public_asset),
    )
}

pub fn image_cache_routes() -> Router<crate::state::AppState> {
    Router::new().route(
        "/image-cache/{subdir}/{file}",
        get(serve_phantasi_cache).head(serve_phantasi_cache),
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
    let outcome = match resolve_public_asset(&db, &store, public_id, &filename).await {
        Ok(outcome @ ServeOutcome::File(_)) => outcome,
        Ok(ServeOutcome::NotFound { .. }) => {
            // Same address for a private asset: only a signed-in reader who may
            // read it gets the bytes, never cacheable. Anonymous requests stop
            // before any session lookup.
            let Some(actor) = reader(&db, req.headers()).await else {
                return hide();
            };
            match resolve_private_asset(&db, &store, public_id, &filename, &actor).await {
                Ok(outcome) => outcome,
                Err(_) => return hide(),
            }
        }
        Err(_) => return hide(),
    };
    send(req, outcome).await
}

async fn reader(
    db: &sea_orm::DatabaseConnection,
    headers: &axum::http::HeaderMap,
) -> Option<crate::services::media::MediaActor> {
    let claims = crate::middleware::auth::authenticate_optional_request(headers, db)
        .await
        .ok()??;
    crate::api::media::actor_from_claims(&claims).ok()
}

async fn serve_phantasi_cache(
    Db(db): Db,
    Path((subdir, file)): Path<(String, String)>,
    req: Request,
) -> Response {
    let local_path = format!("/api/phantasi/image-cache/{subdir}/{file}");
    serve_cached(db, local_path, req).await
}

async fn serve_cached(
    db: sea_orm::DatabaseConnection,
    local_path: String,
    req: Request,
) -> Response {
    let store = MediaStore::new(paths().media.clone());
    match resolve_cached_image(&db, &store, &local_path).await {
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
    // Cached SVG comes from arbitrary feeds and is same-origin: opened as a
    // document it must not run script. Images embedded via <img> are unaffected.
    if file.mime == "image/svg+xml" {
        headers.insert(
            header::CONTENT_SECURITY_POLICY,
            HeaderValue::from_static("default-src 'none'; style-src 'unsafe-inline'; sandbox"),
        );
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
        assert!(src.contains("resolve_cached_image"));
    }
}
