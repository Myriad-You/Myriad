//! 公开 AP 文档的 HTTP 缓存校验（ETag / If-None-Match）。
//!
//! # 这解决什么
//!
//! Actor、followers/following、Outbox 都是**远端反复拉取**的公开文档。一个
//! 联邦网络里每个认识你的实例都会周期性重新获取 Actor 来刷新公钥与头像。
//!
//! # 这**不**解决什么
//!
//! ETag 由渲染后的文档内容算出，所以命中 304 时数据库查询**仍然发生**了 ——
//! 省下的是响应体传输与远端的解析成本。`Cache-Control: public, max-age=300`
//! is emitted; remotes may still GET / If-None-Match (not in our control).
//!
//! 不用 `updated_at` 之类的廉价校验器，是因为 Actor 文档由多张表拼成
//! （users + federation_keys + federation_domain_aliases），没有单一的
//! 修改时间戳能覆盖全部输入；用内容哈希则天然不会漏。

use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::Json;
use sha2::{Digest, Sha256};

/// 公开 AP 文档的缓存时长。
///
/// 300s. Actor JSON uses a stable avatar proxy URL (image updates do not
/// change this ETag). Key rotation changes `public_key_pem`. Inbound
/// `verify_request_signature` refetches on keyId mismatch, not on PEM verify fail.
pub const PUBLIC_DOC_MAX_AGE_SECS: u32 = 300;

/// 由文档内容算出强 ETag。
///
/// 用 SHA-256 前 16 个 hex 字符：碰撞概率可忽略，头部又不至于太长。
pub fn etag_for(value: &serde_json::Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap_or_default();
    let digest = Sha256::digest(&bytes);
    format!("\"{}\"", hex::encode(&digest[..8]))
}

/// 请求的 `If-None-Match` 是否匹配该 ETag。
///
/// 按 RFC 9110：`*` 匹配任何现有表示；否则逐项比较，并容忍 `W/` 弱前缀
/// （我们发强 ETag，但有些中间层会把它弱化后再回传）。
pub fn if_none_match_hits(headers: &HeaderMap, etag: &str) -> bool {
    let Some(raw) = headers
        .get(header::IF_NONE_MATCH)
        .and_then(|v| v.to_str().ok())
    else {
        return false;
    };
    let raw = raw.trim();
    if raw == "*" {
        return true;
    }
    raw.split(',')
        .map(str::trim)
        .map(|candidate| candidate.strip_prefix("W/").unwrap_or(candidate))
        .any(|candidate| candidate == etag)
}

/// 把一个公开 AP 文档渲染成带校验器的响应。
///
/// 命中 `If-None-Match` 时返回 304 且**不带 body**（RFC 9110 要求），
/// 但仍然回 `ETag` 与 `Cache-Control`，好让远端刷新有效期。
pub fn public_ap_document(
    headers: &HeaderMap,
    content_type: &str,
    body: serde_json::Value,
) -> Response {
    let etag = etag_for(&body);
    let cache_control = format!("public, max-age={PUBLIC_DOC_MAX_AGE_SECS}");

    if if_none_match_hits(headers, &etag) {
        return (
            StatusCode::NOT_MODIFIED,
            [(header::ETAG, etag), (header::CACHE_CONTROL, cache_control)],
        )
            .into_response();
    }

    (
        StatusCode::OK,
        [
            (header::ETAG, etag),
            (header::CACHE_CONTROL, cache_control),
            (header::CONTENT_TYPE, content_type.to_string()),
        ],
        Json(body),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn headers_with(value: &str) -> HeaderMap {
        let mut h = HeaderMap::new();
        h.insert(header::IF_NONE_MATCH, value.parse().unwrap());
        h
    }

    #[test]
    fn etag_is_stable_for_equal_documents() {
        let a = json!({"type": "Person", "id": "https://x/users/a"});
        let b = json!({"type": "Person", "id": "https://x/users/a"});
        assert_eq!(etag_for(&a), etag_for(&b));
    }

    #[test]
    fn etag_changes_when_content_changes() {
        let before = json!({"type": "Person", "name": "old"});
        let after = json!({"type": "Person", "name": "new"});
        assert_ne!(
            etag_for(&before),
            etag_for(&after),
            "a changed actor document must invalidate the cached validator"
        );
    }

    #[test]
    fn etag_is_a_quoted_token() {
        let e = etag_for(&json!({}));
        assert!(e.starts_with('"') && e.ends_with('"'), "got {e}");
        assert_eq!(e.len(), 18, "16 hex chars plus quotes");
    }

    #[test]
    fn if_none_match_matches_exact_and_star() {
        let etag = etag_for(&json!({"a": 1}));
        assert!(if_none_match_hits(&headers_with(&etag), &etag));
        assert!(if_none_match_hits(&headers_with("*"), &etag));
        assert!(!if_none_match_hits(&headers_with("\"nope\""), &etag));
        assert!(!if_none_match_hits(&HeaderMap::new(), &etag));
    }

    #[test]
    fn if_none_match_handles_lists_and_weak_prefix() {
        let etag = etag_for(&json!({"a": 1}));
        let list = format!("\"other\", {etag}");
        assert!(if_none_match_hits(&headers_with(&list), &etag));

        // 中间层可能把强 ETag 弱化后回传
        let weak = format!("W/{etag}");
        assert!(if_none_match_hits(&headers_with(&weak), &etag));
    }

    #[tokio::test]
    async fn matching_validator_yields_304_without_a_body() {
        let doc = json!({"type": "Person", "id": "https://x/users/a"});
        let etag = etag_for(&doc);

        let resp = public_ap_document(&headers_with(&etag), AP_CONTENT_TYPE, doc.clone());
        assert_eq!(resp.status(), StatusCode::NOT_MODIFIED);
        assert_eq!(
            resp.headers().get(header::ETAG).unwrap().to_str().unwrap(),
            etag
        );
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        assert!(body.is_empty(), "304 must not carry a body");
    }

    #[tokio::test]
    async fn stale_validator_yields_the_full_document() {
        let doc = json!({"type": "Person", "id": "https://x/users/a"});
        let resp = public_ap_document(&headers_with("\"stale\""), AP_CONTENT_TYPE, doc.clone());
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), 64 * 1024)
            .await
            .unwrap();
        let got: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(got, doc);
    }

    use crate::federation::types::AP_CONTENT_TYPE;
}

#[cfg(test)]
mod roundtrip_tests {
    /// 端到端验证条件请求：第一次拿 200 + ETag，带着它再问一次拿 304。
    ///
    /// 这条走真实 Router，覆盖"我们生成的 ETag 能被自己识别"这个闭环 ——
    /// 单测各自验证生成与匹配都通过、串起来却对不上，是这类改动的典型失败方式。
    #[tokio::test]
    async fn conditional_request_roundtrip() {
        use axum::http::{header, Request, StatusCode};
        use axum::{body::Body, routing::get, Router};
        use tower::ServiceExt;

        async fn handler(headers: axum::http::HeaderMap) -> axum::response::Response {
            super::public_ap_document(
                &headers,
                crate::federation::types::AP_CONTENT_TYPE,
                serde_json::json!({"type": "Person", "id": "https://x/users/a"}),
            )
        }

        let app = Router::new().route("/users/a", get(handler));

        // 第一次：完整文档 + 校验器
        let first = app
            .clone()
            .oneshot(Request::get("/users/a").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(first.status(), StatusCode::OK);
        let etag = first
            .headers()
            .get(header::ETAG)
            .expect("ETag must be present")
            .to_str()
            .unwrap()
            .to_string();
        assert!(first
            .headers()
            .get(header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("max-age"));

        // 第二次：带上刚拿到的校验器
        let second = app
            .oneshot(
                Request::get("/users/a")
                    .header(header::IF_NONE_MATCH, &etag)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(
            second.status(),
            StatusCode::NOT_MODIFIED,
            "an unchanged document must revalidate to 304"
        );
        let body = axum::body::to_bytes(second.into_body(), 64 * 1024)
            .await
            .unwrap();
        assert!(body.is_empty());
    }
}
