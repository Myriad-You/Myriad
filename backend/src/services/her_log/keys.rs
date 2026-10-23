//! The server's key ring for her log, and payloads sealed at rest.
//!
//! The server can read her log (problem 5 of the sync plan: she has to
//! live here while the PCs sleep), so it holds the keys; sealing does not
//! keep the server from reading, it keeps every copy of the database (a
//! dump, a backup, a replica) from reading without the installation's data
//! key, and it makes forgetting reach those copies at once:
//!
//! - one key per audience, sealed by the installation's data key
//!   (`myriad_data_key`, kept outside the database and its backups);
//! - one key per object, sealed by its audience's key, with the subject it
//!   is about, so forgetting everything about someone finds their keys;
//! - each payload sealed by its object's key (AES-256-GCM, a fresh nonce
//!   each time, the row's id as associated data, so a payload cannot be
//!   moved to another row).
//!
//! Forgetting an object deletes its key: a payload left in any copy of the
//! database no longer opens. Keys never leave this module: not in a row, a
//! frame, an answer, an error or the log.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};

use super::Error;

const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 12;

/// What seals the audience keys: the installation's data key, or in tests
/// a stand-in.
pub trait Root: Send + Sync {
    fn seal(&self, plain: &str) -> anyhow::Result<String>;
    fn open(&self, sealed: &str) -> anyhow::Result<String>;
}

impl Root for myriad_data_key::DataKey {
    fn seal(&self, plain: &str) -> anyhow::Result<String> {
        self.encrypt(plain)
    }

    fn open(&self, sealed: &str) -> anyhow::Result<String> {
        self.decrypt(sealed)
    }
}

/// The installation's data key.
pub fn root() -> &'static dyn Root {
    myriad_data_key::data_key()
}

fn cipher(key: &[u8; KEY_BYTES]) -> Result<aes_gcm::Aes256Gcm, Error> {
    aes_gcm::Aes256Gcm::new_from_slice(key).map_err(|_| Error::Key)
}

/// `data` sealed with `key`: nonce, then ciphertext and tag.
fn seal_with(key: &[u8; KEY_BYTES], data: &[u8], aad: &[u8]) -> Result<Vec<u8>, Error> {
    let nonce: [u8; NONCE_BYTES] = rand::random();
    let sealed = cipher(key)?
        .encrypt(&aes_gcm::Nonce::from(nonce), Payload { msg: data, aad })
        .map_err(|_| Error::Key)?;
    let mut out = nonce.to_vec();
    out.extend(sealed);
    Ok(out)
}

fn open_with(key: &[u8; KEY_BYTES], sealed: &[u8], aad: &[u8]) -> Result<Vec<u8>, Error> {
    if sealed.len() < NONCE_BYTES {
        return Err(Error::Key);
    }
    let (nonce, body) = sealed.split_at(NONCE_BYTES);
    let nonce: [u8; NONCE_BYTES] = nonce.try_into().map_err(|_| Error::Key)?;
    cipher(key)?
        .decrypt(&aes_gcm::Nonce::from(nonce), Payload { msg: body, aad })
        .map_err(|_| Error::Key)
}

fn key_of(bytes: &[u8]) -> Result<[u8; KEY_BYTES], Error> {
    bytes.try_into().map_err(|_| Error::Key)
}

/// What an object key is sealed with besides its audience's key: which
/// object it is, so a key cannot be moved to another.
fn object_aad(audience: &str, object: &str) -> Vec<u8> {
    format!("myriad-her-log/object-key\n{audience}\n{object}").into_bytes()
}

fn payload_aad(row_id: &str) -> Vec<u8> {
    format!("myriad-her-log/payload\n{row_id}").into_bytes()
}

/// The audience's key, made the first time it is needed.
async fn audience_key<C: ConnectionTrait>(
    db: &C,
    root: &dyn Root,
    audience: &str,
) -> Result<[u8; KEY_BYTES], Error> {
    let read = || {
        Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT sealed FROM agent_log_audience_keys WHERE audience = $1",
            [audience.into()],
        )
    };
    if db.query_one_raw(read()).await?.is_none() {
        let fresh: [u8; KEY_BYTES] = rand::random();
        let sealed = root.seal(&BASE64.encode(fresh)).map_err(|_| Error::Key)?;
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_log_audience_keys (audience, sealed, created_at) \
             VALUES ($1, $2, NOW()) ON CONFLICT (audience) DO NOTHING",
            [audience.into(), sealed.into()],
        ))
        .await?;
    }
    // Read back: if another writer made it first, its key is the one.
    let row = db
        .query_one_raw(read())
        .await?
        .ok_or_else(|| Error::Db(sea_orm::DbErr::RecordNotFound("audience key".into())))?;
    let sealed: String = row.try_get("", "sealed")?;
    let plain = root.open(&sealed).map_err(|_| Error::Key)?;
    key_of(&BASE64.decode(plain).map_err(|_| Error::Key)?)
}

/// The object's key, made the first time it is needed. None when it was
/// forgotten: its key is gone.
async fn object_key<C: ConnectionTrait>(
    db: &C,
    root: &dyn Root,
    audience: &str,
    object: &str,
    subject: Option<&str>,
    make: bool,
) -> Result<Option<[u8; KEY_BYTES]>, Error> {
    let read = || {
        Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "SELECT sealed FROM agent_log_object_keys WHERE audience = $1 AND object = $2",
            [audience.into(), object.into()],
        )
    };
    let mut row = db.query_one_raw(read()).await?;
    if row.is_none() && make {
        let wrap = audience_key(db, root, audience).await?;
        let fresh: [u8; KEY_BYTES] = rand::random();
        let sealed = seal_with(&wrap, &fresh, &object_aad(audience, object))?;
        db.execute_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "INSERT INTO agent_log_object_keys (audience, object, subject, sealed, created_at) \
             VALUES ($1, $2, $3, $4, NOW()) ON CONFLICT (audience, object) DO NOTHING",
            [
                audience.into(),
                object.into(),
                subject.map(str::to_string).into(),
                sealed.into(),
            ],
        ))
        .await?;
        row = db.query_one_raw(read()).await?;
    }
    let Some(row) = row else {
        return Ok(None);
    };
    let sealed: Vec<u8> = row.try_get("", "sealed")?;
    let wrap = audience_key(db, root, audience).await?;
    let key = open_with(&wrap, &sealed, &object_aad(audience, object))?;
    key_of(&key).map(Some)
}

/// `data`, the payload of row `row_id` about `object`, sealed for keeping.
pub async fn seal_payload<C: ConnectionTrait>(
    db: &C,
    root: &dyn Root,
    audience: &str,
    object: &str,
    subject: Option<&str>,
    row_id: &str,
    data: &[u8],
) -> Result<Vec<u8>, Error> {
    let key = object_key(db, root, audience, object, subject, true)
        .await?
        .ok_or(Error::Key)?;
    seal_with(&key, data, &payload_aad(row_id))
}

/// The payload of row `row_id` about `object`, opened. None when its
/// object's key is gone (forgotten).
pub async fn open_payload<C: ConnectionTrait>(
    db: &C,
    root: &dyn Root,
    audience: &str,
    object: &str,
    row_id: &str,
    sealed: &[u8],
) -> Result<Option<Vec<u8>>, Error> {
    let Some(key) = object_key(db, root, audience, object, None, false).await? else {
        return Ok(None);
    };
    open_with(&key, sealed, &payload_aad(row_id)).map(Some)
}

/// Forgets the keys a ledger entry reaches: the object's, or for
/// `subject:<id>` every key of an object about that subject in the
/// audience. Returns how many went.
pub async fn forget<C: ConnectionTrait>(
    db: &C,
    audience: &str,
    target: &str,
) -> Result<u64, Error> {
    let subject = target
        .strip_prefix(myriad_persona_log::kinds::SUBJECT)
        .and_then(|rest| rest.strip_prefix(':'));
    let result = match subject {
        Some(subject) => {
            db.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM agent_log_object_keys WHERE audience = $1 AND subject = $2",
                [audience.into(), subject.into()],
            ))
            .await?
        }
        None => {
            db.execute_raw(Statement::from_sql_and_values(
                DatabaseBackend::Postgres,
                "DELETE FROM agent_log_object_keys WHERE audience = $1 AND object = $2",
                [audience.into(), target.into()],
            ))
            .await?
        }
    };
    Ok(result.rows_affected())
}

#[cfg(test)]
pub(crate) mod test_root {
    use super::*;

    /// A stand-in for the installation's data key: a fixed key, the same
    /// sealing.
    pub(crate) struct TestRoot(pub [u8; KEY_BYTES]);

    impl Root for TestRoot {
        fn seal(&self, plain: &str) -> anyhow::Result<String> {
            seal_with(&self.0, plain.as_bytes(), b"test-root")
                .map(|sealed| BASE64.encode(sealed))
                .map_err(|error| anyhow::anyhow!("{error}"))
        }

        fn open(&self, sealed: &str) -> anyhow::Result<String> {
            let bytes = BASE64.decode(sealed)?;
            let plain = open_with(&self.0, &bytes, b"test-root")
                .map_err(|error| anyhow::anyhow!("{error}"))?;
            Ok(String::from_utf8(plain)?)
        }
    }

    #[test]
    fn a_payload_opens_only_for_its_own_row() {
        let key = [7u8; KEY_BYTES];
        let sealed = seal_with(&key, b"what she thought", &payload_aad("row_1")).unwrap();
        assert!(!sealed.windows(4).any(|w| w == b"what"), "not in the clear");
        assert_eq!(
            open_with(&key, &sealed, &payload_aad("row_1")).unwrap(),
            b"what she thought"
        );
        assert!(open_with(&key, &sealed, &payload_aad("row_2")).is_err());
        assert!(open_with(&[8u8; KEY_BYTES], &sealed, &payload_aad("row_1")).is_err());
    }
}
