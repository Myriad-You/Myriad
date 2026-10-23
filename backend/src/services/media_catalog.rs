//! 媒体目录查询。常规写入走 `services::media`，这里不再登记新文件。

use sea_orm::{
    ColumnTrait, Condition, DatabaseConnection, DbErr, EntityTrait, PaginatorTrait, QueryFilter,
    QueryOrder, QuerySelect,
};
use serde::{Deserialize, Serialize};

use crate::models::entities::media_assets;

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
    /// Stored `media_assets.source`; rows from before the column read as `legacy`.
    pub source: String,
}

/// Where a catalog asset came from, as the studio groups it. Each origin is a
/// fixed set of `media_assets.source` values; `kind` stays for old callers.
pub const MEDIA_ORIGINS: [&str; 5] = ["upload", "generated", "kept", "channel", "import"];

pub const MEDIA_FORMATS: [&str; 8] = [
    "jpeg", "png", "gif", "webp", "mp4", "webm", "mov", "other",
];

fn origin_sources(origin: &str) -> Option<&'static [&'static str]> {
    match origin {
        "upload" => Some(&["upload"]),
        "generated" => Some(&["generated"]),
        "kept" => Some(&["kept"]),
        "channel" => Some(&["channel"]),
        "import" => Some(&["import", "legacy"]),
        _ => None,
    }
}

#[derive(Debug, Default, Deserialize)]
pub struct MediaListQuery {
    pub kind: Option<String>,
    pub origin: Option<String>,
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
            && self
                .origin
                .as_deref()
                .is_none_or(|v| v == "all" || origin_sources(v).is_some())
            && self
                .format
                .as_deref()
                .is_none_or(|v| v == "all" || MEDIA_FORMATS.contains(&v))
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub origins: Option<Vec<String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub formats: Option<Vec<String>>,
}

#[derive(Clone, Debug, PartialEq, Eq, sea_orm::FromQueryResult)]
struct FacetPair {
    source: String,
    format: String,
}

fn facets_from_pairs(
    pairs: &[FacetPair],
    origin_filter: Option<&str>,
    kind_filter: Option<&str>,
) -> (Vec<String>, Vec<String>) {
    let origins = MEDIA_ORIGINS
        .iter()
        .filter(|origin| {
            origin_sources(origin)
                .is_some_and(|sources| pairs.iter().any(|p| sources.contains(&p.source.as_str())))
        })
        .map(|origin| (*origin).to_string())
        .collect();

    let allowed_sources = origin_filter
        .filter(|v| *v != "all")
        .and_then(origin_sources);
    let kind_filter = kind_filter.filter(|v| *v != "all");

    let formats = MEDIA_FORMATS
        .iter()
        .filter(|format| {
            pairs.iter().any(|pair| {
                pair.format == **format
                    && allowed_sources.is_none_or(|sources| sources.contains(&pair.source.as_str()))
                    && kind_filter.is_none_or(|kind| {
                        let pair_kind = if pair.source == "generated" {
                            "generated"
                        } else {
                            "upload"
                        };
                        pair_kind == kind
                    })
            })
        })
        .map(|format| (*format).to_string())
        .collect();

    (origins, formats)
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
    source: Option<String>,
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
    let mut query =
        media_assets::Entity::find().filter(C::State.is_not_in(["deleted", "deleting", "staging"]));
    // Workbench catalog is the note/blog image+video library. Local-music audio
    // and covers are managed only in the music admin UI — never list them here.
    query = query.filter(
        Condition::all()
            .add(Expr::cust(
                "COALESCE(lower(btrim(mime)), '') NOT LIKE 'audio/%'",
            ))
            .add(Expr::cust(
                "id NOT IN (\
                 SELECT audio_media_id FROM local_music_tracks \
                 UNION ALL \
                 SELECT cover_media_id FROM local_music_tracks WHERE cover_media_id IS NOT NULL\
                 )",
            )),
    );
    let base_query = query.clone();
    if let Some(kind) = params.kind.as_deref().filter(|v| *v != "all") {
        query = query.filter(C::Kind.eq(kind));
    }
    if let Some(sources) = params.origin.as_deref().and_then(origin_sources) {
        let mut origin = Condition::any().add(C::Source.is_in(sources.iter().copied()));
        if sources.contains(&"legacy") {
            origin = origin.add(C::Source.is_null());
        }
        query = query.filter(origin);
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
    let (total, origins, formats) = if params.before_id.is_none() {
        let total = query.clone().count(db).await?;
        let pairs = base_query
            .select_only()
            .column_as(Expr::cust("COALESCE(source, 'legacy')"), "source")
            .column_as(Expr::cust(FORMAT_SQL), "format")
            .distinct()
            .into_model::<FacetPair>()
            .all(db)
            .await?;
        let (origins, formats) = facets_from_pairs(
            &pairs,
            params.origin.as_deref(),
            params.kind.as_deref(),
        );
        (Some(total), Some(origins), Some(formats))
    } else {
        (None, None, None)
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
            C::Source,
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
        origins,
        formats,
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
        // Still being written: retry later instead of reporting a server error.
        Err(crate::services::media::MediaError::NotReady) => Ok(Err(vec!["pending".into()])),
        Err(crate::services::media::MediaError::InUse)
        | Err(crate::services::media::MediaError::PublicInUse) => {
            Ok(Err(vec!["in_use".into()]))
        }
        Err(error) => Err(DbErr::Custom(error.to_string())),
    }
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
        source: row.source.unwrap_or_else(|| "legacy".into()),
    }
}

#[cfg(test)]
mod tests {
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
                before_created_at: Some(crate::clock::now().fixed_offset()),
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
                origin: Some("invalid".into()),
                ..Default::default()
            }
            .valid()
        );
        for origin in super::MEDIA_ORIGINS {
            assert!(
                MediaListQuery {
                    origin: Some(origin.into()),
                    ..Default::default()
                }
                .valid()
            );
        }
        assert!(
            !MediaListQuery {
                format: Some("invalid".into()),
                ..Default::default()
            }
            .valid()
        );
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
    fn list_hides_audio_and_local_music_bindings() {
        let src = include_str!("media_catalog.rs");
        let list = src
            .split("pub async fn list_assets")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn delete_asset").next())
            .expect("list_assets");
        assert!(list.contains("NOT LIKE 'audio/%'"));
        assert!(list.contains("local_music_tracks"));
        assert!(list.contains("audio_media_id"));
        assert!(list.contains("cover_media_id"));
    }

    #[test]
    fn origins_cover_every_source_once() {
        use crate::services::media::MediaSource;
        for source in MediaSource::ALL {
            let hits = super::MEDIA_ORIGINS
                .iter()
                .filter(|origin| super::origin_sources(origin).unwrap().contains(&source.as_str()))
                .count();
            assert_eq!(hits, 1, "{} must belong to exactly one origin", source.as_str());
        }
        // Agent-kept stickers and images people sent her are never the same origin.
        assert_eq!(super::origin_sources("kept"), Some(&["kept"][..]));
        assert_eq!(super::origin_sources("channel"), Some(&["channel"][..]));
    }

    #[test]
    fn facets_only_include_present_origins_and_formats() {
        use super::{FacetPair, facets_from_pairs};
        let pairs = vec![
            FacetPair {
                source: "upload".into(),
                format: "jpeg".into(),
            },
            FacetPair {
                source: "upload".into(),
                format: "mp4".into(),
            },
            FacetPair {
                source: "kept".into(),
                format: "png".into(),
            },
        ];
        let (origins, all_formats) = facets_from_pairs(&pairs, None, None);
        assert_eq!(origins, vec!["upload", "kept"]);
        assert_eq!(all_formats, vec!["jpeg", "png", "mp4"]);

        let (origins_in_kept, kept_formats) = facets_from_pairs(&pairs, Some("kept"), None);
        assert_eq!(origins_in_kept, vec!["upload", "kept"]);
        assert_eq!(kept_formats, vec!["png"]);
    }
}
