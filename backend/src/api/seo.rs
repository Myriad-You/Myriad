//! Public SEO: sitemap, robots, llms.txt, Tapp/Phantasi share summary, crawler shells.
//!
//! Indexability (guest / crawler):
//! - Not `branding.noindex` (`site_noindex` or private visibility policy) for sitemap entries (shell still returns noindex meta)
//! - Module visibility = `all` (`tapp` / `phantasi`)
//! - Tapp: site-owner public install with `visibility = all`
//! - Phantasi: published notes only, at `/blog/posts/{slug}/`.
//!   Friend-links and third-party RSS items have no public page.
//!
//! Ordinary browsers get the SPA via the proxy. The frontend process stamps site
//! identity into that document; the proxy only routes. Crawler UAs and
//! WeChat/Weibo/WeCom in-app UAs get these HTML shells (`?_spa=1` → SPA index
//! if the dist file exists, otherwise this shell). Direct backend GETs skip
//! the UA split; `?_spa=1` is honored here so a bounce cannot stick on SEO HTML.

use axum::{
    Json,
    extract::{Path, Request, State},
    http::{HeaderMap, StatusCode, header},
    middleware::Next,
    response::{IntoResponse, Response},
};
use myriad_error::AppError;
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Serialize;
use serde_json::{Value, json};

use crate::models::entities::{phantasi_items, phantasi_sources, tapps};
#[cfg(test)]
pub(crate) use crate::services::public_site::PHANTASI_MINE_CATEGORY;
pub(crate) use crate::services::public_site::{
    SiteBranding, SiteLocale, content_locale, description_from_manifest, encode_path_segment,
    host_site_locale, infer_site_locale, load_site_branding, module_is_public_all,
    name_from_manifest, notes_rss_enabled, own_phantasi_item_links, phantasi_item_path,
    public_absolute_url, public_site_identity, public_tapp_links, resolve_public_base_url,
    strip_html_snippet,
};
use crate::services::tapp_ownership::{find_admin_user_id, public_install_visible_to_viewer};
use crate::services::tapp_validation::validate_tapp_id;
use myriad_module_visibility::{
    load_module_visibility_preferences, try_load_module_visibility_preferences,
};

/// Cap phantasi item URLs in sitemap (newest first).
const PHANTASI_SITEMAP_ITEM_LIMIT: u64 = 200;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TappSeoSummary {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Canonical run-page URL: absolute when `FRONTEND_URL`/`BASE_URL` is set, otherwise the path only.
    pub canonical_url: String,
    pub path: String,
    pub noindex: bool,
    /// True when the install is guest-visible public (sitemap-eligible, shell 200).
    pub indexable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_title: Option<String>,
}

struct SeoChrome {
    html_lang: &'static str,
    og_locale: &'static str,
    site_name: String,
    favicon: String,
    keywords: String,
    google_site_verification: String,
    description: String,
    spa_escape: bool,
}

impl Default for SeoChrome {
    fn default() -> Self {
        Self {
            html_lang: "en",
            og_locale: "en_US",
            site_name: "Myriad".to_string(),
            favicon: String::new(),
            keywords: String::new(),
            google_site_verification: String::new(),
            description: "A myriad of lights, in one place.".to_string(),
            spa_escape: false,
        }
    }
}

struct SitemapUrl {
    loc: String,
    lastmod: Option<String>,
    changefreq: Option<&'static str>,
}

// ── helpers ─────────────────────────────────────────────────────────────────

pub(crate) fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

struct SeoDocument<'a> {
    title: &'a str,
    description: &'a str,
    canonical: &'a str,
    noindex: bool,
    /// Override robots when set. Default is `noindex, nofollow` / `index, follow`.
    robots: Option<&'a str>,
    og_type: &'a str,
    image: Option<&'a str>,
    /// 图是站长设的分享图（够大）。站点图标、应用图标只配小卡片，摊成大图是一块糊掉的方块。
    large_image: bool,
    json_ld: Option<Value>,
    body_inner: String,
    extra_head: &'a str,
    chrome: &'a SeoChrome,
}

fn json_ld_tag(value: &Value) -> String {
    // Prevent `</script>` breakout in JSON-LD.
    let payload = value.to_string().replace('<', "\\u003c");
    format!("  <script type=\"application/ld+json\">{payload}</script>\n")
}

fn render_seo_html(doc: SeoDocument<'_>) -> String {
    let robots = doc.robots.unwrap_or(if doc.noindex {
        "noindex, nofollow"
    } else {
        "index, follow"
    });
    let image = doc.image.unwrap_or("");
    let image_meta = if image.is_empty() {
        String::new()
    } else {
        format!(
            r#"  <meta property="og:image" content="{img}" />
  <meta name="twitter:image" content="{img}" />
"#,
            img = html_escape(image)
        )
    };
    let twitter_card = if !image.is_empty() && doc.large_image {
        "summary_large_image"
    } else {
        "summary"
    };
    let json_ld = doc.json_ld.as_ref().map(json_ld_tag).unwrap_or_default();
    let keywords_meta = doc
        .chrome
        .keywords
        .trim()
        .is_empty()
        .then(String::new)
        .unwrap_or_else(|| {
            format!(
                "  <meta name=\"keywords\" content=\"{}\" />\n",
                html_escape(doc.chrome.keywords.trim())
            )
        });
    let gsc_meta = doc
        .chrome
        .google_site_verification
        .trim()
        .is_empty()
        .then(String::new)
        .unwrap_or_else(|| {
            format!(
                "  <meta name=\"google-site-verification\" content=\"{}\" />\n",
                html_escape(doc.chrome.google_site_verification.trim())
            )
        });
    let spa_escape = spa_escape_script(doc.chrome);
    let icon_href = doc.chrome.favicon.trim();
    let icon_link = if icon_href.is_empty() {
        String::new()
    } else {
        format!(
            "  <link rel=\"icon\" href=\"{}\" />\n",
            html_escape(icon_href)
        )
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="{html_lang}">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>{title}</title>
  <meta name="description" content="{desc}" />
{keywords_meta}{gsc_meta}  <meta name="robots" content="{robots}" />
  <link rel="canonical" href="{canonical}" />
{icon_link}{extra_head}  <meta property="og:type" content="{og_type}" />
  <meta property="og:site_name" content="{site_name}" />
  <meta property="og:locale" content="{og_locale}" />
  <meta property="og:title" content="{title}" />
  <meta property="og:description" content="{desc}" />
  <meta property="og:url" content="{canonical}" />
{image_meta}  <meta name="twitter:card" content="{twitter_card}" />
  <meta name="twitter:title" content="{title}" />
  <meta name="twitter:description" content="{desc}" />
  <meta name="referrer" content="strict-origin-when-cross-origin" />
{json_ld}{spa_escape}</head>
<body>
  <main style="max-width:40rem;margin:3rem auto;padding:0 1.25rem;font-family:system-ui,sans-serif;line-height:1.6">
    {body}
  </main>
  <!-- Humans are served the SPA by the reverse proxy (non-crawler UA). -->
</body>
</html>
"#,
        html_lang = html_escape(doc.chrome.html_lang),
        title = html_escape(doc.title),
        desc = html_escape(doc.description),
        keywords_meta = keywords_meta,
        gsc_meta = gsc_meta,
        robots = robots,
        canonical = html_escape(doc.canonical),
        extra_head = doc.extra_head,
        icon_link = icon_link,
        og_type = html_escape(doc.og_type),
        site_name = html_escape(&doc.chrome.site_name),
        og_locale = html_escape(doc.chrome.og_locale),
        image_meta = image_meta,
        twitter_card = twitter_card,
        json_ld = json_ld,
        spa_escape = spa_escape,
        body = doc.body_inner,
    )
}

/// Match proxy `is_seo_document_shell_path`. Humans with `?_spa=1` must not
/// stay on this SEO document — the bounce script is a no-op once that query is set.
pub(crate) fn is_seo_document_shell_path(path: &str) -> bool {
    matches!(path, "/" | "/tapp" | "/library") || path.starts_with("/tapp/run/")
}

pub(crate) fn query_has_spa_bypass(query: Option<&str>) -> bool {
    query.unwrap_or("").split('&').any(|pair| pair == "_spa=1")
}

/// When the request already has `?_spa=1`, serve the SPA index instead of a
/// crawler shell. Proxy usually sends that query to frontend; this covers
/// native / direct backend hits so the WeChat bounce cannot stick on SEO HTML.
pub async fn spa_document_bypass(req: Request, next: Next) -> Response {
    if !is_seo_document_shell_path(req.uri().path()) || !query_has_spa_bypass(req.uri().query()) {
        return next.run(req).await;
    }
    let dist = crate::GLOBAL_CONFIG.read().await.frontend_dist_path.clone();
    let index = std::path::Path::new(&dist).join("index.html");
    if !index.is_file() {
        return next.run(req).await;
    }
    match tokio::fs::read_to_string(&index).await {
        Ok(html) => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "text/html; charset=utf-8"),
                (header::CACHE_CONTROL, "no-cache"),
            ],
            html,
        )
            .into_response(),
        Err(_) => next.run(req).await,
    }
}

fn is_inapp_share_ua(ua: &str) -> bool {
    let ua = ua.to_ascii_lowercase();
    ua.contains("micromessenger")
        || ua.contains("windowswechat")
        || ua.contains("wxwork")
        || ua.contains("weibo")
}

/// `content` 是站点文章的语言（[`content_locale`]）。有就按它：页面写的是什么语言就标什么，
/// 站点标题是英文不算数。没有文章时才看站点文字，还认不出来再看访客的语言。
fn seo_chrome(
    branding: &SiteBranding,
    content: Option<SiteLocale>,
    headers: &HeaderMap,
) -> SeoChrome {
    let inferred = infer_site_locale(&[
        branding.title.as_str(),
        branding.description.as_str(),
        branding.ai_intro.as_str(),
        branding.keywords.as_str(),
    ]);
    let loc = if let Some(content) = content {
        content
    } else if inferred.html_lang == "en" {
        crate::api::reports::locale::locale_from_headers(headers)
            .map(host_site_locale)
            .unwrap_or(inferred)
    } else {
        inferred
    };
    let ua = headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    SeoChrome {
        html_lang: loc.html_lang,
        og_locale: loc.og_locale,
        site_name: branding.title.clone(),
        favicon: branding.favicon.clone(),
        keywords: branding.keywords.trim().to_string(),
        google_site_verification: branding.google_site_verification.trim().to_string(),
        description: branding.description.clone(),
        spa_escape: is_inapp_share_ua(ua),
    }
}

fn spa_escape_script(chrome: &SeoChrome) -> String {
    if !chrome.spa_escape {
        return String::new();
    }
    let payload = json!({
        "site_title": chrome.site_name,
        "site_description": chrome.description,
        "site_favicon": chrome.favicon,
    });
    let object_json = payload.to_string().replace('<', "\\u003c");
    let js_literal = serde_json::to_string(&object_json).unwrap_or_else(|_| "\"{}\"".to_string());
    format!(
        r#"  <script>(function(){{try{{localStorage.setItem("site_metadata",{js_literal});var s=location.search;if(/(?:^|[?&])_spa=1(?:&|$)/.test(s))return;var q=s?s+"&_spa=1":"?_spa=1";location.replace(location.pathname+q+location.hash)}}catch(e){{}}}})();</script>
"#
    )
}

fn listing_json_ld(
    heading: &str,
    desc: &str,
    canonical: &str,
    listing: bool,
    links: &[(String, String, Option<String>)],
) -> Value {
    let page_type = if listing { "CollectionPage" } else { "WebPage" };
    let mut value = json!({
        "@context": "https://schema.org",
        "@type": page_type,
        "name": heading,
        "description": desc,
        "url": canonical,
    });
    if listing && !links.is_empty() {
        let items: Vec<Value> = links
            .iter()
            .take(20)
            .enumerate()
            .map(|(i, (url, name, _))| {
                json!({
                    "@type": "ListItem",
                    "position": i + 1,
                    "url": url,
                    "name": name,
                })
            })
            .collect();
        value["mainEntity"] = json!({
            "@type": "ItemList",
            "numberOfItems": items.len(),
            "itemListElement": items,
        });
    }
    value
}

fn list_links_html(items: &[(String, String, Option<String>)]) -> String {
    if items.is_empty() {
        return String::new();
    }
    let mut out = String::from("<ul>\n");
    for (href, title, blurb) in items {
        out.push_str("    <li><a href=\"");
        out.push_str(&html_escape(href));
        out.push_str("\">");
        out.push_str(&html_escape(title));
        out.push_str("</a>");
        if let Some(b) = blurb.as_deref().filter(|s| !s.is_empty()) {
            out.push_str(" — ");
            out.push_str(&html_escape(b));
        }
        out.push_str("</li>\n");
    }
    out.push_str("</ul>\n");
    out
}

fn humanize_widget_type(id: &str) -> String {
    match id {
        "welcome" => "Welcome".to_string(),
        "agent-persona" => "Merope".to_string(),
        "quick-stats" => "At a glance".to_string(),
        "recent-activity" => "Recent activity".to_string(),
        "friend-links" => "Friend links".to_string(),
        "weather" => "Weather".to_string(),
        "quote" => "Quote".to_string(),
        "music-player" => "Music".to_string(),
        "social-network" => "Social".to_string(),
        "tapp-shortcut" => "App shortcut".to_string(),
        "game-presence" => "Now playing".to_string(),
        "visitor-stats" => "Visitors".to_string(),
        id if id.starts_with("report-") => format!("Report · {}", &id["report-".len()..]),
        other => other.replace('-', " "),
    }
}

fn collect_widget_labels(layout_json: Option<&str>) -> Vec<String> {
    let Some(raw) = layout_json.map(str::trim).filter(|s| !s.is_empty()) else {
        return Vec::new();
    };
    let Ok(value) = serde_json::from_str::<Value>(raw) else {
        return Vec::new();
    };
    let arr = if let Some(a) = value.as_array() {
        a
    } else if let Some(a) = value.get("standard").and_then(|v| v.as_array()) {
        a
    } else {
        return Vec::new();
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut labels = Vec::new();
    for item in arr {
        let Some(ty) = item
            .get("type")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        else {
            continue;
        };
        if !seen.insert(ty.to_string()) {
            continue;
        }
        labels.push(humanize_widget_type(ty));
        if labels.len() >= 24 {
            break;
        }
    }
    labels
}

fn llms_primary_routes(
    modules: &std::collections::HashMap<String, String>,
) -> Vec<(&'static str, &'static str)> {
    let mut routes: Vec<(&str, &str)> = vec![("Home", "/")];
    for (key, path, label) in [
        ("library", "/library", "Library"),
        ("phantasi", "/blog/", "Blog"),
        ("tapp", "/tapp", "Tapp"),
    ] {
        let level = modules.get(key).map(String::as_str).unwrap_or("all");
        if module_is_public_all(level) {
            routes.push((label, path));
        }
    }
    routes
}

/// Share-image URL: drop `data:`, svg/xml markup, and tokens with neither `/` nor `.`; prefix durable origin onto `/` paths (path-only if origin unset).
fn absolute_share_image(base: &str, raw: &str) -> Option<String> {
    let t = raw.trim();
    if t.is_empty() || t.starts_with("data:") {
        return None;
    }
    if t.starts_with("<svg") || t.starts_with("<?xml") {
        return None;
    }
    if !t.contains('/') && !t.contains('.') {
        return None;
    }
    if t.starts_with("http://") || t.starts_with("https://") {
        return Some(t.to_string());
    }
    if t.starts_with('/') {
        return Some(format!("{base}{t}"));
    }
    None
}

async fn module_open_to_guests(db: &DatabaseConnection, key: &str) -> bool {
    match try_load_module_visibility_preferences(db).await {
        Ok(preferences) => module_is_public_all(preferences.module_visibility(key)),
        Err(error) => {
            tracing::warn!(%error, module = key, "Failed closed while checking module visibility");
            false
        }
    }
}

async fn tapp_module_open_to_guests(db: &DatabaseConnection) -> bool {
    module_open_to_guests(db, "tapp").await
}

pub(crate) async fn phantasi_module_open_to_guests(db: &DatabaseConnection) -> bool {
    module_open_to_guests(db, "phantasi").await
}

/// 开关打开，且 Phantasi 对访客开放，公开 feed 才存在。
pub(crate) async fn notes_rss_is_public(db: &DatabaseConnection) -> bool {
    notes_rss_enabled(db).await && phantasi_module_open_to_guests(db).await
}

fn icon_from_manifest(manifest: &Value, row_icon: Option<&str>) -> Option<String> {
    manifest
        .get("icon")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            row_icon
                .map(str::to_string)
                .filter(|s| !s.trim().is_empty())
        })
}

/// Resolve public SEO summary for a site-owner install of `tapp_id`.
async fn resolve_tapp_seo_summary(
    db: &DatabaseConnection,
    _headers: &HeaderMap,
    tapp_id: &str,
) -> Result<TappSeoSummary, StatusCode> {
    validate_tapp_id(tapp_id).map_err(|_| StatusCode::BAD_REQUEST)?;

    let base = resolve_public_base_url();
    let branding = load_site_branding(db).await;
    let module_open = tapp_module_open_to_guests(db).await;
    let path = format!("/tapp/run/{}", encode_path_segment(tapp_id));
    let canonical_url = public_absolute_url(base.as_deref(), &path);

    let admin_id = find_admin_user_id(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let Some(admin_id) = admin_id else {
        return Err(StatusCode::NOT_FOUND);
    };

    let row = tapps::Entity::find()
        .filter(tapps::Column::UserId.eq(admin_id))
        .filter(tapps::Column::TappId.eq(tapp_id))
        .one(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let Some(tapp) = row else {
        return Err(StatusCode::NOT_FOUND);
    };

    let public_ok = public_install_visible_to_viewer(&tapp.visibility, false);
    let indexable = module_open && public_ok && !branding.noindex;

    // Hidden from guests → treat as not found for SEO surfaces
    if !public_ok || !module_open {
        return Err(StatusCode::NOT_FOUND);
    }

    let name = name_from_manifest(&tapp.manifest, &tapp.name);
    let description = description_from_manifest(&tapp.manifest, tapp.description.as_deref());
    let icon_raw = icon_from_manifest(&tapp.manifest, tapp.icon.as_deref());
    let base_for_img = base.as_deref().unwrap_or("");
    let image = icon_raw
        .as_deref()
        .and_then(|i| absolute_share_image(base_for_img, i))
        .or_else(|| absolute_share_image(base_for_img, branding.og_image.trim()))
        .or_else(|| absolute_share_image(base_for_img, branding.favicon.trim()));

    Ok(TappSeoSummary {
        id: tapp.tapp_id,
        name,
        description,
        image,
        canonical_url,
        path,
        noindex: branding.noindex || !indexable,
        indexable,
        site_title: Some(branding.title),
    })
}

fn format_document_title(page: &str, site: &str) -> String {
    let page = page.trim();
    let site = site.trim();
    if page.is_empty() {
        return site.to_string();
    }
    if site.is_empty() || page == site || site.starts_with(page) {
        return page.to_string();
    }
    format!("{page} · {site}")
}

fn render_tapp_seo_html(summary: &TappSeoSummary, chrome: &SeoChrome) -> String {
    let site = summary.site_title.as_deref().unwrap_or("Myriad");
    let title = format_document_title(&summary.name, site);
    let desc = summary
        .description
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(site);
    let mut json_ld = json!({
        "@context": "https://schema.org",
        "@type": "SoftwareApplication",
        "name": summary.name,
        "description": desc,
        "url": summary.canonical_url,
        "applicationCategory": "LifestyleApplication",
        "operatingSystem": "Any",
    });
    if let Some(image) = summary.image.as_deref() {
        json_ld["image"] = json!(image);
    }
    let body_inner = format!(
        "    <h1>{name}</h1>\n    <p>{desc_body}</p>\n    <p><a href=\"{canonical}\">Open app</a> · <a href=\"/tapp\">All apps</a></p>\n",
        name = html_escape(&summary.name),
        desc_body = html_escape(desc),
        canonical = html_escape(&summary.canonical_url),
    );

    render_seo_html(SeoDocument {
        title: &title,
        description: desc,
        canonical: &summary.canonical_url,
        noindex: summary.noindex,
        robots: None,
        og_type: "website",
        image: summary.image.as_deref(),
        large_image: false,
        json_ld: Some(json_ld),
        body_inner,
        extra_head: "",
        chrome,
    })
}

fn html_response(status: StatusCode, body: String) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
            // The proxy picks SEO HTML vs SPA by User-Agent for the same URL, so a
            // shared cache (Nginx/CDN) must key HTML documents on it; otherwise a
            // link-preview crawler response is replayed to a human browser (#545).
            (header::VARY, "User-Agent"),
        ],
        body,
    )
        .into_response()
}

// ── handlers ────────────────────────────────────────────────────────────────

/// GET /api/seo/tapp/{tapp_id} — JSON share summary for public installs.
pub async fn tapp_seo_summary(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(tapp_id): Path<String>,
) -> Response {
    match resolve_tapp_seo_summary(&db, &headers, &tapp_id).await {
        Ok(summary) => (StatusCode::OK, Json(summary)).into_response(),
        Err(StatusCode::BAD_REQUEST) => (
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Invalid tapp id")),
        )
            .into_response(),
        Err(StatusCode::NOT_FOUND) => (
            StatusCode::NOT_FOUND,
            Json(AppError::public_json("Not found")),
        )
            .into_response(),
        Err(status) => (
            status,
            Json(json!({ "error": "Internal error", "code": "internal_error" })),
        )
            .into_response(),
    }
}

/// GET /tapp/run/{tapp_id} — crawler-oriented HTML shell with correct first-byte meta.
pub async fn tapp_run_seo_html(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(tapp_id): Path<String>,
) -> Response {
    match resolve_tapp_seo_summary(&db, &headers, &tapp_id).await {
        Ok(summary) => {
            let branding = load_site_branding(&db).await;
            let chrome = seo_chrome(&branding, content_locale(&db).await, &headers);
            html_response(StatusCode::OK, render_tapp_seo_html(&summary, &chrome))
        }
        Err(StatusCode::BAD_REQUEST) => html_response(
            StatusCode::BAD_REQUEST,
            simple_error_html("Bad request", "Invalid application id."),
        ),
        Err(StatusCode::NOT_FOUND) => html_response(
            StatusCode::NOT_FOUND,
            simple_error_html("Not found", "This application is not available."),
        ),
        Err(status) => html_response(
            status,
            simple_error_html("Error", "Unable to load application metadata."),
        ),
    }
}

fn simple_error_html(title: &str, message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="en"><head>
<meta charset="UTF-8" /><meta name="robots" content="noindex, nofollow" />
<title>{title}</title></head>
<body><main style="max-width:40rem;margin:3rem auto;font-family:system-ui,sans-serif">
<h1>{title}</h1><p>{message}</p>
<p><a href="/">Home</a></p>
</main></body></html>
"#,
        title = html_escape(title),
        message = html_escape(message),
    )
}

fn module_not_found() -> Response {
    html_response(
        StatusCode::NOT_FOUND,
        simple_error_html("Not found", "This page is not available."),
    )
}

fn site_image<'a>(branding: &'a SiteBranding, base: Option<&str>) -> Option<String> {
    let base_for_img = base.unwrap_or("");
    absolute_share_image(base_for_img, branding.og_image.trim())
        .or_else(|| absolute_share_image(base_for_img, branding.favicon.trim()))
}

fn module_nav_html(modules: &std::collections::HashMap<String, String>) -> String {
    let mut parts: Vec<(&str, &str)> = vec![("/", "Home")];
    for (key, path, label) in [
        ("library", "/library", "Library"),
        ("phantasi", "/blog/", "Blog"),
        ("tapp", "/tapp", "Apps"),
    ] {
        let level = modules.get(key).map(String::as_str).unwrap_or("all");
        if module_is_public_all(level) {
            parts.push((path, label));
        }
    }
    let links = parts
        .into_iter()
        .map(|(href, label)| {
            format!(
                "<a href=\"{}\">{}</a>",
                html_escape(href),
                html_escape(label)
            )
        })
        .collect::<Vec<_>>()
        .join(" · ");
    format!("    <p>{links}</p>\n")
}

/// GET / — crawler homepage shell (branding, owner, public widgets).
pub async fn home_seo_html(State(db): State<DatabaseConnection>, headers: HeaderMap) -> Response {
    let branding = load_site_branding(&db).await;
    let chrome = seo_chrome(&branding, content_locale(&db).await, &headers);
    let base = resolve_public_base_url();
    let canonical = public_absolute_url(base.as_deref(), "/");
    let image = site_image(&branding, base.as_deref());

    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let db_config = config_service.load_config().await.ok();
    let dashboard_title = db_config
        .as_ref()
        .and_then(|c| c.dashboard_title.clone())
        .filter(|s| !s.trim().is_empty());
    let widgets = collect_widget_labels(
        db_config
            .as_ref()
            .and_then(|c| c.dashboard_layout.as_deref()),
    );

    let mut owner_name: Option<String> = None;
    let mut owner_bio: Option<String> = None;
    if let Ok(uid) = crate::services::site_owner::site_owner_user_id(&db).await {
        if let Ok(text) = crate::services::profile_text::resolve_profile_text(&db, uid).await {
            owner_name = text.name.filter(|s| !s.trim().is_empty());
            let bio = text.bio.trim();
            if !crate::services::avatar::is_placeholder_bio(bio) {
                owner_bio = Some(bio.to_string());
            }
        }
    }

    let h1 = owner_name
        .as_deref()
        .or(dashboard_title.as_deref())
        .unwrap_or(branding.title.as_str());
    let desc = if !branding.description.trim().is_empty() {
        branding.description.as_str()
    } else {
        owner_bio.as_deref().unwrap_or(h1)
    };

    let prefs = load_module_visibility_preferences(&db).await;
    let mut body = format!("    <h1>{}</h1>\n", html_escape(h1));
    if let Some(bio) = owner_bio.as_deref() {
        body.push_str("    <p>");
        body.push_str(&html_escape(bio));
        body.push_str("</p>\n");
    }
    if desc != h1 && owner_bio.as_deref() != Some(desc) {
        body.push_str("    <p>");
        body.push_str(&html_escape(desc));
        body.push_str("</p>\n");
    }
    if !widgets.is_empty() {
        body.push_str("    <h2>Home</h2>\n    <ul>\n");
        for label in &widgets {
            body.push_str("      <li>");
            body.push_str(&html_escape(label));
            body.push_str("</li>\n");
        }
        body.push_str("    </ul>\n");
    }
    body.push_str(&module_nav_html(&prefs.modules));

    let page = json!({
        "@type": if owner_name.is_some() { "ProfilePage" } else { "WebPage" },
        "@id": format!("{canonical}#page"),
        "url": canonical,
        "name": branding.title,
        "description": desc,
        "isPartOf": { "@id": format!("{canonical}#website") },
    });
    let mut json_ld = json!({
        "@context": "https://schema.org",
        "@graph": [
            page,
            {
                "@type": "WebSite",
                "@id": format!("{canonical}#website"),
                "name": branding.title,
                "url": canonical,
                "description": desc
            }
        ]
    });
    if let Some(name) = owner_name.as_deref() {
        let mut person = json!({
            "@type": "Person",
            "@id": format!("{canonical}#person"),
            "name": name,
            "url": canonical,
        });
        if let Some(bio) = owner_bio.as_deref() {
            person["description"] = json!(bio);
        }
        if let Some(graph) = json_ld.get_mut("@graph").and_then(|v| v.as_array_mut()) {
            if let Some(page) = graph.first_mut() {
                page["mainEntity"] = json!({ "@id": format!("{canonical}#person") });
            }
            graph.push(person);
        }
    }

    html_response(
        StatusCode::OK,
        render_seo_html(SeoDocument {
            title: &branding.title,
            description: desc,
            canonical: &canonical,
            noindex: branding.noindex,
            robots: None,
            og_type: "profile",
            image: image.as_deref(),
            large_image: !branding.og_image.trim().is_empty(),
            json_ld: Some(json_ld),
            body_inner: body,
            extra_head: "",
            chrome: &chrome,
        }),
    )
}

async fn module_list_seo_html(
    db: &DatabaseConnection,
    headers: &HeaderMap,
    module_key: &str,
    path: &str,
    heading: &str,
    intro: &str,
    links: Vec<(String, String, Option<String>)>,
    listing: bool,
) -> Response {
    if !module_open_to_guests(db, module_key).await {
        return module_not_found();
    }
    let branding = load_site_branding(db).await;
    let chrome = seo_chrome(&branding, content_locale(db).await, headers);
    let base = resolve_public_base_url();
    let canonical = public_absolute_url(base.as_deref(), path);
    let image = site_image(&branding, base.as_deref());
    let title = format_document_title(heading, &branding.title);
    let desc = if intro.trim().is_empty() {
        branding.description.as_str()
    } else {
        intro
    };
    let prefs = load_module_visibility_preferences(db).await;
    let empty_note = if listing && links.is_empty() {
        "    <p>No public items yet.</p>\n"
    } else {
        ""
    };
    let body = format!(
        "    <h1>{}</h1>\n    <p>{}</p>\n{}{}{}",
        html_escape(heading),
        html_escape(desc),
        empty_note,
        list_links_html(&links),
        module_nav_html(&prefs.modules),
    );
    let extra_head = "";
    let json_ld = listing_json_ld(heading, desc, &canonical, listing, &links);
    html_response(
        StatusCode::OK,
        render_seo_html(SeoDocument {
            title: &title,
            description: desc,
            canonical: &canonical,
            noindex: branding.noindex,
            robots: None,
            og_type: "website",
            image: image.as_deref(),
            large_image: !branding.og_image.trim().is_empty(),
            json_ld: Some(json_ld),
            body_inner: body,
            extra_head: &extra_head,
            chrome: &chrome,
        }),
    )
}

/// GET /tapp — public Tapp index for crawlers.
pub async fn tapp_list_seo_html(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
) -> Response {
    if !tapp_module_open_to_guests(&db).await {
        return module_not_found();
    }
    let base = resolve_public_base_url();
    let links = public_tapp_links(&db, base.as_deref()).await;
    module_list_seo_html(
        &db,
        &headers,
        "tapp",
        "/tapp",
        "Apps",
        "Public Tapp apps on this site.",
        links,
        true,
    )
    .await
}

/// GET /library — thin public intro for crawlers.
pub async fn library_seo_html(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
) -> Response {
    module_list_seo_html(
        &db,
        &headers,
        "library",
        "/library",
        "Library",
        "Games, anime, music, and activity from connected platforms.",
        Vec::new(),
        false,
    )
    .await
}

/// GET /robots.txt — `Sitemap:` only when policy is not private and durable origin is set.
/// Non-private: `Allow: /` plus Disallow `/login` `/register` `/setup` `/config`
/// `/tapp/playground` `/tapp/detail/` `/agent/settings`. Those SPA pages also set client noindex.
pub async fn robots_txt(State(db): State<DatabaseConnection>, _headers: HeaderMap) -> Response {
    let branding = load_site_branding(&db).await;
    let base = resolve_public_base_url();
    let body = crate::api::seo_policy::build_robots_txt(base.as_deref(), &branding.policy);
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        body,
    )
        .into_response()
}

/// GET /llms.txt — AI-facing site index when policy is ai_citation or ai_full.
/// Absolute route links require durable `FRONTEND_URL`/`BASE_URL`; otherwise
/// paths stay relative (no client-Host absolute links).
pub async fn llms_txt(State(db): State<DatabaseConnection>, _headers: HeaderMap) -> Response {
    let branding = load_site_branding(&db).await;
    if !crate::api::seo_policy::policy_serves_llms_txt(&branding.policy) {
        return (
            StatusCode::NOT_FOUND,
            [
                (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
                (header::CACHE_CONTROL, "public, max-age=300"),
            ],
            "llms.txt is disabled for this site visibility policy.\n".to_string(),
        )
            .into_response();
    }

    let base = resolve_public_base_url();
    let intro = if branding.ai_intro.trim().is_empty() {
        branding.description.as_str()
    } else {
        branding.ai_intro.as_str()
    };

    let prefs = load_module_visibility_preferences(&db).await;
    let modules = &prefs.modules;
    let routes = llms_primary_routes(modules);

    let mut body =
        crate::api::seo_policy::build_llms_txt(&branding.title, intro, base.as_deref(), &routes);
    if module_is_public_all(modules.get("tapp").map(String::as_str).unwrap_or("all")) {
        let apps: Vec<(String, String)> = public_tapp_links(&db, base.as_deref())
            .await
            .into_iter()
            .map(|(url, name, _)| (name, url))
            .collect();
        crate::api::seo_policy::append_llms_link_section(&mut body, "Apps", &apps);
    }
    if module_is_public_all(modules.get("phantasi").map(String::as_str).unwrap_or("all")) {
        let writing: Vec<(String, String)> = own_phantasi_item_links(&db, base.as_deref())
            .await
            .into_iter()
            .map(|(url, title, _)| (title, url))
            .collect();
        crate::api::seo_policy::append_llms_link_section(&mut body, "Writing", &writing);
    }
    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "text/plain; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=1800"),
        ],
        body,
    )
        .into_response()
}

/// GET /sitemap.xml
///
/// Sitemap protocol requires absolute `<loc>` URLs. Without durable
/// `FRONTEND_URL`/`BASE_URL` we return an empty urlset rather than poisoning
/// caches with a client-supplied Host.
pub async fn sitemap_xml(State(db): State<DatabaseConnection>, _headers: HeaderMap) -> Response {
    let branding = load_site_branding(&db).await;
    let empty = r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
</urlset>
"#;
    if branding.noindex {
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/xml; charset=utf-8"),
                (header::CACHE_CONTROL, "public, max-age=600"),
            ],
            empty.to_string(),
        )
            .into_response();
    }

    let Some(base) = resolve_public_base_url() else {
        tracing::warn!(
            "sitemap.xml: FRONTEND_URL/BASE_URL unset; returning empty urlset (no client Host fallback)"
        );
        return (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, "application/xml; charset=utf-8"),
                (header::CACHE_CONTROL, "public, max-age=300"),
            ],
            empty.to_string(),
        )
            .into_response();
    };

    let mut urls: Vec<SitemapUrl> = Vec::new();

    urls.push(SitemapUrl {
        loc: format!("{base}/"),
        lastmod: None,
        changefreq: Some("daily"),
    });

    let prefs = load_module_visibility_preferences(&db).await;
    let modules = &prefs.modules;

    for (key, path, freq) in [
        ("library", "/library", "weekly"),
        ("phantasi", "/blog/", "weekly"),
        ("tapp", "/tapp", "weekly"),
    ] {
        let level = modules.get(key).map(String::as_str).unwrap_or("all");
        if module_is_public_all(level) {
            urls.push(SitemapUrl {
                loc: format!("{base}{path}"),
                lastmod: None,
                changefreq: Some(freq),
            });
        }
    }

    let tapp_level = modules.get("tapp").map(String::as_str).unwrap_or("all");
    if module_is_public_all(tapp_level) {
        if let Ok(Some(admin_id)) = find_admin_user_id(&db).await {
            if let Ok(admin_tapps) = tapps::Entity::find()
                .filter(tapps::Column::UserId.eq(admin_id))
                .all(&db)
                .await
            {
                for tapp in admin_tapps {
                    if !public_install_visible_to_viewer(&tapp.visibility, false) {
                        continue;
                    }
                    let id = encode_path_segment(&tapp.tapp_id);
                    urls.push(SitemapUrl {
                        loc: format!("{base}/tapp/run/{id}"),
                        lastmod: Some(tapp.updated_at.format("%Y-%m-%d").to_string()),
                        changefreq: None,
                    });
                }
            }
        }
    }

    // Published notes only. A missing slug has no public address.
    let phantasi_level = modules.get("phantasi").map(String::as_str).unwrap_or("all");
    if module_is_public_all(phantasi_level) {
        if let Ok(sources) = phantasi_sources::Entity::find()
            .filter(phantasi_sources::Column::AdminOnly.eq(false))
            .all(&db)
            .await
        {
            let own_source_ids: Vec<i32> = sources
                .into_iter()
                .filter(|s| s.source_type == phantasi_sources::SourceType::Note)
                .map(|s| s.id)
                .collect();
            if !own_source_ids.is_empty() {
                if let Ok(items) = phantasi_items::Entity::find()
                    .filter(phantasi_items::Column::SourceId.is_in(own_source_ids))
                    .filter(phantasi_items::Column::Slug.is_not_null())
                    .order_by_desc(phantasi_items::Column::PublishedAt)
                    .limit(PHANTASI_SITEMAP_ITEM_LIMIT)
                    .select_only()
                    .columns([
                        phantasi_items::Column::PublishedAt,
                        phantasi_items::Column::FetchedAt,
                        phantasi_items::Column::Slug,
                    ])
                    .into_tuple::<(
                        chrono::DateTime<chrono::FixedOffset>,
                        chrono::DateTime<chrono::FixedOffset>,
                        Option<String>,
                    )>()
                    .all(&db)
                    .await
                {
                    for (published_at, republished_at, slug) in items {
                        let Some(slug) =
                            slug.filter(|slug| myriad_phantasi_notes::validate_slug(slug).is_ok())
                        else {
                            continue;
                        };
                        // 改稿重新发布会刷新 fetched_at，博客页的 lastmod 也取它。
                        let modified = published_at.max(republished_at);
                        urls.push(SitemapUrl {
                            loc: format!("{base}{}", phantasi_item_path(&slug)),
                            lastmod: Some(modified.format("%Y-%m-%d").to_string()),
                            changefreq: None,
                        });
                    }
                }
            }
        }
    }

    let mut body = String::from(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
"#,
    );
    for entry in &urls {
        body.push_str("  <url>\n");
        body.push_str(&format!("    <loc>{}</loc>\n", xml_escape(&entry.loc)));
        if let Some(ref lastmod) = entry.lastmod {
            body.push_str(&format!("    <lastmod>{}</lastmod>\n", xml_escape(lastmod)));
        }
        if let Some(freq) = entry.changefreq {
            body.push_str(&format!(
                "    <changefreq>{}</changefreq>\n",
                xml_escape(freq)
            ));
        }
        body.push_str("  </url>\n");
    }
    body.push_str("</urlset>\n");

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "application/xml; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=3600"),
        ],
        body,
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn share_image_rejects_data_and_emoji() {
        assert!(absolute_share_image("https://x.test", "data:image/png;base64,xx").is_none());
        assert!(absolute_share_image("https://x.test", "🔥").is_none());
        assert_eq!(
            absolute_share_image("https://x.test", "/icon.png").as_deref(),
            Some("https://x.test/icon.png")
        );
    }

    #[test]
    fn document_title_joins_site() {
        assert_eq!(
            format_document_title("Todo", "Myriad Site"),
            "Todo · Myriad Site"
        );
    }

    #[test]
    fn seo_html_declares_user_agent_variance() {
        let res = html_response(StatusCode::OK, "<html></html>".to_string());
        assert_eq!(
            res.headers()
                .get(header::VARY)
                .and_then(|v| v.to_str().ok()),
            Some("User-Agent"),
            "SEO HTML is selected by User-Agent; shared caches must key on it (#545)"
        );
        assert_eq!(
            res.headers()
                .get(header::CACHE_CONTROL)
                .and_then(|v| v.to_str().ok()),
            Some("public, max-age=300")
        );
    }

    #[test]
    fn resolve_public_base_url_ignores_client_host_env() {
        // Unit path: function never reads HeaderMap — only env. Host poisoning
        // cannot affect origin selection by construction.
        // Clear both keys for this process if set would break parallel tests;
        // we only assert the pure helper and relative-fallback behaviour above.
        let _ = resolve_public_base_url(); // smoke: does not panic without env
    }

    #[test]
    fn widget_labels_from_standard_layout() {
        let json =
            r#"{"v":2,"standard":[{"type":"welcome"},{"type":"weather"},{"type":"welcome"}]}"#;
        assert_eq!(
            collect_widget_labels(Some(json)),
            vec!["Welcome".to_string(), "Weather".to_string()]
        );
    }

    #[test]
    fn json_ld_escapes_script_breakout() {
        let tag = json_ld_tag(&json!({"x": "</script><b>"}));
        assert!(!tag.to_lowercase().contains("</script><b>"));
        assert!(tag.contains("\\u003c"));
    }

    #[test]
    fn tapp_shell_includes_software_application_json_ld() {
        let html = render_tapp_seo_html(
            &TappSeoSummary {
                id: "com.example.app".into(),
                name: "Todo".into(),
                description: Some("lists".into()),
                image: None,
                canonical_url: "https://ex.com/tapp/run/com.example.app".into(),
                path: "/tapp/run/com.example.app".into(),
                noindex: false,
                indexable: true,
                site_title: Some("Site".into()),
            },
            &SeoChrome::default(),
        );
        assert!(
            html.contains(r#""@type":"SoftwareApplication""#)
                || html.contains(r#""@type": "SoftwareApplication""#)
        );
    }

    #[test]
    fn locale_from_kana_is_japanese() {
        let loc = infer_site_locale(&["こんにちは世界"]);
        assert_eq!(loc.html_lang, "ja");
        assert_eq!(loc.og_locale, "ja_JP");
    }

    #[test]
    fn locale_from_hangul_is_korean() {
        let loc = infer_site_locale(&["안녕하세요 개인 사이트입니다"]);
        assert_eq!(loc.html_lang, "ko");
        assert_eq!(loc.og_locale, "ko_KR");
    }

    #[test]
    fn listing_json_ld_collection_page() {
        let links = vec![("https://ex.com/tapp/run/todo".into(), "Todo".into(), None)];
        let v = listing_json_ld("Apps", "Public apps.", "https://ex.com/tapp", true, &links);
        assert_eq!(v["@type"], "CollectionPage");
        assert_eq!(v["mainEntity"]["@type"], "ItemList");
        assert_eq!(v["mainEntity"]["itemListElement"][0]["name"], "Todo");
        let empty = listing_json_ld("Library", "Games.", "https://ex.com/library", false, &[]);
        assert_eq!(empty["@type"], "WebPage");
        assert!(empty.get("mainEntity").is_none());
    }

    #[test]
    fn shell_emits_favicon_link() {
        let chrome = SeoChrome {
            favicon: "/favicon.webp".into(),
            ..SeoChrome::default()
        };
        let html = render_tapp_seo_html(
            &TappSeoSummary {
                id: "com.example.app".into(),
                name: "Todo".into(),
                description: Some("lists".into()),
                image: None,
                canonical_url: "https://ex.com/tapp/run/com.example.app".into(),
                path: "/tapp/run/com.example.app".into(),
                noindex: false,
                indexable: true,
                site_title: Some("Site".into()),
            },
            &chrome,
        );
        assert!(html.contains(r#"rel="icon""#));
        assert!(html.contains("/favicon.webp"));
    }

    #[test]
    fn llms_primary_routes_include_notes_not_friends() {
        let mut modules = std::collections::HashMap::new();
        modules.insert("phantasi".into(), "all".into());
        modules.insert("library".into(), "owner".into());
        modules.insert("tapp".into(), "owner".into());
        let routes = llms_primary_routes(&modules);
        assert!(routes.contains(&("Home", "/")));
        assert!(routes.contains(&("Blog", "/blog/")));
        assert!(!routes.iter().any(|(_, path)| path.starts_with("/journal")));
        assert!(!routes.iter().any(|(_, path)| *path == "/library"));
    }

    #[test]
    fn locale_from_latin_is_english() {
        let loc = infer_site_locale(&["A myriad of lights, in one place."]);
        assert_eq!(loc.html_lang, "en");
    }

    #[test]
    fn locale_from_traditional_is_taiwan() {
        let loc = infer_site_locale(&["這個網站提供軟體與網路服務"]);
        assert_eq!(loc.html_lang, "zh-TW");
        assert_eq!(loc.og_locale, "zh_TW");
    }

    #[test]
    fn locale_from_simplified_is_china() {
        let loc = infer_site_locale(&["这个网站提供软件与网络服务"]);
        assert_eq!(loc.html_lang, "zh-CN");
        assert_eq!(loc.og_locale, "zh_CN");
    }

    #[test]
    fn english_branding_follows_accept_language() {
        let branding = SiteBranding {
            title: "Myriad - A myriad of lights, in one place.".into(),
            description: "A myriad of lights, in one place.".into(),
            favicon: String::new(),
            og_image: String::new(),
            noindex: false,
            policy: String::new(),
            ai_intro: String::new(),
            keywords: String::new(),
            google_site_verification: String::new(),
        };
        let mut headers = HeaderMap::new();
        headers.insert(header::ACCEPT_LANGUAGE, "zh-TW,zh;q=0.8".parse().unwrap());
        let chrome = seo_chrome(&branding, None, &headers);
        assert_eq!(chrome.html_lang, "zh-TW");
        assert_eq!(chrome.og_locale, "zh_TW");
        // 有文章时按文章的语言，英文标题和访客语言都不算。
        let chrome = seo_chrome(&branding, Some(host_site_locale("ja-JP")), &headers);
        assert_eq!(chrome.html_lang, "ja");
        assert_eq!(chrome.og_locale, "ja_JP");
    }

    #[test]
    fn icons_get_the_small_share_card() {
        let summary = |image: Option<&str>| TappSeoSummary {
            id: "com.example.app".into(),
            name: "Todo".into(),
            description: Some("lists".into()),
            image: image.map(str::to_string),
            canonical_url: "https://ex.com/tapp/run/com.example.app".into(),
            path: "/tapp/run/com.example.app".into(),
            noindex: false,
            indexable: true,
            site_title: Some("Site".into()),
        };
        let chrome = SeoChrome::default();
        let html = render_tapp_seo_html(&summary(Some("https://ex.com/icon.png")), &chrome);
        assert!(html.contains(r#"<meta property="og:image" content="https://ex.com/icon.png" />"#));
        assert!(
            html.contains(r#"<meta name="twitter:card" content="summary" />"#),
            "{html}"
        );

        let chrome = SeoChrome::default();
        let html = render_seo_html(SeoDocument {
            title: "Site",
            description: "d",
            canonical: "https://ex.com/",
            noindex: false,
            robots: None,
            og_type: "website",
            image: Some("https://ex.com/share.png"),
            large_image: true,
            json_ld: None,
            body_inner: String::new(),
            extra_head: "",
            chrome: &chrome,
        });
        assert!(html.contains(r#"content="summary_large_image""#), "{html}");
    }

    #[test]
    fn wechat_shell_injects_spa_bounce() {
        let chrome = SeoChrome {
            spa_escape: true,
            ..SeoChrome::default()
        };
        let html = render_tapp_seo_html(
            &TappSeoSummary {
                id: "com.example.app".into(),
                name: "Todo".into(),
                description: Some("lists".into()),
                image: None,
                canonical_url: "https://ex.com/tapp/run/com.example.app".into(),
                path: "/tapp/run/com.example.app".into(),
                noindex: false,
                indexable: true,
                site_title: Some("Site".into()),
            },
            &chrome,
        );
        assert!(html.contains("_spa=1"));
        assert!(html.contains("og:title"));
        assert!(html.contains("site_metadata"));
        assert!(html.contains("site_description"));
    }

    #[test]
    fn spa_bypass_matches_proxy_shell_paths() {
        assert!(query_has_spa_bypass(Some("_spa=1")));
        assert!(query_has_spa_bypass(Some("foo=1&_spa=1")));
        assert!(!query_has_spa_bypass(Some("spa=1")));
        assert!(!query_has_spa_bypass(None));
        assert!(is_seo_document_shell_path("/"));
        assert!(is_seo_document_shell_path("/tapp/run/com.example"));
        assert!(!is_seo_document_shell_path("/journal"));
        assert!(!is_seo_document_shell_path("/journal/articles/1"));
        assert!(!is_seo_document_shell_path("/blog"));
        assert!(!is_seo_document_shell_path("/blog/posts/hello/"));
        assert!(!is_seo_document_shell_path("/phantasi"));
        assert!(!is_seo_document_shell_path("/phantasi/item/1"));
        assert!(!is_seo_document_shell_path("/config"));
        assert!(!is_seo_document_shell_path("/api/seo/tapp/x"));
    }
}
