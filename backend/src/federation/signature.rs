//! HTTP Signature 签名与验签
//!
//! 实现 HTTP Signatures (draft-cavage-http-signatures) 用于 ActivityPub 联邦通信。
//! Inbox POST 必须验签。出站投递 POST 走 `sign_request`；WebFinger / Actor GET 不签名。

use anyhow::{Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use chrono::{DateTime, Duration, Utc};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

use super::keys::KeyPair;

/// 签名算法标识
const SIGNATURE_ALGORITHM: &str = "rsa-sha256";

/// HTTP 签名请求所需的参数
pub struct SignatureParams<'a> {
    /// Key ID（如 https://domain/users/alice#main-key）
    pub key_id: &'a str,
    /// HTTP 方法（GET, POST 等）
    pub method: &'a str,
    /// 请求路径（如 /users/bob/inbox）
    pub path: &'a str,
    /// 目标主机（如 bob.example.com）
    pub host: &'a str,
    /// 原始请求体（有则算 Digest）
    pub body: Option<&'a [u8]>,
}

/// 签名结果（包含需要附加到 HTTP 请求的所有头部）
pub struct SignedHeaders {
    /// Date 头
    pub date: String,
    /// Digest 头（有 body 时）
    pub digest: Option<String>,
    /// Signature 头
    pub signature: String,
}

/// 对 HTTP 请求进行签名
pub fn sign_request(key_pair: &KeyPair, params: &SignatureParams) -> Result<SignedHeaders> {
    let date = Utc::now().format("%a, %d %b %Y %H:%M:%S GMT").to_string();

    // 计算 body digest（有 body 时）
    let digest = params.body.map(|body| {
        let hash = Sha256::digest(body);
        format!("SHA-256={}", BASE64.encode(hash))
    });

    // 构建签名字符串
    let mut signed_headers_list: Vec<&str> = vec!["(request-target)", "host", "date"];
    let request_target = format!("{} {}", params.method.to_lowercase(), params.path);
    let mut signing_string = format!(
        "(request-target): {}\nhost: {}\ndate: {}",
        request_target, params.host, date
    );

    if let Some(ref d) = digest {
        signed_headers_list.push("digest");
        signing_string.push_str(&format!("\ndigest: {}", d));
    }

    // RSA-SHA256 签名
    let signature_bytes = key_pair
        .sign(signing_string.as_bytes())
        .context("Failed to sign request")?;
    let signature_b64 = BASE64.encode(&signature_bytes);

    // 构造 Signature 头
    let headers_param = signed_headers_list.join(" ");
    let signature_header = format!(
        r#"keyId="{}",algorithm="{}",headers="{}",signature="{}""#,
        params.key_id, SIGNATURE_ALGORITHM, headers_param, signature_b64
    );

    Ok(SignedHeaders {
        date,
        digest,
        signature: signature_header,
    })
}

/// 解析 Signature 头中的参数
#[derive(Debug)]
#[allow(dead_code)]
pub struct ParsedSignature {
    pub key_id: String,
    pub algorithm: String,
    pub headers: Vec<String>,
    pub signature: Vec<u8>,
}

/// 从 Signature 头解析签名参数
pub fn parse_signature_header(header: &str) -> Result<ParsedSignature> {
    let mut key_id = None;
    let mut algorithm = None;
    let mut headers = None;
    let mut signature = None;

    // 解析 key="value" 格式的参数。
    //
    // 重复的参数一律拒绝，不做"后者覆盖前者"。歧义的签名头是经典的解析器差异
    // 攻击面：中间环节按第一个值判断、我们按最后一个值验证，两边就会对同一个
    // 请求得出不同结论。宁可 401。
    macro_rules! set_once {
        ($slot:expr, $name:literal, $value:expr) => {{
            if $slot.is_some() {
                anyhow::bail!(concat!(
                    "Duplicate `",
                    $name,
                    "` parameter in Signature header"
                ));
            }
            $slot = Some($value);
        }};
    }

    for part in split_signature_params(header) {
        let part = part.trim();
        if let Some((key, value)) = part.split_once('=') {
            let key = key.trim();
            let value = value.trim().trim_matches('"');
            match key {
                "keyId" => set_once!(key_id, "keyId", value.to_string()),
                "algorithm" => set_once!(algorithm, "algorithm", value.to_string()),
                "headers" => set_once!(
                    headers,
                    "headers",
                    value
                        .split_whitespace()
                        .map(|s| s.to_ascii_lowercase())
                        .collect::<Vec<String>>()
                ),
                "signature" => set_once!(
                    signature,
                    "signature",
                    BASE64
                        .decode(value)
                        .context("Invalid base64 in signature")?
                ),
                _ => {} // 忽略未知参数
            }
        }
    }

    Ok(ParsedSignature {
        key_id: key_id.ok_or_else(|| anyhow::anyhow!("Missing keyId"))?,
        algorithm: algorithm.unwrap_or_else(|| SIGNATURE_ALGORITHM.to_string()),
        headers: headers.ok_or_else(|| anyhow::anyhow!("Missing headers"))?,
        signature: signature.ok_or_else(|| anyhow::anyhow!("Missing signature"))?,
    })
}

/// 验证 HTTP 请求签名
///
/// # Arguments
/// * `public_key_pem` - 签名者的 PEM 格式公钥
/// * `parsed` - 解析后的签名参数
/// * `method` - HTTP 方法
/// * `path` - 请求路径
/// * `headers` - HTTP 请求头（用于重建签名字符串）
pub fn verify_signature(
    public_key_pem: &str,
    parsed: &ParsedSignature,
    method: &str,
    path: &str,
    headers: &std::collections::HashMap<String, String>,
) -> Result<bool> {
    if !parsed.algorithm.eq_ignore_ascii_case(SIGNATURE_ALGORITHM) {
        anyhow::bail!("Unsupported signature algorithm: {}", parsed.algorithm);
    }
    require_covered_headers(parsed, false)?;

    // 重建签名字符串
    let mut signing_parts: Vec<String> = Vec::new();

    for header_name in &parsed.headers {
        let part = if header_name == "(request-target)" {
            format!("(request-target): {} {}", method.to_lowercase(), path)
        } else {
            let value = headers
                .get(header_name.as_str())
                .ok_or_else(|| anyhow::anyhow!("Missing header for signature: {}", header_name))?;
            format!("{}: {}", header_name, value)
        };
        signing_parts.push(part);
    }

    let signing_string = signing_parts.join("\n");

    // 验证签名
    KeyPair::verify(public_key_pem, signing_string.as_bytes(), &parsed.signature)
}

/// Require the security-critical headers to be covered by the signature.
pub fn require_covered_headers(parsed: &ParsedSignature, body_present: bool) -> Result<()> {
    for required in ["(request-target)", "host", "date"] {
        if !parsed.headers.iter().any(|header| header == required) {
            anyhow::bail!("Signature does not cover required header: {required}");
        }
    }
    if body_present && !parsed.headers.iter().any(|header| header == "digest") {
        anyhow::bail!("Signature does not cover required header: digest");
    }
    Ok(())
}

/// Allowed clock skew for the HTTP `Date` header on signed federation requests.
/// Durable activity-id/body replay protection is enforced separately by the
/// inbox receipt transaction after signature verification.
pub const HTTP_DATE_MAX_SKEW: Duration = Duration::minutes(5);

/// Reject stale or far-future HTTP Date values to bound replay attacks.
pub fn verify_date_freshness(
    date_header: &str,
    now: DateTime<Utc>,
    max_skew: Duration,
) -> Result<()> {
    let date = DateTime::parse_from_rfc2822(date_header)
        .context("Invalid HTTP Date header")?
        .with_timezone(&Utc);
    let skew_seconds = now.signed_duration_since(date).num_seconds().abs();
    if skew_seconds > max_skew.num_seconds() {
        anyhow::bail!("HTTP Date is outside the allowed clock-skew window");
    }
    Ok(())
}

/// 验证请求体 Digest 头（常量时间比较，防止时序攻击）
pub fn verify_digest(body: &[u8], digest_header: &str) -> bool {
    if let Some(expected) = digest_header.strip_prefix("SHA-256=") {
        let actual_hash = Sha256::digest(body);
        let actual_b64 = BASE64.encode(actual_hash);
        // 使用常量时间比较防止时序侧信道攻击
        actual_b64.as_bytes().ct_eq(expected.as_bytes()).into()
    } else {
        false
    }
}

/// 分割 Signature 头参数（处理值中可能包含逗号和转义引号的情况）
fn split_signature_params(header: &str) -> Vec<String> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut prev_was_backslash = false;

    for ch in header.chars() {
        if prev_was_backslash {
            current.push(ch);
            prev_was_backslash = false;
            continue;
        }
        match ch {
            '\\' if in_quotes => {
                prev_was_backslash = true;
                // 不将反斜杠本身推入结果
            }
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            ',' if !in_quotes => {
                if !current.trim().is_empty() {
                    result.push(current.trim().to_string());
                }
                current.clear();
            }
            _ => current.push(ch),
        }
    }
    if !current.trim().is_empty() {
        result.push(current.trim().to_string());
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sign_and_verify_get_request() {
        let kp = KeyPair::generate().unwrap();
        let params = SignatureParams {
            key_id: "https://example.com/users/alice#main-key",
            method: "GET",
            path: "/users/bob",
            host: "bob.example.com",
            body: None,
        };

        let signed = sign_request(&kp, &params).unwrap();
        let parsed = parse_signature_header(&signed.signature).unwrap();

        let mut headers = std::collections::HashMap::new();
        headers.insert("host".to_string(), "bob.example.com".to_string());
        headers.insert("date".to_string(), signed.date.clone());

        let pem = kp.public_key_pem().unwrap();
        let valid = verify_signature(&pem, &parsed, "GET", "/users/bob", &headers).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_sign_and_verify_post_request() {
        let kp = KeyPair::generate().unwrap();
        let body = br#"{"type":"Follow","actor":"https://a.com/users/alice"}"#;
        let params = SignatureParams {
            key_id: "https://a.com/users/alice#main-key",
            method: "POST",
            path: "/users/bob/inbox",
            host: "b.com",
            body: Some(body),
        };

        let signed = sign_request(&kp, &params).unwrap();
        assert!(signed.digest.is_some());
        assert!(verify_digest(body, signed.digest.as_deref().unwrap()));

        let parsed = parse_signature_header(&signed.signature).unwrap();
        let mut headers = std::collections::HashMap::new();
        headers.insert("host".to_string(), "b.com".to_string());
        headers.insert("date".to_string(), signed.date.clone());
        headers.insert("digest".to_string(), signed.digest.unwrap());

        let pem = kp.public_key_pem().unwrap();
        let valid = verify_signature(&pem, &parsed, "POST", "/users/bob/inbox", &headers).unwrap();
        assert!(valid);
    }

    #[test]
    fn test_digest_verification() {
        let body = b"hello world";
        let hash = sha2::Sha256::digest(body);
        let digest = format!("SHA-256={}", BASE64.encode(hash));

        assert!(verify_digest(body, &digest));
        assert!(!verify_digest(b"different", &digest));
    }

    #[test]
    fn test_parse_signature_header() {
        let header = r##"keyId="https://a.com/users/alice#main-key",algorithm="rsa-sha256",headers="(request-target) host date",signature="dGVzdA==""##;
        let parsed = parse_signature_header(header).unwrap();
        assert_eq!(parsed.key_id, "https://a.com/users/alice#main-key");
        assert_eq!(parsed.algorithm, "rsa-sha256");
        assert_eq!(parsed.headers, vec!["(request-target)", "host", "date"]);
    }

    #[test]
    fn requires_digest_to_be_covered_for_bodied_requests() {
        let parsed = parse_signature_header(
            r##"keyId="https://a.example/key",algorithm="rsa-sha256",headers="(request-target) host date",signature="dGVzdA==""##,
        )
        .unwrap();
        assert!(require_covered_headers(&parsed, false).is_ok());
        assert!(require_covered_headers(&parsed, true).is_err());
    }

    #[test]
    fn enforces_http_date_replay_window() {
        let now = Utc::now();
        let fresh = now.format("%a, %d %b %Y %H:%M:%S GMT").to_string();
        let stale = (now - Duration::minutes(6))
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string();

        assert!(verify_date_freshness(&fresh, now, HTTP_DATE_MAX_SKEW).is_ok());
        assert!(verify_date_freshness(&stale, now, HTTP_DATE_MAX_SKEW).is_err());
    }

    #[test]
    fn require_covered_headers_body_requires_digest() {
        let mut parsed = ParsedSignature {
            key_id: "k".into(),
            algorithm: "rsa-sha256".into(),
            headers: vec!["(request-target)".into(), "host".into(), "date".into()],
            signature: vec![0u8; 32],
        };
        assert!(require_covered_headers(&parsed, false).is_ok());
        assert!(require_covered_headers(&parsed, true).is_err());
        parsed.headers.push("digest".into());
        assert!(require_covered_headers(&parsed, true).is_ok());
    }

    #[test]
    fn verify_digest_rejects_non_sha256_prefix() {
        assert!(!verify_digest(b"hello", "md5=deadbeef"));
        assert!(!verify_digest(b"hello", "SHA-256"));
        assert!(!verify_digest(b"hello", ""));
    }

    #[test]
    fn require_covered_headers_missing_host() {
        let parsed = ParsedSignature {
            key_id: "k".into(),
            algorithm: "rsa-sha256".into(),
            headers: vec!["(request-target)".into(), "date".into()],
            signature: vec![0u8; 8],
        };
        assert!(require_covered_headers(&parsed, false).is_err());
    }

    #[test]
    fn verify_date_freshness_rejects_future_skew() {
        let now = Utc::now();
        let future = (now + Duration::minutes(10))
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string();
        assert!(verify_date_freshness(&future, now, HTTP_DATE_MAX_SKEW).is_err());
    }

    #[test]
    fn w175_verify_digest_bad_prefix() {
        assert!(!verify_digest(b"hello", "md5=deadbeef"));
        assert!(!verify_digest(b"hello", "SHA-256"));
        assert!(!verify_digest(b"hello", ""));
    }

    #[test]
    fn w175_require_headers_missing_host() {
        let parsed = ParsedSignature {
            key_id: "k".into(),
            algorithm: "rsa-sha256".into(),
            headers: vec!["(request-target)".into(), "date".into()],
            signature: vec![0u8; 8],
        };
        assert!(require_covered_headers(&parsed, false).is_err());
    }

    #[test]
    fn w175_date_freshness_future() {
        let now = Utc::now();
        let future = (now + Duration::minutes(10))
            .format("%a, %d %b %Y %H:%M:%S GMT")
            .to_string();
        assert!(verify_date_freshness(&future, now, HTTP_DATE_MAX_SKEW).is_err());
    }

    #[test]
    fn http_date_max_skew_is_five_minutes() {
        assert_eq!(HTTP_DATE_MAX_SKEW, Duration::minutes(5));
    }
}
