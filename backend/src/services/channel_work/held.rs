//! What comes by chat app while she is not on the site (moving between her
//! ends, or living on another one) is held here, in the order it came, and
//! read when she is back (D3). Nobody is answered meanwhile, not even with
//! a refusal: she is simply not there to read it.
//!
//! When she is back, every chat where she may write unasked is read at
//! once, one chat after another in the order their first held message came
//! (D3-5). Where a bot may only answer what is fresh (the official QQ bot,
//! Feishu), what is held waits for that person's next message and is read
//! with it (D3-7). Nothing held expires: a row goes when she reads it, when
//! its pairing is gone, or when the owner clears what is held (D3-4).
//!
//! Held while she lives on another end too (D3-6): from H4 the site hands
//! it to that end instead (`lease::Where::Elsewhere`).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex as StdMutex};

use chrono::{DateTime, Utc};
use myriad_agent_rules::channel::ChannelImageRef;
use sea_orm::{ConnectionTrait, DatabaseBackend, DatabaseConnection, DbErr, Statement};
use serde_json::{Value, json};

use super::{continue_text, runtime::with_chat_lock};
use crate::services::channel_platform::ChannelPlatform;
use crate::services::persona_sync::lease;

tokio::task_local! {
    /// The platform message being handled, as the channel claimed it: what a
    /// held message is known by, so one message is held once.
    pub(crate) static INBOUND: String;
}

/// What she is told when she reads what was held, by chat, until her turn
/// there takes it (see `chat::answer`).
static CAME_WHILE_AWAY: LazyLock<StdMutex<HashMap<String, String>>> =
    LazyLock::new(|| StdMutex::new(HashMap::new()));

/// She is not on the site now: what comes is held, not answered.
pub(crate) fn away() -> bool {
    !lease::place().here()
}

/// A refusal because she is not on the site (a race with a move starting):
/// held like anything else that came, never sent back.
pub(super) fn is_away_refusal(body: &Value) -> bool {
    matches!(
        body.get("code").and_then(Value::as_str),
        Some("host_frozen" | "host_moved")
    )
}

fn statement(sql: &str, values: Vec<sea_orm::Value>) -> Statement {
    Statement::from_sql_and_values(DatabaseBackend::Postgres, sql, values)
}

fn images_json(images: &[ChannelImageRef]) -> Value {
    Value::Array(
        images
            .iter()
            .map(|image| {
                json!({ "url": image.url, "name": image.name, "mime": image.mime, "size": image.size })
            })
            .collect(),
    )
}

fn images_of(value: &Value) -> Vec<ChannelImageRef> {
    value
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|image| {
            Some(ChannelImageRef {
                url: image.get("url")?.as_str()?.to_string(),
                name: image.get("name")?.as_str()?.to_string(),
                mime: image.get("mime")?.as_str()?.to_string(),
                size: image.get("size")?.as_u64()?,
            })
        })
        .collect()
}

/// Hold one message of a chat until she is back. Its id is the claimed
/// platform message when the caller has one; a message held twice is kept
/// once.
pub(super) async fn hold(
    db: &DatabaseConnection,
    sink: &super::ChannelSink,
    session_key: &str,
    user_id: i32,
    text: &str,
    images: &[ChannelImageRef],
) {
    let platform = sink.platform();
    // What a piece of hers had to say is held for the chat it came in.
    let session_key = super::works::chat_of(session_key);
    // Answered from the session once she is back: where, and paired with whom.
    super::remember_sink(db, user_id, session_key, sink).await;
    let inbound = INBOUND
        .try_with(|id| id.clone())
        .ok()
        .filter(|id| !id.is_empty())
        .unwrap_or_else(|| format!("held:{}", uuid::Uuid::new_v4()));
    let result = db
        .execute_raw(statement(
            "INSERT INTO agent_channel_held \
             (platform, session_key, inbound_id, user_id, text, images) \
             VALUES ($1, $2, $3, $4, $5, $6) ON CONFLICT (platform, inbound_id) DO NOTHING",
            vec![
                platform.slug().into(),
                session_key.into(),
                inbound.into(),
                user_id.into(),
                text.into(),
                images_json(images).into(),
            ],
        ))
        .await;
    match result {
        Ok(_) => tracing::info!(
            %platform,
            "[Channel] she is not on the site; what came is held until she is back"
        ),
        Err(error) => tracing::warn!(%error, %platform, "[Channel] could not hold what came"),
    }
}

/// What was held in one chat, taken to be read now.
pub(super) struct Taken {
    pub(super) text: String,
    pub(super) images: Vec<ChannelImageRef>,
}

/// Takes what is held in `session_key`, oldest first, and leaves for her
/// turn there the fact that it came while she was away and when.
pub(super) async fn take(db: &DatabaseConnection, session_key: &str) -> Option<Taken> {
    let rows = db
        .query_all_raw(statement(
            "DELETE FROM agent_channel_held WHERE session_key = $1 \
             RETURNING id, text, images, arrived_at",
            vec![session_key.into()],
        ))
        .await
        .map_err(|error| tracing::warn!(%error, "[Channel] could not read what was held"))
        .ok()?;
    let mut held: Vec<(i64, String, Vec<ChannelImageRef>, DateTime<Utc>)> = rows
        .iter()
        .filter_map(|row| {
            Some((
                row.try_get("", "id").ok()?,
                row.try_get("", "text").ok()?,
                images_of(&row.try_get::<Value>("", "images").ok()?),
                row.try_get::<DateTime<chrono::FixedOffset>>("", "arrived_at")
                    .ok()?
                    .with_timezone(&Utc),
            ))
        })
        .collect();
    if held.is_empty() {
        return None;
    }
    held.sort_by_key(|(id, ..)| *id);
    let first = held.first().map(|row| row.3)?;
    let last = held.last().map(|row| row.3)?;
    if let Ok(mut notes) = CAME_WHILE_AWAY.lock() {
        notes.insert(
            session_key.to_string(),
            came_while_away(first, last, held.len()),
        );
    }
    let text = held
        .iter()
        .map(|(_, text, ..)| text.trim())
        .filter(|text| !text.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let images = held
        .into_iter()
        .flat_map(|(_, _, images, _)| images)
        .collect();
    Some(Taken { text, images })
}

/// The fact she reads with what was held: that it came while she was not
/// on the site, and when, in her time.
fn came_while_away(first: DateTime<Utc>, last: DateTime<Utc>, count: usize) -> String {
    let zone = crate::services::agent::merope::clock::zone();
    let at = |when: DateTime<Utc>| when.with_timezone(&zone).format("%m-%d %H:%M").to_string();
    let when = if count == 1 || at(first) == at(last) {
        format!("at {}", at(first))
    } else {
        format!("from {} to {}", at(first), at(last))
    };
    format!(
        "What they wrote here came while you were not on this site ({count} message(s), \
         {when}, your time): you could not read it then, and are reading it now."
    )
}

/// The fact for this chat's turn, if what she reads was held.
pub(super) fn note_for(session_key: &str) -> Option<String> {
    CAME_WHILE_AWAY
        .lock()
        .ok()
        .and_then(|mut notes| notes.remove(session_key))
}

/// How many messages are held now (for the owner's devices page).
pub async fn count<C: ConnectionTrait>(db: &C) -> Result<i64, DbErr> {
    Ok(db
        .query_one_raw(statement(
            "SELECT COUNT(*)::BIGINT AS held FROM agent_channel_held",
            Vec::new(),
        ))
        .await?
        .and_then(|row| row.try_get::<i64>("", "held").ok())
        .unwrap_or(0))
}

/// The owner lets go of everything held: it will not be read.
pub async fn clear<C: ConnectionTrait>(db: &C) -> Result<u64, DbErr> {
    Ok(db
        .execute_raw(statement("DELETE FROM agent_channel_held", Vec::new()))
        .await?
        .rows_affected())
}

/// She is back: every chat where she may write unasked is read, one after
/// another in the order its first held message came. A chat whose pairing
/// is gone has nobody to answer: what it held goes.
pub async fn replay(db: DatabaseConnection) {
    if away() {
        return;
    }
    let chats = match db
        .query_all_raw(statement(
            "SELECT session_key, platform, user_id, MIN(id) AS first FROM agent_channel_held \
             GROUP BY session_key, platform, user_id ORDER BY first",
            Vec::new(),
        ))
        .await
    {
        Ok(rows) => rows,
        Err(error) => {
            tracing::warn!(%error, "[Channel] could not read what was held");
            return;
        }
    };
    for row in &chats {
        let (Ok(session_key), Ok(slug), Ok(user_id)) = (
            row.try_get::<String>("", "session_key"),
            row.try_get::<String>("", "platform"),
            row.try_get::<i32>("", "user_id"),
        ) else {
            continue;
        };
        let Some(platform) = ChannelPlatform::ALL
            .into_iter()
            .find(|platform| platform.slug() == slug)
        else {
            continue;
        };
        // Answered only while fresh: read with their next message.
        if !platform.may_send_unasked() {
            continue;
        }
        if away() {
            return;
        }
        match super::chat_waiting::sink_for(&db, user_id, &session_key).await {
            Some(sink) => {
                let Some(taken) = take(&db, &session_key).await else {
                    continue;
                };
                tracing::info!(%platform, "[Channel] she is back: reading what was held");
                let (db, key) = (db.clone(), session_key.clone());
                with_chat_lock(&session_key, async move {
                    continue_text(&db, user_id, "", &taken.text, &taken.images, &key, sink).await;
                })
                .await;
            }
            None if !super::chat_waiting::paired(&db, platform, user_id, &session_key).await => {
                let _ = db
                    .execute_raw(statement(
                        "DELETE FROM agent_channel_held WHERE session_key = $1",
                        vec![session_key.clone().into()],
                    ))
                    .await;
                tracing::info!(%platform, "[Channel] what was held for a chat no longer paired is let go");
            }
            // Paired, but the bot is not there now: kept for next time.
            None => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_she_is_told_says_when_and_how_many() {
        let first = DateTime::parse_from_rfc3339("2026-10-13T05:02:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let last = first + chrono::Duration::minutes(88);
        let one = came_while_away(first, first, 1);
        assert!(one.contains("1 message(s), at "), "{one}");
        let many = came_while_away(first, last, 3);
        assert!(many.contains("3 message(s), from "), "{many}");
        assert!(many.contains("not on this site"), "{many}");
    }

    #[test]
    fn only_a_refusal_for_her_absence_is_held() {
        assert!(is_away_refusal(&json!({"code": "host_frozen"})));
        assert!(is_away_refusal(&json!({"code": "host_moved"})));
        assert!(!is_away_refusal(&json!({"code": "rate_limited"})));
        assert!(!is_away_refusal(&json!({"error": "x"})));
    }

    /// Held in the order it came, one message once however often it comes,
    /// read together and then gone; cleared, nothing is left to read.
    #[tokio::test]
    async fn what_is_held_is_read_once_in_the_order_it_came() {
        use sea_orm::ConnectionTrait;
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        db.execute_unprepared("INSERT INTO users (id, username) VALUES (41, 'owner41')")
            .await
            .unwrap();
        let sink = onebot_sink(db);
        for (id, text) in [
            ("tg:1:7", "下雨了"),
            ("tg:1:8", "你在吗"),
            ("tg:1:7", "下雨了"),
        ] {
            INBOUND
                .scope(id.to_string(), hold(db, &sink, "tg:1", 41, text, &[]))
                .await;
        }
        hold(db, &sink, "tg:2", 41, "另一个对话", &[]).await;
        assert_eq!(count(db).await.unwrap(), 3);
        let taken = take(db, "tg:1").await.unwrap();
        assert_eq!(taken.text, "下雨了\n你在吗");
        let note = note_for("tg:1").unwrap();
        assert!(note.contains("2 message(s)"), "{note}");
        assert!(note_for("tg:1").is_none());
        assert!(take(db, "tg:1").await.is_none());
        assert_eq!(count(db).await.unwrap(), 1);
        assert_eq!(clear(db).await.unwrap(), 1);
        assert_eq!(count(db).await.unwrap(), 0);
        fixture.close().await;
    }

    fn onebot_sink(db: &DatabaseConnection) -> super::super::ChannelSink {
        super::super::ChannelSink {
            transport: super::super::transport::ChannelTransport::OneBot {
                user_id: "20002".into(),
                temp_group: None,
            },
            binding: crate::services::channel_pairing::ChannelBinding {
                user_id: 41,
                provider: "onebot".into(),
                identity_id: 1,
                scope: String::new(),
            },
            db: db.clone(),
        }
    }

    /// What is held keeps where to answer its chat, so she can answer it once
    /// she is back even where no Work run ever kept it (a chat that only
    /// talked to her).
    #[tokio::test]
    async fn a_held_chat_keeps_where_to_answer_it() {
        use sea_orm::ConnectionTrait;
        let Some(fixture) = crate::federation::test_db::SchemaDb::new_or_media().await else {
            return;
        };
        let db = &fixture.db;
        db.execute_unprepared("INSERT INTO users (id, username) VALUES (41, 'owner41')")
            .await
            .unwrap();
        let platform = ChannelPlatform::OneBot;
        let stored = super::super::StoredSession {
            chat_session_id: Some("ses_x".into()),
            ..Default::default()
        };
        super::super::put_session(db, platform, 41, "ob:1", stored)
            .await
            .unwrap();
        let sink = onebot_sink(db);
        hold(db, &sink, "ob:1", 41, "下雨了", &[]).await;
        let kept = super::super::load_session(db, platform, "ob:1")
            .await
            .unwrap();
        assert_eq!(kept.binding.map(|binding| binding.user_id), Some(41));
        assert!(kept.address.is_some());
        assert_eq!(kept.chat_session_id.as_deref(), Some("ses_x"));
        assert_eq!(count(db).await.unwrap(), 1);
        fixture.close().await;
    }

    #[test]
    fn images_come_back_as_they_were_held() {
        let images = vec![ChannelImageRef {
            url: "https://example.invalid/a.png".into(),
            name: "a.png".into(),
            mime: "image/png".into(),
            size: 3,
        }];
        assert_eq!(images_of(&images_json(&images)), images);
    }
}
