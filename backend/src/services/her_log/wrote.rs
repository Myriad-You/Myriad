//! What the site writes in her log itself, told to the ends linked now.
//!
//! Rows the site takes from an end are told to its other computers where
//! they are taken (`relay`). Rows the site writes itself (her lines and
//! state while she is here, the owner's lines, her memories and nodes, a
//! forget) are written down many paths, and so are the rows a file brings
//! (`persona_sync::import`); each of them keeps the row through one place
//! (`store::insert`, with the site or the file as the side), and that place
//! signals [`CHANNEL`] in the row's transaction. Postgres delivers a signal
//! only once its transaction commits, and one signal for the same account
//! and audience however many rows the transaction kept, so each batch tells
//! once per audience and only of rows that are there to ask for.
//!
//! One LISTEN per database schema (the site has one; each test its own)
//! hears them and sends `log.vector` of that audience to every link of the
//! account that syncs her now, when the partner's computers may hold it
//! (`Owner::pc().holds`: her own and the partner's private talk, never a
//! group or someone else's private talk). An end that lacks something asks
//! for it as after any vector.

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, Statement};
use tokio::sync::watch;

use super::{Error, store};

/// The channel the site's own rows are signalled on: payload
/// `<schema> <user> <audience>`.
pub const CHANNEL: &str = "her_log_rows";

/// How long the link that starts the listener waits for it to be listening
/// before it goes on without (it hears every vector at its next hello
/// anyway).
const READY_WITHIN: Duration = Duration::from_secs(5);

/// Per schema: whether its listener listens now.
static LISTENING: LazyLock<Mutex<HashMap<String, watch::Receiver<bool>>>> =
    LazyLock::new(Default::default);

/// Signals that the site kept a row of `user`'s `audience` of its own or
/// from a file, inside the transaction that kept it (`store::insert`).
pub(super) async fn signal<C: ConnectionTrait>(
    db: &C,
    user: i32,
    audience: &str,
) -> Result<(), Error> {
    db.execute_raw(Statement::from_sql_and_values(
        DatabaseBackend::Postgres,
        "SELECT pg_notify($1, current_schema() || ' ' || $2::text || ' ' || $3)",
        [CHANNEL.into(), user.into(), audience.into()],
    ))
    .await?;
    Ok(())
}

/// Starts the listener of `db`'s schema once (a link starts syncing her:
/// before that nobody is there to tell); the link that started it waits
/// until it listens.
pub async fn listen(db: &DatabaseConnection) {
    if !matches!(db.get_database_backend(), DatabaseBackend::Postgres) {
        return;
    }
    let schema = match schema_of(db).await {
        Ok(schema) => schema,
        Err(error) => {
            tracing::warn!(%error, "[her-log] which schema the site writes in is not known");
            return;
        }
    };
    // Only the link that starts it waits: once started, one that is not
    // listening (its connection lost, being tried again) holds no link up,
    // and a link that misses a signal meanwhile matches vectors at its next
    // hello.
    let mut ready = {
        let mut listening = LISTENING.lock().unwrap_or_else(|e| e.into_inner());
        if listening.contains_key(&schema) {
            return;
        }
        let (tx, rx) = watch::channel(false);
        listening.insert(schema.clone(), rx.clone());
        start(db.clone(), schema, tx);
        rx
    };
    if tokio::time::timeout(READY_WITHIN, ready.wait_for(|on| *on))
        .await
        .is_err()
    {
        tracing::warn!("[her-log] the site's own rows are not listened for yet");
    }
}

async fn schema_of(db: &DatabaseConnection) -> Result<String, Error> {
    let row = db
        .query_one_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT current_schema() AS schema",
        ))
        .await?
        .ok_or_else(|| Error::Refused("no schema".into()))?;
    Ok(row.try_get("", "schema")?)
}

fn start(db: DatabaseConnection, schema: String, ready: watch::Sender<bool>) {
    let ready = std::sync::Arc::new(ready);
    crate::services::jobs::jobs().supervised(
        "her log rows listener",
        crate::services::jobs::Backoff::new(Duration::from_secs(1), Duration::from_secs(60)),
        move || hear(db.clone(), schema.clone(), ready.clone()),
    );
}

/// One LISTEN session: returns when the connection is lost.
async fn hear(db: DatabaseConnection, schema: String, ready: std::sync::Arc<watch::Sender<bool>>) {
    let pool = db.get_postgres_connection_pool().clone();
    let mut listener = match sea_orm::sqlx::postgres::PgListener::connect_with(&pool).await {
        Ok(listener) => listener,
        Err(error) => {
            tracing::warn!(%error, "[her-log] the site's own rows: LISTEN connect failed");
            return;
        }
    };
    if let Err(error) = listener.listen(CHANNEL).await {
        tracing::warn!(%error, "[her-log] the site's own rows: LISTEN failed");
        return;
    }
    ready.send_replace(true);
    loop {
        match listener.recv().await {
            Ok(notification) => {
                let Some((user, audience)) = parse(notification.payload(), &schema) else {
                    continue;
                };
                if let Err(error) = tell(&db, user, audience).await {
                    tracing::warn!(%error, audience, "[her-log] the links were not told of the site's own rows");
                }
            }
            Err(error) => {
                ready.send_replace(false);
                tracing::warn!(%error, "[her-log] the site's own rows: LISTEN lost");
                return;
            }
        }
    }
}

/// `(user, audience)` of a signal from `schema`; another schema's is not
/// this listener's.
fn parse<'a>(payload: &'a str, schema: &str) -> Option<(i32, &'a str)> {
    let mut parts = payload.splitn(3, ' ');
    if parts.next()? != schema {
        return None;
    }
    let user = parts.next()?.parse().ok()?;
    Some((user, parts.next()?))
}

/// Tells every link of `user` that syncs her now what the site holds of
/// `audience`, when the partner's computers may hold it.
async fn tell(db: &DatabaseConnection, user: i32, audience: &str) -> Result<(), Error> {
    let owner = super::owner(db, user).await?;
    if !owner.pc().holds(audience) {
        return Ok(());
    }
    let ours = store::holdings(db, user, audience).await?;
    crate::services::agent::link::tell_log_links(
        user,
        "log.vector",
        super::relay::vector_body(audience, &ours, &[]),
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signal_is_read_only_in_its_own_schema() {
        assert_eq!(parse("public 41 own:", "public"), Some((41, "own:")));
        assert_eq!(
            parse("public 41 personal:partner_x", "public"),
            Some((41, "personal:partner_x"))
        );
        assert_eq!(parse("test_a 41 own:", "public"), None);
        assert_eq!(parse("public x own:", "public"), None);
        assert_eq!(parse("public 41", "public"), None);
    }
}
