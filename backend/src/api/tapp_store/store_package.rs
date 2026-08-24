//! Remote Tapp store package discovery and resource download.
//!
//! Path/index pure mapping, source resolve, and download plans live in
//! [`crate::services::tapp_store_package`]. This module owns DB source load,
//! outbound HTTP, and status mapping.

use super::prepared_package::{PreparedTappPackage, PreparedTappResources};
use super::{api_http_error, TappManifest, WidgetTemplateContents};
use crate::error::HttpError;
use axum::http::StatusCode;
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use sea_orm::{DatabaseConnection, EntityTrait};
use std::collections::HashMap;

use crate::models::entities::tapp_store_sources;
use crate::services::tapp_store_package::{
    append_store_cache_bust, find_store_app_entry, i18n_downloads, is_invalid_store_source_ref,
    join_store_file_url, module_downloads, nonempty_map_opt, optional_store_text_downloads,
    parse_store_preview_descriptor, prepare_store_catalog_base,
    require_download_page_styles_if_declared, require_download_page_template_if_declared,
    resolve_store_source_among, store_app_download_section, store_asset_download_plan,
    store_download_core_paths, store_index_url, widget_template_downloads, OptionalStoreTextKind,
    StoreSourceRowRef,
};

// Path-stable re-exports for sibling modules / tests.
pub(crate) use crate::services::tapp_store_package::{
    store_package_root, validate_store_manifest_category,
};

/// Append a unique query param so CDN/proxy layers cannot reuse a previous
/// package file (GitHub raw `max-age=300` is a common culprit).
fn with_store_cache_bust(url: &str) -> String {
    let token = format!(
        "{}{}",
        chrono::Utc::now().timestamp_millis(),
        std::process::id()
    );
    append_store_cache_bust(url, &token)
}

/// 从远程商店下载 Tapp 文件
async fn fetch_public_store_url(url: &str) -> Result<reqwest::Response, String> {
    let busted = with_store_cache_bust(url);
    let (target_url, client) = crate::services::outbound_security::build_public_http_client(
        &busted,
        std::time::Duration::from_secs(20),
        Some("Myriad-Tapp-Store/1.0"),
    )
    .await?;
    // Bypass intermediate HTTP caches (GitHub raw max-age=300). Production
    // reinstall/update must not mix a fresh index with stale page.css/html.
    // Server-side requests may set Cache-Control (no browser CORS preflight).
    client
        .get(target_url)
        .header("Cache-Control", "no-cache, no-store, must-revalidate")
        .header("Pragma", "no-cache")
        .send()
        .await
        .map_err(|error| error.to_string())
}

pub(super) async fn fetch_from_store(
    db: &DatabaseConnection,
    store_source: &str,
    tapp_id: &str,
) -> Result<PreparedTappPackage, HttpError> {
    // Reject install-mode placeholders mistaken for catalog refs (Aro legacy bug).
    let trimmed = store_source.trim();
    if is_invalid_store_source_ref(trimmed) {
        return Err(api_http_error(
            StatusCode::BAD_REQUEST,
            "Invalid storeSource: expected catalog URL or store source id, not install mode",
        ));
    }

    // Load configured sources; domain resolves by exact URL / id / normalized base.
    let all_sources = tapp_store_sources::Entity::find()
        .all(db)
        .await
        .map_err(|_| api_http_error(StatusCode::INTERNAL_SERVER_ERROR, "Database error"))?;
    let resolved = resolve_store_source_among(
        all_sources
            .iter()
            .map(|s| StoreSourceRowRef {
                id: s.id,
                url: s.url.as_str(),
            }),
        trimmed,
    )
    .ok_or_else(|| {
        api_http_error(StatusCode::NOT_FOUND, format!(
                "Store source not found for '{}'. Add this catalog URL in Tapp Store settings (official Myriad store is pre-seeded).",
                trimmed
            ))
    })?;
    let source = all_sources
        .iter()
        .find(|s| s.id == resolved.id)
        .expect("resolved id must exist in loaded rows");

    let base_url = prepare_store_catalog_base(&source.url)
        .map_err(|msg| api_http_error(StatusCode::BAD_REQUEST, msg))?;

    // 获取商店索引
    // 注意：生产环境中 backend 容器若无法访问外网（尤其 raw.githubusercontent.com），
    // 这里会返回 502。前端商店列表走浏览器直连，因此可能出现「能浏览、不能安装」。
    let index_url = store_index_url(&base_url);
    tracing::info!(url = %index_url, "fetching tapp store index");
    let index_resp = fetch_public_store_url(&index_url).await.map_err(|e| {
        tracing::error!(url = %index_url, error = %e, "failed to fetch store index");
        api_http_error(StatusCode::BAD_GATEWAY, "Upstream fetch failed")
    })?;

    if !index_resp.status().is_success() {
        let status = index_resp.status();
        tracing::error!(url = %index_url, %status, "store index returned non-success");
        return Err(api_http_error(
            StatusCode::BAD_GATEWAY,
            format!("Failed to fetch store index: remote returned {}", status),
        ));
    }

    let index: serde_json::Value = index_resp.json().await.map_err(|e| {
        tracing::error!(error = %e, "upstream fetch failed");
        api_http_error(StatusCode::BAD_GATEWAY, "Upstream fetch failed")
    })?;

    // 在商店中查找指定的 Tapp
    let app_info = find_store_app_entry(&index, tapp_id).map_err(|err| {
        let status = match &err {
            crate::services::tapp_store_package::StoreIndexError::AppNotFound { .. } => {
                StatusCode::NOT_FOUND
            }
            _ => StatusCode::BAD_GATEWAY,
        };
        api_http_error(status, err.message())
    })?;

    if let Err(error) = parse_store_preview_descriptor(app_info) {
        tracing::warn!(tapp_id, %error, "ignoring invalid optional store preview metadata");
    }

    let download = store_app_download_section(app_info)
        .map_err(|err| api_http_error(StatusCode::BAD_GATEWAY, err.message()))?;

    // 下载 manifest.json
    let (manifest_path, code_path) = store_download_core_paths(download)
        .map_err(|err| api_http_error(StatusCode::BAD_GATEWAY, err.message()))?;
    let manifest_url = join_store_file_url(&base_url, manifest_path);

    let manifest_resp = fetch_public_store_url(&manifest_url).await.map_err(|e| {
        tracing::error!(error = %e, "upstream fetch failed");
        api_http_error(StatusCode::BAD_GATEWAY, "Upstream fetch failed")
    })?;

    let manifest: TappManifest = manifest_resp.json().await.map_err(|e| {
        tracing::error!(error = %e, "upstream fetch failed");
        api_http_error(StatusCode::BAD_GATEWAY, "Upstream fetch failed")
    })?;
    validate_store_manifest_category(app_info, &manifest)
        .map_err(|error| api_http_error(StatusCode::BAD_GATEWAY, error))?;

    // 下载主代码
    let code_url = join_store_file_url(&base_url, code_path);

    let code = fetch_public_store_url(&code_url)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "upstream fetch failed");
            api_http_error(StatusCode::BAD_GATEWAY, "Upstream fetch failed")
        })?
        .text()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "upstream fetch failed");
            api_http_error(StatusCode::BAD_GATEWAY, "Upstream fetch failed")
        })?;

    // 下载可选资源
    let mut styles_content: Option<String> = None;
    let mut widget_styles_content: Option<String> = None;
    let mut page_styles_content: Option<String> = None;
    let mut page_template_content: Option<String> = None;
    let mut widget_templates: WidgetTemplateContents = std::collections::HashMap::new();

    // Optional best-effort styles / widget_styles (domain plan).
    for item in optional_store_text_downloads(download) {
        let url = join_store_file_url(&base_url, &item.path);
        if let Ok(resp) = fetch_public_store_url(&url).await {
            if resp.status().is_success() {
                if let Ok(content) = resp.text().await {
                    match item.kind {
                        OptionalStoreTextKind::Styles => styles_content = Some(content),
                        OptionalStoreTextKind::WidgetStyles => {
                            widget_styles_content = Some(content)
                        }
                    }
                }
            }
        }
    }

    // 下载 Page 专用 CSS（分离模式）
    if let Some(page_styles_path) = require_download_page_styles_if_declared(download, &manifest)
        .map_err(|error| api_http_error(StatusCode::BAD_GATEWAY, error))?
    {
        let page_styles_url = join_store_file_url(&base_url, page_styles_path);
        let resp = fetch_public_store_url(&page_styles_url)
            .await
            .map_err(|e| {
                tracing::error!(error = %e, "upstream fetch failed");
                api_http_error(StatusCode::BAD_GATEWAY, "Upstream fetch failed")
            })?;
        if !resp.status().is_success() {
            return Err(api_http_error(
                StatusCode::BAD_GATEWAY,
                format!(
                    "Failed to fetch page styles: remote returned {}",
                    resp.status()
                ),
            ));
        }
        page_styles_content = Some(resp.text().await.map_err(|e| {
            tracing::error!(error = %e, "upstream fetch failed");
            api_http_error(StatusCode::BAD_GATEWAY, "Upstream fetch failed")
        })?);
    }

    // 下载 Page 模板
    if let Some(page_path) = require_download_page_template_if_declared(download, &manifest)
        .map_err(|error| api_http_error(StatusCode::BAD_GATEWAY, error))?
    {
        let page_url = join_store_file_url(&base_url, page_path);
        let resp = fetch_public_store_url(&page_url).await.map_err(|e| {
            tracing::error!(error = %e, "upstream fetch failed");
            api_http_error(StatusCode::BAD_GATEWAY, "Upstream fetch failed")
        })?;
        if !resp.status().is_success() {
            return Err(api_http_error(
                StatusCode::BAD_GATEWAY,
                format!(
                    "Failed to fetch page template: remote returned {}",
                    resp.status()
                ),
            ));
        }
        page_template_content = Some(resp.text().await.map_err(|e| {
            tracing::error!(error = %e, "upstream fetch failed");
            api_http_error(StatusCode::BAD_GATEWAY, "Upstream fetch failed")
        })?);
    }

    // Widget templates / i18n / page modules — domain flattens nested index maps.
    for entry in widget_template_downloads(download) {
        let template_url = join_store_file_url(&base_url, &entry.path);
        if let Ok(resp) = fetch_public_store_url(&template_url).await {
            if resp.status().is_success() {
                if let Ok(content) = resp.text().await {
                    widget_templates
                        .entry(entry.widget_id)
                        .or_default()
                        .insert(entry.size, content);
                }
            }
        }
    }
    let widget_templates_opt = nonempty_map_opt(widget_templates);

    let mut i18n_data: std::collections::HashMap<String, serde_json::Value> =
        std::collections::HashMap::new();
    for entry in i18n_downloads(download) {
        let i18n_url = join_store_file_url(&base_url, &entry.path);
        if let Ok(resp) = fetch_public_store_url(&i18n_url).await {
            if resp.status().is_success() {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    i18n_data.insert(entry.key, json);
                }
            }
        }
    }
    let i18n_opt = nonempty_map_opt(i18n_data);

    // Package-static binary assets (manifest.assets → base64 map)
    let package_root = store_package_root(code_path);
    let assets_opt = download_store_package_assets(&base_url, &package_root, &manifest).await?;

    // `download.code` 是 core 入口；`download.modules` 覆盖其余层入口与层内文件，
    // key 就是包内相对路径。声明的层入口必须齐全，缺了要立刻失败而不是装个半成品。
    let mut modules: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    if let Some(core_entry) = manifest.core.as_ref().map(|core| core.entry.clone()) {
        modules.insert(core_entry, code);
    }
    for entry in module_downloads(download) {
        let module_url = join_store_file_url(&base_url, &entry.path);
        let resp = fetch_public_store_url(&module_url).await.map_err(|e| {
            tracing::error!(error = %e, "upstream fetch failed");
            api_http_error(StatusCode::BAD_GATEWAY, "Upstream fetch failed")
        })?;
        if !resp.status().is_success() {
            return Err(api_http_error(
                StatusCode::BAD_GATEWAY,
                format!(
                    "Failed to fetch module {}: remote returned {}",
                    entry.key,
                    resp.status()
                ),
            ));
        }
        let content = resp.text().await.map_err(|e| {
            tracing::error!(error = %e, "upstream fetch failed");
            api_http_error(StatusCode::BAD_GATEWAY, "Upstream fetch failed")
        })?;
        modules.insert(entry.key, content);
    }
    for entry in manifest.layer_entries() {
        if !modules.contains_key(entry) {
            return Err(api_http_error(
                StatusCode::BAD_GATEWAY,
                format!(
                    "Store index is missing download.modules entry for declared layer entry {entry}"
                ),
            ));
        }
    }

    // 索引里的 widget_styles 是单份文件，复制给每个声明了 styles 的 widget。
    let widget_styles_opt = widget_styles_content.and_then(|content| {
        let styles: std::collections::HashMap<String, String> = manifest
            .widgets
            .iter()
            .flatten()
            .filter(|widget| widget.styles.is_some())
            .map(|widget| (widget.id.clone(), content.clone()))
            .collect();
        nonempty_map_opt(styles)
    });

    Ok(PreparedTappPackage::from_resources(
        manifest,
        PreparedTappResources {
            modules,
            core_styles: styles_content,
            widget_styles: widget_styles_opt,
            page_styles: page_styles_content,
            page_template: page_template_content,
            widget_templates: widget_templates_opt,
            i18n: i18n_opt,
            assets: assets_opt,
            ..PreparedTappResources::default()
        },
    ))
}

/// Download every path declared in `manifest.assets` from the store host.
///
/// Returns `None` when the app declares no assets. Fails if a declared asset
/// is missing or invalid so texture packs cannot install half-empty.
/// Domain builds the plan + size budget; this layer only performs HTTP + base64.
async fn download_store_package_assets(
    base_url: &str,
    package_root: &str,
    manifest: &TappManifest,
) -> Result<Option<HashMap<String, String>>, HttpError> {
    let max_assets = if manifest.uses_game_asset_limits() {
        crate::services::tapp_validation::MAX_TAPP_GAME_ASSETS
    } else {
        crate::services::tapp_validation::MAX_TAPP_ASSETS
    };
    let plan = store_asset_download_plan(
        base_url,
        package_root,
        manifest.assets.as_deref(),
        max_assets,
    )
    .map_err(|error| api_http_error(StatusCode::BAD_GATEWAY, error))?;
    if plan.is_empty() {
        return Ok(None);
    }

    let mut assets: HashMap<String, String> = HashMap::new();
    let mut total: u64 = 0;

    for entry in plan {
        let relative = &entry.relative;
        tracing::info!(url = %entry.url, path = %relative, "fetching tapp store asset");

        let resp = fetch_public_store_url(&entry.url).await.map_err(|e| {
            tracing::error!(error = %e, "upstream fetch failed");
            api_http_error(StatusCode::BAD_GATEWAY, "Upstream fetch failed")
        })?;

        if !resp.status().is_success() {
            return Err(api_http_error(
                StatusCode::BAD_GATEWAY,
                format!(
                    "Failed to fetch asset {relative}: remote returned {}",
                    resp.status()
                ),
            ));
        }

        let bytes = resp.bytes().await.map_err(|e| {
            tracing::error!(error = %e, "upstream fetch failed");
            api_http_error(StatusCode::BAD_GATEWAY, "Upstream fetch failed")
        })?;

        total = crate::services::tapp_install_resources::validate_asset_resource_bytes_with(
            relative,
            bytes.len() as u64,
            total,
            crate::services::tapp_install_resources::AssetBudget::for_manifest(manifest),
        )
        .map_err(|e| api_http_error(StatusCode::BAD_GATEWAY, e))?;

        assets.insert(relative.clone(), B64.encode(&bytes));
    }

    Ok(Some(assets))
}

#[cfg(test)]
mod tests {
    use super::with_store_cache_bust;
    use crate::services::tapp_store_package::{store_asset_store_path, store_package_root};

    #[test]
    fn package_root_from_code_path() {
        assert_eq!(
            store_package_root("apps/com.myriad.doudizhu/core.js"),
            "apps/com.myriad.doudizhu"
        );
        assert_eq!(
            store_package_root("apps/com.myriad.doudizhu/manifest.json"),
            "apps/com.myriad.doudizhu"
        );
        assert_eq!(store_package_root("core.js"), "");
        assert_eq!(store_package_root("/nested/a/b/c.js"), "nested/a/b");
    }

    #[test]
    fn asset_store_path_joins_package_root() {
        assert_eq!(
            store_asset_store_path("apps/com.myriad.doudizhu", "assets/felt/table_felt.png"),
            "apps/com.myriad.doudizhu/assets/felt/table_felt.png"
        );
        assert_eq!(store_asset_store_path("", "assets/x.png"), "assets/x.png");
        assert_eq!(
            store_asset_store_path("apps/foo/", "/assets/x.png"),
            "apps/foo/assets/x.png"
        );
    }

    #[test]
    fn cache_bust_appends_query() {
        let a = with_store_cache_bust("https://example.com/a.json");
        assert!(a.contains("?_myriad_cb="), "{a}");
        let b = with_store_cache_bust("https://example.com/a.json?x=1");
        assert!(b.contains("&_myriad_cb="), "{b}");
        assert!(b.contains("x=1"), "{b}");
    }
}
