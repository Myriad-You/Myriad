//! 管理员媒体目录：列出 / 上传 / 删除 / 公开。工作台上传走平台资产服务。
//! 新上传默认私有；公开阅读走 `/media/assets`，鉴权预览走 `/api/media/{id}/content`。

use axum::{
    Json,
    body::Bytes,
    extract::{Path, Query, Request, State},
    http::{HeaderMap, StatusCode},
    response::Response,
};
use sea_orm::DatabaseConnection;
use serde_json::{Value, json};

use crate::error::HttpError;
use crate::extract::AuthedClaims;
use crate::middleware::auth::{Claims, verify_current_admin_from_headers};
use crate::services::data_paths::paths;
use crate::services::media::{
    MediaActor, MediaContext, MediaExposure, MediaService, MediaSource, MediaStore, NewMediaBytes,
    resolve_authenticated_content,
};
use crate::services::media_catalog::{MediaListQuery, MediaPage, delete_asset, list_assets};
use crate::services::memory_profile::{note_image_limit, note_video_limit};
use myriad_error::AppError;

fn media_http(status: StatusCode, error: impl Into<String>) -> HttpError {
    HttpError(AppError::from_status_u16(status.as_u16(), error.into()))
}

async fn require_admin(headers: &HeaderMap, db: &DatabaseConnection) -> Result<(), HttpError> {
    verify_current_admin_from_headers(headers, db)
        .await
        .map(|_| ())
        .map_err(|(status, body)| HttpError::from((status, body)))
}

fn upload_budget(mime: &str) -> usize {
    if mime.trim().to_ascii_lowercase().starts_with("video/") {
        note_video_limit()
    } else {
        note_image_limit()
    }
}

fn catalog_item(asset: &crate::services::media::MediaAsset) -> Value {
    json!({
        "id": asset.id,
        "public_id": asset.public_id,
        "kind": asset.kind,
        "url": asset.catalog_url(),
        "content_path": asset.content_path,
        "public_path": asset.public_path,
        "mime": asset.mime,
        "name": asset.name,
        "size": asset.size,
        "source": asset.source.as_str(),
        "state": asset.state.as_str(),
        "exposure": asset.exposure.as_str(),
        "created_at": asset.created_at.timestamp_millis(),
        "references": [],
        "references_complete": asset.references_complete,
        "derived_from_id": asset.derived_from_id,
    })
}

/// GET /api/media
pub async fn list_media(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Query(query): Query<MediaListQuery>,
) -> Result<Json<MediaPage>, HttpError> {
    require_admin(&headers, &db).await?;
    if !query.valid() {
        return Err(media_http(StatusCode::BAD_REQUEST, "Invalid media query"));
    }
    let page = list_assets(&db, &query).await.map_err(|err| {
        tracing::error!(%err, "list media catalog");
        media_http(StatusCode::INTERNAL_SERVER_ERROR, "Failed to list media")
    })?;
    Ok(Json(page))
}

/// POST /api/media
pub async fn upload_media(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    AuthedClaims(claims): AuthedClaims,
    multipart: axum::extract::Multipart,
) -> Result<Json<serde_json::Value>, HttpError> {
    require_admin(&headers, &db).await?;
    let user_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| media_http(StatusCode::UNAUTHORIZED, "Invalid user ID"))?;
    let (filename, mime, bytes) = read_file_field(multipart).await?;
    let actor = MediaActor::admin(user_id).map_err(|err| HttpError(err.into()))?;
    let created = MediaService::from_data_paths(paths())
        .create_from_bytes(
            &db,
            MediaContext::site(actor, MediaSource::Upload),
            NewMediaBytes {
                max_bytes: upload_budget(&mime),
                bytes,
                claimed_mime: mime,
                filename,
                derived_from_id: None,
                exposure: MediaExposure::Private,
            },
        )
        .await
        .map_err(|err| HttpError(err.into()))?;
    Ok(Json(json!({
        "success": true,
        "item": catalog_item(&created),
    })))
}

/// DELETE /api/media/{id}
pub async fn delete_media(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<serde_json::Value>, HttpError> {
    require_admin(&headers, &db).await?;
    match delete_asset(&db, id).await.map_err(|err| {
        tracing::error!(%err, "delete media catalog");
        media_http(StatusCode::INTERNAL_SERVER_ERROR, "Failed to delete media")
    })? {
        Ok(()) => Ok(Json(json!({ "success": true }))),
        Err(refs) if refs == ["missing"] => {
            Err(media_http(StatusCode::NOT_FOUND, "Media not found"))
        }
        Err(refs) if refs == ["pending"] => Err(HttpError(
            AppError::from_status_u16(202, "Media deletion is retrying")
                .with_code("MEDIA_NOT_READY"),
        )),
        Err(_refs) => Err(HttpError(
            AppError::conflict("This file is still in use and cannot be deleted.")
                .with_code("MEDIA_IN_USE"),
        )),
    }
}

/// GET/HEAD /api/media/{id}/content
pub async fn serve_media_content(
    State(db): State<DatabaseConnection>,
    AuthedClaims(claims): AuthedClaims,
    Path(id): Path<i32>,
    req: Request,
) -> Response {
    let Ok(actor) = actor_from_claims(&claims) else {
        return crate::api::media_public::send_media_outcome(
            req,
            crate::services::media::ServeOutcome::NotFound { no_store: true },
        )
        .await;
    };
    let store = MediaStore::new(paths().media.clone());
    match resolve_authenticated_content(&db, &store, id, &actor).await {
        Ok(outcome) => crate::api::media_public::send_media_outcome(req, outcome).await,
        Err(_) => {
            crate::api::media_public::send_media_outcome(
                req,
                crate::services::media::ServeOutcome::NotFound { no_store: true },
            )
            .await
        }
    }
}

/// POST /api/media/{id}/publication
pub async fn publish_media(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<Value>, HttpError> {
    require_admin(&headers, &db).await?;
    let asset = MediaService::from_data_paths(paths())
        .publish(&db, id)
        .await
        .map_err(|err| HttpError(err.into()))?;
    Ok(Json(
        json!({ "success": true, "item": catalog_item(&asset) }),
    ))
}

/// DELETE /api/media/{id}/publication
pub async fn unpublish_media(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(id): Path<i32>,
) -> Result<Json<Value>, HttpError> {
    require_admin(&headers, &db).await?;
    let asset = MediaService::from_data_paths(paths())
        .unpublish(&db, id)
        .await
        .map_err(|err| HttpError(err.into()))?;
    Ok(Json(
        json!({ "success": true, "item": catalog_item(&asset) }),
    ))
}

fn actor_from_claims(claims: &Claims) -> Result<MediaActor, HttpError> {
    let user_id: i32 = claims
        .sub
        .parse()
        .map_err(|_| media_http(StatusCode::UNAUTHORIZED, "Invalid user ID"))?;
    if claims.is_admin {
        MediaActor::admin(user_id).or_else(|_| Ok(MediaActor::site_operator(Some(user_id), true)))
    } else {
        MediaActor::user(user_id)
    }
    .map_err(|err| HttpError(err.into()))
}

async fn read_file_field(
    mut multipart: axum::extract::Multipart,
) -> Result<(String, String, Bytes), HttpError> {
    let mut file_bytes = None;
    let mut filename = "upload.bin".to_string();
    let mut mime = "application/octet-stream".to_string();
    while let Ok(Some(field)) = multipart.next_field().await {
        if field.name().unwrap_or("") != "file" {
            continue;
        }
        if let Some(name) = field.file_name() {
            filename = name.to_string();
        }
        if let Some(ct) = field.content_type() {
            mime = ct.to_string();
        }
        file_bytes = Some(
            field
                .bytes()
                .await
                .map_err(|_| media_http(StatusCode::BAD_REQUEST, "Failed to read file field"))?,
        );
        break;
    }
    let bytes = file_bytes
        .ok_or_else(|| media_http(StatusCode::BAD_REQUEST, "Missing multipart field 'file'"))?;
    Ok((filename, mime, bytes))
}

#[cfg(test)]
mod tests {
    use crate::services::media_catalog::catalogs_cache_image;

    #[test]
    fn catalog_api_does_not_index_cache_image() {
        assert!(!catalogs_cache_image());
    }

    #[test]
    fn journal_upload_does_not_call_federation_store() {
        let src = include_str!("media.rs");
        assert!(!src.contains(concat!("store_federation", "_media")));
        assert!(src.contains("MediaService"));
    }

    #[test]
    fn new_uploads_default_private() {
        let src = include_str!("media.rs");
        let upload = src
            .split("pub async fn upload_media")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn delete_media").next())
            .expect("upload_media");
        assert!(upload.contains("MediaExposure::Private"));
        assert!(!upload.contains("MediaExposure::Public"));
    }
}

/// GET /api/media/migration — persisted cursor and last redacted failure.
pub async fn media_migration_status(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
) -> Result<Json<Value>, HttpError> {
    require_admin(&headers, &db).await?;
    let progress = crate::services::media::upgrade::status(&db)
        .await
        .map_err(|error| HttpError(error.into()))?;
    Ok(Json(json!({"success": true, "progress": progress})))
}

#[derive(serde::Deserialize)]
pub struct MediaMigrationRequest {
    #[serde(default)]
    pub restart: bool,
}

/// POST /api/media/migration — one bounded batch; repeat until complete.
pub async fn advance_media_migration(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Json(request): Json<MediaMigrationRequest>,
) -> Result<Json<Value>, HttpError> {
    require_admin(&headers, &db).await?;
    let origins = crate::services::media::upgrade::configured_origins().await;
    let store = MediaStore::new(paths().media.clone());
    let legacy = crate::services::media::LegacyPaths::from_data_paths(paths());
    let progress =
        crate::services::media::upgrade::advance(&db, &store, &legacy, &origins, request.restart)
            .await
            .map_err(|error| HttpError(error.into()))?;
    Ok(Json(
        json!({"success": progress.error.is_none(), "progress": progress}),
    ))
}
