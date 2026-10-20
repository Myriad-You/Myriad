//! 博客设置、主题和 `/blog/*` 静态页。

use axum::body::{Body, Bytes};
use axum::extract::{FromRequest, Multipart, Path, Request, State};
use axum::http::{HeaderName, HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post, put};
use axum::{Json, Router};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::HttpError;
use crate::extract::{AdminClaims, OptionalViewer};
mod page_cache;

use crate::services::blog::{
    self, BlogTarget, content_type_for, resolve_blog_target, theme_not_found_page,
};

pub fn router(app_state: crate::state::AppState) -> Router<crate::state::AppState> {
    use axum::middleware::from_fn_with_state;
    let admin = Router::<crate::state::AppState>::new()
        .route("/api/blog/settings", get(get_settings).put(put_settings))
        .route(
            "/api/blog/themes",
            post(post_theme).layer(axum::extract::DefaultBodyLimit::max(
                myriad_blog::MAX_THEME_ARCHIVE_BYTES as usize + 1024 * 1024,
            )),
        )
        .route("/api/blog/themes/{id}", delete(delete_theme))
        .route("/api/blog/themes/{id}/scheme", put(put_theme_scheme))
        .route("/api/blog/rebuild", post(post_rebuild))
        .route_layer(from_fn_with_state(
            app_state.clone(),
            crate::middleware::auth::admin_middleware,
        ));
    let site = Router::<crate::state::AppState>::new()
        .route(
            myriad_blog::NAV_SPECULATION_RULES_PATH,
            get(speculation_rules),
        )
        .route("/blog", get(serve_blog).head(serve_blog))
        .route("/blog/", get(serve_blog).head(serve_blog))
        .route("/blog/{*path}", get(serve_blog).head(serve_blog))
        .route_layer(from_fn_with_state(
            app_state,
            crate::middleware::auth::optional_current_admin_auth_middleware,
        ));
    admin.merge(site)
}

fn blog_error(status: u16, message: &str) -> HttpError {
    let status = StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_REQUEST);
    HttpError::from((
        status,
        Json(json!({
            "error": message,
        })),
    ))
}

async fn get_settings(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
) -> Result<Json<blog::BlogSnapshot>, HttpError> {
    Ok(Json(blog::snapshot(&db).await))
}

#[derive(Deserialize)]
struct SettingsPut {
    theme_id: String,
    site_title: String,
    description: String,
    params: Value,
}

async fn put_settings(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Json(body): Json<SettingsPut>,
) -> Result<Json<blog::BlogSnapshot>, HttpError> {
    blog::save_settings(
        &db,
        &body.theme_id,
        &body.site_title,
        &body.description,
        body.params,
    )
    .await
    .map(Json)
    .map_err(|(message, status)| blog_error(status, message))
}

#[derive(Deserialize)]
struct ThemeUrl {
    url: String,
}

async fn post_theme(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    request: Request,
) -> Result<Json<Value>, HttpError> {
    let content_type = request
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    let record = if content_type.starts_with("multipart/") {
        let mut multipart = Multipart::from_request(request, &())
            .await
            .map_err(|_| blog_error(400, "Theme upload could not be read"))?;
        let mut collected = None;
        while let Some(mut field) = multipart
            .next_field()
            .await
            .map_err(|_| blog_error(400, "Theme upload could not be read"))?
        {
            if field.name() != Some("file") {
                continue;
            }
            let mut bytes = Vec::new();
            while let Some(chunk) = field
                .chunk()
                .await
                .map_err(|_| blog_error(400, "Theme upload could not be read"))?
            {
                if bytes.len().saturating_add(chunk.len())
                    > myriad_blog::MAX_THEME_ARCHIVE_BYTES as usize
                {
                    return Err(blog_error(413, "Theme archive is too large"));
                }
                bytes.extend_from_slice(&chunk);
            }
            collected = Some(bytes);
            break;
        }
        let Some(bytes) = collected else {
            return Err(blog_error(400, "A theme file is required"));
        };
        blog::install_archive(&db, bytes)
            .await
            .map_err(|(message, status)| blog_error(status, message))?
    } else {
        let bytes = axum::body::to_bytes(request.into_body(), 16 * 1024)
            .await
            .map_err(|_| blog_error(400, "A theme URL is required"))?;
        let body: ThemeUrl = serde_json::from_slice(&bytes)
            .map_err(|_| blog_error(400, "A theme URL is required"))?;
        blog::install_from_url(&db, &body.url)
            .await
            .map_err(|(message, status)| blog_error(status, message))?
    };
    Ok(Json(json!({ "theme": record })))
}

async fn delete_theme(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path(id): Path<String>,
) -> Result<Json<Value>, HttpError> {
    blog::remove_theme(&db, &id)
        .await
        .map_err(|(message, status)| blog_error(status, message))?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct SchemePut {
    switch: myriad_blog::SchemeSwitch,
}

/// 站长确认或改了主题的深浅色切换办法。
async fn put_theme_scheme(
    State(db): State<DatabaseConnection>,
    _admin: AdminClaims,
    Path(id): Path<String>,
    Json(body): Json<SchemePut>,
) -> Result<Json<Value>, HttpError> {
    let theme = blog::set_theme_scheme(&db, &id, body.switch)
        .await
        .map_err(|(message, status)| blog_error(status, message))?;
    Ok(Json(json!({ "theme": theme })))
}

async fn post_rebuild(_admin: AdminClaims) -> Json<Value> {
    blog::request_rebuild();
    let build = blog::build_view().await;
    Json(json!({ "build": build }))
}

async fn serve_blog(
    State(db): State<DatabaseConnection>,
    viewer: OptionalViewer,
    request: Request,
) -> Result<Response, HttpError> {
    let user_id = viewer
        .claims
        .as_ref()
        .and_then(|claims| claims.subject().map(|subject| subject.id()));
    let preferences =
        crate::api::phantasi::require_phantasi_module_access_cached(&db, user_id, viewer.is_admin)
            .await?;
    let path = request.uri().path().to_string();
    let query = request.uri().query().map(str::to_string);
    let current = blog::output_dir();
    let target = resolve_blog_target(&current, &path);
    let anonymous = user_id.is_none();
    let if_none_match = request.headers().get(header::IF_NONE_MATCH);
    // 样式、图片、字体照常返回：导航岛和创作台只有 HTML 才用得上，不为它们拼。
    // 它们不含创作台，站长拿到的和其他登录访客一样不进共享缓存就够了。
    if let BlogTarget::File(file) = &target
        && !is_html(file)
    {
        let mut response = file_response(file, StatusCode::OK, anonymous, if_none_match).await;
        finish_blog_response(&mut response, request.method());
        return Ok(response);
    }
    let site = site_chrome(request.headers());
    let chrome = PageChrome {
        nav: blog_nav(
            &preferences,
            user_id,
            viewer.is_admin,
            crate::api::reports::locale::host_locale_from_headers(request.headers()),
            site.as_ref(),
        ),
        personal: site.is_some(),
        dark: site.as_ref().map(myriad_blog::SiteChrome::is_dark),
        studio: if viewer.is_admin {
            Some(studio_island(&db, &path, query.as_deref()).await)
        } else {
            None
        },
    };
    let response = match target {
        BlogTarget::Redirect(location) => {
            let location = match &query {
                Some(query) => format!("{location}?{query}"),
                None => location,
            };
            let mut response = StatusCode::MOVED_PERMANENTLY.into_response();
            if let Ok(value) = HeaderValue::from_str(&location) {
                response.headers_mut().insert(header::LOCATION, value);
            }
            response
        }
        BlogTarget::File(file) => {
            page_response(&file, StatusCode::OK, anonymous, &chrome, if_none_match).await
        }
        BlogTarget::Missing => match (theme_not_found_page(&current), &chrome.studio) {
            (Some(page), _) => {
                page_response(&page, StatusCode::NOT_FOUND, anonymous, &chrome, None).await
            }
            // 博客还没生成出来：站长照样拿到创作台，才能去博客设置里修。
            (None, Some(island)) => html_response(
                StatusCode::NOT_FOUND,
                myriad_blog::studio_only_page(island, "Not found"),
                "private, no-store",
            ),
            (None, None) => (StatusCode::NOT_FOUND, "Not found").into_response(),
        },
    };
    let mut response = response;
    finish_blog_response(&mut response, request.method());
    Ok(response)
}

fn finish_blog_response(response: &mut Response, method: &axum::http::Method) {
    if let Ok(value) = myriad_blog::BLOG_CSP.parse() {
        response
            .headers_mut()
            .insert(header::CONTENT_SECURITY_POLICY, value);
    }
    if method == axum::http::Method::HEAD {
        *response.body_mut() = Body::empty();
    }
}

fn is_html(path: &std::path::Path) -> bool {
    content_type_for(path).starts_with("text/html")
}

/// 站长看到的创作台覆盖层。`?studio=editor-12` 让它展开并打开那个工具；文章页带上笔记的 id。
async fn studio_island(db: &DatabaseConnection, path: &str, query: Option<&str>) -> String {
    let requested = query.and_then(|query| {
        url::form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == "studio")
            .map(|(_, value)| value.into_owned())
    });
    let (item_id, doc_id) = match post_slug(path) {
        Some(slug) => post_note_ids(db, slug).await.unwrap_or((None, None)),
        None => (None, None),
    };
    myriad_blog::studio_overlay_html(&myriad_blog::StudioPageContext {
        path: trailing_slash(path),
        item_id,
        doc_id,
        open: myriad_blog::studio_open_spec(requested.as_deref()),
    })
}

fn trailing_slash(path: &str) -> String {
    if path.ends_with('/') {
        path.to_string()
    } else {
        format!("{path}/")
    }
}

/// `/blog/posts/{slug}/` 里的 slug。
fn post_slug(path: &str) -> Option<&str> {
    let rest = path.strip_prefix("/blog/posts/")?;
    let slug = rest.strip_suffix('/').unwrap_or(rest);
    (!slug.is_empty() && !slug.contains('/')).then_some(slug)
}

/// 文章对应的笔记条目和文档。slug 为空的笔记在博客上用 `n{id}`。
async fn post_note_ids(db: &DatabaseConnection, slug: &str) -> Option<(Option<i32>, Option<i32>)> {
    use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
    let fallback_id = slug
        .strip_prefix('n')
        .and_then(|digits| digits.parse::<i32>().ok());
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            r#"
            SELECT i.id AS item_id, d.id AS doc_id
            FROM phantasi_items i
            INNER JOIN phantasi_sources s ON s.id = i.source_id
            LEFT JOIN phantasi_note_docs d ON d.item_id = i.id
            WHERE s.source_type = 'note'
              AND (i.slug = $1 OR ($2::int IS NOT NULL AND i.id = $2 AND COALESCE(i.slug, '') = ''))
            ORDER BY d.id
            LIMIT 1
            "#,
            vec![slug.into(), fallback_id.into()],
        ))
        .await
        .ok()??;
    Some((
        row.try_get::<i32>("", "item_id").ok(),
        row.try_get::<Option<i32>>("", "doc_id").ok().flatten(),
    ))
}

/// 每个博客页面都带上的东西：导航岛，站长还有创作台覆盖层。
struct PageChrome {
    nav: String,
    /// 导航岛用了这位访客在主站的颜色：页面不能进共享缓存。
    personal: bool,
    /// 主站现在是不是深色；没有 cookie 就不替主题切，让它照自己的默认来。
    dark: Option<bool>,
    studio: Option<String>,
}

/// 主站写进 cookie 的颜色（frontend/src/utils/siteChromeCookie.ts），格式不对就当没有。
fn site_chrome(headers: &axum::http::HeaderMap) -> Option<myriad_blog::SiteChrome> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(';'))
        .find_map(|pair| {
            let (name, value) = pair.trim().split_once('=')?;
            (name == myriad_blog::SITE_CHROME_COOKIE).then_some(value)
        })
        .and_then(myriad_blog::SiteChrome::parse)
}

/// 博客页上的导航岛：和主站一样按模块可见性挑条目，文案按访客的语言。
fn blog_nav(
    preferences: &myriad_module_visibility::ModuleVisibilityPreferences,
    user_id: Option<i32>,
    is_admin: bool,
    locale: &str,
    site: Option<&myriad_blog::SiteChrome>,
) -> String {
    myriad_blog::nav_island_html(
        |module| {
            crate::api::phantasi::phantasi_access_allowed(
                preferences.module_visibility(module),
                user_id,
                is_admin,
            )
        },
        |key| crate::i18n::nav(locale, key),
        site,
    )
}

/// 博客页回主站的预渲染规则（`Speculation-Rules` 响应头指向这里）。
async fn speculation_rules() -> Response {
    let mut response = myriad_blog::NAV_SPECULATION_RULES.into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/speculationrules+json"),
    );
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("public, max-age=3600"),
    );
    response
}

/// 只有 HTML 才插导航岛和创作台；主题的 404 页之外的样式、图片在 `serve_blog` 里就返回了。
/// 访客的页面拼一次就记下（[`page_cache`]），带上内容摘要做 ETag；站长的带创作台，每次现拼、不缓存。
async fn page_response(
    path: &std::path::Path,
    status: StatusCode,
    anonymous: bool,
    chrome: &PageChrome,
    if_none_match: Option<&HeaderValue>,
) -> Response {
    if !is_html(path) {
        return file_response(path, status, anonymous, if_none_match).await;
    }
    // 主题怎么切深浅色记在构建出的 `<html>` 上，这里按主站现在的那一边替它切。
    let decorate = |html: String, studio: Option<&str>| {
        let html = chrome
            .dark
            .and_then(|dark| myriad_blog::apply_page_scheme(&html, dark))
            .unwrap_or(html);
        // 站长还能看到藏着的伴读岛，没有注释和播客的文章也能在面板里补生成。
        myriad_blog::decorate_page(&html, myriad_blog::nav_island_head(), &chrome.nav, studio)
    };
    let (body, etag, cache) = match &chrome.studio {
        Some(island) => {
            let Ok(html) = tokio::fs::read_to_string(path).await else {
                return (StatusCode::NOT_FOUND, "Not found").into_response();
            };
            (
                Bytes::from(decorate(html, Some(island))),
                None,
                "private, no-store",
            )
        }
        None => {
            let Some(page) =
                page_cache::page(path, chrome.dark, &chrome.nav, |html| decorate(html, None)).await
            else {
                return (StatusCode::NOT_FOUND, "Not found").into_response();
            };
            let cache = if anonymous && !chrome.personal {
                "public, max-age=60"
            } else {
                "private, max-age=60"
            };
            (page.body, Some(page.etag), cache)
        }
    };
    let not_modified = status == StatusCode::OK
        && etag
            .as_deref()
            .is_some_and(|etag| page_cache::etag_matches(if_none_match, etag));
    let mut response = if not_modified {
        cached_response(StatusCode::NOT_MODIFIED, Body::empty(), cache)
    } else {
        html_response(status, body, cache)
    };
    if let Some(value) = etag.and_then(|etag| HeaderValue::from_str(&etag).ok()) {
        response.headers_mut().insert(header::ETAG, value);
    }
    if let Ok(value) =
        HeaderValue::from_str(&format!("\"{}\"", myriad_blog::NAV_SPECULATION_RULES_PATH))
    {
        response
            .headers_mut()
            .insert(HeaderName::from_static("speculation-rules"), value);
    }
    // 导航岛的文案跟着访客的语言走，颜色跟着主站的 cookie。
    response.headers_mut().insert(
        header::VARY,
        HeaderValue::from_static("Accept-Language, Cookie"),
    );
    response
}

fn html_response(status: StatusCode, html: impl Into<Body>, cache: &'static str) -> Response {
    let mut response = cached_response(status, html.into(), cache);
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
}

fn cached_response(status: StatusCode, body: Body, cache: &'static str) -> Response {
    let mut response = (status, body).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    response
}

/// 登录访客能看到的页面可能对匿名访客关着，不能进共享缓存。
/// ETag 用长度和修改时间：没变的样式、图片回 304，连文件都不读。
async fn file_response(
    path: &std::path::Path,
    status: StatusCode,
    anonymous: bool,
    if_none_match: Option<&HeaderValue>,
) -> Response {
    let Ok(meta) = tokio::fs::metadata(path).await else {
        return (StatusCode::NOT_FOUND, "Not found").into_response();
    };
    let cache = if anonymous {
        "public, max-age=60"
    } else {
        "private, max-age=60"
    };
    let etag = page_cache::file_etag(&meta);
    let mut response = if status == StatusCode::OK && page_cache::etag_matches(if_none_match, &etag)
    {
        cached_response(StatusCode::NOT_MODIFIED, Body::empty(), cache)
    } else {
        let Ok(bytes) = tokio::fs::read(path).await else {
            return (StatusCode::NOT_FOUND, "Not found").into_response();
        };
        let mut response = cached_response(status, Body::from(bytes), cache);
        if let Ok(value) = HeaderValue::from_str(content_type_for(path)) {
            response.headers_mut().insert(header::CONTENT_TYPE, value);
        }
        response
    };
    if let Ok(value) = HeaderValue::from_str(&etag) {
        response.headers_mut().insert(header::ETAG, value);
    }
    response
}

#[cfg(test)]
mod tests {
    use super::{post_slug, site_chrome, trailing_slash};
    use axum::http::header;

    #[test]
    fn site_chrome_is_read_from_any_cookie_header() {
        let mut headers = axum::http::HeaderMap::new();
        assert_eq!(site_chrome(&headers), None);
        headers.append(header::COOKIE, "a=1".parse().unwrap());
        headers.append(
            header::COOKIE,
            "myriad_chrome_x=no; myriad_chrome=auto.dark.6b7280.c7d2fe"
                .parse()
                .unwrap(),
        );
        assert_eq!(
            site_chrome(&headers),
            myriad_blog::SiteChrome::parse("auto.dark.6b7280.c7d2fe")
        );
        let mut bad = axum::http::HeaderMap::new();
        bad.insert(
            header::COOKIE,
            "myriad_chrome=auto.dark.6b7280.c7d2f\"".parse().unwrap(),
        );
        assert_eq!(site_chrome(&bad), None);
    }

    #[test]
    fn post_slug_reads_only_post_pages() {
        assert_eq!(post_slug("/blog/posts/hello/"), Some("hello"));
        assert_eq!(post_slug("/blog/posts/hello"), Some("hello"));
        assert_eq!(post_slug("/blog/posts/"), None);
        assert_eq!(post_slug("/blog/posts/a/b/"), None);
        assert_eq!(post_slug("/blog/friends/"), None);
        assert_eq!(trailing_slash("/blog/posts/x"), "/blog/posts/x/");
    }
}
