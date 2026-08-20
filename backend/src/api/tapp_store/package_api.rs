//! Read-only installed package code, resources, assets and export endpoints.
//!
//! Path plans from stored manifests live in
//! [`crate::services::tapp_package_read`]. This module keeps visibility/auth
//! and filesystem IO.

use super::{
    append_directory_to_zip, collect_package_module_paths, find_visible_tapp,
    guess_asset_mime_type, installed_tapp_dir, is_safe_path_component,
    optional_authenticated_user_id, read_tapp_text_resource, regular_resource_directory,
    regular_resource_path, unsupported_package_structure, validate_asset_path,
    WidgetTemplateContents, MAX_TAPP_GAME_ASSET_BYTES,
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
use crate::services::tapp_install_resources::collect_tapp_module_graph;
use crate::services::tapp_package_read::{
    asset_bytes_within_limit, filter_widget_paths, installed_core_entry,
    installed_manifest_declares_asset, installed_page_entry, installed_text_resource_plan,
    installed_widget_layer_paths, installed_widget_template_paths, require_known_widget_id,
    HOST_PAGE_CSS, HOST_WIDGET_CSS,
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

#[derive(Debug, Serialize)]
pub(super) struct TappResourcesResponse {
    /// 该 mode 需要的包内 `.js` 文件：相对路径 → 源码。
    ///
    /// 至少包含相关层的入口。层内被 require 的文件随依赖图一起进来，
    /// 与本层无关的层入口不会下发。
    modules: HashMap<String, String>,
    /// 安装期同语义的静态解析表：模块路径 → require 原文 → 目标模块。
    ///
    /// 客户端有这个字段时不再扫描源码；缺失时仍可兼容旧后端。
    module_resolutions:
        std::collections::BTreeMap<String, std::collections::BTreeMap<String, String>>,
    /// 各层入口的相对路径，供客户端知道从哪个模块开始执行。
    #[serde(skip_serializing_if = "Option::is_none")]
    core_entry: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_entry: Option<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    widget_entries: HashMap<String, String>,
    /// 作者样式（层声明）。
    #[serde(skip_serializing_if = "Option::is_none")]
    core_styles: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_styles: Option<String>,
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    widget_styles: HashMap<String, String>,
    /// 宿主预编译 Tailwind 产物，与作者样式是两条通道。
    #[serde(skip_serializing_if = "Option::is_none")]
    widget_css: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_css: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    widget_templates: Option<WidgetTemplateContents>,
    #[serde(skip_serializing_if = "Option::is_none")]
    page_template: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    i18n: Option<HashMap<String, serde_json::Value>>,
}

/// Resource projection for dashboard widgets vs full page runtimes.
///
/// **Bandwidth projection only** — mode controls which package sections are
/// included in the HTTP response payload. It does not change auth, visibility,
/// or runtime capability grants.
///
/// - `full` (default): every layer
/// - `core`: core dependency closure only
/// - `widget`: core + one requested widget dependency closure
/// - `page`: core + page layers only
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ResourceMode {
    Full,
    Core,
    Widget,
    Page,
}

impl ResourceMode {
    fn parse(raw: Option<&str>) -> Self {
        match raw
            .map(str::trim)
            .map(|s| s.to_ascii_lowercase())
            .as_deref()
        {
            Some("core") => Self::Core,
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
    /// `full` | `core` | `widget` | `page`. Unknown values fall back to full.
    #[serde(default)]
    mode: Option<String>,
    /// Widget mode may select one manifest widget. Omitted keeps the old
    /// all-widget projection for backward-compatible callers.
    #[serde(default)]
    widget_id: Option<String>,
}

async fn read_optional_text(tapp_dir: &std::path::Path, path: Option<&str>) -> Option<String> {
    let path = path?;
    read_tapp_text_resource(tapp_dir, path).await.ok()
}

async fn load_widget_templates(
    tapp_dir: &std::path::Path,
    manifest: &serde_json::Value,
    widget_id: Option<&str>,
) -> WidgetTemplateContents {
    let entries: Vec<_> = installed_widget_template_paths(manifest)
        .into_iter()
        .filter(|entry| widget_id.is_none_or(|selected| entry.widget_id == selected))
        .collect();
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

async fn load_i18n(tapp_dir: &std::path::Path) -> Option<HashMap<String, serde_json::Value>> {
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

/// Read the JS files a mode needs, keyed by package-relative path.
///
/// 层入口加同层可 require 的文件一起下发；客户端从入口出发做闭包，只把用到的
/// 模块包装进 iframe。
async fn load_layer_modules(
    tapp_dir: &std::path::Path,
    entries: &[String],
) -> Result<HashMap<String, String>, HttpError> {
    if entries.is_empty() {
        return Ok(HashMap::new());
    }
    let reads = entries.iter().map(|relative| {
        let dir = tapp_dir.to_path_buf();
        let relative = relative.clone();
        async move {
            let content = read_tapp_text_resource(&dir, &relative).await;
            (relative, content)
        }
    });
    let results = futures::future::join_all(reads).await;
    let mut modules = HashMap::with_capacity(results.len());
    for (relative, content) in results {
        let content = content.map_err(|_| {
            unsupported_package_structure("a declared layer entry could not be read")
        })?;
        modules.insert(relative, content);
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
    let manifest = &tapp.manifest;
    let plan = installed_text_resource_plan(manifest);

    let want_widget = mode.wants_widget();
    let want_page = mode.wants_page();
    let selected_widget_id = if mode == ResourceMode::Widget {
        require_known_widget_id(manifest, query.widget_id.as_deref()).map_err(|error| {
            unsupported_package_structure(&format!("Unknown widget id: {}", error.0))
        })?
    } else {
        None
    };

    // 入口完全来自 manifest。目录名不参与层归属。
    let core_entry = installed_core_entry(manifest);
    let page_entry = want_page.then(|| installed_page_entry(manifest)).flatten();
    let widget_entries: HashMap<String, String> = if want_widget {
        let entries = filter_widget_paths(
            installed_widget_layer_paths(manifest, "entry"),
            selected_widget_id,
        );
        if let Some(widget_id) = selected_widget_id {
            if entries.is_empty() {
                return Err(unsupported_package_structure(&format!(
                    "Widget {widget_id} has no layer entry"
                )));
            }
        }
        entries.into_iter().collect()
    } else {
        HashMap::new()
    };
    let widget_styles_paths = if want_widget {
        filter_widget_paths(
            installed_widget_layer_paths(manifest, "styles"),
            selected_widget_id,
        )
    } else {
        Vec::new()
    };

    let (core_styles, page_styles, widget_css, page_css, page_template, widget_templates, i18n) = tokio::join!(
        read_optional_text(&tapp_dir, plan.core_styles.as_deref()),
        async {
            if want_page {
                read_optional_text(&tapp_dir, plan.page_styles.as_deref()).await
            } else {
                None
            }
        },
        async {
            if want_widget {
                read_optional_text(&tapp_dir, Some(HOST_WIDGET_CSS)).await
            } else {
                None
            }
        },
        async {
            if want_page {
                read_optional_text(&tapp_dir, Some(HOST_PAGE_CSS)).await
            } else {
                None
            }
        },
        async {
            if want_page {
                read_optional_text(&tapp_dir, plan.page_template.as_deref()).await
            } else {
                None
            }
        },
        async {
            if want_widget {
                load_widget_templates(&tapp_dir, manifest, selected_widget_id).await
            } else {
                WidgetTemplateContents::new()
            }
        },
        load_i18n(&tapp_dir),
    );

    // 扫描包内模块后只返回所选入口的精确闭包。`page/` / `widget/` 只是推荐布局，
    // 不再决定一个文件能否进入某层，也不会让同一目录的其它 Widget 源码旁路进入。
    let package_module_paths = collect_package_module_paths(&tapp_dir);
    let all_modules = load_layer_modules(&tapp_dir, &package_module_paths).await?;
    let mut layer_entries = Vec::new();
    layer_entries.extend(core_entry.clone());
    layer_entries.extend(page_entry.clone());
    layer_entries.extend(widget_entries.values().cloned());
    let graph = collect_tapp_module_graph(&all_modules, &layer_entries)
        .map_err(|error| unsupported_package_structure(&error))?;
    let modules: HashMap<String, String> = graph
        .included
        .iter()
        .filter_map(|path| {
            all_modules
                .get(path)
                .map(|source| (path.clone(), source.clone()))
        })
        .collect();

    let mut widget_styles = HashMap::new();
    for (widget_id, path) in widget_styles_paths {
        if let Some(content) = read_optional_text(&tapp_dir, Some(&path)).await {
            widget_styles.insert(widget_id, content);
        }
    }

    Ok(Json(TappResourcesResponse {
        modules,
        module_resolutions: graph.resolutions,
        core_entry,
        page_entry,
        widget_entries,
        core_styles,
        page_styles,
        widget_styles,
        widget_css,
        page_css,
        widget_templates: (!widget_templates.is_empty()).then_some(widget_templates),
        page_template,
        i18n,
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
    validate_asset_path(&query.path)
        .map_err(|_| HttpError(AppError::bad_request("Bad request")))?;
    if !installed_manifest_declares_asset(&tapp.manifest, &query.path) {
        return Err(HttpError(AppError::not_found("Not found")));
    }

    let tapp_dir = installed_tapp_dir(&tapp)?;
    let file_path = regular_resource_path(&tapp_dir, &query.path)
        .ok_or_else(|| HttpError(AppError::not_found("Not found")))?;
    let bytes = fs::read(file_path)
        .await
        .map_err(|_| HttpError(AppError::not_found("Not found")))?;
    if !asset_bytes_within_limit(bytes.len() as u64, MAX_TAPP_GAME_ASSET_BYTES) {
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
        assert_eq!(ResourceMode::parse(Some("CORE")), ResourceMode::Core);
        assert_eq!(ResourceMode::parse(Some("WIDGET")), ResourceMode::Widget);
        assert_eq!(ResourceMode::parse(Some(" page ")), ResourceMode::Page);
        assert_eq!(ResourceMode::parse(Some("unknown")), ResourceMode::Full);
    }

    /// 层投影靠 manifest 声明，不再靠在单文件里找注释标记：widget 模式下
    /// page 入口不进 modules 表，page 模式下 widget 入口同理。
    #[test]
    fn layer_projection_excludes_the_other_surface() {
        assert!(ResourceMode::Widget.wants_widget());
        assert!(!ResourceMode::Widget.wants_page());
        assert!(ResourceMode::Page.wants_page());
        assert!(!ResourceMode::Page.wants_widget());
        assert!(ResourceMode::Full.wants_widget() && ResourceMode::Full.wants_page());
        assert!(!ResourceMode::Core.wants_widget());
        assert!(!ResourceMode::Core.wants_page());
    }

    /// 钉住出站 JSON 的键名。
    ///
    /// 这个响应体是新造的，键名靠 `TappPackageResourceApi.ts` 里手写的 interface
    /// 对接。两边各自 mock 的测试都不会发现改名，运行时表现是沙箱拿不到入口的白屏，
    /// 所以这里把键集合固定下来。
    #[test]
    fn resource_response_keys_match_the_client_interface() {
        let response = TappResourcesResponse {
            modules: HashMap::from([("core.js".to_string(), "0".to_string())]),
            module_resolutions: std::collections::BTreeMap::from([(
                "core.js".to_string(),
                std::collections::BTreeMap::from([(
                    "./shared.js".to_string(),
                    "shared.js".to_string(),
                )]),
            )]),
            core_entry: Some("core.js".to_string()),
            page_entry: Some("page/index.js".to_string()),
            widget_entries: HashMap::from([("card".to_string(), "widget/index.js".to_string())]),
            core_styles: Some("a{}".to_string()),
            page_styles: Some("b{}".to_string()),
            widget_styles: HashMap::from([("card".to_string(), "c{}".to_string())]),
            widget_css: Some("d{}".to_string()),
            page_css: Some("e{}".to_string()),
            widget_templates: None,
            page_template: None,
            i18n: None,
        };

        let value = serde_json::to_value(&response).unwrap();
        let mut keys: Vec<&str> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "core_entry",
                "core_styles",
                "module_resolutions",
                "modules",
                "page_css",
                "page_entry",
                "page_styles",
                "widget_css",
                "widget_entries",
                "widget_styles",
            ]
        );
    }
}
