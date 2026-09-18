//! 上传/生成媒体目录。`cache_image` 拉来的外链缓存不进这里。

use chrono::Utc;
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectionTrait, DatabaseConnection, DbErr, EntityTrait,
    QueryFilter, QueryOrder, Set,
};
use serde::Serialize;
use std::path::Path;

use crate::federation::content::federation_media_root;
use crate::models::entities::media_assets;
use crate::services::image_cache::ImageCacheService;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaKind {
    Upload,
    Generated,
}

impl MediaKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Upload => "upload",
            Self::Generated => "generated",
        }
    }
}

#[derive(Clone, Debug)]
pub struct RegisterMedia {
    pub kind: MediaKind,
    pub url: String,
    pub mime: String,
    pub name: String,
    pub size: i64,
}

#[derive(Clone, Debug, Serialize)]
pub struct MediaAssetView {
    pub id: i32,
    pub kind: String,
    pub url: String,
    pub mime: String,
    pub name: String,
    pub size: i64,
    pub created_at: i64,
    pub references: Vec<String>,
}

/// 目录只收本站上传/生成路径。外链和 `cache_image` 结果不进。
pub fn canonical_media_url(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(path) = path_from_url(trimmed) {
        return catalog_path(path);
    }
    catalog_path(trimmed)
}

fn path_from_url(raw: &str) -> Option<&str> {
    let rest = raw.split_once("://")?.1;
    let path = rest.find('/').map(|i| &rest[i..])?;
    Some(path)
}

fn catalog_path(path: &str) -> Option<String> {
    if path.starts_with("/media/federation/") || path.starts_with("/api/phantasi/image-cache/") {
        Some(path.to_string())
    } else {
        None
    }
}

/// `cache_image` 是外链缓存，不进目录。
#[cfg(test)]
pub fn catalogs_cache_image() -> bool {
    false
}

pub fn should_register_store_bytes(created: bool) -> bool {
    created
}

pub async fn register_if_created(
    db: &DatabaseConnection,
    kind: MediaKind,
    url: impl Into<String>,
    mime: impl Into<String>,
    name: impl Into<String>,
    size: i64,
    created: bool,
) {
    if !should_register_store_bytes(created) {
        return;
    }
    let _ = register(
        db,
        RegisterMedia {
            kind,
            url: url.into(),
            mime: mime.into(),
            name: name.into(),
            size,
        },
    )
    .await;
}

pub async fn register(
    db: &DatabaseConnection,
    input: RegisterMedia,
) -> Result<media_assets::Model, DbErr> {
    let url = canonical_media_url(&input.url)
        .ok_or_else(|| DbErr::Custom("media url is not a catalog path".into()))?;
    if let Some(existing) = media_assets::Entity::find()
        .filter(media_assets::Column::Url.eq(&url))
        .one(db)
        .await?
    {
        return Ok(existing);
    }
    let now = Utc::now().fixed_offset();
    let row = media_assets::ActiveModel {
        kind: Set(input.kind.as_str().to_string()),
        url: Set(url),
        mime: Set(input.mime),
        name: Set(input.name),
        size: Set(input.size),
        created_at: Set(now),
        ..Default::default()
    };
    match row.insert(db).await {
        Ok(model) => Ok(model),
        Err(err) if is_unique_violation(&err) => media_assets::Entity::find()
            .filter(
                media_assets::Column::Url.eq(canonical_media_url(&input.url).unwrap_or_default()),
            )
            .one(db)
            .await?
            .ok_or(err),
        Err(err) => Err(err),
    }
}

fn is_unique_violation(err: &DbErr) -> bool {
    err.to_string().contains("duplicate key") || err.to_string().contains("UNIQUE")
}

pub async fn list_assets(db: &DatabaseConnection) -> Result<Vec<MediaAssetView>, DbErr> {
    backfill_federation(db).await?;
    let rows = media_assets::Entity::find()
        .order_by_desc(media_assets::Column::CreatedAt)
        .all(db)
        .await?;
    let mut items = Vec::with_capacity(rows.len());
    for row in rows {
        let references = media_references(db, &row.url).await?;
        items.push(to_view(row, references));
    }
    Ok(items)
}

pub async fn get_asset(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Option<media_assets::Model>, DbErr> {
    media_assets::Entity::find_by_id(id).one(db).await
}

pub async fn delete_asset(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Result<(), Vec<String>>, DbErr> {
    let Some(row) = get_asset(db, id).await? else {
        return Ok(Err(vec!["missing".into()]));
    };
    let refs = media_references(db, &row.url).await?;
    if !refs.is_empty() {
        return Ok(Err(refs));
    }
    remove_file(&row.url).await.map_err(DbErr::Custom)?;
    media_assets::Entity::delete_by_id(id).exec(db).await?;
    Ok(Ok(()))
}

pub(crate) fn fs_remove_result(result: std::io::Result<()>) -> Result<(), String> {
    match result {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.to_string()),
    }
}

async fn remove_file(url: &str) -> Result<(), String> {
    if url.starts_with("/api/phantasi/image-cache/") {
        return ImageCacheService::new().remove_stored_url(url).await;
    }
    if let Some(path) = federation_disk_path(url) {
        return fs_remove_result(tokio::fs::remove_file(path).await);
    }
    Ok(())
}

fn federation_disk_path(url: &str) -> Option<std::path::PathBuf> {
    let path = canonical_media_url(url)?;
    let rest = path.strip_prefix("/media/federation/")?;
    let (user, file) = rest.split_once('/')?;
    if file.contains('/') || file.contains("..") {
        return None;
    }
    if !user.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(federation_media_root().join(user).join(file))
}

pub async fn backfill_federation(db: &DatabaseConnection) -> Result<(), DbErr> {
    let root = federation_media_root();
    let Ok(mut users) = tokio::fs::read_dir(&root).await else {
        return Ok(());
    };
    while let Ok(Some(user_ent)) = users.next_entry().await {
        if !user_ent
            .file_type()
            .await
            .map(|t| t.is_dir())
            .unwrap_or(false)
        {
            continue;
        }
        let user = user_ent.file_name();
        let user = user.to_string_lossy();
        if !user.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(mut files) = tokio::fs::read_dir(user_ent.path()).await else {
            continue;
        };
        while let Ok(Some(file_ent)) = files.next_entry().await {
            if !file_ent
                .file_type()
                .await
                .map(|t| t.is_file())
                .unwrap_or(false)
            {
                continue;
            }
            let name = file_ent.file_name();
            let name = name.to_string_lossy();
            if name.contains('/') || name.contains("..") {
                continue;
            }
            let url = format!("/media/federation/{user}/{name}");
            let meta = file_ent.metadata().await.ok();
            let size = meta.as_ref().map(|m| m.len() as i64).unwrap_or(0);
            let mime = mime_from_name(&name);
            let _ = register(
                db,
                RegisterMedia {
                    kind: MediaKind::Upload,
                    url,
                    mime,
                    name: name.to_string(),
                    size,
                },
            )
            .await;
        }
    }
    Ok(())
}

fn mime_from_name(name: &str) -> String {
    match Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "mp4" => "video/mp4",
        "webm" => "video/webm",
        "mov" => "video/quicktime",
        other => return format!("application/{other}"),
    }
    .to_string()
}

async fn media_references(db: &DatabaseConnection, url: &str) -> Result<Vec<String>, DbErr> {
    let needle = canonical_media_url(url).unwrap_or_else(|| url.to_string());
    let mut refs = Vec::new();
    let notes = db
        .query_all_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            r#"
            SELECT id FROM phantasi_note_docs
            WHERE image LIKE $1 OR content_md LIKE $1
            LIMIT 8
            "#,
            [format!("%{needle}%").into()],
        ))
        .await;
    if let Ok(rows) = notes {
        if !rows.is_empty() {
            refs.push("notes".into());
        }
    }
    let items = db
        .query_all_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            r#"
            SELECT id FROM phantasi_items
            WHERE image LIKE $1 OR content_md LIKE $1 OR content LIKE $1
            LIMIT 8
            "#,
            [format!("%{needle}%").into()],
        ))
        .await;
    if let Ok(rows) = items {
        if !rows.is_empty() {
            refs.push("articles".into());
        }
    }
    let configs = db
        .query_all_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            r#"
            SELECT key FROM configurations
            WHERE value::text LIKE $1
            LIMIT 4
            "#,
            [format!("%{needle}%").into()],
        ))
        .await;
    if let Ok(rows) = configs {
        if !rows.is_empty() {
            refs.push("site".into());
        }
    }
    Ok(refs)
}

fn to_view(row: media_assets::Model, references: Vec<String>) -> MediaAssetView {
    MediaAssetView {
        id: row.id,
        kind: row.kind,
        url: row.url,
        mime: row.mime,
        name: row.name,
        size: row.size,
        created_at: row.created_at.timestamp_millis(),
        references,
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_media_url, catalog_path, catalogs_cache_image};

    #[test]
    fn canonical_url_keeps_hosted_paths() {
        assert_eq!(
            canonical_media_url("https://site.example/media/federation/1/a.jpg"),
            Some("/media/federation/1/a.jpg".into())
        );
        assert_eq!(
            canonical_media_url("/api/phantasi/image-cache/ab/abcdef.png"),
            Some("/api/phantasi/image-cache/ab/abcdef.png".into())
        );
        assert_eq!(canonical_media_url("https://cdn.example/pic.jpg"), None);
        assert_eq!(catalog_path("/tmp/x.png"), None);
    }

    #[test]
    fn cache_image_stays_out_of_catalog() {
        assert!(!catalogs_cache_image());
    }

    #[test]
    fn store_bytes_only_registers_new_writes() {
        assert!(super::should_register_store_bytes(true));
        assert!(!super::should_register_store_bytes(false));
    }

    #[test]
    fn file_delete_error_blocks_catalog_row_delete() {
        assert!(super::fs_remove_result(Ok(())).is_ok());
        assert!(
            super::fs_remove_result(Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "gone"
            )))
            .is_ok()
        );
        let error = super::fs_remove_result(Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "locked",
        )))
        .unwrap_err();
        assert!(error.contains("locked"), "{error}");
    }
}
