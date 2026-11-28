//! API keys: what a tenant calls the rig API with.
//!
//! A key is `mrk_<12 characters>_<43 characters>`, both parts base62. The
//! first part, with `mrk_`, is the key's prefix: not a secret, it is how
//! the key is found and how its owner tells keys apart. The second part is
//! 256 random bits. The whole key is shown once, when it is issued; only its
//! HMAC-SHA256 under a key derived from the installation's data key is kept,
//! so the database alone cannot even check a guess. The hash is compared in
//! constant time. A key may expire; it stops working when it is revoked; it
//! records when it was last used, at most once a minute.
//!
//! Neither a key nor its hash is ever logged or returned after issuing; the
//! prefix may be.

use chrono::{DateTime, Utc};
use hmac::{Hmac, KeyInit, Mac};
use sea_orm::{ConnectionTrait, DbErr, QueryResult};
use sha2::Sha256;
use subtle::ConstantTimeEq;

use super::statement;
use super::tenants::{self, Tenant};

/// Every key starts with this.
pub const KEY_PREFIX: &str = "mrk_";
/// The public part after [`KEY_PREFIX`].
const PUBLIC_CHARS: usize = 12;
/// The secret part: 32 random bytes as base62.
const SECRET_CHARS: usize = 43;
/// The whole key: prefix, public part, `_`, secret part.
pub const KEY_LEN: usize = KEY_PREFIX.len() + PUBLIC_CHARS + 1 + SECRET_CHARS;
/// Longest key name kept (its column's width).
pub const MAX_NAME_CHARS: usize = 80;
/// Last use is recorded at most this often.
const TOUCH_EVERY_SECS: i64 = 60;
/// What the hashing key is derived for, from the installation's data key.
#[cfg(not(test))]
const HASH_KEY_CONTEXT: &[u8] = b"myriad-rig-api-key-hash-v1";

const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

#[derive(Clone, Debug, PartialEq)]
pub struct ApiKey {
    /// `rak_<uuid simple>`.
    pub id: String,
    pub tenant_id: String,
    pub name: String,
    /// `mrk_` and the key's public part.
    pub prefix: String,
    pub created_by_user_id: Option<i32>,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub revoked_at: Option<DateTime<Utc>>,
    pub last_used_at: Option<DateTime<Utc>>,
}

const COLUMNS: &str = "id, tenant_id, name, prefix, created_by_user_id, created_at, expires_at, revoked_at, \
     last_used_at";

impl ApiKey {
    fn from_row(row: &QueryResult, prefix: &str) -> Result<Self, DbErr> {
        let col = |name: &str| format!("{prefix}{name}");
        Ok(Self {
            id: row.try_get("", &col("id"))?,
            tenant_id: row.try_get("", &col("tenant_id"))?,
            name: row.try_get("", &col("name"))?,
            prefix: row.try_get("", &col("prefix"))?,
            created_by_user_id: row.try_get("", &col("created_by_user_id"))?,
            created_at: row.try_get("", &col("created_at"))?,
            expires_at: row.try_get("", &col("expires_at"))?,
            revoked_at: row.try_get("", &col("revoked_at"))?,
            last_used_at: row.try_get("", &col("last_used_at"))?,
        })
    }

    /// Whether the key would be taken at `now`.
    pub fn live(&self, now: DateTime<Utc>) -> bool {
        self.revoked_at.is_none() && self.expires_at.is_none_or(|at| at > now)
    }
}

/// `chars` base62 characters, each uniformly random.
fn random_base62(chars: usize) -> String {
    let mut out = String::with_capacity(chars);
    while out.len() < chars {
        // 248 = 4 × 62: a byte below it maps uniformly onto the alphabet.
        let byte: u8 = rand::random();
        if byte < 248 {
            out.push(char::from(BASE62[usize::from(byte % 62)]));
        }
    }
    out
}

/// A new key and its prefix.
fn generate() -> (String, String) {
    let prefix = format!("{KEY_PREFIX}{}", random_base62(PUBLIC_CHARS));
    let key = format!("{prefix}_{}", random_base62(SECRET_CHARS));
    (prefix, key)
}

/// The prefix of a well-formed key; `None` for anything else.
pub fn prefix_of(key: &str) -> Option<&str> {
    if key.len() != KEY_LEN || !key.starts_with(KEY_PREFIX) {
        return None;
    }
    let prefix_end = KEY_PREFIX.len() + PUBLIC_CHARS;
    let (prefix, rest) = key.split_at(prefix_end);
    let secret = rest.strip_prefix('_')?;
    let base62 = |part: &str| part.bytes().all(|byte| byte.is_ascii_alphanumeric());
    (base62(&prefix[KEY_PREFIX.len()..]) && base62(secret)).then_some(prefix)
}

/// The key keys are hashed under.
fn hash_key() -> &'static [u8; 32] {
    static KEY: std::sync::OnceLock<[u8; 32]> = std::sync::OnceLock::new();
    KEY.get_or_init(|| {
        #[cfg(test)]
        {
            [0x5a; 32]
        }
        #[cfg(not(test))]
        {
            myriad_data_key::data_key().derive_subkey(HASH_KEY_CONTEXT)
        }
    })
}

/// HMAC-SHA256 (hex) of `key` under `hash_key`.
fn hash_with(hash_key: &[u8; 32], key: &str) -> String {
    let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(hash_key)
        .expect("HMAC takes a key of any length");
    mac.update(key.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

fn hash(key: &str) -> String {
    hash_with(hash_key(), key)
}

/// `name` as kept: trimmed and cut to the column's width.
fn clean_name(name: &str) -> String {
    name.trim().chars().take(MAX_NAME_CHARS).collect()
}

#[derive(Debug)]
pub enum IssueError {
    TenantMissing,
    /// The expiry given is not after now.
    ExpiryPast,
    Db(DbErr),
}

impl From<DbErr> for IssueError {
    fn from(error: DbErr) -> Self {
        Self::Db(error)
    }
}

/// Issues a key for `tenant_id`. The key is returned this once and never
/// again.
pub async fn issue<C: ConnectionTrait>(
    db: &C,
    tenant_id: &str,
    name: &str,
    expires_at: Option<DateTime<Utc>>,
    created_by_user_id: Option<i32>,
    now: DateTime<Utc>,
) -> Result<(ApiKey, String), IssueError> {
    if expires_at.is_some_and(|at| at <= now) {
        return Err(IssueError::ExpiryPast);
    }
    if tenants::get(db, tenant_id).await?.is_none() {
        return Err(IssueError::TenantMissing);
    }
    let (prefix, key) = generate();
    let row = db
        .query_one_raw(statement(
            &format!(
                "INSERT INTO rig_api_keys (id, tenant_id, name, prefix, secret_hash, \
                 created_by_user_id, created_at, expires_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8) RETURNING {COLUMNS}"
            ),
            vec![
                format!("rak_{}", uuid::Uuid::new_v4().simple()).into(),
                tenant_id.into(),
                clean_name(name).into(),
                prefix.into(),
                hash(&key).into(),
                created_by_user_id.into(),
                now.into(),
                expires_at.into(),
            ],
        ))
        .await?
        .ok_or(DbErr::RecordNotInserted)?;
    Ok((ApiKey::from_row(&row, "")?, key))
}

/// A tenant's keys, newest first, revoked ones included.
pub async fn list<C: ConnectionTrait>(db: &C, tenant_id: &str) -> Result<Vec<ApiKey>, DbErr> {
    db.query_all_raw(statement(
        &format!(
            "SELECT {COLUMNS} FROM rig_api_keys WHERE tenant_id = $1 \
             ORDER BY created_at DESC, id"
        ),
        vec![tenant_id.into()],
    ))
    .await?
    .iter()
    .map(|row| ApiKey::from_row(row, ""))
    .collect()
}

/// Revokes a key of `tenant_id`'s: it is refused from now on. A key revoked
/// already stays as it was. `None` when the tenant has no such key.
pub async fn revoke<C: ConnectionTrait>(
    db: &C,
    tenant_id: &str,
    key_id: &str,
    now: DateTime<Utc>,
) -> Result<Option<ApiKey>, DbErr> {
    db.query_one_raw(statement(
        &format!(
            "UPDATE rig_api_keys SET revoked_at = COALESCE(revoked_at, $3) \
             WHERE tenant_id = $1 AND id = $2 RETURNING {COLUMNS}"
        ),
        vec![tenant_id.into(), key_id.into(), now.into()],
    ))
    .await?
    .map(|row| ApiKey::from_row(&row, ""))
    .transpose()
}

/// Why a key presented was refused.
#[derive(Debug)]
pub enum Refused {
    /// Not the shape of a key.
    Malformed,
    /// No key has that prefix, or the key is not the one with it.
    Unknown,
    Revoked,
    Expired,
    /// The key is good, its tenant disabled.
    TenantDisabled,
    Db(DbErr),
}

impl From<DbErr> for Refused {
    fn from(error: DbErr) -> Self {
        Self::Db(error)
    }
}

/// The key a request was made with, and its tenant.
#[derive(Clone, Debug)]
pub struct Authenticated {
    pub key: ApiKey,
    pub tenant: Tenant,
}

/// The key `presented` and its tenant, if it is a key that is taken at
/// `now`. Only a caller holding the whole key learns that it was revoked or
/// has expired, or that its tenant is disabled: the hash is compared before
/// anything else is told.
pub async fn authenticate<C: ConnectionTrait>(
    db: &C,
    presented: &str,
    now: DateTime<Utc>,
) -> Result<Authenticated, Refused> {
    let prefix = prefix_of(presented).ok_or(Refused::Malformed)?;
    let key_columns = super::aliased(COLUMNS, "k");
    let tenant_columns = super::aliased(tenants::COLUMNS, "t");
    let row = db
        .query_one_raw(statement(
            &format!(
                "SELECT {key_columns}, k.secret_hash, {tenant_columns} \
                 FROM rig_api_keys k JOIN rig_api_tenants t ON t.id = k.tenant_id \
                 WHERE k.prefix = $1"
            ),
            vec![prefix.into()],
        ))
        .await?
        .ok_or(Refused::Unknown)?;
    let stored: String = row.try_get("", "secret_hash")?;
    if !bool::from(stored.as_bytes().ct_eq(hash(presented).as_bytes())) {
        return Err(Refused::Unknown);
    }
    let key = ApiKey::from_row(&row, "k_")?;
    let tenant = Tenant::from_row(&row, "t_")?;
    if key.revoked_at.is_some() {
        return Err(Refused::Revoked);
    }
    if key.expires_at.is_some_and(|at| at <= now) {
        return Err(Refused::Expired);
    }
    if tenant.disabled() {
        return Err(Refused::TenantDisabled);
    }
    Ok(Authenticated { key, tenant })
}

/// Records that the key `key_id` was used at `now`, unless that was
/// recorded less than a minute before.
pub async fn touch<C: ConnectionTrait>(
    db: &C,
    key_id: &str,
    now: DateTime<Utc>,
) -> Result<(), DbErr> {
    db.execute_raw(statement(
        "UPDATE rig_api_keys SET last_used_at = $2 WHERE id = $1 \
         AND (last_used_at IS NULL OR last_used_at < $2 - make_interval(secs => $3))",
        vec![key_id.into(), now.into(), (TOUCH_EVERY_SECS as f64).into()],
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn a_key_is_its_prefix_an_underscore_and_43_base62_characters() {
        let (prefix, key) = generate();
        assert_eq!(key.len(), KEY_LEN);
        assert_eq!(prefix_of(&key), Some(prefix.as_str()));
        assert!(prefix.starts_with(KEY_PREFIX));
        assert_eq!(prefix.len(), KEY_PREFIX.len() + PUBLIC_CHARS);
        let (_, other) = generate();
        assert_ne!(key, other);
    }

    #[test]
    fn anything_but_a_key_is_malformed() {
        let (_, key) = generate();
        let session_jwt = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.c2ln";
        for bad in [
            "",
            "mrk_",
            session_jwt,
            &key[..KEY_LEN - 1],
            &format!("{key}x"),
            &key.replacen(KEY_PREFIX, "mys_", 1),
            &key.replacen('_', "-", 2),
            &format!("{}!{}", &key[..20], &key[21..]),
            &format!("{} ", &key[..KEY_LEN - 1]),
        ] {
            assert_eq!(prefix_of(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn the_hash_is_keyed_and_never_the_key() {
        let (_, key) = generate();
        let one = hash_with(&[1; 32], &key);
        assert_eq!(one.len(), 64);
        assert_eq!(one, hash_with(&[1; 32], &key));
        assert_ne!(one, hash_with(&[2; 32], &key));
        assert!(!one.contains(&key[KEY_PREFIX.len()..]));
    }
}
