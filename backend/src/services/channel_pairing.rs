//! Shared pairing I/O: mint, consume, lookup, unpair.
//!
//! Provider and registry namespace vary; the bind rules do not.

use chrono::{Duration as ChronoDuration, Utc};
use myriad_agent_rules::channel::{
    encode_pairing_code, extract_pairing_code, format_pairing_code, PairingBindResult,
    PairingLookup,
};
use rand::Rng;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement, TransactionTrait,
    Value as SeaValue,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::services::tapp_registry::{self as shared_registry, RegistryIdentity};

const CODE_TTL_SECS: i64 = 10 * 60;

#[derive(Debug, Clone, Copy)]
pub struct PairingChannel {
    pub provider: &'static str,
    pub code_namespace: &'static str,
}

pub const QQ: PairingChannel = PairingChannel {
    provider: "qq",
    code_namespace: "qq_pairing_code",
};

pub const TELEGRAM: PairingChannel = PairingChannel {
    provider: "telegram",
    code_namespace: "telegram_pairing_code",
};

pub const DISCORD_DM: PairingChannel = PairingChannel {
    provider: "discord_dm",
    code_namespace: "discord_dm_pairing_code",
};

/// Pairing rows share `user_identities` with OAuth, but they are not login identities.
/// `discord` is the login / data-platform slug and must stay out of this set.
pub fn is_pairing_provider(provider: &str) -> bool {
    matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "qq" | "telegram" | "discord_dm"
    )
}

/// SQL predicate excluding pairing rows from OAuth / avatar identity queries.
pub const SQL_NOT_PAIRING_PROVIDER: &str =
    "LOWER(provider) NOT IN ('qq', 'telegram', 'discord_dm')";

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

pub async fn lookup_openid(
    db: &DatabaseConnection,
    channel: PairingChannel,
    openid: &str,
) -> Result<PairingLookup, DbErr> {
    let openid = openid.trim();
    if openid.is_empty() {
        return Ok(PairingLookup::Unpaired);
    }
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT user_id FROM user_identities WHERE provider = $1 AND provider_user_id = $2",
            vec![
                SeaValue::String(Some(channel.provider.to_string())),
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
    channel: PairingChannel,
    user_id: i32,
) -> Result<PairingStatus, DbErr> {
    let identity = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT id, provider_user_id, linked_at FROM user_identities \
             WHERE user_id = $1 AND provider = $2 ORDER BY linked_at DESC LIMIT 1",
            vec![
                SeaValue::Int(Some(user_id)),
                SeaValue::String(Some(channel.provider.to_string())),
            ],
        ))
        .await?;

    let mut pending_code = None;
    let mut pending_expires_at = None;
    if identity.is_none() {
        let rows = shared_registry::list(db, channel.code_namespace, Some(user_id), None).await?;
        if let Some(row) = rows.into_iter().next() {
            if let Ok(stored) = serde_json::from_value::<StoredPairingCode>(row.payload) {
                pending_expires_at = live_code_expiry(db, channel, user_id).await?;
                if pending_expires_at.is_some() {
                    pending_code = Some(format_pairing_code(&stored.code));
                }
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

async fn live_code_expiry(
    db: &DatabaseConnection,
    channel: PairingChannel,
    user_id: i32,
) -> Result<Option<String>, DbErr> {
    let row = db
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT expires_at FROM tapp_runtime_registry \
             WHERE namespace = $1 AND subject_id = $2 \
               AND expires_at > EXTRACT(EPOCH FROM NOW())::BIGINT \
             ORDER BY updated_at DESC LIMIT 1",
            vec![
                SeaValue::String(Some(channel.code_namespace.to_string())),
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

pub async fn mint_code(
    db: &DatabaseConnection,
    channel: PairingChannel,
    user_id: i32,
) -> Result<IssuedPairingCode, DbErr> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM tapp_runtime_registry WHERE namespace = $1 AND subject_id = $2",
        vec![
            SeaValue::String(Some(channel.code_namespace.to_string())),
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
        channel.code_namespace,
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

pub async fn unpair(
    db: &DatabaseConnection,
    channel: PairingChannel,
    user_id: i32,
) -> Result<bool, DbErr> {
    let result = db
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM user_identities WHERE user_id = $1 AND provider = $2",
            vec![
                SeaValue::Int(Some(user_id)),
                SeaValue::String(Some(channel.provider.to_string())),
            ],
        ))
        .await?;
    Ok(result.rows_affected() > 0)
}

pub async fn consume_code(
    db: &DatabaseConnection,
    channel: PairingChannel,
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
        shared_registry::take::<StoredPairingCode>(db, channel.code_namespace, &token_hash(&code))
            .await?
    else {
        return Ok(PairingBindResult::InvalidOrExpired);
    };
    if stored.code != code {
        return Ok(PairingBindResult::InvalidOrExpired);
    }

    bind_openid(db, channel, stored.user_id, openid).await
}

async fn bind_openid(
    db: &DatabaseConnection,
    channel: PairingChannel,
    user_id: i32,
    openid: &str,
) -> Result<PairingBindResult, DbErr> {
    let txn = db.begin().await?;
    let existing = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT user_id FROM user_identities WHERE provider = $1 AND provider_user_id = $2",
            vec![
                SeaValue::String(Some(channel.provider.to_string())),
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
            SeaValue::String(Some(channel.provider.to_string())),
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
                SeaValue::String(Some(channel.provider.to_string())),
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

#[cfg(test)]
mod tests {
    use super::is_pairing_provider;

    #[test]
    fn pairing_providers_are_not_oauth_slugs() {
        assert!(is_pairing_provider("qq"));
        assert!(is_pairing_provider("Telegram"));
        assert!(is_pairing_provider("discord_dm"));
        assert!(!is_pairing_provider("discord"));
        assert!(!is_pairing_provider("github"));
        assert!(!is_pairing_provider(""));
    }
}
