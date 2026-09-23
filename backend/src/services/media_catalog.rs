//! 媒体目录查询。常规写入走 `services::media`，这里不再登记新文件。

use sea_orm::{
    ColumnTrait, Condition, ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr,
    EntityTrait, PaginatorTrait, QueryFilter, QueryOrder, QuerySelect, Statement,
};
use serde::{Deserialize, Serialize};

use crate::federation::content::federation_media_root;
use crate::models::entities::media_assets;
use crate::services::image_cache::ImageCacheService;

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
    pub state: Option<String>,
    pub exposure: Option<String>,
    pub content_path: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub public_path: Option<String>,
    pub references_complete: bool,
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

#[derive(Debug, Default, Deserialize)]
pub struct MediaListQuery {
    pub kind: Option<String>,
    pub format: Option<String>,
    pub query: Option<String>,
    pub before_created_at: Option<chrono::DateTime<chrono::FixedOffset>>,
    pub before_id: Option<i32>,
    pub limit: Option<u64>,
}

impl MediaListQuery {
    pub fn valid(&self) -> bool {
        self.before_id.is_some() == self.before_created_at.is_some()
            && self
                .kind
                .as_deref()
                .is_none_or(|v| matches!(v, "all" | "upload" | "generated"))
            && self.format.as_deref().is_none_or(|v| {
                matches!(
                    v,
                    "all" | "jpeg" | "png" | "gif" | "webp" | "mp4" | "webm" | "mov" | "other"
                )
            })
    }
}

#[derive(Serialize)]
pub struct MediaCursor {
    pub created_at: chrono::DateTime<chrono::FixedOffset>,
    pub id: i32,
}

#[derive(Serialize)]
pub struct MediaPage {
    pub items: Vec<MediaAssetView>,
    pub next_cursor: Option<MediaCursor>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total: Option<u64>,
}

#[derive(sea_orm::FromQueryResult)]
struct CatalogAsset {
    id: i32,
    kind: String,
    url: String,
    mime: String,
    name: String,
    size: i64,
    created_at: chrono::DateTime<chrono::FixedOffset>,
    state: Option<String>,
    exposure: Option<String>,
    references_complete: bool,
}

// Matches the workbench's supported MIME types; legacy filenames supply missing MIME formats.
const FORMAT_SQL: &str = "COALESCE(CASE split_part(lower(btrim(mime)), ';', 1)
    WHEN 'image/jpeg' THEN 'jpeg' WHEN 'image/jpg' THEN 'jpeg' WHEN 'image/pjpeg' THEN 'jpeg'
    WHEN 'image/png' THEN 'png' WHEN 'image/gif' THEN 'gif' WHEN 'image/webp' THEN 'webp'
    WHEN 'video/mp4' THEN 'mp4' WHEN 'video/webm' THEN 'webm' WHEN 'video/quicktime' THEN 'mov' END,
    CASE regexp_replace(lower(btrim(name)), '^.*\\.', '')
    WHEN 'jpg' THEN 'jpeg' WHEN 'jpeg' THEN 'jpeg' WHEN 'png' THEN 'png' WHEN 'gif' THEN 'gif'
    WHEN 'webp' THEN 'webp' WHEN 'mp4' THEN 'mp4' WHEN 'webm' THEN 'webm' WHEN 'mov' THEN 'mov' ELSE 'other' END)";

pub async fn list_assets(
    db: &DatabaseConnection,
    params: &MediaListQuery,
) -> Result<MediaPage, DbErr> {
    use media_assets::Column as C;
    use sea_orm::sea_query::Expr;
    let mut query = media_assets::Entity::find().filter(
        Condition::any()
            .add(C::State.is_null())
            .add(C::State.is_not_in(["deleted", "deleting", "staging"])),
    );
    if let Some(kind) = params.kind.as_deref().filter(|v| *v != "all") {
        query = query.filter(C::Kind.eq(kind));
    }
    if let Some(format) = params.format.as_deref().filter(|v| *v != "all") {
        query = query.filter(Expr::cust_with_values(
            format!("{FORMAT_SQL} = $1"),
            [format],
        ));
    }
    if let Some(needle) = params
        .query
        .as_deref()
        .map(str::trim)
        .filter(|v| !v.is_empty())
    {
        query = query.filter(Expr::cust_with_values(
            "strpos(lower(name || chr(10) || mime || chr(10) || kind), $1) > 0",
            [needle.to_lowercase()],
        ));
    }
    let total = if params.before_id.is_none() {
        Some(query.clone().count(db).await?)
    } else {
        None
    };
    if let (Some(created), Some(id)) = (params.before_created_at, params.before_id) {
        query = query.filter(Expr::cust_with_values(
            "(created_at, id) < ($1, $2)",
            [sea_orm::Value::from(created), sea_orm::Value::from(id)],
        ));
    }
    let limit = params.limit.unwrap_or(48).clamp(1, 100) as usize;
    let mut rows = query
        .select_only()
        .columns([
            C::Id,
            C::Kind,
            C::Url,
            C::Mime,
            C::Name,
            C::Size,
            C::CreatedAt,
            C::State,
            C::Exposure,
            C::ReferencesComplete,
        ])
        .order_by_desc(C::CreatedAt)
        .order_by_desc(C::Id)
        .limit((limit + 1) as u64)
        .into_model::<CatalogAsset>()
        .all(db)
        .await?;
    let more = rows.len() > limit;
    rows.truncate(limit);
    let next_cursor = rows.last().filter(|_| more).map(|row| MediaCursor {
        created_at: row.created_at,
        id: row.id,
    });
    let ids: Vec<i32> = rows.iter().map(|row| row.id).collect();
    let mut references = crate::services::media::catalog_labels_for_assets(db, &ids)
        .await
        .map_err(|error| DbErr::Custom(error.to_string()))?;
    let items = rows
        .into_iter()
        .map(|row| {
            let refs = references.remove(&row.id).unwrap_or_default();
            to_view(row, refs)
        })
        .collect();
    Ok(MediaPage {
        items,
        next_cursor,
        total,
    })
}

pub async fn delete_asset(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Result<(), Vec<String>>, DbErr> {
    match crate::services::media::MediaService::from_data_paths(
        crate::services::data_paths::paths(),
    )
    .delete(db, id)
    .await
    {
        Ok(crate::services::media::DeleteOutcome::Deleted) => Ok(Ok(())),
        Ok(crate::services::media::DeleteOutcome::PendingRetry) => {
            Ok(Err(vec!["pending".into()]))
        }
        Err(crate::services::media::MediaError::Missing) => Ok(Err(vec!["missing".into()])),
        Err(crate::services::media::MediaError::InUse)
        | Err(crate::services::media::MediaError::PublicInUse) => {
            Ok(Err(vec!["in_use".into()]))
        }
        Err(crate::services::media::MediaError::Invalid { .. }) => {
            delete_unmigrated_asset(db, id).await
        }
        Err(error) => Err(DbErr::Custom(error.to_string())),
    }
}

/// Legacy (pre-migration, `state IS NULL`) delete.
///
/// Failure protocol: one SELECT decides eligibility (row, legacy state and
/// every reference kind); any query or decode failure aborts before touching
/// disk. The file goes first (NotFound counts as removed), then the row. If the
/// row delete fails after the file is gone, the error surfaces and a retry of
/// the same id converges: the file is NotFound and the row is deleted.
async fn delete_unmigrated_asset(
    db: &DatabaseConnection,
    id: i32,
) -> Result<Result<(), Vec<String>>, DbErr> {
    let Some(row) = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
            SELECT a.url, a.state IS NULL AS unmigrated,
                   EXISTS (
                       SELECT 1 FROM phantasi_note_docs
                       WHERE image LIKE p.pat ESCAPE '\' OR content_md LIKE p.pat ESCAPE '\'
                   ) AS notes,
                   EXISTS (
                       SELECT 1 FROM phantasi_items
                       WHERE image LIKE p.pat ESCAPE '\'
                          OR content_md LIKE p.pat ESCAPE '\'
                          OR content LIKE p.pat ESCAPE '\'
                   ) AS articles,
                   EXISTS (
                       SELECT 1 FROM configurations WHERE value::text LIKE p.pat ESCAPE '\'
                   ) AS site
            FROM media_assets a
            CROSS JOIN LATERAL (
                SELECT '%' || replace(replace(replace(a.url, '\', '\\'), '%', '\%'), '_', '\_')
                       || '%' AS pat
            ) p
            WHERE a.id = $1
            "#,
            [id.into()],
        ))
        .await?
    else {
        return Ok(Err(vec!["missing".into()]));
    };
    let url: String = row.try_get("", "url")?;
    if !row.try_get::<bool>("", "unmigrated")? {
        // Reached only through `MediaError::Invalid`; a migrated row with a
        // corrupt state must not fall back to the legacy delete.
        return Err(DbErr::Custom(format!(
            "media asset {id} has an unrecognised lifecycle state"
        )));
    }
    let mut refs = Vec::new();
    for kind in ["notes", "articles", "site"] {
        if row.try_get::<bool>("", kind)? {
            refs.push(kind.to_string());
        }
    }
    if !refs.is_empty() {
        return Ok(Err(refs));
    }
    remove_file(&url).await.map_err(DbErr::Custom)?;
    media_assets::Entity::delete_many()
        .filter(media_assets::Column::Id.eq(id))
        .filter(media_assets::Column::State.is_null())
        .exec(db)
        .await?;
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

fn to_view(row: CatalogAsset, references: Vec<String>) -> MediaAssetView {
    let content_path = crate::services::media::content_path(row.id);
    let public_path = (row.exposure.as_deref() == Some("public")).then(|| row.url.clone());
    MediaAssetView {
        id: row.id,
        kind: row.kind,
        url: row.url,
        mime: row.mime,
        name: row.name,
        size: row.size,
        created_at: row.created_at.timestamp_millis(),
        references,
        state: row.state,
        exposure: row.exposure,
        content_path,
        public_path,
        references_complete: row.references_complete,
    }
}

#[cfg(test)]
mod tests {
    use super::{canonical_media_url, catalog_path, catalogs_cache_image};

    #[test]
    fn cursor_requires_both_parts_and_filters_match_supported_values() {
        use super::MediaListQuery;
        assert!(MediaListQuery::default().valid());
        assert!(
            !MediaListQuery {
                before_id: Some(1),
                ..Default::default()
            }
            .valid()
        );
        assert!(
            !MediaListQuery {
                before_created_at: Some(chrono::Utc::now().fixed_offset()),
                ..Default::default()
            }
            .valid()
        );
        assert!(
            !MediaListQuery {
                kind: Some("invalid".into()),
                ..Default::default()
            }
            .valid()
        );
        assert!(
            !MediaListQuery {
                format: Some("invalid".into()),
                ..Default::default()
            }
            .valid()
        );
    }

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
    fn list_does_not_scan_disk_and_does_not_register() {
        let src = include_str!("media_catalog.rs");
        let list = src
            .split("pub async fn list_assets")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn delete_asset").next())
            .expect("list_assets");
        assert!(!list.contains("read_dir"));
        assert!(list.contains("catalog_labels_for_assets"));
        assert!(!src.contains(concat!("backfill", "_federation")));
        assert!(!src.contains(concat!("pub async fn ", "register(")));
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

    #[tokio::test]
    async fn legacy_delete_checks_references_in_one_select_and_converges() {
        use sea_orm::ConnectionTrait;
        let Ok(url) = std::env::var("MYRIAD_MEDIA_TEST_DATABASE_URL") else {
            return;
        };
        let isolated = crate::db::IsolatedSchema::migrated(&url, "media_catalog_test").await;
        let db = isolated.db.clone();
        let exec = |sql: String| {
            let db = db.clone();
            async move { db.execute_unprepared(&sql).await.unwrap() }
        };
        let legacy_url = "/media/federation/987654/legacy_a%b.png";
        exec(format!(
            "INSERT INTO media_assets (id, kind, url, mime) VALUES \
             (1, 'upload', '{legacy_url}', 'image/png'), \
             (2, 'upload', '/media/federation/987654/other.png', 'image/png')"
        ))
        .await;
        exec(format!(
            "INSERT INTO configurations (key, value) VALUES \
             ('media_catalog_test', to_jsonb('see https://x.example{legacy_url}'::text))"
        ))
        .await;

        // Referenced by site config (URL with LIKE metacharacters matches literally).
        assert_eq!(
            super::delete_asset(&db, 1).await.unwrap(),
            Err(vec!["site".to_string()])
        );
        exec("DELETE FROM configurations WHERE key = 'media_catalog_test'".into()).await;
        assert_eq!(super::delete_asset(&db, 1).await.unwrap(), Ok(()));
        assert_eq!(
            super::delete_asset(&db, 1).await.unwrap(),
            Err(vec!["missing".to_string()])
        );

        // A reference-table failure aborts the delete instead of reading as "unused".
        exec("ALTER TABLE phantasi_items RENAME TO phantasi_items_off".into()).await;
        assert!(super::delete_asset(&db, 2).await.is_err());
        exec("ALTER TABLE phantasi_items_off RENAME TO phantasi_items".into()).await;
        let remaining = db
            .query_one_raw(sea_orm::Statement::from_string(
                sea_orm::DatabaseBackend::Postgres,
                "SELECT count(*)::int AS n FROM media_assets WHERE id = 2",
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get::<i32>("", "n")
            .unwrap();
        assert_eq!(remaining, 1);
        isolated.drop().await;
    }
}
