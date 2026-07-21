//! Remote Tapp store package discovery and resource download.

use super::prepared_package::{PreparedTappPackage, PreparedTappResources};
use super::validation::{
    validate_asset_path, MAX_TAPP_ASSETS, MAX_TAPP_ASSETS_TOTAL_BYTES, MAX_TAPP_ASSET_BYTES,
};
use super::{api_error, ApiResponse, TappCategory, TappManifest, WidgetTemplateContents};
use axum::{http::StatusCode, Json};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter};
use std::collections::HashMap;

use crate::models::entities::tapp_store_sources;

/// Package directory on the store host (parent of main.js / manifest.json).
///
/// Example: `apps/com.myriad.doudizhu/main.js` → `apps/com.myriad.doudizhu`
pub(crate) fn store_package_root(code_or_manifest_path: &str) -> String {
    let path = code_or_manifest_path.trim().trim_start_matches('/');
    match path.rfind('/') {
        Some(i) => path[..i].to_string(),
        None => String::new(),
    }
}

/// Store-relative path for a package asset.
///
/// Example: root `apps/com.myriad.doudizhu` + `assets/felt/table_felt.png`
/// → `apps/com.myriad.doudizhu/assets/felt/table_felt.png`
pub(crate) fn store_asset_store_path(package_root: &str, asset_path: &str) -> String {
    let asset = asset_path.trim().trim_start_matches('/');
    let root = package_root.trim().trim_start_matches('/').trim_end_matches('/');
    if root.is_empty() {
        asset.to_string()
    } else {
        format!("{root}/{asset}")
    }
}

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
        let resp = fetch_public_store_url(&page_styles_url).await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                api_error(format!("Failed to fetch page styles: {e}")),
            )
        })?;
        if !resp.status().is_success() {
            return Err((
                StatusCode::BAD_GATEWAY,
                api_error(format!(
                    "Failed to fetch page styles: remote returned {}",
                    resp.status()
                )),
            ));
        }
        page_styles_content = Some(resp.text().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                api_error(format!("Failed to read page styles: {e}")),
            )
        })?);
    } else if manifest.page_styles.is_some() {
        // Manifest declares pageStyles but store index omitted page_styles download path
        return Err((
            StatusCode::BAD_GATEWAY,
            api_error(
                "Store index is missing download.page_styles for a manifest that declares pageStyles",
            ),
        ));
    }

    // 下载 Page 模板
    if let Some(page_path) = download.get("page_template").and_then(|v| v.as_str()) {
        let page_url = format!("{}/{}", base_url, page_path);
        let resp = fetch_public_store_url(&page_url).await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                api_error(format!("Failed to fetch page template: {e}")),
            )
        })?;
        if !resp.status().is_success() {
            return Err((
                StatusCode::BAD_GATEWAY,
                api_error(format!(
                    "Failed to fetch page template: remote returned {}",
                    resp.status()
                )),
            ));
        }
        page_template_content = Some(resp.text().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                api_error(format!("Failed to read page template: {e}")),
            )
        })?);
    } else if manifest.page_template.is_some() {
        return Err((
            StatusCode::BAD_GATEWAY,
            api_error(
                "Store index is missing download.page_template for a manifest that declares pageTemplate",
            ),
        ));
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

    // Package-static binary assets (manifest.assets → base64 map)
    let package_root = store_package_root(code_path);
    let assets_opt = download_store_package_assets(base_url, &package_root, &manifest).await?;

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
            assets: assets_opt,
            ..PreparedTappResources::default()
        },
    ))
}

/// Download every path declared in `manifest.assets` from the store host.
///
/// Returns `None` when the app declares no assets. Fails if a declared asset
/// is missing or invalid so texture packs cannot install half-empty.
async fn download_store_package_assets(
    base_url: &str,
    package_root: &str,
    manifest: &TappManifest,
) -> Result<Option<HashMap<String, String>>, (StatusCode, Json<ApiResponse<()>>)> {
    let Some(declared) = manifest.assets.as_ref() else {
        return Ok(None);
    };
    if declared.is_empty() {
        return Ok(None);
    }
    if declared.len() > MAX_TAPP_ASSETS {
        return Err((
            StatusCode::BAD_GATEWAY,
            api_error(format!(
                "Tapp assets accepts at most {MAX_TAPP_ASSETS} entries (got {})",
                declared.len()
            )),
        ));
    }

    let mut assets: HashMap<String, String> = HashMap::new();
    let mut total: u64 = 0;

    for relative in declared {
        validate_asset_path(relative).map_err(|e| (StatusCode::BAD_GATEWAY, api_error(e)))?;

        let store_rel = store_asset_store_path(package_root, relative);
        let asset_url = format!("{}/{}", base_url.trim_end_matches('/'), store_rel);
        tracing::info!(url = %asset_url, path = %relative, "fetching tapp store asset");

        let resp = fetch_public_store_url(&asset_url).await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                api_error(format!("Failed to fetch asset {relative}: {e}")),
            )
        })?;

        if !resp.status().is_success() {
            return Err((
                StatusCode::BAD_GATEWAY,
                api_error(format!(
                    "Failed to fetch asset {relative}: remote returned {}",
                    resp.status()
                )),
            ));
        }

        let bytes = resp.bytes().await.map_err(|e| {
            (
                StatusCode::BAD_GATEWAY,
                api_error(format!("Failed to read asset {relative}: {e}")),
            )
        })?;

        let size = bytes.len() as u64;
        if size > MAX_TAPP_ASSET_BYTES {
            return Err((
                StatusCode::BAD_GATEWAY,
                api_error(format!(
                    "Tapp asset exceeds {MAX_TAPP_ASSET_BYTES} bytes: {relative}"
                )),
            ));
        }
        total = total.checked_add(size).ok_or_else(|| {
            (
                StatusCode::BAD_GATEWAY,
                api_error("Tapp assets total size overflow"),
            )
        })?;
        if total > MAX_TAPP_ASSETS_TOTAL_BYTES {
            return Err((
                StatusCode::BAD_GATEWAY,
                api_error(format!(
                    "Tapp assets total size exceeds {MAX_TAPP_ASSETS_TOTAL_BYTES} bytes"
                )),
            ));
        }

        assets.insert(relative.clone(), B64.encode(&bytes));
    }

    Ok(Some(assets))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn package_root_from_code_path() {
        assert_eq!(
            store_package_root("apps/com.myriad.doudizhu/main.js"),
            "apps/com.myriad.doudizhu"
        );
        assert_eq!(
            store_package_root("apps/com.myriad.doudizhu/manifest.json"),
            "apps/com.myriad.doudizhu"
        );
        assert_eq!(store_package_root("main.js"), "");
        assert_eq!(store_package_root("/nested/a/b/c.js"), "nested/a/b");
    }

    #[test]
    fn asset_store_path_joins_package_root() {
        assert_eq!(
            store_asset_store_path(
                "apps/com.myriad.doudizhu",
                "assets/felt/table_felt.png"
            ),
            "apps/com.myriad.doudizhu/assets/felt/table_felt.png"
        );
        assert_eq!(
            store_asset_store_path("", "assets/x.png"),
            "assets/x.png"
        );
        assert_eq!(
            store_asset_store_path("apps/foo/", "/assets/x.png"),
            "apps/foo/assets/x.png"
        );
    }
}
