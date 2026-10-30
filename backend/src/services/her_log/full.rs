//! A new host core's first full sync: one audience's log, downloaded once
//! over HTTPS (`GET /api/agent/sync/log/full/{link}`, behind the device
//! gate) from a link the server gave it in `log.full`. Everything after
//! goes over her link.
//!
//! A link is single use, for the device it was given to, and expires after
//! [`VALID_FOR`]; only the sha256 of its secret part is kept. The download
//! is what `nodes.wire_send` gives the partner's computer that holds
//! nothing: every forget first, a body only when it may travel (`withheld`
//! says why the others do not), and nothing of an audience a computer may
//! not hold.

use myriad_persona_log::Vector;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement, TransactionTrait};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{Error, keys, store};

/// How long a link may be used: ten minutes.
pub const VALID_FOR_MICROS: i64 = 10 * 60 * 1_000_000;

/// Where the download is, relative to the server the device connected to.
pub const PATH: &str = "/api/agent/sync/log/full/";

fn hash(secret: &str) -> String {
    hex::encode(Sha256::digest(secret.as_bytes()))
}

/// A link given out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Link {
    pub path: String,
    pub expires_at_ms: i64,
}

/// A link for `device` to download `audience`, from the server's time `now`.
pub async fn issue<C: ConnectionTrait>(
    db: &C,
    device: &str,
    audience: &str,
    now: i64,
) -> Result<Link, Error> {
    let secret = format!("full_{}", hex::encode(rand::random::<[u8; 24]>()));
    let expires = now + VALID_FOR_MICROS;
    let expires_at = chrono::DateTime::from_timestamp_micros(expires)
        .ok_or_else(|| Error::Refused("time".into()))?;
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "INSERT INTO agent_log_full_links (token_hash, device_id, audience, expires_at) \
         VALUES ($1, $2, $3, $4)",
        [
            hash(&secret).into(),
            device.into(),
            audience.into(),
            expires_at.into(),
        ],
    ))
    .await?;
    Ok(Link {
        path: format!("{PATH}{secret}"),
        expires_at_ms: expires / 1000,
    })
}

/// The download behind `secret` for `device`, one of `owner`'s computers:
/// `{audience, vector, rows, payloads, withheld}`, the link used up. None
/// when there is no such link for this device, or it was used or has
/// expired.
pub async fn redeem(
    db: &DatabaseConnection,
    root: &dyn keys::Root,
    owner: i32,
    device: &str,
    secret: &str,
    now: i64,
) -> Result<Option<Value>, Error> {
    let txn = db.begin().await?;
    let now_at = chrono::DateTime::from_timestamp_micros(now)
        .ok_or_else(|| Error::Refused("time".into()))?;
    let Some(link) = txn
        .query_one_raw(Statement::from_sql_and_values(
            DatabaseBackend::Postgres,
            "UPDATE agent_log_full_links SET used_at = $3 \
             WHERE token_hash = $1 AND device_id = $2 AND used_at IS NULL AND expires_at > $3 \
             RETURNING audience",
            [hash(secret).into(), device.into(), now_at.into()],
        ))
        .await?
    else {
        txn.commit().await?;
        return Ok(None);
    };
    let audience: String = link.try_get("", "audience")?;
    let holdings = store::holdings(&txn, &audience).await?;
    let sent = store::send(
        &txn,
        root,
        &audience,
        &Vector::new(),
        Vec::new(),
        &super::pc(owner),
    )
    .await?;
    txn.commit().await?;
    let (mut rows, mut payloads, mut withheld) = (Vec::new(), Vec::new(), Vec::new());
    for batch in sent.batches {
        rows.extend(batch.rows.iter().map(|row| row.to_value()));
        payloads.extend(batch.payloads);
        withheld.extend(batch.withheld);
    }
    Ok(Some(json!({
        "audience": audience,
        "vector": holdings.vector,
        "rows": rows,
        "payloads": payloads,
        "withheld": withheld,
    })))
}
