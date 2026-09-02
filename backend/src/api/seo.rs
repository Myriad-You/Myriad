//! Public SEO: sitemap, robots, llms.txt, Tapp/Brew share summary, crawler shells.
//!
//! Indexability (guest / crawler):
//! - Site not `site_noindex` for sitemap entries (shell still returns noindex meta)
//! - Module visibility = `all` (`tapp` / `brew`)
//! - Tapp: site-owner public install with `visibility = all`
//! - Brew: only sources categorized as site-owner original content (`我`);
//! never index friend-links or third-party RSS items
//!
//! Humans keep using the SPA via the reverse proxy; only known crawler UAs
//! (and direct API clients) hit HTML shells on `/tapp/run/{id}` and
//! `/brew/item/{id}`.

use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use sea_orm::{ColumnTrait, DatabaseConnection, EntityTrait, QueryFilter, QueryOrder, QuerySelect};
use serde::Serialize;
use serde_json::{json, Value};

use crate::models::entities::{brew_items, brew_sources, tapps};
use crate::services::tapp_ownership::{find_admin_user_id, public_install_visible_to_viewer};
use crate::services::tapp_validation::validate_tapp_id;
use myriad_module_visibility::load_module_visibility_preferences;

/// Fixed DB category label for site-owner original Brew content.
/// Must match frontend `BREW_MINE_CATEGORY` (`frontend/src/components/brew/constants.ts`).
const BREW_MINE_CATEGORY: &str = "我";
/// Cap brew item URLs in sitemap (newest first).
const BREW_SITEMAP_ITEM_LIMIT: u64 = 200;

// ── types ───────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TappSeoSummary {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    /// Absolute canonical URL for this install (run page).
    pub canonical_url: String,
    pub path: String,
    pub noindex: bool,
    /// True when the install is guest-visible public (sitemap-eligible, shell 200).
    pub indexable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_title: Option<String>,
}

/// Public SEO summary for a site-owner original Brew article (`我` category only).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrewItemSeoSummary {
    pub id: i32,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image: Option<String>,
    pub canonical_url: String,
    pub path: String,
    pub noindex: bool,
    pub indexable: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub site_title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
}

struct SiteBranding {
    title: String,
    description: String,
    favicon: String,
    og_image: String,
    noindex: bool,
    policy: String,
    ai_intro: String,
}

struct SitemapUrl {
    loc: String,
    lastmod: Option<String>,
    changefreq: Option<&'static str>,
}

// ── helpers ─────────────────────────────────────────────────────────────────

fn xml_escape(s: &str) -> String {
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

/// Durable public origin only: `FRONTEND_URL`, then `BASE_URL`.
///
/// **Never** derive absolute SEO URLs from client `Host` / `X-Forwarded-Host`
/// (or `X-Forwarded-Proto`) — a poisoned Host can poison cached sitemap /
/// robots / canonical absolute links. When unset, callers must omit absolute
/// URLs (fail closed) rather than invent an origin.
fn resolve_public_base_url() -> Option<String> {
    for key in ["FRONTEND_URL", "BASE_URL"] {
        if let Ok(raw) = std::env::var(key) {
            let trimmed = raw.trim().trim_end_matches('/');
            if !trimmed.is_empty() {
                return Some(trimmed.to_string());
            }
        }
    }
    None
}

/// Absolute URL when a durable origin is configured; otherwise the path only
/// (relative — no host poisoning surface).
fn public_absolute_url(base: Option<&str>, path: &str) -> String {
    match base {
        Some(b) if !b.is_empty() => format!("{b}{path}"),
        _ => path.to_string(),
    }
}

fn module_is_public_all(level: &str) -> bool {
    level == "all"
}

fn encode_path_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char);
            }
            _ => {
                out.push('%');
                out.push_str(&format!("{b:02X}"));
            }
        }
    }
    out
}

/// Absolute URL for share images; drop data: and bare emoji/svg markup.
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

async fn load_site_branding(db: &DatabaseConnection) -> SiteBranding {
    let config_service = crate::services::config_service::ConfigService::new(db.clone());
    let db_config = config_service.load_config().await.ok();

    let branding = |db_val: Option<String>, env_key: &str, default: &str| -> String {
        db_val
            .filter(|v| !v.is_empty())
            .or_else(|| std::env::var(env_key).ok().filter(|v| !v.is_empty()))
            .unwrap_or_else(|| default.to_string())
    };
    let clearable = |db_val: Option<String>, env_key: &str| -> String {
        if let Some(v) = db_val {
            return v;
        }
        std::env::var(env_key).unwrap_or_default()
    };

    let noindex_flag = db_config
        .as_ref()
        .map(|c| c.site_noindex)
        .unwrap_or_else(|| {
            std::env::var("SITE_NOINDEX")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false)
        });

    let policy_raw = db_config
        .as_ref()
        .map(|c| c.site_visibility_policy.clone())
        .filter(|s| !s.trim().is_empty())
        .or_else(|| std::env::var("SITE_VISIBILITY_POLICY").ok())
        .unwrap_or_default();
    let policy =
        crate::api::seo_policy::normalize_visibility_policy(&policy_raw, noindex_flag).to_string();
    let noindex = noindex_flag || !crate::api::seo_policy::policy_is_indexable(&policy);

    SiteBranding {
        title: branding(
            db_config.as_ref().and_then(|c| c.site_title.clone()),
            "SITE_TITLE",
            "Myriad - A myriad of lights, in one place.",
        ),
        description: branding(
            db_config.as_ref().and_then(|c| c.site_description.clone()),
            "SITE_DESCRIPTION",
            "A myriad of lights, in one place.",
        ),
        favicon: branding(
            db_config.as_ref().and_then(|c| c.site_favicon.clone()),
            "SITE_FAVICON",
            "/favicon.webp",
        ),
        og_image: clearable(
            db_config.as_ref().and_then(|c| c.site_og_image.clone()),
            "SITE_OG_IMAGE",
        ),
        noindex,
        policy,
        ai_intro: clearable(
            db_config.as_ref().and_then(|c| c.site_ai_intro.clone()),
            "SITE_AI_INTRO",
        ),
    }
}

async fn module_open_to_guests(db: &DatabaseConnection, key: &str) -> bool {
    let prefs = load_module_visibility_preferences(db).await;
    prefs
        .modules
        .get(key)
        .map(|s| module_is_public_all(s))
        .unwrap_or(true)
}

async fn tapp_module_open_to_guests(db: &DatabaseConnection) -> bool {
    module_open_to_guests(db, "tapp").await
}

async fn brew_module_open_to_guests(db: &DatabaseConnection) -> bool {
    module_open_to_guests(db, "brew").await
}

/// Whether a Brew source is site-owner original content (category contains `我`).
/// Friend links and third-party feeds must never be treated as own content.
fn brew_source_is_own(category: &Option<String>, admin_only: bool) -> bool {
    if admin_only {
        return false;
    }
    category
        .as_ref()
        .map(|c| {
            c.split(',')
                .map(str::trim)
                .any(|part| part == BREW_MINE_CATEGORY)
        })
        .unwrap_or(false)
}

fn strip_html_snippet(raw: &str, max_len: usize) -> String {
    let mut plain = String::with_capacity(raw.len().min(max_len * 2));
    let mut in_tag = false;
    for ch in raw.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => plain.push(ch),
            _ => {}
        }
    }
    let plain = plain
        .replace("&nbsp;", " ")
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if plain.chars().count() <= max_len {
        return plain;
    }
    let truncated: String = plain.chars().take(max_len.saturating_sub(1)).collect();
    format!("{}…", truncated.trim_end())
}

fn brew_item_path(item_id: i32) -> String {
    format!("/brew/item/{item_id}")
}

async fn resolve_brew_item_seo_summary(
    db: &DatabaseConnection,
    _headers: &HeaderMap,
    item_id: i32,
) -> Result<BrewItemSeoSummary, StatusCode> {
    if item_id <= 0 {
        return Err(StatusCode::BAD_REQUEST);
    }

    let base = resolve_public_base_url();
    let branding = load_site_branding(db).await;
    let module_open = brew_module_open_to_guests(db).await;
    let path = brew_item_path(item_id);
    let canonical_url = public_absolute_url(base.as_deref(), &path);

    if !module_open {
        return Err(StatusCode::NOT_FOUND);
    }

    let item = brew_items::Entity::find_by_id(item_id)
        .one(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    let source = brew_sources::Entity::find_by_id(item.source_id)
        .one(db)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .ok_or(StatusCode::NOT_FOUND)?;

    // Hard gate: only `我` category, never friend-links / third-party feeds
    if !brew_source_is_own(&source.category, source.admin_only) {
        return Err(StatusCode::NOT_FOUND);
    }

    let indexable = !branding.noindex;
    let description = item
        .summary
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or(item.content.as_deref().filter(|s| !s.trim().is_empty()))
        .map(|s| strip_html_snippet(s, 160));
    let base_for_img = base.as_deref().unwrap_or("");
    let image = item
        .image
        .as_deref()
        .and_then(|i| absolute_share_image(base_for_img, i))
        .or_else(|| absolute_share_image(base_for_img, branding.og_image.trim()))
        .or_else(|| absolute_share_image(base_for_img, branding.favicon.trim()));

    Ok(BrewItemSeoSummary {
        id: item.id,
        title: item.title,
        description,
        image,
        canonical_url,
        path,
        noindex: branding.noindex || !indexable,
        indexable,
        site_title: Some(branding.title),
        source_name: Some(source.name),
    })
}

fn render_brew_item_seo_html(summary: &BrewItemSeoSummary) -> String {
    let site = summary.site_title.as_deref().unwrap_or("Myriad");
    let title = format_document_title(&summary.title, site);
    let desc = summary
        .description
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(site);
    let robots = if summary.noindex {
        "noindex, nofollow"
    } else {
        "index, follow"
    };
    let image = summary.image.as_deref().unwrap_or("");
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
    let twitter_card = if image.is_empty() {
        "summary"
    } else {
        "summary_large_image"
    };
    let source_line = summary
        .source_name
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|s| format!("<p style=\"color:#666\">{}</p>", html_escape(s)))
        .unwrap_or_default();

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>{title}</title>
  <meta name="description" content="{desc}" />
  <meta name="robots" content="{robots}" />
  <link rel="canonical" href="{canonical}" />
  <meta property="og:type" content="article" />
  <meta property="og:title" content="{title}" />
  <meta property="og:description" content="{desc}" />
  <meta property="og:url" content="{canonical}" />
{image_meta}  <meta name="twitter:card" content="{twitter_card}" />
  <meta name="twitter:title" content="{title}" />
  <meta name="twitter:description" content="{desc}" />
  <meta name="referrer" content="strict-origin-when-cross-origin" />
</head>
<body>
  <main style="max-width:40rem;margin:3rem auto;padding:0 1.25rem;font-family:system-ui,sans-serif;line-height:1.6">
    <h1>{name}</h1>
    {source_line}
    <p>{desc_body}</p>
    <p><a href="{canonical}">Read article</a> · <a href="/brew">Brew</a></p>
  </main>
  <!-- Humans are served the SPA by the reverse proxy (non-crawler UA). -->
</body>
</html>
"#,
        title = html_escape(&title),
        desc = html_escape(desc),
        robots = robots,
        canonical = html_escape(&summary.canonical_url),
        image_meta = image_meta,
        twitter_card = twitter_card,
        name = html_escape(&summary.title),
        source_line = source_line,
        desc_body = html_escape(desc),
    )
}

fn name_from_manifest(manifest: &Value, fallback: &str) -> String {
    manifest
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(fallback)
        .to_string()
}

fn description_from_manifest(manifest: &Value, fallback: Option<&str>) -> Option<String> {
    manifest
        .get("description")
        .and_then(|v| v.as_str())
        .filter(|s| !s.trim().is_empty())
        .map(str::to_string)
        .or_else(|| {
            fallback
                .map(str::to_string)
                .filter(|s| !s.trim().is_empty())
        })
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

fn render_tapp_seo_html(summary: &TappSeoSummary) -> String {
    let site = summary.site_title.as_deref().unwrap_or("Myriad");
    let title = format_document_title(&summary.name, site);
    let desc = summary
        .description
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(site);
    let robots = if summary.noindex {
        "noindex, nofollow"
    } else {
        "index, follow"
    };
    let image = summary.image.as_deref().unwrap_or("");
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
    let twitter_card = if image.is_empty() {
        "summary"
    } else {
        "summary_large_image"
    };

    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN">
<head>
  <meta charset="UTF-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1.0" />
  <title>{title}</title>
  <meta name="description" content="{desc}" />
  <meta name="robots" content="{robots}" />
  <link rel="canonical" href="{canonical}" />
  <meta property="og:type" content="website" />
  <meta property="og:title" content="{title}" />
  <meta property="og:description" content="{desc}" />
  <meta property="og:url" content="{canonical}" />
{image_meta}  <meta name="twitter:card" content="{twitter_card}" />
  <meta name="twitter:title" content="{title}" />
  <meta name="twitter:description" content="{desc}" />
  <meta name="referrer" content="strict-origin-when-cross-origin" />
</head>
<body>
  <main style="max-width:40rem;margin:3rem auto;padding:0 1.25rem;font-family:system-ui,sans-serif;line-height:1.6">
    <h1>{name}</h1>
    <p>{desc_body}</p>
    <p><a href="{canonical}">Open app</a> · <a href="/tapp">All apps</a></p>
  </main>
  <!-- Humans are served the SPA by the reverse proxy (non-crawler UA). -->
</body>
</html>
"#,
        title = html_escape(&title),
        desc = html_escape(desc),
        robots = robots,
        canonical = html_escape(&summary.canonical_url),
        image_meta = image_meta,
        twitter_card = twitter_card,
        name = html_escape(&summary.name),
        desc_body = html_escape(desc),
    )
}

fn html_response(status: StatusCode, body: String) -> Response {
    (
        status,
        [
            (header::CONTENT_TYPE, "text/html; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
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
            Json(json!({ "error": "Invalid tapp id" })),
        )
            .into_response(),
        Err(StatusCode::NOT_FOUND) => {
            (StatusCode::NOT_FOUND, Json(json!({ "error": "Not found" }))).into_response()
        }
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
        Ok(summary) => html_response(StatusCode::OK, render_tapp_seo_html(&summary)),
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

/// GET /api/seo/brew/{item_id} — JSON share summary for site-owner original articles only.
pub async fn brew_item_seo_summary(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(item_id): Path<i32>,
) -> Response {
    match resolve_brew_item_seo_summary(&db, &headers, item_id).await {
        Ok(summary) => (StatusCode::OK, Json(summary)).into_response(),
        Err(StatusCode::BAD_REQUEST) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "Invalid item id" })),
        )
            .into_response(),
        Err(StatusCode::NOT_FOUND) => {
            (StatusCode::NOT_FOUND, Json(json!({ "error": "Not found" }))).into_response()
        }
        Err(status) => (
            status,
            Json(json!({ "error": "Internal error", "code": "internal_error" })),
        )
            .into_response(),
    }
}

/// GET /brew/item/{item_id} — crawler HTML shell (own content only; 404 otherwise).
pub async fn brew_item_seo_html(
    State(db): State<DatabaseConnection>,
    headers: HeaderMap,
    Path(item_id): Path<i32>,
) -> Response {
    match resolve_brew_item_seo_summary(&db, &headers, item_id).await {
        Ok(summary) => html_response(StatusCode::OK, render_brew_item_seo_html(&summary)),
        Err(StatusCode::BAD_REQUEST) => html_response(
            StatusCode::BAD_REQUEST,
            simple_error_html("Bad request", "Invalid article id."),
        ),
        Err(StatusCode::NOT_FOUND) => html_response(
            StatusCode::NOT_FOUND,
            simple_error_html("Not found", "This article is not available."),
        ),
        Err(status) => html_response(
            status,
            simple_error_html("Error", "Unable to load article metadata."),
        ),
    }
}

fn simple_error_html(title: &str, message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN"><head>
<meta charset="UTF-8" /><meta name="robots" content="noindex, nofollow" />
<title>{title}</title></head>
<body><main style="max-width:40rem;margin:3rem auto;font-family:system-ui,sans-serif">
<h1>{title}</h1><p>{message}</p>
<p><a href="/tapp">All apps</a></p>
</main></body></html>
"#,
        title = html_escape(title),
        message = html_escape(message),
    )
}

/// GET /robots.txt — absolute Sitemap line only when durable env origin is set.
/// Disallow private SPA routes (login/setup/admin/playground); public modules
/// remain Allow. Client-side noindex is still applied on those pages for bots
/// that execute JS.
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
    let mut routes: Vec<(&str, &str)> = vec![("Home", "/")];
    for (key, path, label) in [
        ("library", "/library", "Library"),
        ("brew", "/brew", "Brew"),
        ("reports", "/reports", "Reports"),
        ("tapp", "/tapp", "Tapp"),
    ] {
        let level = modules.get(key).map(String::as_str).unwrap_or("all");
        if module_is_public_all(level) {
            routes.push((label, path));
        }
    }

    let body =
        crate::api::seo_policy::build_llms_txt(&branding.title, intro, base.as_deref(), &routes);
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
        ("brew", "/brew", "weekly"),
        ("reports", "/reports", "weekly"),
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

    // Brew: only site-owner original articles (category contains `我`).
    // Never include friend-links or third-party RSS items.
    let brew_level = modules.get("brew").map(String::as_str).unwrap_or("all");
    if module_is_public_all(brew_level) {
        if let Ok(sources) = brew_sources::Entity::find()
            .filter(brew_sources::Column::AdminOnly.eq(false))
            .all(&db)
            .await
        {
            let own_source_ids: Vec<i32> = sources
                .into_iter()
                .filter(|s| brew_source_is_own(&s.category, s.admin_only))
                .map(|s| s.id)
                .collect();
            if !own_source_ids.is_empty() {
                if let Ok(items) = brew_items::Entity::find()
                    .filter(brew_items::Column::SourceId.is_in(own_source_ids))
                    .order_by_desc(brew_items::Column::PublishedAt)
                    .limit(BREW_SITEMAP_ITEM_LIMIT)
                    .all(&db)
                    .await
                {
                    for item in items {
                        urls.push(SitemapUrl {
                            loc: format!("{base}{}", brew_item_path(item.id)),
                            lastmod: Some(item.published_at.format("%Y-%m-%d").to_string()),
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
    fn path_segment_encodes_unsafe_chars() {
        assert_eq!(encode_path_segment("com.example.app"), "com.example.app");
        assert!(encode_path_segment("a/b").contains("%2F"));
    }

    #[test]
    fn brew_own_category_gate() {
        assert!(brew_source_is_own(&Some("我".into()), false));
        assert!(brew_source_is_own(&Some("博客, 我".into()), false));
        assert!(brew_source_is_own(&Some("我, 随笔".into()), false));
        // Friend links and third-party feeds must never pass
        assert!(!brew_source_is_own(&Some("友情链接".into()), false));
        assert!(!brew_source_is_own(&Some("科技".into()), false));
        assert!(!brew_source_is_own(&None, false));
        assert!(!brew_source_is_own(&Some("我".into()), true)); // admin_only
                                                                // Substring false positive: 「我们」 is not the mine preset
        assert!(!brew_source_is_own(&Some("我们".into()), false));
    }

    #[test]
    fn strip_html_snippet_truncates() {
        let s = strip_html_snippet("<p>Hello <b>world</b> &amp; friends</p>", 200);
        assert_eq!(s, "Hello world & friends");
        let long = "a".repeat(200);
        let out = strip_html_snippet(&long, 20);
        assert!(out.ends_with('…'));
        assert!(out.chars().count() <= 20);
    }

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
    fn public_absolute_url_omits_origin_when_base_missing() {
        assert_eq!(public_absolute_url(None, "/tapp/run/x"), "/tapp/run/x");
        assert_eq!(
            public_absolute_url(Some("https://ex.com"), "/tapp/run/x"),
            "https://ex.com/tapp/run/x"
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
}
