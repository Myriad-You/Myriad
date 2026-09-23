//! Shared pairing I/O: mint, consume, lookup, unpair.
//!
//! Provider and registry namespace vary; the bind rules do not.

use chrono::{Duration as ChronoDuration, Utc};
use myriad_agent_rules::channel::{
    PairingBindResult, PairingLookup, encode_pairing_code, extract_pairing_code,
    format_pairing_code,
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
    match row {
        Some(row) => Ok(PairingLookup::Paired {
            user_id: pairing_user_id(&row)?,
        }),
        None => Ok(PairingLookup::Unpaired),
    }
}

fn pairing_user_id(row: &sea_orm::QueryResult) -> Result<i32, DbErr> {
    let user_id = row
        .try_get::<i32>("", "user_id")
        .map_err(|error| DbErr::Custom(error.to_string()))?;
    if user_id <= 0 {
        return Err(DbErr::Custom("pairing user_id is invalid".into()));
    }
    Ok(user_id)
}

/// Trim, drop empty and de-duplicate keys, keeping the caller's order.
fn normalized_keys<S: AsRef<str>>(keys: &[S]) -> Vec<String> {
    let mut out: Vec<String> = Vec::with_capacity(keys.len());
    for key in keys {
        let key = key.as_ref().trim();
        if !key.is_empty() && !out.iter().any(|seen| seen == key) {
            out.push(key.to_string());
        }
    }
    out
}

/// `$first, $first+1, …` for `count` bind parameters.
fn placeholder_list(first: usize, count: usize) -> String {
    (first..first + count)
        .map(|index| format!("${index}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Pairing rows for `keys` under `scope`, as `(provider_user_id, row)`.
async fn scoped_identity_rows(
    db: &impl ConnectionTrait,
    provider: &str,
    scope: &str,
    keys: &[String],
) -> Result<Vec<(String, sea_orm::QueryResult)>, DbErr> {
    let mut values: Vec<SeaValue> = Vec::with_capacity(keys.len() + 2);
    values.push(provider.into());
    values.push(scope.into());
    values.extend(keys.iter().map(|key| SeaValue::from(key.as_str())));
    let rows = db
        .query_all_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            format!(
                "SELECT provider_user_id, user_id FROM user_identities \
                 WHERE provider = $1 AND raw_profile->>'channel_scope' = $2 \
                 AND provider_user_id IN ({})",
                placeholder_list(3, keys.len())
            ),
            values,
        ))
        .await?;
    rows.into_iter()
        .map(|row| Ok((row.try_get::<String>("", "provider_user_id")?, row)))
        .collect()
}

/// First matching key (in caller order) wins. Empty keys are skipped.
pub async fn lookup_any(
    db: &DatabaseConnection,
    channel: PairingChannel,
    keys: &[String],
) -> Result<PairingLookup, DbErr> {
    let keys = normalized_keys(keys);
    if keys.is_empty() {
        return Ok(PairingLookup::Unpaired);
    }
    let scope = credential_scope(channel.provider).await;
    let rows = scoped_identity_rows(db, channel.provider, &scope, &keys).await?;
    for key in &keys {
        if let Some((_, row)) = rows.iter().find(|(bound, _)| bound == key) {
            return Ok(PairingLookup::Paired {
                user_id: pairing_user_id(row)?,
            });
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
                if stored.scope == credential_scope(channel.provider).await
                    && pending_expires_at.is_some()
                {
                    pending_code = Some(format_pairing_code(&stored.code));
                } else {
                    pending_expires_at = None;
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
    let txn = db.begin().await?;
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_advisory_xact_lock(hashtextextended($1, 0))",
        [format!("channel-pairing:{}:{user_id}", channel.provider).into()],
    ))
    .await?;
    txn.execute_raw(Statement::from_sql_and_values(
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
        &txn,
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

    txn.commit().await?;
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
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "DELETE FROM tapp_runtime_registry WHERE subject_id = $1 AND namespace = $2",
        [user_id.into(), channel.code_namespace.into()],
    ))
    .await?;
    txn.commit().await?;
    crate::services::channel_work::revoke_pairing(db, channel.provider, user_id).await;
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
        shared_registry::get::<StoredPairingCode>(db, channel.code_namespace, &token_hash(&code))
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
        &code,
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
        shared_registry::get::<StoredPairingCode>(db, channel.code_namespace, &token_hash(&code))
            .await?
    else {
        return Ok(PairingBindResult::InvalidOrExpired);
    };
    if stored.code != code || stored.scope != credential_scope(channel.provider).await {
        return Ok(PairingBindResult::InvalidOrExpired);
    }

    bind_openids(db, channel, stored.user_id, &keys, &stored.scope, &code).await
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
    let keys = normalized_keys(keys);
    if keys.is_empty() {
        return Ok(());
    }
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
    for (_, row) in scoped_identity_rows(&txn, channel.provider, &scope, &keys).await? {
        if row.try_get::<i32>("", "user_id")? != user_id {
            return Err(DbErr::Custom("Conflicting channel identity aliases".into()));
        }
        has_current_identity = true;
    }
    if !has_current_identity {
        return Ok(());
    }
    txn.execute_raw(insert_pairing_rows(
        user_id,
        channel.provider,
        &scope,
        &keys,
        "",
    ))
    .await?;
    txn.commit().await
}

/// One multi-row pairing INSERT for `keys`; `ON CONFLICT (provider,
/// provider_user_id) DO NOTHING` keeps the global UNIQUE as the authority.
fn insert_pairing_rows(
    user_id: i32,
    provider: &str,
    scope: &str,
    keys: &[String],
    returning: &str,
) -> Statement {
    let mut values: Vec<SeaValue> = Vec::with_capacity(keys.len() + 3);
    values.push(user_id.into());
    values.push(provider.into());
    values.push(scope.into());
    let rows = keys
        .iter()
        .enumerate()
        .map(|(index, key)| {
            values.push(key.as_str().into());
            format!(
                "($1, $2, ${}, false, NOW(), jsonb_build_object('channel_scope', $3::text))",
                index + 4
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        format!(
            "INSERT INTO user_identities ( \
                user_id, provider, provider_user_id, is_primary, linked_at, raw_profile \
             ) VALUES {rows} \
             ON CONFLICT (provider, provider_user_id) DO NOTHING{returning}"
        ),
        values,
    )
}

async fn bind_openids(
    db: &DatabaseConnection,
    channel: PairingChannel,
    user_id: i32,
    keys: &[String],
    scope: &str,
    code: &str,
) -> Result<PairingBindResult, DbErr> {
    let keys = normalized_keys(keys);
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
    // Consume under the same per-user lock as mint/unpair. A code looked up before
    // revocation must not bind after that revocation has committed.
    let stored =
        shared_registry::take::<StoredPairingCode>(&txn, channel.code_namespace, &token_hash(code))
            .await?;
    if !stored.is_some_and(|stored| {
        stored.user_id == user_id && stored.code == code && stored.scope == scope
    }) {
        return Ok(PairingBindResult::InvalidOrExpired);
    }
    for (_, row) in scoped_identity_rows(&txn, channel.provider, scope, &keys).await? {
        let bound_user = pairing_user_id(&row)?;
        if bound_user != user_id {
            txn.commit().await?;
            return Ok(PairingBindResult::OpenidTaken);
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

    let mut stale_scope: Vec<SeaValue> = Vec::with_capacity(keys.len() + 2);
    stale_scope.push(channel.provider.into());
    stale_scope.push(scope.into());
    stale_scope.extend(keys.iter().map(|key| SeaValue::from(key.as_str())));
    txn.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        format!(
            "DELETE FROM user_identities WHERE provider = $1 \
             AND raw_profile->>'channel_scope' IS DISTINCT FROM $2 \
             AND provider_user_id IN ({})",
            placeholder_list(3, keys.len())
        ),
        stale_scope,
    ))
    .await?;
    // Every key must land; a row kept by DO NOTHING belongs to another user.
    let inserted = txn
        .query_all_raw(insert_pairing_rows(
            user_id,
            channel.provider,
            scope,
            &keys,
            " RETURNING user_id",
        ))
        .await?;
    if inserted.len() != keys.len() {
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
    use super::{is_pairing_provider, normalized_keys, placeholder_list};

    #[test]
    fn pairing_keys_are_trimmed_deduplicated_and_ordered() {
        assert_eq!(
            normalized_keys(&[" ou_1 ", "", "on_2", "ou_1", "  "]),
            vec!["ou_1".to_string(), "on_2".to_string()]
        );
        assert!(normalized_keys::<&str>(&[]).is_empty());
        assert_eq!(placeholder_list(3, 2), "$3, $4");
    }

    #[test]
    fn pairing_lookup_does_not_treat_decode_failure_as_unpaired() {
        let src = include_str!("channel_pairing.rs");
        let lookup = src
            .split("pub async fn lookup_openid")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn lookup_any").next())
            .expect("lookup_openid");
        assert!(lookup.contains("pairing_user_id"));
        assert!(!lookup.contains("unwrap_or(0)"));
        let bind = src
            .split("let bound_user")
            .nth(1)
            .and_then(|rest| rest.split("txn.execute_raw").next())
            .expect("bound_user");
        let lookup_any = src
            .split("pub async fn lookup_any")
            .nth(1)
            .and_then(|rest| rest.split("pub async fn status_for_user").next())
            .expect("lookup_any");
        assert!(lookup_any.contains("pairing_user_id"));
        assert!(bind.contains("pairing_user_id"));
        assert!(!bind.contains("unwrap_or(0)"));
    }

    #[tokio::test]
    async fn batched_alias_bind_lookup_and_conflicts_when_db_provided() {
        use super::{FEISHU, consume_code_keys, ensure_aliases, lookup_any, mint_code};
        use myriad_agent_rules::channel::{PairingBindResult, PairingLookup};
        use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

        let Ok(database_url) = std::env::var("CHANNEL_TEST_DATABASE_URL") else {
            return;
        };
        let isolated =
            crate::db::IsolatedSchema::migrated(&database_url, "channel_pairing_test").await;
        let db = isolated.db.clone();
        let tag = uuid::Uuid::new_v4().simple().to_string();
        let mut users = Vec::new();
        for who in ["a", "b"] {
            let row = db
                .query_one_raw(Statement::from_sql_and_values(
                    DatabaseBackend::Postgres,
                    "INSERT INTO users (username) VALUES ($1) RETURNING id",
                    [format!("pairing-{who}-{tag}").into()],
                ))
                .await
                .expect("insert user")
                .expect("row");
            users.push(row.try_get::<i32>("", "id").expect("id"));
        }
        let (user_a, user_b) = (users[0], users[1]);
        let key = |name: &str| format!("{name}-{tag}");

        // Duplicate / padded keys collapse into one row per key.
        let code = mint_code(&db, FEISHU, user_a).await.expect("mint a").code;
        let bound = consume_code_keys(
            &db,
            FEISHU,
            &[key("k1"), format!(" {} ", key("k1")), key("k2"), String::new()],
            &code,
        )
        .await
        .expect("bind a");
        assert_eq!(bound, PairingBindResult::Bound { user_id: user_a });
        assert_eq!(
            lookup_any(&db, FEISHU, &[key("missing"), key("k2"), key("k1")])
                .await
                .expect("lookup"),
            PairingLookup::Paired { user_id: user_a }
        );

        ensure_aliases(&db, FEISHU, user_a, &[key("k1"), key("k3")])
            .await
            .expect("alias");
        assert_eq!(
            lookup_any(&db, FEISHU, &[key("k3")]).await.expect("lookup alias"),
            PairingLookup::Paired { user_id: user_a }
        );

        // Another user's key rejects the whole bind; nothing is written for B.
        let code = mint_code(&db, FEISHU, user_b).await.expect("mint b").code;
        let taken = consume_code_keys(&db, FEISHU, &[key("k4"), key("k1")], &code)
            .await
            .expect("bind b");
        assert_eq!(taken, PairingBindResult::OpenidTaken);
        assert_eq!(
            lookup_any(&db, FEISHU, &[key("k4")]).await.expect("lookup k4"),
            PairingLookup::Unpaired
        );
        assert!(
            ensure_aliases(&db, FEISHU, user_b, &[key("k5"), key("k1")])
                .await
                .is_err()
        );
        assert_eq!(
            lookup_any(&db, FEISHU, &[key("k5")]).await.expect("lookup k5"),
            PairingLookup::Unpaired
        );

        drop(db);
        isolated.drop().await;
    }

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
pub(crate) use binding::{ChannelBinding, credential_scope, provider_for_platform};
