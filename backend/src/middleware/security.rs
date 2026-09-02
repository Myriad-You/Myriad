use axum::{extract::Request, http::header, middleware::Next, response::Response};
use std::env;

/// Security headers middleware - adds CSP and other security headers
pub async fn security_headers_middleware(req: Request, next: Next) -> Response {
    let mut response = next.run(req).await;
    let headers = response.headers_mut();

    // Content Security Policy (CSP)
    let is_production =
        env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string()) == "production";

    // img-src: dual-path image loading (MYR-039 / image proxy).
    // Non-hotlink https hosts are used as original URLs in <img src>, so CSP must
    // allow http(s) remote images. We avoid the bare `*` scheme wildcard (which
    // would also permit data-adjacent exotic schemes) while keeping dual-path working.
    // See docs/deployment/SECURITY_HEADERS.md §CSP img-src dual-path.
    const IMG_SRC: &str = "img-src 'self' data: blob: https: http:";

    let csp = if is_production {
        // PRODUCTION: Get allowed API origins from env (fallback to default)
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
            frame-ancestors 'none'; \
            frame-src 'none'; \
            worker-src 'self' blob:; \
            manifest-src 'self'; \
            upgrade-insecure-requests; ",
            allowed_api_origins
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

    if is_production || env::var("ENABLE_CSP_DEV").unwrap_or_default() == "true" {
        if let Ok(header_value) = csp.parse() {
            headers.insert(header::CONTENT_SECURITY_POLICY, header_value);
            tracing::debug!(
                "CSP enabled: {}",
                if is_production {
                    "production"
                } else {
                    "development"
                }
            );
        } else {
            tracing::error!("Failed to parse CSP header value");
        }
    } else {
        tracing::debug!("CSP disabled in development mode (set ENABLE_CSP_DEV=true to enable)");
    }

    // X-Content-Type-Options: 防止MIME类型嗅探
    headers.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());

    // X-Frame-Options: 防止点击劫持
    headers.insert(header::X_FRAME_OPTIONS, "DENY".parse().unwrap());

    // X-XSS-Protection: XSS过滤器（虽然现代浏览器已不需要，但为了兼容性保留）
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
    // 文档级策略由 proxy（补齐）+ Astro dev middleware 共同保证；此处覆盖 API 响应。
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

    response
}
