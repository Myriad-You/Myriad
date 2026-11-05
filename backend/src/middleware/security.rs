use axum::{extract::Request, http::header, middleware::Next, response::Response};
use std::env;

/// `/blog` 和 `/blog/`。`/blogger`、`/api/blog` 不算。
fn is_blog_path(path: &str) -> bool {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    path == "/blog" || path.starts_with("/blog/")
}

/// `/embed/*` 是给本站 `/blog` 页面用 iframe 嵌的评论和友链申请页，只允许同源框住。
fn is_embed_path(path: &str) -> bool {
    let path = path.split(['?', '#']).next().unwrap_or(path);
    path == "/embed" || path.starts_with("/embed/")
}

/// Security headers middleware - adds CSP and other security headers.
///
/// `/blog` 的 CSP 由页面自己定（`script-src 'none'`，但允许同源 iframe）。
/// 其它路由仍按环境覆盖 CSP，不能靠事先写入响应头绕过。
pub async fn security_headers_middleware(req: Request, next: Next) -> Response {
    let path = req.uri().path().to_string();
    let mut response = next.run(req).await;
    let is_production =
        env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string()) == "production";
    let csp_in_development = env::var("ENABLE_CSP_DEV").unwrap_or_default() == "true";
    finish_security_headers(
        &path,
        response.headers_mut(),
        is_production,
        csp_in_development,
    );
    response
}

pub(crate) fn finish_security_headers(
    path: &str,
    headers: &mut axum::http::HeaderMap,
    is_production: bool,
    csp_in_development: bool,
) {
    if is_blog_path(path) {
        if let Ok(value) = myriad_blog::BLOG_CSP.parse() {
            headers.insert(header::CONTENT_SECURITY_POLICY, value);
        }
        headers.insert(header::X_FRAME_OPTIONS, "SAMEORIGIN".parse().unwrap());
    } else {
        // img-src: dual-path image loading (image proxy).
        // Non-hotlink https hosts are used as original URLs in <img src>, so CSP must
        // allow http(s) remote images. We avoid the bare `*` scheme wildcard (which
        // would also permit data-adjacent exotic schemes) while keeping dual-path working.
        // See docs/deployment/SECURITY_HEADERS.md §CSP img-src dual-path.
        const IMG_SRC: &str = "img-src 'self' data: blob: https: http:";
        let embed = is_embed_path(path);
        let frame_ancestors = if embed { "'self'" } else { "'none'" };

        let csp = if is_production {
            // PRODUCTION: connect-src list from CSP_CONNECT_SRC (fallback to default)
            // wss/stun/turn: Shengwang realtime talk (Agora RTC) plus any other WebRTC.
            let allowed_api_origins = env::var("CSP_CONNECT_SRC")
                .unwrap_or_else(|_| "'self' https: wss: stun: turn:".to_string());

            // PRODUCTION: Strict CSP for executable content, relaxed for assets
            format!(
                "default-src 'self'; \
            script-src 'self'; \
            style-src 'self' 'unsafe-inline' https:; \
            {IMG_SRC}; \
            font-src 'self' data: https: blob:; \
            media-src 'self' https: blob:; \
            connect-src {}; \
            object-src 'none'; \
            base-uri 'self'; \
            form-action 'self'; \
            frame-ancestors {}; \
            frame-src 'none'; \
            worker-src 'self' blob:; \
            manifest-src 'self' blob:; \
            upgrade-insecure-requests; ",
                allowed_api_origins, frame_ancestors
            )
        } else {
            // DEVELOPMENT: Relaxed CSP for hot reload and dev tools
            format!(
                "default-src 'self'; \
            script-src 'self' 'unsafe-inline' 'unsafe-eval'; \
            style-src 'self' 'unsafe-inline' https:; \
            {IMG_SRC}; \
            font-src 'self' data: https: blob:; \
            media-src 'self' https: blob:; \
            connect-src 'self' ws: wss: https: http:; \
            object-src 'none'; \
            base-uri 'self'; \
            worker-src 'self' blob:; "
            )
        };

        if is_production || csp_in_development {
            match csp.parse() {
                Ok(header_value) => {
                    headers.insert(header::CONTENT_SECURITY_POLICY, header_value);
                    tracing::debug!(
                        "CSP enabled: {}",
                        if is_production {
                            "production"
                        } else {
                            "development"
                        }
                    );
                }
                _ => {
                    tracing::error!("Failed to parse CSP header value");
                }
            }
        } else {
            tracing::debug!("CSP disabled in development mode (set ENABLE_CSP_DEV=true to enable)");
        }

        // X-Frame-Options: 防止点击劫持。嵌入页用 SAMEORIGIN，和 frame-ancestors 'self' 一致。
        let frame_options = if embed { "SAMEORIGIN" } else { "DENY" };
        headers.insert(header::X_FRAME_OPTIONS, frame_options.parse().unwrap());
    }

    // X-Content-Type-Options: 防止MIME类型嗅探
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());

    // X-XSS-Protection: 1; mode=block
    headers.insert(
        "X-XSS-Protection".parse::<header::HeaderName>().unwrap(),
        "1; mode=block".parse().unwrap(),
    );

    // Referrer-Policy: 控制Referrer信息泄漏
    headers.insert(
        header::REFERRER_POLICY,
        "strict-origin-when-cross-origin".parse().unwrap(),
    );

    // Permissions-Policy: 本站可定位（天气）和用麦克风（听/说）；摄像头仍禁用。
    // 文档级策略由 proxy（补齐）+ Vite dev middleware 共同保证；此处覆盖 API 响应。
    // 字符串须与 proxy PERMISSIONS_POLICY / frontend DOCUMENT_PERMISSIONS_POLICY 保持一致。
    headers.insert(
        "Permissions-Policy".parse::<header::HeaderName>().unwrap(),
        "geolocation=(self), microphone=(self), camera=()"
            .parse()
            .unwrap(),
    );

    // Strict-Transport-Security: 强制HTTPS（仅生产环境）
    if is_production {
        headers.insert(
            header::STRICT_TRANSPORT_SECURITY,
            "max-age=31536000; includeSubDomains".parse().unwrap(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderMap;
    use axum::response::IntoResponse;

    #[test]
    fn blog_csp_is_kept_in_development_and_other_routes_cannot_opt_out() {
        let mut blog = HeaderMap::new();
        blog.insert(
            header::CONTENT_SECURITY_POLICY,
            "default-src 'none'".parse().unwrap(),
        );
        finish_security_headers("/blog/posts/hello/", &mut blog, false, false);
        assert_eq!(
            blog.get(header::CONTENT_SECURITY_POLICY).unwrap(),
            myriad_blog::BLOG_CSP
        );
        assert_eq!(blog.get(header::X_FRAME_OPTIONS).unwrap(), "SAMEORIGIN");
        assert!(!myriad_blog::BLOG_CSP.contains("frame-src 'none'"));
        assert!(myriad_blog::BLOG_CSP.contains("frame-src 'self'"));
        assert!(myriad_blog::BLOG_CSP.contains("script-src 'none'"));
        // 笔记里的外链图片要能显示；脚本和 frame 的封锁不因此放松。
        assert!(myriad_blog::BLOG_CSP.contains("img-src 'self' data: https: http:"));

        let mut bare = HeaderMap::new();
        finish_security_headers("/blog", &mut bare, false, false);
        assert_eq!(
            bare.get(header::CONTENT_SECURITY_POLICY).unwrap(),
            myriad_blog::BLOG_CSP
        );

        let mut api = HeaderMap::new();
        api.insert(
            header::CONTENT_SECURITY_POLICY,
            myriad_blog::BLOG_CSP.parse().unwrap(),
        );
        finish_security_headers("/api/blog/settings", &mut api, true, false);
        let csp = api
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(csp.contains("frame-src 'none'"));
        assert!(csp.contains("frame-ancestors 'none'"));
        assert_ne!(csp, myriad_blog::BLOG_CSP);
        assert_eq!(api.get(header::X_FRAME_OPTIONS).unwrap(), "DENY");

        let mut dev = HeaderMap::new();
        finish_security_headers("/api/health", &mut dev, false, false);
        assert!(dev.get(header::CONTENT_SECURITY_POLICY).is_none());
        assert_eq!(dev.get(header::X_FRAME_OPTIONS).unwrap(), "DENY");

        let mut near = HeaderMap::new();
        finish_security_headers("/blogger", &mut near, false, false);
        assert!(near.get(header::CONTENT_SECURITY_POLICY).is_none());
    }

    #[test]
    fn embed_pages_may_only_be_framed_by_this_origin() {
        let mut embed = HeaderMap::new();
        finish_security_headers("/embed/comments/12", &mut embed, true, false);
        let csp = embed
            .get(header::CONTENT_SECURITY_POLICY)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(csp.contains("frame-ancestors 'self'"));
        assert!(csp.contains("script-src 'self';"));
        assert!(csp.contains("manifest-src 'self' blob:;"));
        assert_eq!(embed.get(header::X_FRAME_OPTIONS).unwrap(), "SAMEORIGIN");

        let mut dev = HeaderMap::new();
        finish_security_headers("/embed/friend-apply", &mut dev, false, false);
        assert_eq!(dev.get(header::X_FRAME_OPTIONS).unwrap(), "SAMEORIGIN");

        let mut near = HeaderMap::new();
        finish_security_headers("/embedded", &mut near, true, false);
        assert_eq!(near.get(header::X_FRAME_OPTIONS).unwrap(), "DENY");
    }

    #[tokio::test]
    async fn middleware_sends_blog_csp_without_production() {
        use axum::body::Body;
        use axum::routing::get;
        use tower::ServiceExt;

        let app = axum::Router::new()
            .route(
                "/blog/",
                get(|| async {
                    let mut response = "ok".into_response();
                    response.headers_mut().insert(
                        header::CONTENT_SECURITY_POLICY,
                        "default-src 'none'".parse().unwrap(),
                    );
                    response
                }),
            )
            .route("/api/health", get(|| async { "ok" }))
            .layer(axum::middleware::from_fn(security_headers_middleware));
        let response = app
            .oneshot(
                axum::http::Request::builder()
                    .uri("/blog/")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_SECURITY_POLICY)
                .unwrap(),
            myriad_blog::BLOG_CSP
        );
        assert_eq!(
            response.headers().get(header::X_FRAME_OPTIONS).unwrap(),
            "SAMEORIGIN"
        );
    }
}
