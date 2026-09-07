//! QQ C2C pairing: one-time codes and `user_identities` rows.
//!
//! Pairing is "this openid is which site user", not QQ login OAuth.
//! Codes live in `tapp_runtime_registry`; the bound id is `provider = qq`.

use chrono::{Duration as ChronoDuration, Utc};
use myriad_agent_rules::channel::{
    encode_pairing_code, extract_pairing_code, format_pairing_code, ingest_c2c_text,
    pairing_bind_reply, InboundC2cText, InboundDecision, PairingBindResult, PairingLookup,
    PAIRING_REQUIRED_REPLY,
};
use rand::Rng;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement, TransactionTrait,
    Value as SeaValue,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tracing::{info, warn};

use crate::services::http_client;
use crate::services::tapp_registry::{self as shared_registry, RegistryIdentity};
use crate::GLOBAL_DYNAMIC_CONFIG;

pub const QQ_IDENTITY_PROVIDER: &str = "qq";
const CODE_NAMESPACE: &str = "qq_pairing_code";
const CODE_TTL_SECS: i64 = 10 * 60;
const API_BASE: &str = "https://api.bot.qq.com";
const HTTP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(15);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPairingCode {
    user_id: i32,
    code: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct PairingStatus {
    pub paired: bool,
    pub identity_id: Option<i32>,
    pub openid_masked: Option<String>,
    pub linked_at: Option<String>,
    pub pending_code: Option<String>,
    pub pending_expires_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct IssuedPairingCode {
    pub code: String,
    pub display: String,
    pub expires_at: String,
}

pub fn mask_openid(openid: &str) -> String {
    let trimmed = openid.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= 4 {
        return "****".to_string();
    }
    let tail: String = trimmed
        .chars()
        .rev()
        .take(4)
        .collect::<String>()
        .chars()
        .rev()
        .collect();
    format!("****{tail}")
}

fn token_hash(code: &str) -> String {
    hex::encode(Sha256::digest(code.as_bytes()))
}

pub async fn lookup_openid(db: &DatabaseConnection, openid: &str) -> Result<PairingLookup, DbErr> {
    let openid = openid.trim();
    if openid.is_empty() {
        return Ok(PairingLookup::Unpaired);
    }
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT user_id FROM user_identities WHERE provider = $1 AND provider_user_id = $2",
            vec![
                SeaValue::String(Some(QQ_IDENTITY_PROVIDER.to_string())),
                SeaValue::String(Some(openid.to_string())),
            ],
        ))
        .await?;
    Ok(match row {
        Some(row) => PairingLookup::Paired {
            user_id: row.try_get::<i32>("", "user_id").unwrap_or(0),
        },
        None => PairingLookup::Unpaired,
    })
}

pub async fn status_for_user(
    db: &DatabaseConnection,
    user_id: i32,
) -> Result<PairingStatus, DbErr> {
    let identity = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, provider_user_id, linked_at FROM user_identities \
             WHERE user_id = $1 AND provider = $2 ORDER BY linked_at DESC LIMIT 1",
            vec![
                SeaValue::Int(Some(user_id)),
                SeaValue::String(Some(QQ_IDENTITY_PROVIDER.to_string())),
            ],
        ))
        .await?;

    let mut pending_code = None;
    let mut pending_expires_at = None;
    if identity.is_none() {
        let rows = shared_registry::list(db, CODE_NAMESPACE, Some(user_id), None).await?;
        if let Some(row) = rows.into_iter().next() {
            if let Ok(stored) = serde_json::from_value::<StoredPairingCode>(row.payload) {
                pending_code = Some(format_pairing_code(&stored.code));
                pending_expires_at = live_code_expiry(db, user_id).await?;
            }
        }
    }

    Ok(match identity {
        Some(row) => PairingStatus {
            paired: true,
            identity_id: row.try_get::<i32>("", "id").ok(),
            openid_masked: row
                .try_get::<String>("", "provider_user_id")
                .ok()
                .map(|id| mask_openid(&id)),
            linked_at: row
                .try_get::<chrono::DateTime<chrono::Utc>>("", "linked_at")
                .ok()
                .map(|t| t.to_rfc3339()),
            pending_code: None,
            pending_expires_at: None,
        },
        None => PairingStatus {
            paired: false,
            identity_id: None,
            openid_masked: None,
            linked_at: None,
            pending_code,
            pending_expires_at,
        },
    })
}

async fn live_code_expiry(db: &DatabaseConnection, user_id: i32) -> Result<Option<String>, DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT expires_at FROM tapp_runtime_registry \
             WHERE namespace = $1 AND subject_id = $2 \
               AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT \
             ORDER BY updated_at DESC LIMIT 1",
            vec![
                SeaValue::String(Some(CODE_NAMESPACE.to_string())),
                SeaValue::Int(Some(user_id)),
            ],
        ))
        .await?;
    Ok(row.and_then(|row| {
        row.try_get::<i64>("", "expires_at")
            .ok()
            .and_then(unix_to_rfc3339)
    }))
}

fn unix_to_rfc3339(ts: i64) -> Option<String> {
    chrono::DateTime::<Utc>::from_timestamp(ts, 0).map(|t| t.to_rfc3339())
}

pub async fn mint_code(db: &DatabaseConnection, user_id: i32) -> Result<IssuedPairingCode, DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM tapp_runtime_registry WHERE namespace = $1 AND subject_id = $2",
        vec![
            SeaValue::String(Some(CODE_NAMESPACE.to_string())),
            SeaValue::Int(Some(user_id)),
        ],
    ))
    .await?;

    let mut bytes = [0u8; 5];
    rand::rng().fill_bytes(&mut bytes);
    let code = encode_pairing_code(bytes);
    let expires_at = Utc::now() + ChronoDuration::seconds(CODE_TTL_SECS);
    let stored = StoredPairingCode {
        user_id,
        code: code.clone(),
    };
    shared_registry::put(
        db,
        CODE_NAMESPACE,
        &token_hash(&code),
        RegistryIdentity {
            subject_id: Some(user_id),
            owner_id: Some(user_id),
            tapp_id: None,
            runtime_id: None,
        },
        &stored,
        expires_at.timestamp(),
    )
    .await?;

    Ok(IssuedPairingCode {
        display: format_pairing_code(&code),
        code,
        expires_at: expires_at.to_rfc3339(),
    })
}

pub async fn unpair(db: &DatabaseConnection, user_id: i32) -> Result<bool, DbErr> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM user_identities WHERE user_id = $1 AND provider = $2",
            vec![
                SeaValue::Int(Some(user_id)),
                SeaValue::String(Some(QQ_IDENTITY_PROVIDER.to_string())),
            ],
        ))
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn consume_code(
    db: &DatabaseConnection,
    openid: &str,
    raw_code: &str,
) -> Result<PairingBindResult, DbErr> {
    let Some(code) = extract_pairing_code(raw_code) else {
        return Ok(PairingBindResult::InvalidOrExpired);
    };
    let openid = openid.trim();
    if openid.is_empty() {
        return Ok(PairingBindResult::InvalidOrExpired);
    }

    let Some(stored) =
        shared_registry::take::<StoredPairingCode>(db, CODE_NAMESPACE, &token_hash(&code)).await?
    else {
        return Ok(PairingBindResult::InvalidOrExpired);
    };
    if stored.code != code {
        return Ok(PairingBindResult::InvalidOrExpired);
    }

    bind_openid(db, stored.user_id, openid).await
}

async fn bind_openid(
    db: &DatabaseConnection,
    user_id: i32,
    openid: &str,
) -> Result<PairingBindResult, DbErr> {
    let txn = db.begin().await?;
    let existing = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT user_id FROM user_identities WHERE provider = $1 AND provider_user_id = $2",
            vec![
                SeaValue::String(Some(QQ_IDENTITY_PROVIDER.to_string())),
                SeaValue::String(Some(openid.to_string())),
            ],
        ))
        .await?;
    if let Some(row) = existing {
        let bound_user: i32 = row.try_get("", "user_id").unwrap_or(0);
        txn.commit().await?;
        return Ok(if bound_user == user_id {
            PairingBindResult::Bound { user_id }
        } else {
            PairingBindResult::OpenidTaken
        });
    }

    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM user_identities WHERE user_id = $1 AND provider = $2",
        vec![
            SeaValue::Int(Some(user_id)),
            SeaValue::String(Some(QQ_IDENTITY_PROVIDER.to_string())),
        ],
    ))
    .await?;

    let inserted = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO user_identities ( \
                user_id, provider, provider_user_id, is_primary, linked_at \
             ) VALUES ($1, $2, $3, false, NOW()) \
             ON CONFLICT (provider, provider_user_id) DO NOTHING \
             RETURNING user_id",
            vec![
                SeaValue::Int(Some(user_id)),
                SeaValue::String(Some(QQ_IDENTITY_PROVIDER.to_string())),
                SeaValue::String(Some(openid.to_string())),
            ],
        ))
        .await?;
    if inserted.is_none() {
        txn.rollback().await?;
        return Ok(PairingBindResult::OpenidTaken);
    }
    txn.commit().await?;
    Ok(PairingBindResult::Bound { user_id })
}

/// Gateway worker entry: classify the C2C text and reply for pairing. Work ingest is later.
pub async fn handle_inbound_c2c(event: InboundC2cText, auth_header: &str) {
    let Ok(db) = crate::services::tapp_registry::database().await else {
        warn!("QQ pairing skipped: database is not connected");
        return;
    };
    let pairing = match lookup_openid(&db, &event.user_openid).await {
        Ok(value) => value,
        Err(error) => {
            warn!(error = %error, "QQ pairing lookup failed");
            return;
        }
    };
    match ingest_c2c_text(&event, pairing, false) {
        InboundDecision::Duplicate { .. } => {}
        InboundDecision::PairingRequired { reply, msg_id, .. } => {
            send_passive_text(auth_header, &event.user_openid, &reply, &msg_id).await;
        }
        InboundDecision::ConsumePairingCode {
            user_openid,
            code,
            msg_id,
        } => {
            let result = match consume_code(&db, &user_openid, &code).await {
                Ok(value) => value,
                Err(error) => {
                    warn!(error = %error, "QQ pairing consume failed");
                    PairingBindResult::InvalidOrExpired
                }
            };
            if let PairingBindResult::Bound { user_id } = result {
                info!(user_id, "QQ C2C paired");
            }
            send_passive_text(
                auth_header,
                &event.user_openid,
                pairing_bind_reply(result),
                &msg_id,
            )
            .await;
        }
        InboundDecision::StartWork {
            user_id, msg_id, ..
        } => {
            info!(user_id, msg_id = %msg_id, "QQ C2C paired text received; Work ingest is a later ticket");
        }
    }
}

async fn send_passive_text(auth_header: &str, openid: &str, content: &str, msg_id: &str) {
    if content.is_empty() || openid.is_empty() || msg_id.is_empty() {
        return;
    }
    let enabled = {
        let config = GLOBAL_DYNAMIC_CONFIG.read().await;
        config.qq_bot_enabled
    };
    if !enabled {
        return;
    }
    let client = http_client::get_global_client().await;
    let url = format!("{API_BASE}/v2/users/{openid}/messages");
    let body = serde_json::json!({
        "content": content,
        "msg_type": 0,
        "msg_id": msg_id,
        "msg_seq": 1,
    });
    match client
        .post(&url)
        .timeout(HTTP_TIMEOUT)
        .header("Authorization", auth_header)
        .json(&body)
        .send()
        .await
    {
        Ok(resp) if resp.status().is_success() => {}
        Ok(resp) => {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            warn!(
                status = status.as_u16(),
                body = %myriad_error::redact_secrets(&text),
                "QQ pairing reply failed"
            );
        }
        Err(error) => {
            warn!(
                error = %myriad_error::redact_secrets(&error.to_string()),
                "QQ pairing reply request failed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn masks_openid_without_returning_the_full_id() {
        assert_eq!(mask_openid("abcdefg"), "****defg");
        assert_eq!(mask_openid("ab"), "****");
        assert_eq!(mask_openid(""), "");
        assert!(!mask_openid("openid-secret-value").contains("openid-secret"));
    }

    #[test]
    fn unpaired_plain_text_is_not_a_work_request() {
        let event = InboundC2cText {
            msg_id: "m1".into(),
            user_openid: "oid".into(),
            content: "帮我查天气".into(),
        };
        let decision = ingest_c2c_text(&event, PairingLookup::Unpaired, false);
        match decision {
            InboundDecision::PairingRequired { reply, .. } => {
                assert_eq!(reply, PAIRING_REQUIRED_REPLY);
            }
            other => panic!("{other:?}"),
        }
    }
}
