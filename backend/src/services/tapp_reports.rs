//! Tapp / runtime report catalog + custom Tapp report CRUD.
//!
//! - Platform catalog: `platform_reports` rows + payload projection
//! - Custom CRUD: subject-scoped `tapp_storage` keys `_report:{id}`
//!
//! Domain lives in services so agent context and runtime paths share one shape
//! without importing `api::tapp_runtime::reports`.

use sea_orm::{
    ActiveModelTrait, ActiveValue::NotSet, ColumnTrait, DatabaseConnection, EntityTrait,
    QueryFilter, QueryOrder, QuerySelect, Set,
};
use serde_json::{json, Value};
use uuid::Uuid;

use crate::models::entities::{platform_reports, tapp_storage};
use crate::services::platform_cache::validate_platform_name;

const REPORT_KEY_PREFIX: &str = "_report:";

/// Domain errors for report catalog reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ReportCatalogError {
    Database,
    NotFound,
    InvalidPlatform(String),
}

impl ReportCatalogError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::Database => "REPORT_DATABASE_ERROR",
            Self::NotFound => "REPORT_NOT_FOUND",
            Self::InvalidPlatform(_) => "INVALID_PLATFORM",
        }
    }

    pub fn message(&self) -> String {
        match self {
            Self::Database => "Failed to fetch reports".to_string(),
            Self::NotFound => "Report not found".to_string(),
            Self::InvalidPlatform(msg) => msg.clone(),
        }
    }

    pub fn status_hint(&self) -> u16 {
        match self {
            Self::Database => 500,
            Self::NotFound => 404,
            Self::InvalidPlatform(_) => 400,
        }
    }
}

impl std::fmt::Display for ReportCatalogError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for ReportCatalogError {}

/// Project a platform_reports row for host + Tapp clients.
///
/// Exposes nested `content` (legacy) and top-level `card_visuals` / `cardVisuals`
/// so home/catalog cards render without field-mapping bugs.
pub fn platform_report_payload(report: &platform_reports::Model) -> Value {
    let card_visuals = report
        .report
        .get("card_visuals")
        .cloned()
        .filter(|v| !v.is_null())
        .unwrap_or_else(|| json!({}));
    let insights = report
        .report
        .get("insights")
        .cloned()
        .unwrap_or_else(|| json!([]));
    json!({
        "id": report.id,
        "platform": report.platform,
        "type": "platform",
        "summary": report.report.get("summary").and_then(Value::as_str).unwrap_or(""),
        "insights": insights,
        "content": report.report,
        "metadata": report.metadata,
        "card_visuals": card_visuals.clone(),
        "cardVisuals": card_visuals,
        "createdAt": report.created_at.to_string()
    })
}

/// Compact list row used by host `GET /api/reports/list`.
pub fn platform_report_list_item(report: &platform_reports::Model) -> Value {
    json!({
        "id": report.id,
        "platform": report.platform,
        "type": "platform",
        "createdAt": report.created_at.to_string(),
        "summary": report.report.get("summary").and_then(|v| v.as_str()).unwrap_or("")
    })
}

pub async fn list_user_platform_reports(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<Vec<platform_reports::Model>, ReportCatalogError> {
    platform_reports::Entity::find()
        .filter(platform_reports::Column::UserId.eq(user_id))
        .order_by_desc(platform_reports::Column::CreatedAt)
        .all(db)
        .await
        .map_err(|error| {
            tracing::error!(%error, "[TAPP] Failed to list platform reports");
            ReportCatalogError::Database
        })
}

pub async fn get_user_platform_report(
    db: &DatabaseConnection,
    user_id: i32,
    report_id: i32,
) -> Result<platform_reports::Model, ReportCatalogError> {
    platform_reports::Entity::find_by_id(report_id)
        .filter(platform_reports::Column::UserId.eq(user_id))
        .one(db)
        .await
        .map_err(|_| ReportCatalogError::Database)?
        .ok_or(ReportCatalogError::NotFound)
}

/// Latest report for a platform (or None when the user has no report yet).
pub async fn get_latest_user_platform_report(
    db: &DatabaseConnection,
    user_id: i32,
    platform: &str,
) -> Result<Option<platform_reports::Model>, ReportCatalogError> {
    validate_platform_name(platform).map_err(ReportCatalogError::InvalidPlatform)?;
    platform_reports::Entity::find()
        .filter(platform_reports::Column::UserId.eq(user_id))
        .filter(platform_reports::Column::Platform.eq(platform.to_lowercase()))
        .order_by_desc(platform_reports::Column::CreatedAt)
        .one(db)
        .await
        .map_err(|_| ReportCatalogError::Database)
}

// Custom Tapp report CRUD (tapp_storage)

/// Domain errors for subject-scoped custom report CRUD.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TappReportCrudError {
    InvalidReportType,
    Database,
    NotFound,
    CreateFailed,
    UpdateFailed,
    DeleteFailed,
}

impl TappReportCrudError {
    #[allow(dead_code)] // Stable machine code for future API adapters.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidReportType => "INVALID_REPORT_TYPE",
            Self::Database => "REPORT_DATABASE_ERROR",
            Self::NotFound => "REPORT_NOT_FOUND",
            Self::CreateFailed => "REPORT_CREATE_FAILED",
            Self::UpdateFailed => "REPORT_UPDATE_FAILED",
            Self::DeleteFailed => "REPORT_DELETE_FAILED",
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::InvalidReportType => "report_type must be platform or custom",
            Self::Database => "Database error",
            Self::NotFound => "Report not found",
            Self::CreateFailed => "Failed to create report",
            Self::UpdateFailed => "Failed to update report",
            Self::DeleteFailed => "Failed to delete report",
        }
    }

    pub fn status_hint(&self) -> u16 {
        match self {
            Self::InvalidReportType => 400,
            Self::NotFound => 404,
            Self::Database | Self::CreateFailed | Self::UpdateFailed | Self::DeleteFailed => 500,
        }
    }
}

impl std::fmt::Display for TappReportCrudError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for TappReportCrudError {}

/// Validate custom report type (`platform` | `custom`).
pub fn validate_report_type(report_type: &str) -> Result<(), TappReportCrudError> {
    if matches!(report_type, "platform" | "custom") {
        Ok(())
    } else {
        Err(TappReportCrudError::InvalidReportType)
    }
}

pub fn report_storage_key(report_id: &str) -> String {
    format!("{REPORT_KEY_PREFIX}{report_id}")
}

/// Compact list projection for custom reports stored as JSON documents.
pub fn compact_tapp_report_list_item(data: &Value) -> Option<Value> {
    Some(json!({
        "id": data.get("id")?,
        "title": data.get("title")?,
        "type": data.get("type")?,
        "createdAt": data.get("createdAt")?,
        "updatedAt": data.get("updatedAt")?
    }))
}

/// Result of a successful create.
#[derive(Debug, Clone)]
pub struct CreatedTappReport {
    pub id: String,
    pub title: String,
    pub report_type: String,
    pub created_at: String,
    #[allow(dead_code)] // Full document available for callers that need it.
    pub document: Value,
}

/// Create a custom Tapp report in subject-scoped storage.
pub async fn create_tapp_report(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    title: &str,
    report_type: &str,
    content: Value,
    metadata: Option<Value>,
) -> Result<CreatedTappReport, TappReportCrudError> {
    validate_report_type(report_type)?;
    let now = chrono::Utc::now().fixed_offset();
    let report_id = format!("report_{report_type}_{}", Uuid::new_v4());
    let storage_key = report_storage_key(&report_id);
    let created_at = now.to_rfc3339();

    let report_data = json!({
        "id": report_id,
        "title": title,
        "type": report_type,
        "content": content,
        "metadata": metadata,
        "createdAt": created_at,
        "updatedAt": created_at
    });

    let storage = tapp_storage::ActiveModel {
        id: NotSet,
        tapp_id: Set(tapp_id.to_string()),
        user_id: Set(user_id),
        key: Set(storage_key),
        value: Set(report_data.clone()),
        encrypted_value: NotSet,
        binding_fingerprint: NotSet,
        created_at: Set(now),
        updated_at: Set(now),
    };

    storage.insert(db).await.map_err(|error| {
        tracing::error!(%error, "[TAPP] Failed to create report");
        TappReportCrudError::CreateFailed
    })?;

    Ok(CreatedTappReport {
        id: report_id,
        title: title.to_string(),
        report_type: report_type.to_string(),
        created_at,
        document: report_data,
    })
}

/// List compact custom report summaries for a subject+tapp pair.
pub async fn list_tapp_reports(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    limit: u64,
    offset: u64,
) -> Result<Vec<Value>, TappReportCrudError> {
    let items = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(tapp_id))
        .filter(tapp_storage::Column::Key.starts_with(REPORT_KEY_PREFIX))
        .order_by_desc(tapp_storage::Column::CreatedAt)
        .offset(offset)
        .limit(limit)
        .all(db)
        .await
        .map_err(|_| TappReportCrudError::Database)?;

    Ok(items
        .into_iter()
        .filter_map(|item| compact_tapp_report_list_item(&item.value))
        .collect())
}

/// Load the full custom report document.
pub async fn get_tapp_report(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    report_id: &str,
) -> Result<Value, TappReportCrudError> {
    let storage_key = report_storage_key(report_id);
    let item = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .one(db)
        .await
        .map_err(|_| TappReportCrudError::Database)?
        .ok_or(TappReportCrudError::NotFound)?;
    Ok(item.value)
}

/// Patch title/content/metadata on a custom report document.
pub async fn update_tapp_report(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    report_id: &str,
    title: Option<String>,
    content: Option<Value>,
    metadata: Option<Value>,
) -> Result<Value, TappReportCrudError> {
    let storage_key = report_storage_key(report_id);
    let item = tapp_storage::Entity::find()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .one(db)
        .await
        .map_err(|_| TappReportCrudError::Database)?
        .ok_or(TappReportCrudError::NotFound)?;

    let now = chrono::Utc::now().fixed_offset();
    let mut report_data = item.value.clone();
    if let Some(title) = title {
        report_data["title"] = json!(title);
    }
    if let Some(content) = content {
        report_data["content"] = content;
    }
    if let Some(metadata) = metadata {
        report_data["metadata"] = metadata;
    }
    report_data["updatedAt"] = json!(now.to_rfc3339());

    let mut active: tapp_storage::ActiveModel = item.into();
    active.value = Set(report_data.clone());
    active.updated_at = Set(now);
    active
        .update(db)
        .await
        .map_err(|_| TappReportCrudError::UpdateFailed)?;
    Ok(report_data)
}

/// Delete a custom report by id.
pub async fn delete_tapp_report(
    db: &DatabaseConnection,
    user_id: i32,
    tapp_id: &str,
    report_id: &str,
) -> Result<(), TappReportCrudError> {
    let storage_key = report_storage_key(report_id);
    let result = tapp_storage::Entity::delete_many()
        .filter(tapp_storage::Column::UserId.eq(user_id))
        .filter(tapp_storage::Column::TappId.eq(tapp_id))
        .filter(tapp_storage::Column::Key.eq(&storage_key))
        .exec(db)
        .await
        .map_err(|_| TappReportCrudError::DeleteFailed)?;
    if result.rows_affected == 0 {
        return Err(TappReportCrudError::NotFound);
    }
    Ok(())
}

/// Clamp list pagination to the historical API contract (default 50, max 100).
pub fn clamp_report_list_pagination(limit: Option<u32>, offset: Option<u32>) -> (u64, u64) {
    let limit = u64::from(limit.unwrap_or(50).min(100));
    let offset = u64::from(offset.unwrap_or(0));
    (limit, offset)
}

#[cfg(test)]
mod tests {
    use super::{
        clamp_report_list_pagination, compact_tapp_report_list_item, platform_report_list_item,
        platform_report_payload, report_storage_key, validate_report_type, ReportCatalogError,
        TappReportCrudError,
    };
    use crate::models::entities::platform_reports;
    use chrono::Utc;
    use serde_json::json;

    fn sample_report() -> platform_reports::Model {
        let now = Utc::now().naive_utc();
        platform_reports::Model {
            id: 7,
            user_id: 1,
            platform: "steam".into(),
            report: json!({
                "summary": "Played a lot",
                "insights": ["a"],
                "card_visuals": { "accent": "#f00" }
            }),
            metadata: json!({ "source": "test" }),
            created_at: now,
            expires_at: now,
            report_title: Some("Weekly".into()),
        }
    }

    #[test]
    fn platform_payload_exposes_card_visuals_aliases() {
        let payload = platform_report_payload(&sample_report());
        assert_eq!(payload["id"], 7);
        assert_eq!(payload["platform"], "steam");
        assert_eq!(payload["summary"], "Played a lot");
        assert_eq!(payload["card_visuals"]["accent"], "#f00");
        assert_eq!(payload["cardVisuals"]["accent"], "#f00");
        assert_eq!(payload["content"]["summary"], "Played a lot");
        assert_eq!(payload["type"], "platform");
    }

    #[test]
    fn list_item_is_compact() {
        let item = platform_report_list_item(&sample_report());
        assert_eq!(item["id"], 7);
        assert_eq!(item["summary"], "Played a lot");
        assert!(item.get("card_visuals").is_none());
        assert!(item.get("content").is_none());
    }

    #[test]
    fn error_codes_preserve_api_contract() {
        assert_eq!(ReportCatalogError::NotFound.code(), "REPORT_NOT_FOUND");
        assert_eq!(
            ReportCatalogError::Database.message(),
            "Failed to fetch reports"
        );
        assert_eq!(ReportCatalogError::NotFound.status_hint(), 404);
        assert_eq!(
            ReportCatalogError::InvalidPlatform("bad".into()).status_hint(),
            400
        );
    }

    #[test]
    fn report_type_and_storage_key_contract() {
        assert!(validate_report_type("platform").is_ok());
        assert!(validate_report_type("custom").is_ok());
        assert!(validate_report_type("other").is_err());
        assert_eq!(
            report_storage_key("report_custom_abc"),
            "_report:report_custom_abc"
        );
    }

    #[test]
    fn compact_list_item_requires_core_fields() {
        let full = json!({
            "id": "report_custom_1",
            "title": "Weekly",
            "type": "custom",
            "createdAt": "2026-01-01T00:00:00Z",
            "updatedAt": "2026-01-02T00:00:00Z",
            "content": { "x": 1 }
        });
        let item = compact_tapp_report_list_item(&full).unwrap();
        assert_eq!(item["id"], "report_custom_1");
        assert_eq!(item["title"], "Weekly");
        assert!(item.get("content").is_none());

        let missing = json!({ "id": "x", "title": "t" });
        assert!(compact_tapp_report_list_item(&missing).is_none());
    }

    #[test]
    fn pagination_clamps_to_api_contract() {
        assert_eq!(clamp_report_list_pagination(None, None), (50, 0));
        assert_eq!(clamp_report_list_pagination(Some(200), Some(10)), (100, 10));
        assert_eq!(clamp_report_list_pagination(Some(5), Some(0)), (5, 0));
    }

    #[test]
    fn crud_error_messages_match_http_bodies() {
        assert_eq!(
            TappReportCrudError::InvalidReportType.message(),
            "report_type must be platform or custom"
        );
        assert_eq!(TappReportCrudError::NotFound.message(), "Report not found");
        assert_eq!(
            TappReportCrudError::CreateFailed.message(),
            "Failed to create report"
        );
        assert_eq!(TappReportCrudError::Database.message(), "Database error");
        assert_eq!(TappReportCrudError::InvalidReportType.status_hint(), 400);
        assert_eq!(TappReportCrudError::NotFound.status_hint(), 404);
        assert_eq!(TappReportCrudError::CreateFailed.status_hint(), 500);
    }
}
