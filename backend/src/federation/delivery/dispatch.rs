//! HTTP delivery of a queued activity and signing-key load.

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use std::time::Duration;

use crate::federation::keys::KeyPair;
use crate::federation::signature::{SignatureParams, sign_request};
use crate::federation::types::{AP_CONTENT_TYPE, is_internal_url, key_id};

#[derive(Debug)]
pub(crate) struct DeliveryAttemptError {
    pub(crate) message: String,
    pub(crate) counts_toward_remote_failure: bool,
}

impl DeliveryAttemptError {
    fn local(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            counts_toward_remote_failure: false,
        }
    }

    fn remote(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            counts_toward_remote_failure: true,
        }
    }
}

pub(crate) fn outbound_client_error_counts_as_remote_failure(error: &str) -> bool {
    error.starts_with("DNS resolution failed") || error == "DNS resolution returned no addresses"
}

/// 投递 Activity 到目标 inbox
///
/// `stored_key_id`: canonical HTTP Signature keyId from `federation_keys` when
/// present (survives domain-move G retarget). Fallback: recompute from base_url.
/// For `Move`, always recompute from `base_url` (the **old** actor origin) so
/// peers verifying against the departing actor document succeed.
pub(crate) async fn deliver_activity(
    keypair: &KeyPair,
    base_url: &str,
    username: &str,
    target_inbox: &str,
    body: Vec<u8>,
    stored_key_id: Option<&str>,
    activity_type: &str,
) -> Result<(), DeliveryAttemptError> {
    // 纵深防御：即使 inbox URL 已入库，投递前仍验证不指向内网
    if is_internal_url(target_inbox) {
        return Err(DeliveryAttemptError::local(format!(
            "Refusing to deliver to internal URL: {}",
            target_inbox
        )));
    }

    let kid = resolve_signing_key_id(activity_type, base_url, username, stored_key_id);

    let (path, host_header) = parse_delivery_inbox(target_inbox)
        .map_err(DeliveryAttemptError::local)?;

    let params = SignatureParams {
        key_id: &kid,
        host: &host_header,
        path: &path,
        method: "POST",
        body: Some(&body),
    };

    let signed = sign_request(keypair, &params)
        .map_err(|e| DeliveryAttemptError::local(format!("Signing failed: {}", e)))?;

    let user_agent = format!("Myriad/{} (+{})", env!("CARGO_PKG_VERSION"), base_url);
    let (target_url, client) = crate::services::outbound_security::build_public_http_client(
        target_inbox,
        Duration::from_secs(30),
        Some(&user_agent),
    )
    .await
    .map_err(|message| {
        if outbound_client_error_counts_as_remote_failure(&message) {
            DeliveryAttemptError::remote(message)
        } else {
            DeliveryAttemptError::local(message)
        }
    })?;

    let resp = client
        .post(target_url)
        .header("Host", &host_header)
        .header("Date", &signed.date)
        .header("Digest", &signed.digest.unwrap_or_default())
        .header("Signature", &signed.signature)
        .header("Content-Type", AP_CONTENT_TYPE)
        .body(body)
        .send()
        .await
        .map_err(|e| DeliveryAttemptError::remote(format!("Request failed: {}", e)))?;

    let status = resp.status();
    if status.is_success() || status.as_u16() == 202 {
        Ok(())
    } else {
        // 只读错误正文的前若干字节。`resp.text()` 会把整个响应缓冲进内存，
        // 而这条路径上的对端是任意联邦实例 —— 一个恶意或故障的远端可以对每次
        // 投递失败回一个巨大的 body，把内存打爆。我们只需要够写日志的一小段。
        const MAX_ERROR_BODY: usize = 8 * 1024;
        let body_text = crate::services::outbound_security::read_limited_body(resp, MAX_ERROR_BODY)
            .await
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
            .unwrap_or_default();
        let snippet = body_text.chars().take(200).collect::<String>();
        let code = status.as_u16();
        // Permanent client errors: do not burn max_attempts with useless retries.
        // Keep retrying 408/429 (timeout / rate limit) as transient.
        if status.is_client_error() && code != 408 && code != 429 {
            Err(DeliveryAttemptError::remote(format!(
                "PERMANENT HTTP {}: {}",
                code, snippet
            )))
        } else {
            Err(DeliveryAttemptError::remote(format!(
                "HTTP {}: {}",
                code, snippet
            )))
        }
    }
}

/// Parse the queued inbox URL for signing Host/path. Invalid URLs error
/// immediately — forging `/inbox` cannot complete delivery.
pub(crate) fn parse_delivery_inbox(target_inbox: &str) -> Result<(String, String), String> {
    let parsed = url::Url::parse(target_inbox)
        .map_err(|_| format!("Invalid inbox URL: {target_inbox}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(format!("Invalid inbox URL: {target_inbox}"));
    }
    let path = match parsed.query() {
        Some(query) if !query.is_empty() => format!("{}?{query}", parsed.path()),
        _ => parsed.path().to_string(),
    };
    let host = match (parsed.host_str(), parsed.port()) {
        (Some(host), Some(port)) => format!("{host}:{port}"),
        (Some(host), None) => host.to_string(),
        _ => return Err(format!("Invalid inbox URL: {target_inbox}")),
    };
    Ok((path, host))
}

/// Choose base URL + username for HTTP Signature keyId.
///
/// For `Move`, use the activity's `actor` URL origin so the signature keyId
/// matches the departing actor document (`{old}/users/{u}#main-key`).
pub(crate) fn signing_identity_for_activity(
    activity_type: &str,
    object_json: &serde_json::Value,
    default_base: &str,
    default_username: &str,
) -> (String, String) {
    if activity_type != "Move" {
        return (
            default_base.trim_end_matches('/').to_string(),
            default_username.to_string(),
        );
    }
    let actor = object_json
        .get("actor")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if actor.is_empty() {
        return (
            default_base.trim_end_matches('/').to_string(),
            default_username.to_string(),
        );
    }
    // actor = `{base}/users/{username}`
    if let Some((base, uname)) = actor.rsplit_once("/users/") {
        let uname = uname.trim_end_matches('/');
        if !base.is_empty() && !uname.is_empty() && !uname.contains('/') {
            return (base.trim_end_matches('/').to_string(), uname.to_string());
        }
    }
    (
        default_base.trim_end_matches('/').to_string(),
        default_username.to_string(),
    )
}

/// Loaded signing material + optional canonical keyId from storage.
pub(crate) struct LoadedSigningKey {
    pub(crate) keypair: KeyPair,
    /// From `federation_keys.key_id` when non-empty (post domain-move G).
    pub(crate) stored_key_id: Option<String>,
}

/// Pick HTTP Signature keyId for outbound delivery.
///
/// - **Move**: always `key_id(sign_base, username)` (old actor origin).
/// - **Else**: prefer non-empty stored keyId; else recompute from sign_base.
pub(crate) fn resolve_signing_key_id(
    activity_type: &str,
    sign_base: &str,
    username: &str,
    stored_key_id: Option<&str>,
) -> String {
    if activity_type == "Move" {
        return key_id(sign_base, username);
    }
    if let Some(kid) = stored_key_id.map(str::trim).filter(|s| !s.is_empty()) {
        return kid.to_string();
    }
    key_id(sign_base, username)
}

/// Load keypair; if none (or empty), generate once via ensure then reload.
///
/// Universal choke point for all `federation_delivery_queue` producers.
pub(crate) async fn load_user_keypair_ensuring(
    db: &DatabaseConnection,
    user_id: i32,
    username: &str,
) -> Result<LoadedSigningKey, String> {
    match load_user_keypair(db, user_id).await {
        Ok(loaded) => Ok(loaded),
        Err(e) if is_missing_federation_keys_error(&e) => {
            if username.trim().is_empty() {
                tracing::error!(
                    user_id = user_id,
                    error = %e,
                    "No federation keys and username lookup empty; cannot ensure keys"
                );
                return Err(
                    "No federation keys found for user and username empty (cannot ensure)"
                        .to_string(),
                );
            }
            tracing::warn!(
                user_id = user_id,
                username = %username,
                error = %e,
                "Federation keys missing for outbound delivery; ensuring once"
            );
            match crate::federation::actor::ensure_user_federation_keys(db, user_id, username).await
            {
                Ok(_) => match load_user_keypair(db, user_id).await {
                    Ok(loaded) => {
                        tracing::info!(
                            user_id = user_id,
                            username = %username,
                            "Ensured federation keys; retrying delivery sign"
                        );
                        Ok(loaded)
                    }
                    Err(reload_e) => {
                        tracing::error!(
                            user_id = user_id,
                            username = %username,
                            error = %reload_e,
                            "Federation keys still missing after ensure"
                        );
                        Err(reload_e)
                    }
                },
                Err(ensure_e) => {
                    tracing::error!(
                        user_id = user_id,
                        username = %username,
                        error = %ensure_e,
                        "Failed to ensure federation keys before delivery"
                    );
                    Err(format!(
                        "Key load failed (ensure): {}; original: {}",
                        ensure_e, e
                    ))
                }
            }
        }
        // Decrypt / other load errors: return as-is. Never ensure/regenerate.
        Err(e) => Err(e),
    }
}

/// True when `err.contains("No federation keys found for user")` (no row or empty PEMs).
/// Decrypt failures must not match — regenerating would rotate the public key.
pub(crate) fn is_missing_federation_keys_error(err: &str) -> bool {
    err.contains("No federation keys found for user")
}

/// Errors that cannot self-heal via ensure (mark dead immediately).
/// Decrypt failures are *not* included: they must not re-generate, but may
/// still back off until max_attempts if ops fix JWT/storage.
pub(crate) fn is_unrecoverable_key_load_error(err: &str) -> bool {
    err.contains("username empty (cannot ensure)")
}

/// 加载用户的密钥对 + 库内 key_id（domain-move 后的规范 keyId）
async fn load_user_keypair(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<LoadedSigningKey, String> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT public_key_pem, private_key_encrypted, key_id FROM federation_keys WHERE user_id = $1",
            [user_id.into()],
        ))
        .await
        .map_err(|e| { tracing::error!("DB error: {}", e); "Database error".to_string() })?
        .ok_or_else(|| "No federation keys found for user".to_string())?;

    let pub_pem: String = row.try_get("", "public_key_pem").map_err(|error| {
        tracing::error!(%error, "failed to decode federation public_key_pem");
        "Database error".to_string()
    })?;
    let encrypted: String = row.try_get("", "private_key_encrypted").map_err(|error| {
        tracing::error!(%error, "failed to decode federation private_key_encrypted");
        "Database error".to_string()
    })?;
    let stored_kid: String = row.try_get("", "key_id").map_err(|error| {
        tracing::error!(%error, "failed to decode federation key_id");
        "Database error".to_string()
    })?;

    if pub_pem.trim().is_empty() || encrypted.trim().is_empty() {
        return Err("No federation keys found for user".to_string());
    }

    let jwt_secret = {
        let config = crate::GLOBAL_CONFIG.read().await;
        config.jwt_secret.clone()
    };

    let keypair = KeyPair::from_encrypted(&pub_pem, &encrypted, &jwt_secret)
        .map_err(|e| format!("Key decryption failed: {}", e))?;
    Ok(LoadedSigningKey {
        keypair,
        stored_key_id: if stored_kid.trim().is_empty() {
            None
        } else {
            Some(stored_kid)
        },
    })
}
