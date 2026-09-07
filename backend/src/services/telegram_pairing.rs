//! Telegram DM pairing: one-time codes and `user_identities` rows.
//!
//! Pairing is "this from.id is which site user", not Telegram login OAuth.
//! Codes live in `tapp_runtime_registry`; the bound id is `provider = telegram`.

use chrono::{Duration as ChronoDuration, Utc};
use myriad_agent_rules::channel::{
    encode_pairing_code, extract_pairing_code, format_pairing_code, ingest_channel_text,
    pairing_bind_reply_for, InboundDecision, PairingBindResult, PairingLookup, TelegramPrivateText,
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

use crate::services::tapp_registry::{self as shared_registry, RegistryIdentity};

pub const TELEGRAM_IDENTITY_PROVIDER: &str = "telegram";
const CODE_NAMESPACE: &str = "telegram_pairing_code";
const CODE_TTL_SECS: i64 = 10 * 60;

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
    crate::services::qq_pairing::mask_openid(openid)
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
                SeaValue::String(Some(TELEGRAM_IDENTITY_PROVIDER.to_string())),
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
                SeaValue::String(Some(TELEGRAM_IDENTITY_PROVIDER.to_string())),
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
                SeaValue::String(Some(TELEGRAM_IDENTITY_PROVIDER.to_string())),
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
                SeaValue::String(Some(TELEGRAM_IDENTITY_PROVIDER.to_string())),
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
            SeaValue::String(Some(TELEGRAM_IDENTITY_PROVIDER.to_string())),
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
                SeaValue::String(Some(TELEGRAM_IDENTITY_PROVIDER.to_string())),
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

/// Worker entry: classify private text, pair, or start Work.
pub async fn handle_inbound(event: TelegramPrivateText, token: &str) {
    let Ok(db) = crate::services::tapp_registry::database().await else {
        warn!("Telegram pairing skipped: database is not connected");
        return;
    };
    let inbound = event.inbound();
    let pairing = match lookup_openid(&db, &inbound.user_openid).await {
        Ok(value) => value,
        Err(error) => {
            warn!(error = %error, "Telegram pairing lookup failed");
            return;
        }
    };
    let chat_id = event.chat_id_key();
    match ingest_channel_text(&inbound, pairing, false, "telegram", &chat_id) {
        InboundDecision::Duplicate { .. } => {}
        InboundDecision::PairingRequired { reply, .. } => {
            send_text(token, &chat_id, &reply).await;
        }
        InboundDecision::ConsumePairingCode {
            user_openid, code, ..
        } => {
            let result = match consume_code(&db, &user_openid, &code).await {
                Ok(value) => value,
                Err(error) => {
                    warn!(error = %error, "Telegram pairing consume failed");
                    PairingBindResult::InvalidOrExpired
                }
            };
            if let PairingBindResult::Bound { user_id } = result {
                info!(user_id, "Telegram DM paired");
            }
            send_text(token, &chat_id, pairing_bind_reply_for(result, "telegram")).await;
        }
        InboundDecision::StartWork {
            user_id,
            input,
            session_key,
            msg_id,
            ..
        } => {
            crate::services::telegram_work::start_paired_work(
                &db,
                user_id,
                &chat_id,
                &input,
                &session_key,
                &msg_id,
                token,
            )
            .await;
        }
    }
}

async fn send_text(token: &str, chat_id: &str, content: &str) {
    if let Err(error) = crate::services::telegram_bot::send_message(token, chat_id, content).await {
        warn!(?error, "Telegram pairing reply failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use myriad_agent_rules::channel::{ingest_channel_text, InboundC2cText};

    #[test]
    fn unpaired_plain_text_is_not_a_work_request() {
        let event = InboundC2cText {
            msg_id: "10".into(),
            user_openid: "1001".into(),
            content: "帮我查天气".into(),
        };
        let decision =
            ingest_channel_text(&event, PairingLookup::Unpaired, false, "telegram", "1001");
        match decision {
            InboundDecision::PairingRequired { reply, .. } => {
                assert_eq!(reply, PAIRING_REQUIRED_REPLY);
            }
            other => panic!("{other:?}"),
        }
    }
}
