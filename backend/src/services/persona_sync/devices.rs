//! Devices that sync her with the site, and the token each one holds.
//!
//! The site owner adds a device and copies its token once; only the token's
//! sha256 is kept. A token is good for syncing her and for her link, and
//! nothing else: it is not a JWT, so every other route refuses it. A device stays until the
//! owner removes it or it leaves; tokens do not expire. Which end she lives
//! on is not a device's to say: that is her host lease (`super::lease`).

use chrono::{DateTime, Duration, FixedOffset, Utc};
use rand::Rng;
use sea_orm::{
    ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, QueryResult, Statement,
    TransactionTrait,
};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

/// Every device token starts with this.
pub const TOKEN_PREFIX: &str = "mys_";
/// The random part: 32 bytes in base62, always this many characters.
const TOKEN_BODY_CHARS: usize = 43;
/// Shown in the list so the owner can tell tokens apart: the prefix and
/// the first 8 characters after it.
const TOKEN_HINT_CHARS: usize = TOKEN_PREFIX.len() + 8;
/// Live devices one site may have.
pub const MAX_DEVICES: i64 = 10;
/// A device not seen for this long is shown as not syncing lately. It is
/// never removed for it.
pub const STALE_AFTER_DAYS: i64 = 30;
pub const MAX_NAME_CHARS: usize = 80;
/// The only scope a device token has.
pub const SCOPE: &str = "persona-sync";
/// Seen at most this often is recorded: a pull in pages is one visit.
const SEEN_EVERY_SECS: i64 = 60;
/// Serializes adding devices so the limit holds under concurrent requests.
const DEVICES_LOCK: i64 = 0x4D59_5359_4E43_4456; // "MYSYNCDV"

const BASE62: &[u8; 62] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";

/// Why a device stopped being one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RevokeReason {
    ByOwner,
    /// The site has another owner now: the device was the old owner's.
    OwnerChanged,
    DeviceLeft,
}

impl RevokeReason {
    pub fn as_str(self) -> &'static str {
        match self {
            RevokeReason::ByOwner => "owner_revoked",
            RevokeReason::OwnerChanged => "owner_changed",
            RevokeReason::DeviceLeft => "device_left",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    /// `dev_<uuid simple>`; also the device's node in revisions.
    pub id: String,
    pub user_id: i32,
    pub name: String,
    pub token_hint: String,
    pub lineage: Option<String>,
    pub created_at: DateTime<FixedOffset>,
    pub last_seen_at: Option<DateTime<FixedOffset>>,
    pub revoked_at: Option<DateTime<FixedOffset>>,
    pub revoked_reason: Option<String>,
}

impl Device {
    /// Not seen (or, never seen, not added) within [`STALE_AFTER_DAYS`].
    pub fn stale(&self, now: DateTime<Utc>) -> bool {
        let seen = self.last_seen_at.unwrap_or(self.created_at);
        self.revoked_at.is_none()
            && now - seen.with_timezone(&Utc) > Duration::days(STALE_AFTER_DAYS)
    }

    fn from_row(row: &QueryResult) -> Result<Self, DbErr> {
        Ok(Self {
            id: row.try_get("", "id")?,
            user_id: row.try_get("", "user_id")?,
            name: row.try_get("", "name")?,
            token_hint: row.try_get("", "token_hint")?,
            lineage: row.try_get("", "lineage")?,
            created_at: row.try_get("", "created_at")?,
            last_seen_at: row.try_get("", "last_seen_at")?,
            revoked_at: row.try_get("", "revoked_at")?,
            revoked_reason: row.try_get("", "revoked_reason")?,
        })
    }
}

const COLUMNS: &str = "id, user_id, name, token_hint, lineage, created_at, last_seen_at, \
                       revoked_at, revoked_reason";

/// 32 random bytes as a fixed-width base62 number.
fn base62(bytes: [u8; 32]) -> String {
    let mut number = bytes.to_vec();
    let mut digits = Vec::with_capacity(TOKEN_BODY_CHARS);
    for _ in 0..TOKEN_BODY_CHARS {
        let mut remainder = 0u32;
        for byte in &mut number {
            let value = (remainder << 8) | u32::from(*byte);
            *byte = (value / 62) as u8;
            remainder = value % 62;
        }
        digits.push(BASE62[remainder as usize]);
    }
    digits.reverse();
    String::from_utf8(digits).expect("base62 is ASCII")
}

fn new_token() -> String {
    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    format!("{TOKEN_PREFIX}{}", base62(bytes))
}

/// The shape of a device token, before anything is looked up.
pub fn well_formed(token: &str) -> bool {
    token.strip_prefix(TOKEN_PREFIX).is_some_and(|body| {
        body.len() == TOKEN_BODY_CHARS && body.bytes().all(|b| b.is_ascii_alphanumeric())
    })
}

fn token_hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values)
}

#[derive(Debug)]
pub enum AddError {
    Limit,
    Db(DbErr),
}

impl From<DbErr> for AddError {
    fn from(error: DbErr) -> Self {
        Self::Db(error)
    }
}

/// Adds a device for the owner `user_id`. The token is returned this once
/// and never again.
pub async fn add(
    db: &DatabaseConnection,
    user_id: i32,
    name: &str,
) -> Result<(Device, String), AddError> {
    let name: String = name.trim().chars().take(MAX_NAME_CHARS).collect();
    let txn = db.begin().await?;
    txn.execute_raw(statement(
        "SELECT pg_advisory_xact_lock($1)",
        vec![DEVICES_LOCK.into()],
    ))
    .await?;
    let live = txn
        .query_one_raw(statement(
            "SELECT COUNT(*)::BIGINT AS count FROM agent_sync_devices \
             WHERE user_id = $1 AND revoked_at IS NULL",
            vec![user_id.into()],
        ))
        .await?
        .map(|row| row.try_get::<i64>("", "count"))
        .transpose()?
        .unwrap_or(0);
    if live >= MAX_DEVICES {
        return Err(AddError::Limit);
    }
    let token = new_token();
    let id = format!("dev_{}", uuid::Uuid::new_v4().simple());
    let row = txn
        .query_one_raw(statement(
            &format!(
                "INSERT INTO agent_sync_devices \
                 (id, user_id, name, scope, token_hash, token_hint, created_at) \
                 VALUES ($1, $2, $3, $4, $5, $6, $7::timestamptz) RETURNING {COLUMNS}"
            ),
            vec![
                id.into(),
                user_id.into(),
                name.into(),
                SCOPE.into(),
                token_hash(&token).into(),
                token[..TOKEN_HINT_CHARS].to_string().into(),
                crate::clock::now().into(),
            ],
        ))
        .await?
        .ok_or_else(|| DbErr::RecordNotInserted)?;
    let device = Device::from_row(&row)?;
    txn.commit().await?;
    Ok((device, token))
}

/// The owner's devices, newest first, removed ones included.
pub async fn list<C: ConnectionTrait>(db: &C, user_id: i32) -> Result<Vec<Device>, DbErr> {
    db.query_all_raw(statement(
        &format!(
            "SELECT {COLUMNS} FROM agent_sync_devices WHERE user_id = $1 \
             ORDER BY created_at DESC, id"
        ),
        vec![user_id.into()],
    ))
    .await?
    .iter()
    .map(Device::from_row)
    .collect()
}

/// Removes a live device of the owner's, closing its link if one is open.
/// `None` when there is no such live device.
pub async fn revoke<C: ConnectionTrait>(
    db: &C,
    user_id: i32,
    id: &str,
    why: RevokeReason,
) -> Result<Option<Device>, DbErr> {
    let removed = db
        .query_one_raw(statement(
            &format!(
                "UPDATE agent_sync_devices SET revoked_at = $4::timestamptz, revoked_reason = $3 \
                 WHERE id = $1 AND user_id = $2 AND revoked_at IS NULL RETURNING {COLUMNS}"
            ),
            vec![
                id.into(),
                user_id.into(),
                why.as_str().into(),
                crate::clock::now().into(),
            ],
        ))
        .await?
        .as_ref()
        .map(Device::from_row)
        .transpose()?;
    if let Some(device) = &removed {
        // She lived on it, or was moving with it: the lease says so now
        // (vacant, or as she was before the move), and every process hears.
        super::lease::device_gone(db, &device.id).await?;
        use crate::services::agent::link::{Close, disconnect};
        // The reason the link was allowed is gone: the device forgets its token.
        let close = match why {
            RevokeReason::OwnerChanged => Close::OwnerChanged,
            RevokeReason::ByOwner | RevokeReason::DeviceLeft => Close::Revoked,
        };
        disconnect(&device.id, close);
    }
    Ok(removed)
}

/// Forgets a removed device of the owner's: its row goes, so it leaves the
/// list. Only a removed one: a live device is removed first ([`revoke`]),
/// which is what closes its link and settles her lease. Its backups stay
/// (they are how she is taken back). `false` when there is no such removed
/// device.
pub async fn forget<C: ConnectionTrait>(db: &C, user_id: i32, id: &str) -> Result<bool, DbErr> {
    let gone = db
        .execute_raw(statement(
            "DELETE FROM agent_sync_devices WHERE id = $1 AND user_id = $2 AND revoked_at IS NOT NULL",
            vec![id.into(), user_id.into()],
        ))
        .await?;
    Ok(gone.rows_affected() > 0)
}

/// Why a request's token is not a device's.
#[derive(Debug)]
pub enum Refused {
    /// No such token (or not one's shape).
    Unknown,
    Revoked,
    /// The device was the old owner's; it is revoked now.
    OwnerChanged,
    /// The site has no owner yet.
    NoOwner,
    Db(DbErr),
}

impl From<DbErr> for Refused {
    fn from(error: DbErr) -> Self {
        Self::Db(error)
    }
}

/// The device holding `token`, while it is one and belongs to the site's
/// owner `owner`. A device of a previous owner is revoked on sight.
pub async fn authenticate<C: ConnectionTrait>(
    db: &C,
    token: &str,
    owner: Option<i32>,
) -> Result<Device, Refused> {
    if !well_formed(token) {
        return Err(Refused::Unknown);
    }
    let hash = token_hash(token);
    let row = db
        .query_one_raw(statement(
            &format!("SELECT {COLUMNS}, token_hash FROM agent_sync_devices WHERE token_hash = $1"),
            vec![hash.clone().into()],
        ))
        .await?
        .ok_or(Refused::Unknown)?;
    let stored: String = row.try_get("", "token_hash")?;
    if !bool::from(stored.as_bytes().ct_eq(hash.as_bytes())) {
        return Err(Refused::Unknown);
    }
    let device = Device::from_row(&row)?;
    if device.revoked_at.is_some() {
        return Err(Refused::Revoked);
    }
    let owner = owner.ok_or(Refused::NoOwner)?;
    if device.user_id != owner {
        revoke(db, device.user_id, &device.id, RevokeReason::OwnerChanged).await?;
        return Err(Refused::OwnerChanged);
    }
    Ok(device)
}

/// The device `id` while it still is one and still belongs to the site's
/// owner `owner`: what a connection that stays open checks now and then, as
/// [`authenticate`] checked it when it opened.
pub async fn recheck<C: ConnectionTrait>(
    db: &C,
    id: &str,
    owner: Option<i32>,
) -> Result<Device, Refused> {
    let row = db
        .query_one_raw(statement(
            &format!("SELECT {COLUMNS} FROM agent_sync_devices WHERE id = $1"),
            vec![id.into()],
        ))
        .await?
        .ok_or(Refused::Unknown)?;
    let device = Device::from_row(&row)?;
    if device.revoked_at.is_some() {
        return Err(Refused::Revoked);
    }
    let owner = owner.ok_or(Refused::NoOwner)?;
    if device.user_id != owner {
        revoke(db, device.user_id, &device.id, RevokeReason::OwnerChanged).await?;
        return Err(Refused::OwnerChanged);
    }
    Ok(device)
}

/// Records that the device was just seen, at most once a minute.
pub async fn seen<C: ConnectionTrait>(db: &C, id: &str) -> Result<(), DbErr> {
    db.execute_raw(statement(
        "UPDATE agent_sync_devices SET last_seen_at = $3::timestamptz \
         WHERE id = $1 AND (last_seen_at IS NULL OR last_seen_at < $3::timestamptz - make_interval(secs => $2))",
        vec![id.into(), (SEEN_EVERY_SECS as f64).into(), crate::clock::now().into()],
    ))
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_token_is_the_prefix_and_43_base62_characters() {
        let token = new_token();
        assert!(well_formed(&token), "{token}");
        assert_ne!(new_token(), token);
        assert_eq!(base62([0; 32]), "0".repeat(43));
        assert_eq!(
            &base62([0xff; 32])[..3],
            "yhj",
            "2^256 - 1 needs all 43 digits"
        );
        let mut one = [0u8; 32];
        one[31] = 62;
        assert_eq!(&base62(one)[41..], "10");
        for bad in [
            "",
            "mys_",
            "mys_short",
            &format!("mys_{}", "-".repeat(43)),
            &format!("jwt_{}", "a".repeat(43)),
        ] {
            assert!(!well_formed(bad), "{bad}");
        }
        assert_eq!(token[..TOKEN_HINT_CHARS].len(), 12);
        assert_eq!(token_hash(&token).len(), 64);
    }

    /// Adding, the token, the limit and removal, on a real database
    /// (an isolated schema; skipped without a test database).
    #[tokio::test]
    async fn devices_on_a_database() {
        use sea_orm::ConnectionTrait;
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        db.execute_unprepared(
            "INSERT INTO users (id, username) VALUES (41, 'owner41'), (42, 'next42')",
        )
        .await
        .unwrap();

        let (mac, token) = add(db, 41, "  MacBook  ").await.unwrap();
        assert_eq!(mac.name, "MacBook");
        assert!(mac.id.starts_with("dev_") && mac.id.len() == 36);
        assert!(token.starts_with(&mac.token_hint));
        let stored: String = db
            .query_one_raw(statement(
                "SELECT token_hash FROM agent_sync_devices WHERE id = $1",
                vec![mac.id.clone().into()],
            ))
            .await
            .unwrap()
            .unwrap()
            .try_get("", "token_hash")
            .unwrap();
        assert_ne!(stored, token, "only the hash is kept");

        assert_eq!(authenticate(db, &token, Some(41)).await.unwrap().id, mac.id);
        assert!(matches!(
            authenticate(db, &new_token(), Some(41)).await,
            Err(Refused::Unknown)
        ));
        assert!(matches!(
            authenticate(db, "Bearer-ish", Some(41)).await,
            Err(Refused::Unknown)
        ));
        seen(db, &mac.id).await.unwrap();
        assert!(list(db, 41).await.unwrap()[0].last_seen_at.is_some());

        // A removed device is not one any more.
        let (other, _) = add(db, 41, "iMac").await.unwrap();
        revoke(db, 41, &other.id, RevokeReason::ByOwner)
            .await
            .unwrap()
            .unwrap();
        assert!(
            revoke(db, 41, &other.id, RevokeReason::ByOwner)
                .await
                .unwrap()
                .is_none()
        );
        // A removed device can be forgotten; a live one, or another owner's, not.
        assert!(!forget(db, 41, &mac.id).await.unwrap(), "live");
        assert!(!forget(db, 42, &other.id).await.unwrap(), "not 42's");
        assert!(forget(db, 41, &other.id).await.unwrap());
        assert!(!forget(db, 41, &other.id).await.unwrap(), "already gone");
        assert!(list(db, 41).await.unwrap().iter().all(|d| d.id != other.id));

        // Ten live devices at most; a removed one does not count.
        for n in 0..9 {
            add(db, 41, &format!("d{n}")).await.unwrap();
        }
        assert!(matches!(
            add(db, 41, "eleventh").await,
            Err(AddError::Limit)
        ));

        // Removed, and a device of a former owner is removed when used.
        revoke(db, 41, &mac.id, RevokeReason::DeviceLeft)
            .await
            .unwrap();
        assert!(matches!(
            authenticate(db, &token, Some(41)).await,
            Err(Refused::Revoked)
        ));
        let (old, old_token) = add(db, 42, "old owner's").await.unwrap();
        assert!(matches!(
            authenticate(db, &old_token, Some(41)).await,
            Err(Refused::OwnerChanged)
        ));
        let gone = list(db, 42).await.unwrap();
        assert_eq!(gone[0].id, old.id);
        assert_eq!(gone[0].revoked_reason.as_deref(), Some("owner_changed"));
        assert!(matches!(
            authenticate(db, &old_token, None).await,
            Err(Refused::Revoked)
        ));

        fixture.close().await;
    }

    #[test]
    fn a_device_is_stale_after_thirty_days_unseen_but_never_once_removed() {
        let now: DateTime<Utc> = "2026-10-09T00:00:00Z".parse().unwrap();
        let at = |days: i64| Some((now - Duration::days(days)).fixed_offset());
        let mut device = Device {
            id: "dev_1".into(),
            user_id: 1,
            name: "Mac".into(),
            token_hint: "mys_abcdefgh".into(),
            lineage: None,
            created_at: at(90).unwrap(),
            last_seen_at: at(29),
            revoked_at: None,
            revoked_reason: None,
        };
        assert!(!device.stale(now));
        device.last_seen_at = at(31);
        assert!(device.stale(now));
        device.last_seen_at = None;
        assert!(device.stale(now));
        device.revoked_at = at(1);
        assert!(!device.stale(now));
    }
}
