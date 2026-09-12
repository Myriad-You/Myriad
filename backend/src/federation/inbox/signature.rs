//! Inbox HTTP Signature pre-parse gate and request verification.

use axum::{
    http::{HeaderMap, StatusCode},
    Json,
};
use sea_orm::DatabaseConnection;
use serde_json::json;

use crate::federation::actor::{
    fetch_remote_actor_for_verify, persist_verified_remote_actor, ResolvedRemoteActor,
};
use crate::federation::signature::{
    parse_signature_header, require_covered_headers, verify_date_freshness, verify_digest,
    verify_signature, HTTP_DATE_MAX_SKEW,
};
use crate::federation::types::*;

fn inbox_auth_reject(
    public: &'static str,
    error: impl std::fmt::Display,
) -> (StatusCode, Json<serde_json::Value>) {
    tracing::warn!(%error, public, "inbox signature rejected");
    (
        StatusCode::UNAUTHORIZED,
        Json(AppError::public_json(public)),
    )
}

// HTTP Signature 验证

/// 读取一个**只允许出现一次**的请求头。
///
/// HTTP 允许同名 header 重复，而这里的两条读取路径对重复的处理正好相反：
/// [`HeaderMap::get`] 取**第一个**值，`headers.iter().collect::<HashMap<_,_>>()`
/// 留下**最后一个**。于是一个带两个 `Date`（或两个 `Digest`）的请求可以用第一个
/// 值通过新鲜度 / 摘要闸门，再用第二个值去重建签名字符串 —— 攻击者只要重放一条
/// 抓到的签名，配上自己构造的 body 和一对重复头，就能让任意内容被认成对方签发。
///
/// 这种歧义从来不是正常流量，一律 401。注意重复检查只作用于签名真正覆盖的
/// header：`Via` / `X-Forwarded-For` 之类的重复是合法且无害的。
fn unique_header<'a>(
    headers: &'a HeaderMap,
    name: &str,
) -> Result<Option<&'a str>, (StatusCode, Json<serde_json::Value>)> {
    let mut values = headers.get_all(name).iter();
    let Some(first) = values.next() else {
        return Ok(None);
    };
    if values.next().is_some() {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "error": "Ambiguous request: a signed header appears more than once",
            })),
        ));
    }
    first.to_str().map(Some).map_err(|_| {
        (
            StatusCode::BAD_REQUEST,
            Json(AppError::public_json("Invalid request header encoding")),
        )
    })
}

/// 为重建签名字符串收集 header 值。
///
/// 只收集签名 `headers` 参数里列出的名字，且每个都走 [`unique_header`]。
/// 缺失的 header 不放进 map —— 由 `verify_signature` 报
/// "Missing header for signature"，保持原有错误语义。
fn signing_header_map(
    headers: &HeaderMap,
    covered: &[String],
) -> Result<std::collections::HashMap<String, String>, (StatusCode, Json<serde_json::Value>)> {
    let mut map = std::collections::HashMap::with_capacity(covered.len());
    for name in covered {
        if name == "(request-target)" {
            continue;
        }
        if let Some(value) = unique_header(headers, name.as_str())? {
            map.insert(name.clone(), value.to_string());
        }
    }
    Ok(map)
}

/// 解析请求体**之前**必须通过的检查。
///
/// inbox 允许 INBOX_BODY_LIMIT 的请求体（更大的文件必须走分块端点）。
/// 只依赖 header 和原始字节、不需要网络往返的检查在解析 JSON 之前：
/// Signature 头存在且可解析、签名覆盖的 header 集合合规、Date 新鲜、
/// Digest 与原始 body 逐字节相符。攻击者要让我们开始解析 JSON，至少得先算出
/// 这段 body 正确的 SHA-256 并附上格式合法的签名头。
///
/// 注意这**不是**认证 —— 真正的签名验证仍然在 [`verify_request_signature`] 里
/// 完成（需要先解析出 actor 才能取公钥）。这只是把最廉价的拒绝点前移。
pub(crate) fn verify_preparse_gate(
    headers: &HeaderMap,
    body: &[u8],
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    let sig_header = unique_header(headers, "signature")?.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(AppError::public_json("Missing Signature header")),
        )
    })?;

    let parsed = parse_signature_header(sig_header)
        .map_err(|error| inbox_auth_reject("Invalid Signature header", error))?;
    require_covered_headers(&parsed, !body.is_empty())
        .map_err(|error| inbox_auth_reject("Invalid signed-header set", error))?;

    let date = unique_header(headers, "date")?.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(AppError::public_json("Missing Date header")),
        )
    })?;
    verify_date_freshness(date, chrono::Utc::now(), HTTP_DATE_MAX_SKEW)
        .map_err(|error| inbox_auth_reject("Invalid request date", error))?;

    if !body.is_empty() {
        let digest_str = unique_header(headers, "digest")?.ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(AppError::public_json(
                    "Missing Digest header for request with body",
                )),
            )
        })?;
        if !verify_digest(body, digest_str) {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(AppError::public_json("Digest verification failed")),
            ));
        }
    }

    Ok(())
}

/// 验证请求的 HTTP Signature
///
/// remote Actor material used for the public key is resolved via
/// [`fetch_remote_actor_for_verify`] (DB cache hit or **ephemeral** HTTP fetch).
/// Failed signatures never write an unauthenticated remote document into
/// `federation_remote_actors`. Successful verification may persist via
/// [`persist_verified_remote_actor`].
pub(crate) async fn verify_request_signature(
    db: &DatabaseConnection,
    headers: &HeaderMap,
    body: &[u8],
    actor_url_str: &str,
    request_path: &str,
) -> Result<(), (StatusCode, Json<serde_json::Value>)> {
    // 获取 Signature header
    let sig_header = unique_header(headers, "signature")?.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(AppError::public_json("Missing Signature header")),
        )
    })?;

    let parsed = parse_signature_header(sig_header)
        .map_err(|error| inbox_auth_reject("Invalid Signature header", error))?;
    require_covered_headers(&parsed, !body.is_empty())
        .map_err(|error| inbox_auth_reject("Invalid signed-header set", error))?;

    let date = unique_header(headers, "date")?.ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(AppError::public_json("Missing Date header")),
        )
    })?;
    verify_date_freshness(date, chrono::Utc::now(), HTTP_DATE_MAX_SKEW)
        .map_err(|error| inbox_auth_reject("Invalid request date", error))?;

    // Digest 验证（非空 body 必须携带 Digest header）
    if !body.is_empty() {
        let digest_str = unique_header(headers, "digest")?.ok_or_else(|| {
            (
                StatusCode::UNAUTHORIZED,
                Json(AppError::public_json(
                    "Missing Digest header for non-empty body",
                )),
            )
        })?;
        if !verify_digest(body, digest_str) {
            return Err((
                StatusCode::UNAUTHORIZED,
                Json(AppError::public_json("Digest verification failed")),
            ));
        }
    }

    // trusted cache or ephemeral remote fetch — never poison DB on 401.
    let mut resolved: ResolvedRemoteActor = fetch_remote_actor_for_verify(db, actor_url_str, false)
        .await
        .map_err(|error| inbox_auth_reject("Cannot verify actor", error))?;

    // If we stored a public_key_id for this actor, Signature keyId must match
    // (normalized). On mismatch, force ephemeral re-fetch once. If no stored key id, PEM-only verify.
    if let Some(ref stored_kid) = resolved.info.public_key_id {
        if !stored_kid.is_empty() && !same_key_id(stored_kid, &parsed.key_id) {
            tracing::warn!(
                "Signature keyId mismatch (will refresh actor ephemerally): stored={}, request={}",
                stored_kid,
                parsed.key_id
            );
            match fetch_remote_actor_for_verify(db, actor_url_str, true).await {
                Ok(fresh) => resolved = fresh,
                Err(e) => {
                    tracing::warn!("Actor refresh after keyId mismatch failed: {}", e);
                }
            }
            if let Some(ref fresh_kid) = resolved.info.public_key_id {
                if !fresh_kid.is_empty() && !same_key_id(fresh_kid, &parsed.key_id) {
                    // Last chance: request keyId may still be a valid id for the
                    // same actor path even if publicKey.id differs slightly —
                    // only accept when the request keyId is under this actor URL
                    // (`{actor}`, `{actor}#…`, `{actor}/…`).
                    let actor_ok = key_id_belongs_to_actor(&parsed.key_id, actor_url_str);
                    if !actor_ok {
                        tracing::warn!(
                            "Signature keyId mismatch after refresh: stored={}, request={}",
                            fresh_kid,
                            parsed.key_id
                        );
                        return Err((
                            StatusCode::UNAUTHORIZED,
                            Json(json!({
                                "error": "Signature keyId does not match actor public key id",
                            })),
                        ));
                    }
                    tracing::info!(
                        "Accepting Signature keyId under actor URL after publicKey.id drift: request={} actor={}",
                        parsed.key_id,
                        actor_url_str
                    );
                }
            }
        }
    }

    let public_key_pem = resolved.info.public_key_pem.as_deref().ok_or_else(|| {
        (
            StatusCode::UNAUTHORIZED,
            Json(AppError::public_json("Remote actor has no public key")),
        )
    })?;

    // 构建请求方法和路径
    let method = "POST"; // Inbox 总是 POST
    let path = request_path;

    // 只取签名覆盖的 header，且每个都必须唯一（见 unique_header）。
    let header_map = signing_header_map(headers, &parsed.headers)?;

    let valid = verify_signature(public_key_pem, &parsed, method, path, &header_map)
        .map_err(|error| inbox_auth_reject("Signature verification failed", error))?;

    if !valid {
        // Ephemeral document is dropped here — never written to DB.
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(AppError::public_json("Invalid signature")),
        ));
    }

    // Signature OK: promote ephemeral actor material into the trusted cache.
    if resolved.needs_persist {
        if let Err(e) = persist_verified_remote_actor(db, &resolved).await {
            // Handlers re-fetch via fetch_remote_actor; log and continue.
            tracing::warn!(
                actor = %actor_url_str,
                error = %e,
                "Failed to persist verified remote actor; handlers may re-fetch"
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, StatusCode};

    /// 重复 header 必须 401，而不是「闸门看第一个、验签看最后一个」。
    ///
    /// 回归的是一条完整的伪造链：抓一条合法签名 → 重发时把 `Date` / `Digest`
    /// 各写两遍（第一个是新鲜时间 / 自造 body 的摘要，第二个是原签名覆盖的值）
    /// → 新鲜度和摘要闸门用第一个值放行，签名用第二个值验过 → 任意内容被认成
    /// 对方签发。`.get()` 是 first-wins、`.iter().collect()` 是 last-wins，
    /// 这个差值就是漏洞本身。
    #[test]
    fn duplicate_signed_header_is_rejected() {
        let mut headers = HeaderMap::new();
        headers.append("date", "Mon, 04 Aug 2025 10:00:00 GMT".parse().unwrap());
        headers.append("date", "Mon, 04 Aug 2025 09:00:00 GMT".parse().unwrap());

        // 底层差值仍然存在（这正是必须显式拒绝的原因）
        assert_ne!(
            headers.get("date").unwrap().to_str().unwrap(),
            headers
                .get_all("date")
                .iter()
                .next_back()
                .unwrap()
                .to_str()
                .unwrap(),
        );

        let err = unique_header(&headers, "date").unwrap_err();
        assert_eq!(err.0, StatusCode::UNAUTHORIZED);
        assert_eq!(
            err.1 .0.get("error").and_then(|v| v.as_str()),
            Some("Ambiguous request: a signed header appears more than once")
        );

        let covered = vec!["(request-target)".to_string(), "date".to_string()];
        assert!(signing_header_map(&headers, &covered).is_err());
    }

    #[test]
    fn unique_header_reads_single_values_case_insensitively() {
        let mut headers = HeaderMap::new();
        headers.insert("Date", "Mon, 04 Aug 2025 10:00:00 GMT".parse().unwrap());
        headers.insert("Digest", "SHA-256=abc".parse().unwrap());
        assert_eq!(
            unique_header(&headers, "date").unwrap(),
            Some("Mon, 04 Aug 2025 10:00:00 GMT")
        );
        assert_eq!(
            unique_header(&headers, "digest").unwrap(),
            Some("SHA-256=abc")
        );
        assert_eq!(unique_header(&headers, "signature").unwrap(), None);
    }

    /// 重复只在签名覆盖的 header 上是致命的。代理常常重复 `Via` /
    /// `X-Forwarded-For`，把它们一起拒掉会误伤正常联邦流量。
    #[test]
    fn duplicate_uncovered_header_does_not_block_verification() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "b.example".parse().unwrap());
        headers.insert("date", "Mon, 04 Aug 2025 10:00:00 GMT".parse().unwrap());
        headers.append("via", "1.1 alpha".parse().unwrap());
        headers.append("via", "1.1 beta".parse().unwrap());

        let covered = vec![
            "(request-target)".to_string(),
            "host".to_string(),
            "date".to_string(),
        ];
        let map = signing_header_map(&headers, &covered).unwrap();
        assert_eq!(map.get("host").map(String::as_str), Some("b.example"));
        assert_eq!(map.len(), 2, "(request-target) is rebuilt, not read");
    }

    /// 签名字符串只由签名自己列出的 header 组成 —— 未覆盖的 header 不得混入。
    #[test]
    fn signing_header_map_only_includes_covered_headers() {
        let mut headers = HeaderMap::new();
        headers.insert("host", "b.example".parse().unwrap());
        headers.insert("date", "Mon, 04 Aug 2025 10:00:00 GMT".parse().unwrap());
        headers.insert("x-extra", "ignored".parse().unwrap());

        let covered = vec!["host".to_string(), "date".to_string()];
        let map = signing_header_map(&headers, &covered).unwrap();
        assert!(!map.contains_key("x-extra"));
    }
}
use myriad_error::AppError;
