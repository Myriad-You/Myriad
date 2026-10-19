//! 博客设置、主题和 `/blog/*` 静态页。

use axum::body::Body;
use axum::extract::{FromRequest, Multipart, Path, Request, State};
use axum::http::{HeaderValue, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use sea_orm::DatabaseConnection;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::error::HttpError;
use crate::extract::{AdminClaims, OptionalViewer};
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
        .route("/api/blog/rebuild", post(post_rebuild))
        .route_layer(from_fn_with_state(
            app_state.clone(),
            crate::middleware::auth::admin_middleware,
        ));
    let site = Router::<crate::state::AppState>::new()
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
    crate::api::phantasi::require_phantasi_module_access(&db, user_id, viewer.is_admin).await?;
    let path = request.uri().path().to_string();
    let query = request.uri().query().map(str::to_string);
    let current = blog::output_dir();
    let target = resolve_blog_target(&current, &path);
    let studio = if viewer.is_admin {
        Some(studio_island(&db, &path, query.as_deref()).await)
    } else {
        None
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
        BlogTarget::File(file) => match &studio {
            Some(island) => studio_file_response(&file, StatusCode::OK, island),
            None => file_response(&file, StatusCode::OK, user_id.is_none()),
        },
        BlogTarget::Missing => match (theme_not_found_page(&current), &studio) {
            (Some(page), Some(island)) => studio_file_response(&page, StatusCode::NOT_FOUND, island),
            (Some(page), None) => file_response(&page, StatusCode::NOT_FOUND, user_id.is_none()),
            // 博客还没生成出来：站长照样拿到创作台，才能去博客设置里修。
            (None, Some(island)) => studio_html_response(
                StatusCode::NOT_FOUND,
                myriad_blog::studio_only_page(island, "Not found"),
            ),
            (None, None) => (StatusCode::NOT_FOUND, "Not found").into_response(),
        },
    };
    let mut response = response;
    if let Ok(value) = myriad_blog::BLOG_CSP.parse() {
        response
            .headers_mut()
            .insert(header::CONTENT_SECURITY_POLICY, value);
    }
    if request.method() == axum::http::Method::HEAD {
        *response.body_mut() = Body::empty();
    }
    Ok(response)
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

/// 只有 HTML 才插岛；样式、图片照常返回，但同样不能进共享缓存。
fn studio_file_response(path: &std::path::Path, status: StatusCode, island: &str) -> Response {
    let content_type = content_type_for(path);
    if !content_type.starts_with("text/html") {
        let mut response = file_response(path, status, false);
        response
            .headers_mut()
            .insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
        return response;
    }
    match std::fs::read_to_string(path) {
        Ok(html) => studio_html_response(status, myriad_blog::inject_studio_island(&html, island)),
        Err(_) => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

fn studio_html_response(status: StatusCode, html: String) -> Response {
    let mut response = (status, html).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("text/html; charset=utf-8"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("private, no-store"));
    response
}

/// 登录访客能看到的页面可能对匿名访客关着，不能进共享缓存。
fn file_response(path: &std::path::Path, status: StatusCode, anonymous: bool) -> Response {
    match std::fs::read(path) {
        Ok(bytes) => {
            let mut response = (status, bytes).into_response();
            if let Ok(value) = HeaderValue::from_str(content_type_for(path)) {
                response.headers_mut().insert(header::CONTENT_TYPE, value);
            }
            response.headers_mut().insert(
                header::CACHE_CONTROL,
                HeaderValue::from_static(if anonymous {
                    "public, max-age=60"
                } else {
                    "private, max-age=60"
                }),
            );
            response
        }
        Err(_) => (StatusCode::NOT_FOUND, "Not found").into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::{post_slug, trailing_slash};

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
