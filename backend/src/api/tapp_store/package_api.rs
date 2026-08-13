//! Read-only installed package code, resources, assets and export endpoints.
//!
//! Path plans from stored manifests live in
//! [`crate::services::tapp_package_read`]. This module keeps visibility/auth
//! and filesystem IO.

use super::{
    append_directory_to_zip, find_visible_tapp, guess_asset_mime_type, installed_code_path,
    installed_tapp_dir, is_safe_path_component, optional_authenticated_user_id,
    read_tapp_text_resource, regular_resource_directory, regular_resource_path,
    validate_asset_path, TappManifest, WidgetTemplateContents, MAX_TAPP_ASSET_BYTES,
};
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use sea_orm::DatabaseConnection;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::fs;

use crate::error::HttpError;
use crate::middleware::auth::{Claims, OptionalClaims};
use crate::services::tapp_package_read::{
    asset_bytes_within_limit, installed_page_module_names, installed_page_module_relative_path,
    installed_text_resource_plan, installed_widget_template_paths, manifest_declares_asset,
};
use myriad_error::AppError;

async fn visible_tapp(
    db: &DatabaseConnection,
    claims: Option<&Claims>,
    tapp_id: &str,
) -> Result<crate::models::entities::tapps::Model, HttpError> {
    let user_id = optional_authenticated_user_id(claims);
    Ok(find_visible_tapp(db, user_id, tapp_id)
        .await?
        .ok_or_else(|| HttpError(AppError::not_found("Not found")))?
        .tapp)
}

pub(super) async fn get_tapp_code(
    State(db): State<DatabaseConnection>,
    Extension(OptionalClaims(claims)): Extension<OptionalClaims>,
    Path(tapp_id): Path<String>,
) -> Result<String, HttpError> {
    let tapp = visible_tapp(&db, claims.as_ref(), &tapp_id).await?;
    fs::read_to_string(installed_code_path(&tapp)?)
        .await
        .map_err(|_| HttpError(AppError::internal("Database error")))
}

#[derive(Debug, Serialize)]
pub(super) struct TappResourcesResponse {
    code: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    styles: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    widget_styles: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_styles: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    widget_css: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_css: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    widget_templates: Option<WidgetTemplateContents>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    css_mode: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    i18n: Option<HashMap<String, serde_json::Value>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_modules: Option<HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_module_order: Option<Vec<String>>,
}

/// Resource projection for dashboard widgets vs full page runtimes.
///
/// **Bandwidth projection only** — mode controls which package sections are
/// included in the HTTP response payload (omit unused templates/CSS/modules).
/// It does not change auth, visibility, or runtime capability grants.
///
/// - `full` (default): everything (legacy clients)
/// - `widget`: omit page template/CSS/modules; strip page section from code
/// - `page`: omit widget templates/CSS; strip widget section from code
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceMode {
    Full,
    Widget,
    Page,
}

impl ResourceMode {
    fn parse(raw: Option<&str>) -> Self {
        match raw.map(str::trim).map(|s| s.to_ascii_lowercase()).as_deref() {
            Some("widget") => Self::Widget,
            Some("page") => Self::Page,
            _ => Self::Full,
        }
    }

    fn wants_widget(self) -> bool {
        matches!(self, Self::Full | Self::Widget)
    }

    fn wants_page(self) -> bool {
        matches!(self, Self::Full | Self::Page)
    }
}

#[derive(Debug, Deserialize)]
pub(super) struct GetTappResourcesQuery {
    /// `full` | `widget` | `page`. Unknown values fall back to full.
    #[serde(default)]
    mode: Option<String>,
}

const WIDGET_CODE_MARKER: &str = "// ========== Widget Code ==========";
const PAGE_CODE_MARKER: &str = "// ========== Page Code ==========";

/// Drop the page section so widget sandboxes never download page modules' JS.
fn strip_page_code_section(code: &str) -> String {
    match code.find(PAGE_CODE_MARKER) {
        Some(idx) => code[..idx].trim_end().to_string(),
        None => code.to_string(),
    }
}

/// Drop the widget section while preserving any trailing page section.
fn strip_widget_code_section(code: &str) -> String {
    let Some(widget_idx) = code.find(WIDGET_CODE_MARKER) else {
        return code.to_string();
    };
    match code.find(PAGE_CODE_MARKER) {
        Some(page_idx) if page_idx > widget_idx => {
            let mut out = String::with_capacity(code.len() - (page_idx - widget_idx));
            out.push_str(code[..widget_idx].trim_end());
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(&code[page_idx..]);
            out
        }
        _ => code[..widget_idx].trim_end().to_string(),
    }
}

async fn read_optional_text(tapp_dir: &std::path::Path, path: Option<&str>) -> Option<String> {
    let path = path?;
    read_tapp_text_resource(tapp_dir, path).await.ok()
}

async fn load_widget_templates(
    tapp_dir: &std::path::Path,
    manifest: &serde_json::Value,
) -> WidgetTemplateContents {
    let entries = installed_widget_template_paths(manifest);
    if entries.is_empty() {
        return WidgetTemplateContents::new();
    }

    let reads = entries.into_iter().map(|entry| {
        let dir = tapp_dir.to_path_buf();
        async move {
            let content = read_tapp_text_resource(&dir, &entry.path).await.ok();
            (entry.widget_id, entry.size, content)
        }
    });
    let results = futures::future::join_all(reads).await;

    let mut widget_templates = WidgetTemplateContents::new();
    for (widget_id, size, content) in results {
        if let Some(content) = content {
            widget_templates
                .entry(widget_id)
                .or_default()
                .insert(size, content);
        }
    }
    widget_templates
}

async fn load_i18n(
    tapp_dir: &std::path::Path,
) -> Option<HashMap<String, serde_json::Value>> {
    let i18n_dir = regular_resource_directory(tapp_dir, "i18n")?;
    let mut translations = HashMap::new();
    let mut entries = fs::read_dir(i18n_dir).await.ok()?;
    while let Ok(Some(entry)) = entries.next_entry().await {
        let path = entry.path();
        if !entry.file_type().await.is_ok_and(|kind| kind.is_file())
            || path.extension().and_then(|extension| extension.to_str()) != Some("json")
        {
            continue;
        }
        let Some(filename) = entry.file_name().to_str().map(String::from) else {
            continue;
        };
        if !is_safe_path_component(&filename) {
            continue;
        }
        let Some(language) = filename.strip_suffix(".json") else {
            continue;
        };
        let relative = format!("i18n/{filename}");
        if let Ok(content) = read_tapp_text_resource(tapp_dir, &relative).await {
            if let Ok(value) = serde_json::from_str(&content) {
                translations.insert(language.to_string(), value);
            }
        }
    }
    (!translations.is_empty()).then_some(translations)
}

async fn load_page_modules(
    tapp_dir: &std::path::Path,
    order: &[String],
) -> Result<HashMap<String, String>, HttpError> {
    if order.is_empty() {
        return Ok(HashMap::new());
    }
    let reads = order.iter().map(|name| {
        let dir = tapp_dir.to_path_buf();
        let name = name.clone();
        async move {
            let relative = installed_page_module_relative_path(&name);
            let content = read_tapp_text_resource(&dir, &relative).await;
            (name, content)
        }
    });
    let results = futures::future::join_all(reads).await;
    let mut modules = HashMap::with_capacity(results.len());
    for (name, content) in results {
        let content =
            content.map_err(|_| HttpError(AppError::internal("Database error")))?;
        modules.insert(name, content);
    }
    Ok(modules)
}

pub(super) async fn get_tapp_resources(
    State(db): State<DatabaseConnection>,
    Extension(OptionalClaims(claims)): Extension<OptionalClaims>,
    Path(tapp_id): Path<String>,
    Query(query): Query<GetTappResourcesQuery>,
) -> Result<Json<TappResourcesResponse>, HttpError> {
    let mode = ResourceMode::parse(query.mode.as_deref());
    let tapp = visible_tapp(&db, claims.as_ref(), &tapp_id).await?;
    let tapp_dir = installed_tapp_dir(&tapp)?;
    let code_path = installed_code_path(&tapp)?;
    let manifest = &tapp.manifest;
    let plan = installed_text_resource_plan(manifest);

    // Parallel independent FS reads: code + shared styles + optional mode slices + i18n.
    let want_widget = mode.wants_widget();
    let want_page = mode.wants_page();

    let (
        code_result,
        styles,
        widget_styles,
        page_styles,
        widget_css,
        page_css,
        page_template,
        widget_templates,
        i18n,
    ) = tokio::join!(
        async {
            fs::read_to_string(&code_path)
                .await
                .map_err(|_| HttpError(AppError::internal("Database error")))
        },
        read_optional_text(&tapp_dir, plan.styles.as_deref()),
        async {
            if want_widget {
                read_optional_text(&tapp_dir, plan.widget_styles.as_deref()).await
            } else {
                None
            }
        },
        async {
            if want_page {
                read_optional_text(&tapp_dir, plan.page_styles.as_deref()).await
            } else {
                None
            }
        },
        async {
            if want_widget {
                read_optional_text(&tapp_dir, plan.widget_css.as_deref()).await
            } else {
                None
            }
        },
        async {
            if want_page {
                read_optional_text(&tapp_dir, plan.page_css.as_deref()).await
            } else {
                None
            }
        },
        async {
            if want_page {
                read_tapp_text_resource(&tapp_dir, &plan.page_template)
                    .await
                    .ok()
            } else {
                None
            }
        },
        async {
            if want_widget {
                load_widget_templates(&tapp_dir, manifest).await
            } else {
                WidgetTemplateContents::new()
            }
        },
        load_i18n(&tapp_dir),
    );

    let mut code = code_result?;
    match mode {
        ResourceMode::Widget => code = strip_page_code_section(&code),
        ResourceMode::Page => code = strip_widget_code_section(&code),
        ResourceMode::Full => {}
    }

    let (page_module_order, page_modules) = if want_page {
        let order = installed_page_module_names(manifest);
        if let Some(order) = order {
            let modules = load_page_modules(&tapp_dir, &order).await?;
            if modules.is_empty() {
                (None, None)
            } else {
                (Some(order), Some(modules))
            }
        } else {
            (None, None)
        }
    } else {
        (None, None)
    };

    Ok(Json(TappResourcesResponse {
        code,
        styles,
        widget_styles,
        page_styles,
        widget_css,
        page_css,
        widget_templates: (!widget_templates.is_empty()).then_some(widget_templates),
        page_template,
        css_mode: plan.css_mode.map(String::from),
        i18n,
        page_module_order,
        page_modules,
    }))
}

#[derive(Debug, Deserialize)]
pub(super) struct GetTappAssetQuery {
    path: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TappAssetResponse {
    path: String,
    mime_type: String,
    size: u64,
    base64: String,
}

pub(super) async fn get_tapp_asset(
    State(db): State<DatabaseConnection>,
    Extension(OptionalClaims(claims)): Extension<OptionalClaims>,
    Path(tapp_id): Path<String>,
    Query(query): Query<GetTappAssetQuery>,
) -> Result<Json<TappAssetResponse>, HttpError> {
    use base64::{engine::general_purpose::STANDARD, Engine};

    let tapp = visible_tapp(&db, claims.as_ref(), &tapp_id).await?;
    validate_asset_path(&query.path).map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    let manifest: TappManifest = serde_json::from_value(tapp.manifest.clone())
        .map_err(|_| HttpError(AppError::internal("Database error")))?;
    if !manifest_declares_asset(&manifest, &query.path) {
        return Err(HttpError(AppError::not_found("Not found")));
    }

    let tapp_dir = installed_tapp_dir(&tapp)?;
    let file_path = regular_resource_path(&tapp_dir, &query.path).ok_or_else(|| HttpError(AppError::not_found("Not found")))?;
    let bytes = fs::read(file_path)
        .await
        .map_err(|_| HttpError(AppError::not_found("Not found")))?;
    if !asset_bytes_within_limit(bytes.len() as u64, MAX_TAPP_ASSET_BYTES) {
        return Err(HttpError(AppError::from_status_u16(
            StatusCode::PAYLOAD_TOO_LARGE.as_u16(),
            "Payload too large",
        )));
    }
    Ok(Json(TappAssetResponse {
        path: query.path.clone(),
        mime_type: guess_asset_mime_type(&query.path).to_string(),
        size: bytes.len() as u64,
        base64: STANDARD.encode(bytes),
    }))
}

pub(super) async fn export_tapp(
    State(db): State<DatabaseConnection>,
    Extension(OptionalClaims(claims)): Extension<OptionalClaims>,
    Path(tapp_id): Path<String>,
) -> Result<impl IntoResponse, HttpError> {
    let tapp = visible_tapp(&db, claims.as_ref(), &tapp_id).await?;
    let tapp_dir = installed_tapp_dir(&tapp)?;
    let filename = format!("{tapp_id}.tapp");
    let zip_data = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, std::io::Error> {
        use zip::write::SimpleFileOptions;
        use zip::ZipWriter;

        let mut zip = ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options =
            SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        if tapp_dir.is_dir() {
            append_directory_to_zip(&mut zip, &tapp_dir, &tapp_dir, options)?;
        }
        Ok(zip.finish()?.into_inner())
    })
    .await
    .map_err(|_| HttpError(AppError::internal("Database error")))?
    .map_err(|_| HttpError(AppError::internal("Database error")))?;

    Ok((
        [
            (header::CONTENT_TYPE.as_str(), "application/zip".to_string()),
            (
                header::CONTENT_DISPOSITION.as_str(),
                format!("attachment; filename=\"{filename}\""),
            ),
        ],
        zip_data,
    ))
}

#[cfg(test)]
mod resource_mode_tests {
    use super::*;

    #[test]
    fn parses_resource_mode() {
        assert_eq!(ResourceMode::parse(None), ResourceMode::Full);
        assert_eq!(ResourceMode::parse(Some("full")), ResourceMode::Full);
        assert_eq!(ResourceMode::parse(Some("WIDGET")), ResourceMode::Widget);
        assert_eq!(ResourceMode::parse(Some(" page ")), ResourceMode::Page);
        assert_eq!(ResourceMode::parse(Some("unknown")), ResourceMode::Full);
    }

    #[test]
    fn strips_page_section_for_widget_projection() {
        let code = "core();\n// ========== Widget Code ==========\nw();\n// ========== Page Code ==========\np();\n";
        let stripped = strip_page_code_section(code);
        assert!(stripped.contains("core()"));
        assert!(stripped.contains("w()"));
        assert!(!stripped.contains("p()"));
        assert!(!stripped.contains("Page Code"));
    }

    #[test]
    fn strips_widget_section_for_page_projection() {
        let code = "core();\n// ========== Widget Code ==========\nw();\n// ========== Page Code ==========\np();\n";
        let stripped = strip_widget_code_section(code);
        assert!(stripped.contains("core()"));
        assert!(!stripped.contains("w()"));
        assert!(stripped.contains("p()"));
        assert!(stripped.contains("Page Code"));
        assert!(!stripped.contains("Widget Code"));
    }

    #[test]
    fn strip_is_noop_without_markers() {
        let code = "just core";
        assert_eq!(strip_page_code_section(code), code);
        assert_eq!(strip_widget_code_section(code), code);
    }
}
