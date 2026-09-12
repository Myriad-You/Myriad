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

pub const FEISHU: PairingChannel = PairingChannel {
    provider: "feishu",
    code_namespace: "feishu_pairing_code",
};

/// Pairing rows share `user_identities` with OAuth, but they are not login identities.
/// `discord` is the login / data-platform slug and must stay out of this set.
pub fn is_pairing_provider(provider: &str) -> bool {
    matches!(
        provider.trim().to_ascii_lowercase().as_str(),
        "qq" | "telegram" | "discord_dm" | "feishu"
    )
}

/// SQL predicate excluding pairing rows from OAuth / avatar identity queries.
pub const SQL_NOT_PAIRING_PROVIDER: &str =
    "LOWER(provider) NOT IN ('qq', 'telegram', 'discord_dm', 'feishu')";

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoredPairingCode {
    user_id: i32,
    code: String,
    #[serde(default)]
    scope: String,
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
            "SELECT user_id FROM user_identities WHERE provider = $1 AND provider_user_id = $2 AND raw_profile->>'channel_scope' = $3",
            vec![
                SeaValue::String(Some(channel.provider.to_string())),
                SeaValue::String(Some(openid.to_string())),
                credential_scope(channel.provider).await.into(),
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

/// First matching key wins. Empty keys are skipped.
pub async fn lookup_any(
    db: &DatabaseConnection,
    channel: PairingChannel,
    keys: &[String],
) -> Result<PairingLookup, DbErr> {
    for key in keys {
        match lookup_openid(db, channel, key).await? {
            PairingLookup::Paired { user_id } if user_id != 0 => {
                return Ok(PairingLookup::Paired { user_id });
            }
            _ => {}
        }
    }
    Ok(PairingLookup::Unpaired)
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
             WHERE user_id = $1 AND provider = $2 AND raw_profile->>'channel_scope' = $3 ORDER BY linked_at DESC LIMIT 1",
            vec![
                SeaValue::Int(Some(user_id)),
                SeaValue::String(Some(channel.provider.to_string())),
                credential_scope(channel.provider).await.into(),
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
        scope: credential_scope(channel.provider).await,
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
    let txn = db.begin().await?;
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        [format!("channel-pairing:{}:{user_id}", channel.provider).into()],
    ))
    .await?;
    let result = txn
        .execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "DELETE FROM user_identities WHERE user_id = $1 AND provider = $2",
            [user_id.into(), channel.provider.into()],
        ))
        .await?;
    // Revoke first; running observers must fail their binding check even if cleanup fails.
    txn.commit().await?;
    crate::services::channel_work::revoke_pairing(db, channel.provider, user_id).await;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM tapp_runtime_registry WHERE subject_id = $1 AND namespace = $2",
        [user_id.into(), channel.code_namespace.into()],
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
    if stored.code != code || stored.scope != credential_scope(channel.provider).await {
        return Ok(PairingBindResult::InvalidOrExpired);
    }

    bind_openids(
        db,
        channel,
        stored.user_id,
        &[openid.to_string()],
        &stored.scope,
    )
    .await
}

/// Bind every non-empty key as the same user's pairing identity.
/// Feishu keeps `open_id` and `user_id` as aliases so a later payload that
/// only has one of them still finds the row.
pub async fn consume_code_keys(
    db: &DatabaseConnection,
    channel: PairingChannel,
    keys: &[String],
    raw_code: &str,
) -> Result<PairingBindResult, DbErr> {
    let Some(code) = extract_pairing_code(raw_code) else {
        return Ok(PairingBindResult::InvalidOrExpired);
    };
    let keys: Vec<String> = keys
        .iter()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
        .collect();
    if keys.is_empty() {
        return Ok(PairingBindResult::InvalidOrExpired);
    }

    let Some(stored) =
        shared_registry::take::<StoredPairingCode>(db, channel.code_namespace, &token_hash(&code))
            .await?
    else {
        return Ok(PairingBindResult::InvalidOrExpired);
    };
    if stored.code != code || stored.scope != credential_scope(channel.provider).await {
        return Ok(PairingBindResult::InvalidOrExpired);
    }

    bind_openids(db, channel, stored.user_id, &keys, &stored.scope).await
}

/// If this user is already paired via one key, write the remaining keys so a
/// later payload that only carries the alias still matches. Keys owned by
/// another user are left alone.
pub async fn ensure_aliases(
    db: &DatabaseConnection,
    channel: PairingChannel,
    user_id: i32,
    keys: &[String],
) -> Result<(), DbErr> {
    let scope = credential_scope(channel.provider).await;
    let txn = db.begin().await?;
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        [format!("channel-pairing:{}:{user_id}", channel.provider).into()],
    ))
    .await?;
    // An in-flight event must not recreate identities after unpair committed.
    let mut has_current_identity = false;
    for key in keys {
        let row = txn.query_one_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
            "SELECT user_id FROM user_identities WHERE provider = $1 AND provider_user_id = $2 AND raw_profile->>'channel_scope' = $3",
            [channel.provider.into(), key.as_str().into(), scope.clone().into()])).await?;
        if let Some(row) = row {
            if row.try_get::<i32>("", "user_id")? != user_id {
                return Err(DbErr::Custom("Conflicting channel identity aliases".into()));
            }
            has_current_identity = true;
        }
    }
    if !has_current_identity {
        return Ok(());
    }
    for key in keys
        .iter()
        .map(|key| key.trim())
        .filter(|key| !key.is_empty())
    {
        txn.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
            "INSERT INTO user_identities (user_id, provider, provider_user_id, is_primary, linked_at, raw_profile)              VALUES ($1, $2, $3, false, NOW(), jsonb_build_object('channel_scope', $4::text))              ON CONFLICT (provider, provider_user_id) DO NOTHING",
            [user_id.into(), channel.provider.into(), key.into(), scope.clone().into()])).await?;
    }
    txn.commit().await
}

async fn bind_openids(
    db: &DatabaseConnection,
    channel: PairingChannel,
    user_id: i32,
    keys: &[String],
    scope: &str,
) -> Result<PairingBindResult, DbErr> {
    let keys: Vec<String> = keys
        .iter()
        .map(|key| key.trim().to_string())
        .filter(|key| !key.is_empty())
        .collect();
    if keys.is_empty() {
        return Ok(PairingBindResult::InvalidOrExpired);
    }

    let txn = db.begin().await?;
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        [format!("channel-pairing:{}:{user_id}", channel.provider).into()],
    ))
    .await?;
    for key in &keys {
        let existing = txn
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "SELECT user_id FROM user_identities WHERE provider = $1 AND provider_user_id = $2 AND raw_profile->>'channel_scope' = $3",
                vec![
                    SeaValue::String(Some(channel.provider.to_string())),
                    SeaValue::String(Some(key.clone())),
                    scope.into(),
                ],
            ))
            .await?;
        if let Some(row) = existing {
            let bound_user: i32 = row.try_get("", "user_id").unwrap_or(0);
            if bound_user != user_id {
                txn.commit().await?;
                return Ok(PairingBindResult::OpenidTaken);
            }
        }
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

    for key in &keys {
        txn.execute_raw(Statement::from_sql_and_values(DatabaseBackend::Postgres,
            "DELETE FROM user_identities WHERE provider = $1 AND provider_user_id = $2 AND raw_profile->>'channel_scope' IS DISTINCT FROM $3",
            [channel.provider.into(), key.as_str().into(), scope.into()])).await?;
    }
    let mut inserted_any = false;
    for key in &keys {
        let inserted = txn
            .query_one_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "INSERT INTO user_identities ( \
                    user_id, provider, provider_user_id, is_primary, linked_at, raw_profile \
                 ) VALUES ($1, $2, $3, false, NOW(), jsonb_build_object('channel_scope', $4::text)) \
                 ON CONFLICT (provider, provider_user_id) DO NOTHING \
                 RETURNING user_id",
                vec![
                    SeaValue::Int(Some(user_id)),
                    SeaValue::String(Some(channel.provider.to_string())),
                    SeaValue::String(Some(key.clone())),
                    scope.into(),
                ],
            ))
            .await?;
        if inserted.is_some() {
            inserted_any = true;
        } else {
            txn.rollback().await?;
            return Ok(PairingBindResult::OpenidTaken);
        }
    }
    if !inserted_any {
        txn.rollback().await?;
        return Ok(PairingBindResult::OpenidTaken);
    }
    if credential_scope(channel.provider).await != scope {
        txn.rollback().await?;
        return Ok(PairingBindResult::InvalidOrExpired);
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
        assert!(is_pairing_provider("feishu"));
        assert!(!is_pairing_provider("discord"));
        assert!(!is_pairing_provider("github"));
        assert!(!is_pairing_provider(""));
    }
}

mod binding;
pub(crate) use binding::{credential_scope, provider_for_platform, ChannelBinding};
