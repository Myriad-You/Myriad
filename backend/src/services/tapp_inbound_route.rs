//! Declared inbound `/tapi/{tappId}/{path}` verification (HMAC + nonce ledger).

use crate::services::tapp_hmac;
use crate::services::tapp_registry::{self, RegistryIdentity};
use axum::http::{HeaderMap, Method};
use myriad_tapp_contract::contract_rules::ROUTE_MAX_BODY_BYTES;
use myriad_tapp_contract::manifest::{
    valid_inbound_nonce, TappApiDef, TappApiRoute, TappRouteVerifyOver,
};
use sea_orm::DatabaseConnection;
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, HashMap};

pub const ROUTE_NONCE_NAMESPACE: &str = "route_nonce";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InboundRouteError {
    RouteNotFound,
    MethodNotAllowed,
    VerifyInvalid,
    VerifyExpired,
    VerifyReplay,
    Blocked { retry_after: u64 },
    Paused,
    BodyTooLarge,
    InvalidParams,
    Database,
    NeedsReauthorization,
}

impl InboundRouteError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::RouteNotFound => "ROUTE_NOT_FOUND",
            Self::MethodNotAllowed => "ROUTE_METHOD_NOT_ALLOWED",
            Self::VerifyInvalid => "ROUTE_VERIFY_INVALID",
            Self::VerifyExpired => "ROUTE_VERIFY_EXPIRED",
            Self::VerifyReplay => "ROUTE_VERIFY_REPLAY",
            Self::Blocked { .. } => "ROUTE_BLOCKED",
            Self::Paused => "ROUTE_PAUSED",
            Self::BodyTooLarge => "ROUTE_BODY_TOO_LARGE",
            Self::InvalidParams => "ROUTE_INVALID_PARAMS",
            Self::Database => "DATABASE_ERROR",
            Self::NeedsReauthorization => "TAPP_NEEDS_REAUTHORIZATION",
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Self::RouteNotFound => "Inbound route is not declared",
            Self::MethodNotAllowed => "Inbound route does not allow this method",
            Self::VerifyInvalid => "Inbound signature is invalid",
            Self::VerifyExpired => "Inbound timestamp is outside the allowed window",
            Self::VerifyReplay => "Inbound nonce has already been used",
            Self::Blocked { .. } => "Inbound caller is blocked",
            Self::Paused => "Inbound routes are paused",
            Self::BodyTooLarge => "Inbound request body exceeds 1 MiB",
            Self::InvalidParams => "Inbound request parameters are invalid",
            Self::Database => "Database error",
            Self::NeedsReauthorization => {
                "Tapp installation requires permission re-authorization"
            }
        }
    }

    pub fn status_hint(&self) -> u16 {
        match self {
            Self::RouteNotFound => 404,
            Self::MethodNotAllowed => 405,
            Self::VerifyInvalid | Self::VerifyExpired | Self::VerifyReplay => 401,
            Self::Blocked { .. } | Self::Paused => 403,
            Self::BodyTooLarge => 413,
            Self::InvalidParams => 400,
            Self::Database => 500,
            Self::NeedsReauthorization => 409,
        }
    }
}

impl std::fmt::Display for InboundRouteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for InboundRouteError {}

pub fn normalize_route_method(method: &Method) -> Option<&'static str> {
    match *method {
        Method::GET => Some("GET"),
        Method::POST => Some("POST"),
        _ => None,
    }
}

pub fn find_declared_route<'a>(
    apis: &'a HashMap<String, TappApiDef>,
    path: &str,
) -> Option<(&'a str, &'a TappApiDef, &'a TappApiRoute)> {
    apis.iter().find_map(|(name, api)| {
        let route = api.route.as_ref()?;
        (route.path == path).then_some((name.as_str(), api, route))
    })
}

pub fn inbound_path(route_segment: &str) -> String {
    format!("/{route_segment}")
}

pub fn signing_full_path(tapp_id: &str, route_path: &str) -> String {
    format!("/tapi/{tapp_id}{route_path}")
}

pub fn signing_material(
    method: &str,
    full_path: &str,
    timestamp: &str,
    nonce: &str,
    payload: &[u8],
) -> Vec<u8> {
    let mut material = Vec::with_capacity(
        method.len() + full_path.len() + timestamp.len() + nonce.len() + payload.len() + 4,
    );
    material.extend_from_slice(method.as_bytes());
    material.push(b'\n');
    material.extend_from_slice(full_path.as_bytes());
    material.push(b'\n');
    material.extend_from_slice(timestamp.as_bytes());
    material.push(b'\n');
    material.extend_from_slice(nonce.as_bytes());
    material.push(b'\n');
    material.extend_from_slice(payload);
    material
}

pub fn canonical_query(raw_query: Option<&str>) -> Result<String, InboundRouteError> {
    let Some(raw) = raw_query.filter(|value| !value.is_empty()) else {
        return Ok(String::new());
    };
    let mut pairs = BTreeMap::new();
    for (key, value) in url::form_urlencoded::parse(raw.as_bytes()) {
        if pairs.insert(key.into_owned(), value.into_owned()).is_some() {
            return Err(InboundRouteError::InvalidParams);
        }
    }
    Ok(pairs
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join("&"))
}

pub fn header_text<'a>(headers: &'a HeaderMap, name: &str) -> Result<&'a str, InboundRouteError> {
    let header_name =
        axum::http::HeaderName::try_from(name).map_err(|_| InboundRouteError::VerifyInvalid)?;
    let mut values = headers.get_all(header_name).iter();
    let value = values.next().ok_or(InboundRouteError::VerifyInvalid)?;
    if values.next().is_some() {
        return Err(InboundRouteError::VerifyInvalid);
    }
    value
        .to_str()
        .map(str::trim)
        .map_err(|_| InboundRouteError::VerifyInvalid)
        .and_then(|value| {
            if value.is_empty() {
                Err(InboundRouteError::VerifyInvalid)
            } else {
                Ok(value)
            }
        })
}

pub fn presented_signature<'a>(
    value: &'a str,
    prefix: Option<&str>,
) -> Result<&'a str, InboundRouteError> {
    match prefix {
        Some(prefix) if !prefix.is_empty() => value
            .strip_prefix(prefix)
            .ok_or(InboundRouteError::VerifyInvalid),
        _ => Ok(value),
    }
}

pub fn parse_unix_timestamp(value: &str) -> Result<i64, InboundRouteError> {
    if value.is_empty() || value.len() > 16 || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return Err(InboundRouteError::VerifyInvalid);
    }
    value
        .parse::<i64>()
        .map_err(|_| InboundRouteError::VerifyInvalid)
}

pub fn timestamp_in_window(timestamp: i64, now: i64, max_skew_secs: u32) -> bool {
    now.abs_diff(timestamp) <= u64::from(max_skew_secs)
}

/// Reserve the nonce for a full server-side window. Using the client timestamp
/// would let a request at `now - maxSkew` expire immediately and be replayed.
pub fn nonce_expires_at(now: i64, max_skew_secs: u32) -> i64 {
    now.saturating_add(i64::from(max_skew_secs))
}

pub fn over_matches_method(over: TappRouteVerifyOver, method: &str) -> bool {
    match over {
        TappRouteVerifyOver::CanonicalQuery => method == "GET",
        TappRouteVerifyOver::RawBody => method == "POST",
    }
}

pub fn nonce_record_id(owner_id: i32, tapp_id: &str, credential_key: &str, nonce: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(owner_id.to_be_bytes());
    hasher.update([0]);
    hasher.update(tapp_id.as_bytes());
    hasher.update([0]);
    hasher.update(credential_key.as_bytes());
    hasher.update([0]);
    hasher.update(nonce.as_bytes());
    hex::encode(hasher.finalize())
}

pub fn verify_signature(
    route: &TappApiRoute,
    method: &str,
    tapp_id: &str,
    headers: &HeaderMap,
    raw_query: Option<&str>,
    raw_body: &[u8],
    secret: &str,
    now: i64,
) -> Result<(String, String, i64), InboundRouteError> {
    if raw_body.len() > ROUTE_MAX_BODY_BYTES {
        return Err(InboundRouteError::BodyTooLarge);
    }
    if !over_matches_method(route.verify.over, method) {
        return Err(InboundRouteError::MethodNotAllowed);
    }
    if route.verify.alg != myriad_tapp_contract::manifest::TappCredentialSignAlg::HmacSha256Raw {
        return Err(InboundRouteError::VerifyInvalid);
    }
    let timestamp = header_text(headers, &route.verify.timestamp_header)?;
    let nonce = header_text(headers, &route.verify.nonce_header)?;
    let signature = header_text(headers, &route.verify.header)?;
    if !valid_inbound_nonce(nonce) {
        return Err(InboundRouteError::VerifyInvalid);
    }
    let presented = presented_signature(signature, route.verify.prefix.as_deref())?;
    let unix = parse_unix_timestamp(timestamp)?;
    let payload = match route.verify.over {
        TappRouteVerifyOver::CanonicalQuery => canonical_query(raw_query)
            .map_err(|_| InboundRouteError::VerifyInvalid)?
            .into_bytes(),
        TappRouteVerifyOver::RawBody => raw_body.to_vec(),
    };
    let material = signing_material(
        method,
        &signing_full_path(tapp_id, &route.path),
        timestamp,
        nonce,
        &payload,
    );
    if !tapp_hmac::hmac_matches(
        secret.as_bytes(),
        &material,
        presented,
        route.verify.encoding,
    ) {
        return Err(InboundRouteError::VerifyInvalid);
    }
    if !timestamp_in_window(unix, now, route.verify.max_skew_secs) {
        return Err(InboundRouteError::VerifyExpired);
    }
    Ok((nonce.to_string(), timestamp.to_string(), unix))
}

pub async fn consume_nonce(
    db: &DatabaseConnection,
    owner_id: i32,
    tapp_id: &str,
    credential_key: &str,
    nonce: &str,
    now: i64,
    max_skew_secs: u32,
) -> Result<(), InboundRouteError> {
    let record_id = nonce_record_id(owner_id, tapp_id, credential_key, nonce);
    let expires_at = nonce_expires_at(now, max_skew_secs);
    let inserted = tapp_registry::put_if_absent(
        db,
        ROUTE_NONCE_NAMESPACE,
        &record_id,
        RegistryIdentity {
            subject_id: None,
            owner_id: Some(owner_id),
            tapp_id: Some(tapp_id),
            runtime_id: None,
        },
        &json!({ "used": true }),
        expires_at,
    )
    .await
    .map_err(|_| InboundRouteError::Database)?;
    if inserted {
        Ok(())
    } else {
        Err(InboundRouteError::VerifyReplay)
    }
}

pub fn merge_params(
    raw_query: Option<&str>,
    method: &str,
    raw_body: &[u8],
    content_type: Option<&str>,
) -> Result<Value, InboundRouteError> {
    let mut params = Map::new();
    // POST HMAC covers the body only. Unsigned query must not become params,
    // override signed fields, or fail the request on duplicate tracking keys.
    if method == "POST" {
        if raw_body.is_empty() {
            return Ok(Value::Object(params));
        }
        let content_type = media_type(content_type.unwrap_or("application/json"));
        if content_type == "application/x-www-form-urlencoded" {
            let mut seen = BTreeMap::new();
            for (key, value) in url::form_urlencoded::parse(raw_body) {
                if seen.insert(key.into_owned(), value.into_owned()).is_some() {
                    return Err(InboundRouteError::InvalidParams);
                }
            }
            for (key, value) in seen {
                params.insert(key, Value::String(value));
            }
            return Ok(Value::Object(params));
        }
        let parsed: Value =
            serde_json::from_slice(raw_body).map_err(|_| InboundRouteError::InvalidParams)?;
        let Value::Object(fields) = parsed else {
            return Err(InboundRouteError::InvalidParams);
        };
        return Ok(Value::Object(fields));
    }
    if let Some(raw) = raw_query.filter(|value| !value.is_empty()) {
        let mut seen = BTreeMap::new();
        for (key, value) in url::form_urlencoded::parse(raw.as_bytes()) {
            if seen
                .insert(key.as_ref().to_string(), value.into_owned())
                .is_some()
            {
                return Err(InboundRouteError::InvalidParams);
            }
        }
        for (key, value) in seen {
            params.insert(key, Value::String(value));
        }
    }
    Ok(Value::Object(params))
}

fn media_type(content_type: &str) -> &str {
    content_type
        .split(';')
        .next()
        .unwrap_or(content_type)
        .trim()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::{HeaderMap, HeaderValue};
    use myriad_tapp_contract::manifest::{
        TappCredentialSignAlg, TappRouteVerify, TappRouteVerifyEncoding, TappRouteVerifyOver,
    };

    fn route(over: TappRouteVerifyOver, methods: &[&str]) -> TappApiRoute {
        TappApiRoute {
            path: "/sponsors".into(),
            methods: methods.iter().map(|method| (*method).to_string()).collect(),
            verify: TappRouteVerify {
                key: "inboundSecret".into(),
                alg: TappCredentialSignAlg::HmacSha256Raw,
                header: "X-Signature".into(),
                prefix: Some("sha256=".into()),
                over,
                encoding: TappRouteVerifyEncoding::Hex,
                timestamp_header: "X-Timestamp".into(),
                nonce_header: "X-Nonce".into(),
                max_skew_secs: 300,
            },
        }
    }

    fn signed_headers(route: &TappApiRoute, material: &[u8], secret: &str) -> HeaderMap {
        let mac = crate::services::tapp_hmac::encode_hmac(
            &crate::services::tapp_hmac::hmac_sha256(secret.as_bytes(), material),
            route.verify.encoding,
        );
        let mut headers = HeaderMap::new();
        headers.insert("X-Timestamp", HeaderValue::from_static("1770000000"));
        headers.insert(
            "X-Nonce",
            HeaderValue::from_static("7f3c0e2a9b1d4c88a6e05f21"),
        );
        headers.insert(
            "X-Signature",
            HeaderValue::from_str(&format!("sha256={mac}")).unwrap(),
        );
        headers
    }

    #[test]
    fn canonical_query_sorts_and_rejects_duplicates() {
        assert_eq!(canonical_query(Some("b=2&a=1")).unwrap(), "a=1&b=2");
        assert!(canonical_query(Some("a=1&a=2")).is_err());
        assert_eq!(canonical_query(None).unwrap(), "");
    }

    #[test]
    fn method_over_mismatch_is_rejected_before_mac() {
        let route = route(TappRouteVerifyOver::RawBody, &["POST"]);
        let mut headers = HeaderMap::new();
        headers.insert("X-Timestamp", HeaderValue::from_static("1770000000"));
        headers.insert(
            "X-Nonce",
            HeaderValue::from_static("7f3c0e2a9b1d4c88a6e05f21"),
        );
        headers.insert("X-Signature", HeaderValue::from_static("sha256=00"));
        assert_eq!(
            verify_signature(
                &route,
                "GET",
                "com.example.afdian",
                &headers,
                Some("page=1"),
                b"",
                "secret",
                1_770_000_000,
            )
            .unwrap_err(),
            InboundRouteError::MethodNotAllowed
        );
    }

    #[test]
    fn valid_hmac_query_and_path_binding() {
        let route = route(TappRouteVerifyOver::CanonicalQuery, &["GET"]);
        let payload = canonical_query(Some("page=1")).unwrap();
        let material = signing_material(
            "GET",
            "/tapi/com.example.afdian/sponsors",
            "1770000000",
            "7f3c0e2a9b1d4c88a6e05f21",
            payload.as_bytes(),
        );
        let headers = signed_headers(&route, &material, "secret");
        verify_signature(
            &route,
            "GET",
            "com.example.afdian",
            &headers,
            Some("page=1"),
            b"",
            "secret",
            1_770_000_000,
        )
        .unwrap();

        let other = signing_material(
            "GET",
            "/tapi/com.example.afdian/webhook",
            "1770000000",
            "7f3c0e2a9b1d4c88a6e05f21",
            payload.as_bytes(),
        );
        let reused = signed_headers(&route, &other, "secret");
        assert_eq!(
            verify_signature(
                &route,
                "GET",
                "com.example.afdian",
                &reused,
                Some("page=1"),
                b"",
                "secret",
                1_770_000_000,
            )
            .unwrap_err(),
            InboundRouteError::VerifyInvalid
        );
    }

    #[test]
    fn expired_timestamp_after_mac() {
        let route = route(TappRouteVerifyOver::RawBody, &["POST"]);
        let material = signing_material(
            "POST",
            "/tapi/com.example.afdian/sponsors",
            "1770000000",
            "7f3c0e2a9b1d4c88a6e05f21",
            b"{}",
        );
        let headers = signed_headers(&route, &material, "secret");
        assert_eq!(
            verify_signature(
                &route,
                "POST",
                "com.example.afdian",
                &headers,
                None,
                b"{}",
                "secret",
                1_770_000_000 + 301,
            )
            .unwrap_err(),
            InboundRouteError::VerifyExpired
        );
    }

    #[test]
    fn post_params_come_from_signed_body_only() {
        let merged = merge_params(
            Some("city=tokyo"),
            "POST",
            br#"{"city":"osaka","page":1}"#,
            Some("application/json; charset=utf-8"),
        )
        .unwrap();
        assert_eq!(merged["city"], json!("osaka"));
        assert_eq!(merged["page"], json!(1));
        assert!(merged.get("injected").is_none());

        let get = merge_params(Some("city=tokyo"), "GET", br#"{"city":"osaka"}"#, None).unwrap();
        assert_eq!(get["city"], json!("tokyo"));
        assert!(get.get("page").is_none());

        let post_with_dup_query = merge_params(
            Some("utm=a&utm=b"),
            "POST",
            br#"{"city":"osaka"}"#,
            Some("application/json"),
        )
        .unwrap();
        assert_eq!(post_with_dup_query["city"], json!("osaka"));
    }

    #[test]
    fn duplicate_verify_headers_are_rejected() {
        let route = route(TappRouteVerifyOver::CanonicalQuery, &["GET"]);
        let payload = canonical_query(Some("page=1")).unwrap();
        let material = signing_material(
            "GET",
            "/tapi/com.example.afdian/sponsors",
            "1770000000",
            "7f3c0e2a9b1d4c88a6e05f21",
            payload.as_bytes(),
        );
        let mut headers = signed_headers(&route, &material, "secret");
        headers.append("X-Timestamp", HeaderValue::from_static("1770000001"));
        assert_eq!(
            verify_signature(
                &route,
                "GET",
                "com.example.afdian",
                &headers,
                Some("page=1"),
                b"",
                "secret",
                1_770_000_000,
            )
            .unwrap_err(),
            InboundRouteError::VerifyInvalid
        );
    }

    #[test]
    fn body_too_large_is_payload_too_large() {
        assert_eq!(InboundRouteError::BodyTooLarge.status_hint(), 413);
        assert_eq!(InboundRouteError::InvalidParams.status_hint(), 400);
        assert_eq!(InboundRouteError::NeedsReauthorization.status_hint(), 409);
        assert_eq!(
            InboundRouteError::NeedsReauthorization.code(),
            "TAPP_NEEDS_REAUTHORIZATION"
        );
    }

    #[test]
    fn head_is_not_treated_as_get() {
        assert_eq!(normalize_route_method(&Method::GET), Some("GET"));
        assert_eq!(normalize_route_method(&Method::POST), Some("POST"));
        assert_eq!(normalize_route_method(&Method::HEAD), None);
        assert_eq!(normalize_route_method(&Method::OPTIONS), None);
    }

    #[test]
    fn nonce_ttl_uses_server_now_not_client_timestamp() {
        assert_eq!(nonce_expires_at(1_770_000_000, 300), 1_770_000_300);
        assert!(over_matches_method(
            TappRouteVerifyOver::CanonicalQuery,
            "GET"
        ));
        assert!(!over_matches_method(
            TappRouteVerifyOver::CanonicalQuery,
            "POST"
        ));
        assert!(over_matches_method(TappRouteVerifyOver::RawBody, "POST"));
        assert!(!over_matches_method(TappRouteVerifyOver::RawBody, "GET"));
    }

    #[test]
    fn oversized_body_rejected_before_mac() {
        let route = route(TappRouteVerifyOver::RawBody, &["POST"]);
        let body = vec![b'a'; ROUTE_MAX_BODY_BYTES + 1];
        let mut headers = HeaderMap::new();
        headers.insert("X-Timestamp", HeaderValue::from_static("1"));
        assert_eq!(
            verify_signature(
                &route,
                "POST",
                "com.example.afdian",
                &headers,
                None,
                &body,
                "secret",
                1,
            )
            .unwrap_err(),
            InboundRouteError::BodyTooLarge
        );
    }
}
