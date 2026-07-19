//! Remote Tapp store package discovery and resource download.

use super::prepared_package::{PreparedTappPackage, PreparedTappResources};
use super::{api_error, ApiResponse, TappCategory, TappManifest, WidgetTemplateContents};
use axum::{http::StatusCode, Json};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};

use crate::models::entities::tapp_store_sources;

/// 从远程商店下载 Tapp 文件
///
/// 返回 (manifest, code, styles, widget_styles, page_styles, page_template, widget_templates)
async fn fetch_public_store_url(url: &str) -> Result<reqwest::Response, String> {
    let (target_url, client) = crate::services::outbound_security::build_public_http_client(
        url,
        std::time::Duration::from_secs(20),
        Some("Myriad-Tapp-Store/1.0"),
    )
    .await?;
    client
        .get(target_url)
        .send()
        .await
        .map_err(|error| error.to_string())
}

pub(super) fn validate_store_manifest_category(
    app_info: &serde_json::Value,
    manifest: &TappManifest,
) -> Result<(), String> {
    let index_category = app_info
        .get("category")
        .cloned()
        .ok_or_else(|| "Store index app is missing category".to_string())?;
    let index_category: TappCategory = serde_json::from_value(index_category)
        .map_err(|_| "Store index app has an invalid category".to_string())?;
    if Some(index_category) != manifest.category {
        return Err(format!(
            "Store index category does not match manifest category for {}",
            manifest.id
        ));
    }
    Ok(())
}

/// Normalize catalog URL for matching: strip trailing slash and optional `/index.json`.
fn normalize_store_catalog_url(url: &str) -> String {
    url.trim()
        .trim_end_matches('/')
        .trim_end_matches("/index.json")
        .trim_end_matches('/')
        .to_string()
}

pub(super) async fn fetch_from_store(
    db: &DatabaseConnection,
    store_source: &str,
    tapp_id: &str,
) -> Result<PreparedTappPackage, (StatusCode, Json<ApiResponse<()>>)> {
    // Reject install-mode placeholders mistaken for catalog refs (Aro legacy bug).
    let trimmed = store_source.trim();
    if trimmed.is_empty() || trimmed.eq_ignore_ascii_case("store") || trimmed.eq_ignore_ascii_case("direct")
    {
        return Err((
            StatusCode::BAD_REQUEST,
            api_error(
                "Invalid storeSource: expected catalog URL or store source id, not install mode",
            ),
        ));
    }

    // 获取商店源信息 — match by id, exact URL, or normalized base (with/without index.json).
    let source = {
        let by_id_or_exact = tapp_store_sources::Entity::find()
            .filter(
                tapp_store_sources::Column::Url
                    .eq(trimmed)
                    .or(tapp_store_sources::Column::Id.eq(trimmed.parse::<i32>().unwrap_or(-1))),
            )
            .one(db)
            .await
            .map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    api_error("Database error"),
                )
            })?;

        if let Some(s) = by_id_or_exact {
            s
        } else {
            let want = normalize_store_catalog_url(trimmed);
            let all = tapp_store_sources::Entity::find().all(db).await.map_err(|_| {
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    api_error("Database error"),
                )
            })?;
            all.into_iter()
                .find(|s| normalize_store_catalog_url(&s.url) == want)
                .ok_or_else(|| {
                    (
                        StatusCode::NOT_FOUND,
                        api_error(format!(
                            "Store source not found for '{}'. Add this catalog URL in Tapp Store settings (official Myriad store is pre-seeded).",
                            trimmed
                        )),
                    )
                })?
        }
    };

    let base_url = source
        .url
        .trim_end_matches("/index.json")
        .trim_end_matches('/');

    // 安全验证：确保 URL 使用 https 且不指向内部网络
    if let Ok(parsed_url) = reqwest::Url::parse(base_url) {
        let scheme = parsed_url.scheme();
        if scheme != "https" && scheme != "http" {
            return Err((
                StatusCode::BAD_REQUEST,
                api_error("Only HTTP(S) URLs are allowed"),
            ));
        }
        if let Some(host) = parsed_url.host_str() {
            if host == "localhost"
                || host == "127.0.0.1"
                || host == "::1"
                || host.starts_with("10.")
                || host.starts_with("172.16.")
                || host.starts_with("192.168.")
                || host == "0.0.0.0"
                || host.ends_with(".local")
                || host.ends_with(".internal")
            {
                return Err((
                    StatusCode::BAD_REQUEST,
                    api_error("Internal network URLs are not allowed"),
                ));
            }
        }
    }

    // 获取商店索引
    // 注意：生产环境中 backend 容器若无法访问外网（尤其 raw.githubusercontent.com），
    // 这里会返回 502。前端商店列表走浏览器直连，因此可能出现「能浏览、不能安装」。
    let index_url = format!("{}/index.json", base_url);
    tracing::info!(url = %index_url, "fetching tapp store index");
    let index_resp = fetch_public_store_url(&index_url).await.map_err(|e| {
        tracing::error!(url = %index_url, error = %e, "failed to fetch store index");
        (
            StatusCode::BAD_GATEWAY,
            api_error(format!(
                "Failed to fetch store index (backend cannot reach store URL): {}",
                e
            )),
        )
    })?;

    if !index_resp.status().is_success() {
        let status = index_resp.status();
        tracing::error!(url = %index_url, %status, "store index returned non-success");
        return Err((
            StatusCode::BAD_GATEWAY,
            api_error(format!(
                "Failed to fetch store index: remote returned {}",
                status
            )),
        ));
    }

    let index: serde_json::Value = index_resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            api_error(format!("Invalid store index format: {}", e)),
        )
    })?;

    // 在商店中查找指定的 Tapp
    let apps = index
        .get("apps")
        .and_then(|v| v.as_array())
        .ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                api_error("Invalid store index: no apps array"),
            )
        })?;

    let app_info = apps
        .iter()
        .find(|app| app.get("id").and_then(|v| v.as_str()) == Some(tapp_id))
        .ok_or_else(|| {
            (
                StatusCode::NOT_FOUND,
                api_error(format!("Tapp {} not found in store", tapp_id)),
            )
        })?;

    let download = app_info.get("download").ok_or_else(|| {
        (
            StatusCode::BAD_GATEWAY,
            api_error("No download info in app"),
        )
    })?;

    // 下载 manifest.json
    let manifest_path = download
        .get("manifest")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_GATEWAY, api_error("No manifest path")))?;
    let manifest_url = format!("{}/{}", base_url, manifest_path);

    let manifest_resp = fetch_public_store_url(&manifest_url).await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            api_error(format!("Failed to fetch manifest: {}", e)),
        )
    })?;

    let manifest: TappManifest = manifest_resp.json().await.map_err(|e| {
        (
            StatusCode::BAD_GATEWAY,
            api_error(format!("Invalid manifest: {}", e)),
        )
    })?;
    validate_store_manifest_category(app_info, &manifest)
        .map_err(|error| (StatusCode::BAD_GATEWAY, api_error(error)))?;

    // 下载主代码
    let code_path = download
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or_else(|| (StatusCode::BAD_GATEWAY, api_error("No code path")))?;
    let code_url = format!("{}/{}", base_url, code_path);

    let code = fetch_public_store_url(&code_url)
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                api_error(format!("Failed to fetch code: {}", e)),
            )
        })?
        .text()
        .await
        .map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                api_error(format!("Failed to read code: {}", e)),
            )
        })?;

    // 下载可选资源
    let mut styles_content: Option<String> = None;
    let mut widget_styles_content: Option<String> = None;
    let mut page_styles_content: Option<String> = None;
    let mut page_template_content: Option<String> = None;
    let mut widget_templates: WidgetTemplateContents = std::collections::HashMap::new();

    // 下载 CSS 样式（统一模式）
    if let Some(styles_path) = download.get("styles").and_then(|v| v.as_str()) {
        let styles_url = format!("{}/{}", base_url, styles_path);
        if let Ok(resp) = fetch_public_store_url(&styles_url).await {
            if resp.status().is_success() {
                if let Ok(content) = resp.text().await {
                    styles_content = Some(content);
                }
            }
        }
    }

    // 下载 Widget 专用 CSS（分离模式）
    if let Some(widget_styles_path) = download.get("widget_styles").and_then(|v| v.as_str()) {
        let widget_styles_url = format!("{}/{}", base_url, widget_styles_path);
        if let Ok(resp) = fetch_public_store_url(&widget_styles_url).await {
            if resp.status().is_success() {
                if let Ok(content) = resp.text().await {
                    widget_styles_content = Some(content);
                }
            }
        }
    }

    // 下载 Page 专用 CSS（分离模式）
    if let Some(page_styles_path) = download.get("page_styles").and_then(|v| v.as_str()) {
        let page_styles_url = format!("{}/{}", base_url, page_styles_path);
        if let Ok(resp) = fetch_public_store_url(&page_styles_url).await {
            if resp.status().is_success() {
                if let Ok(content) = resp.text().await {
                    page_styles_content = Some(content);
                }
            }
        }
    }

    // 下载 Page 模板
    if let Some(page_path) = download.get("page_template").and_then(|v| v.as_str()) {
        let page_url = format!("{}/{}", base_url, page_path);
        if let Ok(resp) = fetch_public_store_url(&page_url).await {
            if resp.status().is_success() {
                if let Ok(content) = resp.text().await {
                    page_template_content = Some(content);
                }
            }
        }
    }

    // 下载 Widget 模板
    if let Some(widgets) = download.get("widget_templates").and_then(|v| v.as_object()) {
        for (widget_id, templates) in widgets {
            let Some(templates) = templates.as_object() else {
                continue;
            };
            let mut downloaded = std::collections::HashMap::new();
            for (size, path) in templates {
                if let Some(template_path) = path.as_str() {
                    let template_url = format!("{}/{}", base_url, template_path);
                    if let Ok(resp) = fetch_public_store_url(&template_url).await {
                        if resp.status().is_success() {
                            if let Ok(content) = resp.text().await {
                                downloaded.insert(size.clone(), content);
                            }
                        }
                    }
                }
            }
            if !downloaded.is_empty() {
                widget_templates.insert(widget_id.clone(), downloaded);
            }
        }
    }

    let widget_templates_opt = if widget_templates.is_empty() {
        None
    } else {
        Some(widget_templates)
    };

    // 下载 i18n 翻译文件
    let mut i18n_data: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    if let Some(i18n_files) = download.get("i18n").and_then(|v| v.as_object()) {
        for (lang_code, path) in i18n_files {
            if let Some(i18n_path) = path.as_str() {
                let i18n_url = format!("{}/{}", base_url, i18n_path);
                if let Ok(resp) = fetch_public_store_url(&i18n_url).await {
                    if resp.status().is_success() {
                        if let Ok(json) = resp.json::<serde_json::Value>().await {
                            i18n_data.insert(lang_code.clone(), json);
                        }
                    }
                }
            }
        }
    }
    let i18n_opt = if i18n_data.is_empty() {
        None
    } else {
        Some(i18n_data)
    };

    // 下载 Page 模块文件
    let mut page_modules_data: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    if let Some(pm_files) = download.get("page_modules").and_then(|v| v.as_object()) {
        for (filename, path) in pm_files {
            if let Some(pm_path) = path.as_str() {
                let pm_url = format!("{}/{}", base_url, pm_path);
                if let Ok(resp) = fetch_public_store_url(&pm_url).await {
                    if resp.status().is_success() {
                        if let Ok(content) = resp.text().await {
                            page_modules_data.insert(filename.clone(), content);
                        }
                    }
                }
            }
        }
    }
    let page_modules_opt = if page_modules_data.is_empty() {
        None
    } else {
        Some(page_modules_data)
    };

    Ok(PreparedTappPackage::from_resources(
        manifest,
        PreparedTappResources {
            code,
            styles: styles_content,
            widget_styles: widget_styles_content,
            page_styles: page_styles_content,
            page_template: page_template_content,
            widget_templates: widget_templates_opt,
            i18n: i18n_opt,
            page_modules: page_modules_opt,
            ..PreparedTappResources::default()
        },
    ))
}
