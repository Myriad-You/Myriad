//! Remote Tapp store package path mapping and index pure rules.
//!
//! HTTP fetch / DB store-source lookup stay in the API layer. This module owns
//! catalog URL normalization, package-relative path joins, cache-bust query
//! shaping, host allow rules, and index entry discovery so install mapping can
//! be unit-tested without network.

use myriad_tapp_contract::manifest::{TappCategory, TappManifest};

use crate::services::tapp_validation::MAX_TAPP_ASSETS;

/// Package directory on the store host (parent of main.js / manifest.json).
///
/// Example: `apps/com.myriad.doudizhu/main.js` → `apps/com.myriad.doudizhu`
pub fn store_package_root(code_or_manifest_path: &str) -> String {
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
pub fn store_asset_store_path(package_root: &str, asset_path: &str) -> String {
    let asset = asset_path.trim().trim_start_matches('/');
    let root = package_root
        .trim()
        .trim_start_matches('/')
        .trim_end_matches('/');
    if root.is_empty() {
        asset.to_string()
    } else {
        format!("{root}/{asset}")
    }
}

/// Join catalog base URL with a store-relative file path.
pub fn join_store_file_url(base_url: &str, relative_path: &str) -> String {
    let base = base_url.trim().trim_end_matches('/');
    let rel = relative_path.trim().trim_start_matches('/');
    format!("{base}/{rel}")
}

/// Catalog base derived from a stored source URL (`…/index.json` stripped).
pub fn store_catalog_base_url(source_url: &str) -> String {
    source_url
        .trim()
        .trim_end_matches('/')
        .trim_end_matches("/index.json")
        .trim_end_matches('/')
        .to_string()
}

/// Normalize catalog URL for matching: strip trailing slash and optional `/index.json`.
pub fn normalize_store_catalog_url(url: &str) -> String {
    store_catalog_base_url(url)
}

/// Append a cache-bust query token so CDN layers cannot reuse stale package files.
pub fn append_store_cache_bust(url: &str, token: &str) -> String {
    if url.contains('?') {
        format!("{url}&_myriad_cb={token}")
    } else {
        format!("{url}?_myriad_cb={token}")
    }
}

/// Reject install-mode placeholders mistaken for catalog refs (Aro legacy bug).
pub fn is_invalid_store_source_ref(store_source: &str) -> bool {
    let trimmed = store_source.trim();
    trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("store")
        || trimmed.eq_ignore_ascii_case("direct")
}

/// Hosts that must never be used as remote store catalogs from the backend.
pub fn is_disallowed_store_host(host: &str) -> bool {
    host == "localhost"
        || host == "127.0.0.1"
        || host == "::1"
        || host.starts_with("10.")
        || host.starts_with("172.16.")
        || host.starts_with("192.168.")
        || host == "0.0.0.0"
        || host.ends_with(".local")
        || host.ends_with(".internal")
}

/// Scheme allow-list for store catalog base URLs.
pub fn is_allowed_store_url_scheme(scheme: &str) -> bool {
    scheme == "https" || scheme == "http"
}

/// Store index `category` must match the downloaded manifest category.
pub fn validate_store_manifest_category(
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

pub const STORE_PREVIEW_MIN_WIDTH: u32 = 1280;
pub const STORE_PREVIEW_MIN_HEIGHT: u32 = 720;
pub const STORE_PREVIEW_MAX_WIDTH: u32 = 3840;
pub const STORE_PREVIEW_MAX_HEIGHT: u32 = 2160;
pub const STORE_PREVIEW_MAX_STYLES: usize = 8;

#[derive(Debug, Clone, PartialEq)]
pub struct StorePreviewDescriptor {
    pub html: String,
    pub styles: Vec<String>,
    pub width: u32,
    pub height: u32,
    pub fit: String,
    pub focus_x: f64,
    pub focus_y: f64,
    pub theme: String,
}

fn preview_dimension(value: Option<u64>, fallback: u32, min: u32, max: u32) -> u32 {
    value
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(fallback)
        .clamp(min, max)
}

fn preview_focus(value: Option<f64>) -> f64 {
    value
        .filter(|value| value.is_finite())
        .unwrap_or(0.5)
        .clamp(0.0, 1.0)
}

/// Parse optional, store-only static preview metadata.
///
/// Preview metadata is never part of the installable Manifest and malformed
/// metadata must not prevent installation. Callers may warn and ignore errors.
pub fn parse_store_preview_descriptor(
    app_info: &serde_json::Value,
) -> Result<Option<StorePreviewDescriptor>, String> {
    let Some(raw) = app_info.get("preview") else {
        return Ok(None);
    };
    let preview = raw
        .as_object()
        .ok_or_else(|| "Store preview must be an object".to_string())?;
    if preview.get("version").and_then(|value| value.as_u64()) != Some(1) {
        return Err("Store preview version must be 1".to_string());
    }
    if preview.get("type").and_then(|value| value.as_str()) != Some("snapshot") {
        return Err("Store preview type must be snapshot".to_string());
    }
    let html = preview
        .get("html")
        .and_then(|value| value.as_str())
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| "Store preview html path is required".to_string())?
        .to_string();

    let styles = preview
        .get("styles")
        .and_then(|value| value.as_array())
        .map(|values| {
            values
                .iter()
                .filter_map(|value| value.as_str())
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .take(STORE_PREVIEW_MAX_STYLES)
                .fold(Vec::<String>::new(), |mut paths, value| {
                    if !paths.iter().any(|path| path == value) {
                        paths.push(value.to_string());
                    }
                    paths
                })
        })
        .unwrap_or_default();
    let viewport = preview.get("viewport").and_then(|value| value.as_object());
    let focus = preview.get("focus").and_then(|value| value.as_object());
    let fit = match preview.get("fit").and_then(|value| value.as_str()) {
        Some("contain") => "contain",
        _ => "cover",
    };
    let theme = match preview.get("theme").and_then(|value| value.as_str()) {
        Some("light") => "light",
        Some("dark") => "dark",
        _ => "auto",
    };

    Ok(Some(StorePreviewDescriptor {
        html,
        styles,
        width: preview_dimension(
            viewport
                .and_then(|value| value.get("width"))
                .and_then(|value| value.as_u64()),
            STORE_PREVIEW_MIN_WIDTH,
            STORE_PREVIEW_MIN_WIDTH,
            STORE_PREVIEW_MAX_WIDTH,
        ),
        height: preview_dimension(
            viewport
                .and_then(|value| value.get("height"))
                .and_then(|value| value.as_u64()),
            STORE_PREVIEW_MIN_HEIGHT,
            STORE_PREVIEW_MIN_HEIGHT,
            STORE_PREVIEW_MAX_HEIGHT,
        ),
        fit: fit.to_string(),
        focus_x: preview_focus(
            focus
                .and_then(|value| value.get("x"))
                .and_then(|value| value.as_f64()),
        ),
        focus_y: preview_focus(
            focus
                .and_then(|value| value.get("y"))
                .and_then(|value| value.as_f64()),
        ),
        theme: theme.to_string(),
    }))
}

/// Errors while reading a store `index.json` structure (before network status).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreIndexError {
    MissingAppsArray,
    AppNotFound { tapp_id: String },
    MissingDownloadInfo,
    MissingManifestPath,
    MissingCodePath,
}

impl StoreIndexError {
    pub fn message(&self) -> String {
        match self {
            Self::MissingAppsArray => "Invalid store index: no apps array".to_string(),
            Self::AppNotFound { tapp_id } => format!("Tapp {tapp_id} not found in store"),
            Self::MissingDownloadInfo => "No download info in app".to_string(),
            Self::MissingManifestPath => "No manifest path".to_string(),
            Self::MissingCodePath => "No code path".to_string(),
        }
    }
}

/// Locate one app entry in a store index document.
pub fn find_store_app_entry<'a>(
    index: &'a serde_json::Value,
    tapp_id: &str,
) -> Result<&'a serde_json::Value, StoreIndexError> {
    let apps = index
        .get("apps")
        .and_then(|v| v.as_array())
        .ok_or(StoreIndexError::MissingAppsArray)?;
    apps.iter()
        .find(|app| app.get("id").and_then(|v| v.as_str()) == Some(tapp_id))
        .ok_or_else(|| StoreIndexError::AppNotFound {
            tapp_id: tapp_id.to_string(),
        })
}

/// Extract the `download` object from a store app entry.
pub fn store_app_download_section(
    app_info: &serde_json::Value,
) -> Result<&serde_json::Value, StoreIndexError> {
    app_info
        .get("download")
        .ok_or(StoreIndexError::MissingDownloadInfo)
}

/// Required `download.manifest` / `download.code` relative paths.
pub fn store_download_core_paths(
    download: &serde_json::Value,
) -> Result<(&str, &str), StoreIndexError> {
    let manifest_path = download
        .get("manifest")
        .and_then(|v| v.as_str())
        .ok_or(StoreIndexError::MissingManifestPath)?;
    let code_path = download
        .get("code")
        .and_then(|v| v.as_str())
        .ok_or(StoreIndexError::MissingCodePath)?;
    Ok((manifest_path, code_path))
}

/// When the manifest declares pageStyles, the store index must list download.page_styles.
pub fn require_download_page_styles_if_declared<'a>(
    download: &'a serde_json::Value,
    manifest: &TappManifest,
) -> Result<Option<&'a str>, String> {
    match (
        download.get("page_styles").and_then(|v| v.as_str()),
        manifest.page_styles.is_some(),
    ) {
        (Some(path), _) => Ok(Some(path)),
        (None, true) => Err(
            "Store index is missing download.page_styles for a manifest that declares pageStyles"
                .to_string(),
        ),
        (None, false) => Ok(None),
    }
}

/// When the manifest declares pageTemplate, the store index must list download.page_template.
pub fn require_download_page_template_if_declared<'a>(
    download: &'a serde_json::Value,
    manifest: &TappManifest,
) -> Result<Option<&'a str>, String> {
    match (
        download.get("page_template").and_then(|v| v.as_str()),
        manifest.page_template.is_some(),
    ) {
        (Some(path), _) => Ok(Some(path)),
        (None, true) => Err(
            "Store index is missing download.page_template for a manifest that declares pageTemplate"
                .to_string(),
        ),
        (None, false) => Ok(None),
    }
}

/// Bound the declared assets list before download (store-side contract).
pub fn validate_store_declared_assets_count(declared_len: usize) -> Result<(), String> {
    validate_store_declared_assets_count_max(declared_len, MAX_TAPP_ASSETS)
}

pub fn validate_store_declared_assets_count_max(
    declared_len: usize,
    max_assets: usize,
) -> Result<(), String> {
    if declared_len > max_assets {
        return Err(format!(
            "Tapp assets accepts at most {max_assets} entries (got {declared_len})"
        ));
    }
    Ok(())
}

/// Whether two catalog URLs refer to the same store source (normalized).
pub fn store_catalog_urls_match(a: &str, b: &str) -> bool {
    normalize_store_catalog_url(a) == normalize_store_catalog_url(b)
}

// ── Download orchestration path plans ───────────────────────────────────────

/// Optional best-effort text resource from the store `download` object.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OptionalStoreTextKind {
    Styles,
    WidgetStyles,
}

/// One optional text download (styles / widget_styles). Missing or failed
/// fetches are ignored by the HTTP layer (best-effort).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OptionalStoreTextDownload {
    pub kind: OptionalStoreTextKind,
    pub path: String,
}

/// Optional styles / widget_styles paths for best-effort fetch.
pub fn optional_store_text_downloads(
    download: &serde_json::Value,
) -> Vec<OptionalStoreTextDownload> {
    let mut out = Vec::new();
    if let Some(path) = download.get("styles").and_then(|v| v.as_str()) {
        out.push(OptionalStoreTextDownload {
            kind: OptionalStoreTextKind::Styles,
            path: path.to_string(),
        });
    }
    if let Some(path) = download.get("widget_styles").and_then(|v| v.as_str()) {
        out.push(OptionalStoreTextDownload {
            kind: OptionalStoreTextKind::WidgetStyles,
            path: path.to_string(),
        });
    }
    out
}

/// Widget template path entry: widget_id + size + store-relative path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WidgetTemplateDownload {
    pub widget_id: String,
    pub size: String,
    pub path: String,
}

/// Flatten `download.widget_templates` into fetchable path entries.
pub fn widget_template_downloads(download: &serde_json::Value) -> Vec<WidgetTemplateDownload> {
    let Some(widgets) = download.get("widget_templates").and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (widget_id, templates) in widgets {
        let Some(templates) = templates.as_object() else {
            continue;
        };
        for (size, path) in templates {
            if let Some(template_path) = path.as_str() {
                out.push(WidgetTemplateDownload {
                    widget_id: widget_id.clone(),
                    size: size.clone(),
                    path: template_path.to_string(),
                });
            }
        }
    }
    out
}

/// Nested string→path map entries (`i18n`, `page_modules`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedPathDownload {
    pub key: String,
    pub path: String,
}

fn named_path_map_downloads(download: &serde_json::Value, field: &str) -> Vec<NamedPathDownload> {
    let Some(map) = download.get(field).and_then(|v| v.as_object()) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, path) in map {
        if let Some(path) = path.as_str() {
            out.push(NamedPathDownload {
                key: key.clone(),
                path: path.to_string(),
            });
        }
    }
    out
}

/// `download.i18n` language → path entries.
pub fn i18n_downloads(download: &serde_json::Value) -> Vec<NamedPathDownload> {
    named_path_map_downloads(download, "i18n")
}

/// `download.page_modules` filename → path entries.
pub fn page_module_downloads(download: &serde_json::Value) -> Vec<NamedPathDownload> {
    named_path_map_downloads(download, "page_modules")
}

/// Collapse a map into `None` when empty (install payload convention).
pub fn nonempty_map_opt<K, V>(map: std::collections::HashMap<K, V>) -> Option<std::collections::HashMap<K, V>> {
    if map.is_empty() {
        None
    } else {
        Some(map)
    }
}

// ── Source resolve + fetch orchestration plans ──────────────────────────────

/// Lightweight view of a configured store source row for pure matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StoreSourceRowRef<'a> {
    pub id: i32,
    pub url: &'a str,
}

/// Resolve `storeSource` against known rows.
///
/// Preference order (matches historical install behavior):
/// 1. exact URL equality
/// 2. numeric source id
/// 3. normalized catalog base (`…/index.json` vs bare path)
pub fn resolve_store_source_among<'a>(
    rows: impl IntoIterator<Item = StoreSourceRowRef<'a>>,
    store_source_ref: &str,
) -> Option<StoreSourceRowRef<'a>> {
    let trimmed = store_source_ref.trim();
    if is_invalid_store_source_ref(trimmed) {
        return None;
    }

    let rows: Vec<StoreSourceRowRef<'a>> = rows.into_iter().collect();

    if let Some(row) = rows.iter().copied().find(|r| r.url == trimmed) {
        return Some(row);
    }
    if let Ok(want_id) = trimmed.parse::<i32>() {
        if let Some(row) = rows.iter().copied().find(|r| r.id == want_id) {
            return Some(row);
        }
    }
    let want = normalize_store_catalog_url(trimmed);
    rows.into_iter()
        .find(|r| normalize_store_catalog_url(r.url) == want)
}

/// Catalog `index.json` absolute URL for a store base.
pub fn store_index_url(base_url: &str) -> String {
    join_store_file_url(base_url, "index.json")
}

/// Validate a resolved catalog base before outbound fetch.
///
/// Unparseable URLs are accepted (legacy: validation was skipped when parse failed).
/// Parsed URLs must be HTTP(S) and not point at internal hosts.
pub fn validate_store_fetch_base_url(base_url: &str) -> Result<(), String> {
    let Ok(parsed) = reqwest::Url::parse(base_url.trim()) else {
        return Ok(());
    };
    if !is_allowed_store_url_scheme(parsed.scheme()) {
        return Err("Only HTTP(S) URLs are allowed".to_string());
    }
    if parsed.host_str().is_some_and(is_disallowed_store_host) {
        return Err("Internal network URLs are not allowed".to_string());
    }
    Ok(())
}

/// Derive catalog base from a stored source URL and validate it for fetch.
pub fn prepare_store_catalog_base(source_url: &str) -> Result<String, String> {
    let base = store_catalog_base_url(source_url);
    validate_store_fetch_base_url(&base)?;
    Ok(base)
}

/// One declared package asset ready to download from the store host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreAssetDownload {
    /// Manifest-relative path (install map key), e.g. `assets/icon.png`.
    pub relative: String,
    /// Store-relative path under the package root.
    pub store_path: String,
    /// Absolute URL on the store host.
    pub url: String,
}

/// Build the asset download plan from `manifest.assets` (no HTTP).
///
/// Returns an empty vector when assets are absent or empty. Fails closed on
/// count / path validation so the HTTP layer only loops over safe entries.
pub fn store_asset_download_plan(
    base_url: &str,
    package_root: &str,
    declared: Option<&[String]>,
    max_assets: usize,
) -> Result<Vec<StoreAssetDownload>, String> {
    use crate::services::tapp_validation::validate_asset_path;

    let Some(declared) = declared else {
        return Ok(Vec::new());
    };
    if declared.is_empty() {
        return Ok(Vec::new());
    }
    validate_store_declared_assets_count_max(declared.len(), max_assets)?;

    let mut plan = Vec::with_capacity(declared.len());
    for relative in declared {
        validate_asset_path(relative)?;
        let store_path = store_asset_store_path(package_root, relative);
        let url = join_store_file_url(base_url, &store_path);
        plan.push(StoreAssetDownload {
            relative: relative.clone(),
            store_path,
            url,
        });
    }
    Ok(plan)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
        let a = append_store_cache_bust("https://example.com/a.json", "tok");
        assert_eq!(a, "https://example.com/a.json?_myriad_cb=tok");
        let b = append_store_cache_bust("https://example.com/a.json?x=1", "tok");
        assert_eq!(b, "https://example.com/a.json?x=1&_myriad_cb=tok");
    }

    #[test]
    fn catalog_url_normalization_strips_index_json() {
        assert_eq!(
            normalize_store_catalog_url("https://ex.com/store/index.json"),
            "https://ex.com/store"
        );
        assert_eq!(
            normalize_store_catalog_url("https://ex.com/store/"),
            "https://ex.com/store"
        );
        assert!(store_catalog_urls_match(
            "https://ex.com/store/index.json",
            "https://ex.com/store/"
        ));
        assert_eq!(
            store_catalog_base_url("https://ex.com/store/index.json"),
            "https://ex.com/store"
        );
        assert_eq!(
            join_store_file_url("https://ex.com/store/", "apps/a/main.js"),
            "https://ex.com/store/apps/a/main.js"
        );
    }

    #[test]
    fn invalid_store_source_placeholders() {
        assert!(is_invalid_store_source_ref(""));
        assert!(is_invalid_store_source_ref(" store "));
        assert!(is_invalid_store_source_ref("DIRECT"));
        assert!(!is_invalid_store_source_ref("https://ex.com/store/index.json"));
        assert!(!is_invalid_store_source_ref("12"));
    }

    #[test]
    fn disallowed_store_hosts() {
        assert!(is_disallowed_store_host("localhost"));
        assert!(is_disallowed_store_host("127.0.0.1"));
        assert!(is_disallowed_store_host("10.0.0.1"));
        assert!(is_disallowed_store_host("192.168.1.1"));
        assert!(is_disallowed_store_host("svc.local"));
        assert!(!is_disallowed_store_host("raw.githubusercontent.com"));
        assert!(is_allowed_store_url_scheme("https"));
        assert!(!is_allowed_store_url_scheme("ftp"));
    }

    #[test]
    fn find_store_app_and_download_paths() {
        let index = json!({
            "apps": [{
                "id": "com.example.app",
                "category": "utility",
                "download": {
                    "manifest": "apps/com.example.app/manifest.json",
                    "code": "apps/com.example.app/main.js",
                    "page_styles": "apps/com.example.app/page.css"
                }
            }]
        });
        let app = find_store_app_entry(&index, "com.example.app").unwrap();
        let download = store_app_download_section(app).unwrap();
        let (manifest_path, code_path) = store_download_core_paths(download).unwrap();
        assert_eq!(manifest_path, "apps/com.example.app/manifest.json");
        assert_eq!(code_path, "apps/com.example.app/main.js");

        assert!(matches!(
            find_store_app_entry(&index, "missing").unwrap_err(),
            StoreIndexError::AppNotFound { .. }
        ));
        assert_eq!(
            find_store_app_entry(&json!({}), "x").unwrap_err(),
            StoreIndexError::MissingAppsArray
        );
    }

    #[test]
    fn optional_store_preview_is_parsed_and_bounded() {
        let app = json!({
            "preview": {
                "version": 1,
                "type": "snapshot",
                "html": "apps/com.example/preview.html",
                "styles": ["apps/com.example/page.css", "apps/com.example/page.css"],
                "viewport": { "width": 320, "height": 99999 },
                "fit": "contain",
                "focus": { "x": -1.0, "y": 2.0 },
                "theme": "dark"
            }
        });
        let preview = parse_store_preview_descriptor(&app).unwrap().unwrap();
        assert_eq!(preview.html, "apps/com.example/preview.html");
        assert_eq!(preview.styles, vec!["apps/com.example/page.css"]);
        assert_eq!(preview.width, STORE_PREVIEW_MIN_WIDTH);
        assert_eq!(preview.height, STORE_PREVIEW_MAX_HEIGHT);
        assert_eq!(preview.fit, "contain");
        assert_eq!(preview.focus_x, 0.0);
        assert_eq!(preview.focus_y, 1.0);
        assert_eq!(preview.theme, "dark");

        assert!(parse_store_preview_descriptor(&json!({}))
            .unwrap()
            .is_none());
        assert!(parse_store_preview_descriptor(&json!({
            "preview": { "version": 2, "type": "snapshot", "html": "preview.html" }
        }))
        .is_err());
    }

    #[test]
    fn required_download_paths_follow_manifest_declarations() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.app",
            "name": "App",
            "version": "1.0.0",
            "main": "main.js",
            "category": "utility",
            "permissions": [],
            "pageStyles": "page.css",
            "pageTemplate": "page.html"
        }))
        .unwrap();
        let download_ok = json!({
            "page_styles": "apps/a/page.css",
            "page_template": "apps/a/page.html"
        });
        assert_eq!(
            require_download_page_styles_if_declared(&download_ok, &manifest).unwrap(),
            Some("apps/a/page.css")
        );
        assert_eq!(
            require_download_page_template_if_declared(&download_ok, &manifest).unwrap(),
            Some("apps/a/page.html")
        );
        let download_missing = json!({});
        assert!(require_download_page_styles_if_declared(&download_missing, &manifest)
            .unwrap_err()
            .contains("page_styles"));
        assert!(
            require_download_page_template_if_declared(&download_missing, &manifest)
                .unwrap_err()
                .contains("page_template")
        );
    }

    #[test]
    fn store_manifest_category_must_match_index() {
        let manifest: TappManifest = serde_json::from_value(json!({
            "id": "com.example.app",
            "name": "App",
            "version": "1.0.0",
            "main": "main.js",
            "category": "music",
            "permissions": []
        }))
        .unwrap();
        assert!(validate_store_manifest_category(
            &json!({ "category": "music" }),
            &manifest
        )
        .is_ok());
        assert!(validate_store_manifest_category(
            &json!({ "category": "productivity" }),
            &manifest
        )
        .is_err());
        assert!(validate_store_manifest_category(&json!({}), &manifest).is_err());
    }

    #[test]
    fn declared_assets_count_is_bounded() {
        assert!(validate_store_declared_assets_count(0).is_ok());
        assert!(validate_store_declared_assets_count(MAX_TAPP_ASSETS).is_ok());
        assert!(validate_store_declared_assets_count(MAX_TAPP_ASSETS + 1).is_err());
    }

    #[test]
    fn optional_and_nested_download_plans_flatten_index_fields() {
        let download = json!({
            "styles": "apps/a/styles.css",
            "widget_styles": "apps/a/widget.css",
            "widget_templates": {
                "card": { "2x2": "apps/a/templates/card-2x2.html" },
                "list": { "4x2": "apps/a/templates/list-4x2.html" }
            },
            "i18n": { "en-US": "apps/a/i18n/en-US.json" },
            "page_modules": { "extra.js": "apps/a/page/extra.js" }
        });
        let optional = optional_store_text_downloads(&download);
        assert_eq!(optional.len(), 2);
        assert_eq!(optional[0].kind, OptionalStoreTextKind::Styles);
        assert_eq!(optional[1].kind, OptionalStoreTextKind::WidgetStyles);

        let templates = widget_template_downloads(&download);
        assert_eq!(templates.len(), 2);
        assert!(templates.iter().any(|t| t.widget_id == "card" && t.size == "2x2"));

        let i18n = i18n_downloads(&download);
        assert_eq!(i18n, vec![NamedPathDownload {
            key: "en-US".into(),
            path: "apps/a/i18n/en-US.json".into()
        }]);
        let modules = page_module_downloads(&download);
        assert_eq!(modules[0].key, "extra.js");

        assert!(optional_store_text_downloads(&json!({})).is_empty());
        assert!(widget_template_downloads(&json!({})).is_empty());
        assert!(nonempty_map_opt(std::collections::HashMap::<String, String>::new()).is_none());
        assert!(nonempty_map_opt(std::collections::HashMap::from([(
            "k".to_string(),
            "v".to_string()
        )]))
        .is_some());
    }

    #[test]
    fn resolve_store_source_prefers_exact_then_id_then_normalized() {
        let official = "https://raw.githubusercontent.com/org/store/main/index.json";
        let mirror = "https://cdn.example.com/store/";
        let rows = [
            StoreSourceRowRef {
                id: 1,
                url: official,
            },
            StoreSourceRowRef {
                id: 2,
                url: mirror,
            },
        ];

        // exact URL
        assert_eq!(
            resolve_store_source_among(rows, official).map(|r| r.id),
            Some(1)
        );
        // numeric id
        assert_eq!(
            resolve_store_source_among(rows, "2").map(|r| r.id),
            Some(2)
        );
        // normalized base (index.json stripped form)
        assert_eq!(
            resolve_store_source_among(
                rows,
                "https://raw.githubusercontent.com/org/store/main"
            )
            .map(|r| r.id),
            Some(1)
        );
        // invalid install-mode placeholders
        assert!(resolve_store_source_among(rows, "store").is_none());
        assert!(resolve_store_source_among(rows, "").is_none());
        // unknown
        assert!(resolve_store_source_among(rows, "https://other.example/store").is_none());
    }

    #[test]
    fn prepare_store_catalog_base_and_index_url() {
        let base = prepare_store_catalog_base("https://ex.com/store/index.json").unwrap();
        assert_eq!(base, "https://ex.com/store");
        assert_eq!(store_index_url(&base), "https://ex.com/store/index.json");

        assert!(prepare_store_catalog_base("ftp://ex.com/store")
            .unwrap_err()
            .contains("HTTP(S)"));
        assert!(prepare_store_catalog_base("http://localhost/store")
            .unwrap_err()
            .contains("Internal"));
        assert!(validate_store_fetch_base_url("https://raw.githubusercontent.com/x").is_ok());
    }

    #[test]
    fn store_asset_download_plan_validates_and_joins() {
        let declared = vec![
            "assets/felt/table.png".to_string(),
            "assets/icon.png".to_string(),
        ];
        let plan = store_asset_download_plan(
            "https://ex.com/store",
            "apps/com.example.app",
            Some(&declared),
            MAX_TAPP_ASSETS,
        )
        .unwrap();
        assert_eq!(plan.len(), 2);
        assert_eq!(plan[0].relative, "assets/felt/table.png");
        assert_eq!(
            plan[0].store_path,
            "apps/com.example.app/assets/felt/table.png"
        );
        assert_eq!(
            plan[0].url,
            "https://ex.com/store/apps/com.example.app/assets/felt/table.png"
        );

        assert!(store_asset_download_plan(
            "https://ex.com/store",
            "apps/a",
            None,
            MAX_TAPP_ASSETS
        )
        .unwrap()
        .is_empty());
        assert!(store_asset_download_plan(
            "https://ex.com/store",
            "apps/a",
            Some(&["main.js".to_string()]),
            MAX_TAPP_ASSETS
        )
        .unwrap_err()
        .contains("assets/"));
    }
}
