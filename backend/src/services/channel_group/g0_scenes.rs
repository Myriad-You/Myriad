//! G0 scenes: the site's group behaviour, played on paused time against a
//! recorded world (see `g0`). Each scene's log is compared with its golden
//! in `g0/`; `MYRIAD_RECORD_G0=1` writes them. The goldens are kept out of
//! the repository: a scene whose golden is missing still runs its own
//! assertions and says how to record it. A scene uses its own chat and its
//! own users, so scenes running side by side never share a group or a seat.

use super::*;
use g0::World;
use myriad_agent_rules::onebot::decode::Muted;
use serde_json::json;

const TOKEN: &str = "g0-token";

mod avalon;
mod games;
mod jubensha;
mod jubensha_writing;
mod members;
mod memory;
mod seats;
mod strangers;
mod undercover;
mod werewolf;

fn line(chat: i64, id: i64, from: i64, name: &str, text: &str, addressed: bool) -> GroupLine {
    GroupLine::from(TelegramGroupMessage {
        update_id: id,
        message_id: id,
        chat_id: chat,
        message_thread_id: None,
        from_id: from,
        display_name: name.into(),
        text: text.into(),
        addressed,
        reply_to: None,
        images: Vec::new(),
    })
}

/// A line heard as a chat app hands it in: kept in mind, then answered
/// when it calls her and otherwise read with the rest of the talk.
async fn hear(line: GroupLine) {
    if !record(&line).await {
        return;
    }
    if line.addressed {
        tokio::spawn(handle(line, TOKEN.to_string()));
    } else {
        notice(line, TOKEN.to_string());
    }
}

async fn wait(seconds: f64) {
    tokio::time::sleep(Duration::from_secs_f64(seconds)).await;
}

fn answer(world: &mut World, operation: &str, raw: &str) {
    world
        .models
        .entry(operation.to_string())
        .or_default()
        .push_back(Some(raw.to_string()));
}

fn done(name: &str) {
    let world = g0::leave().expect("the world of the scene");
    g0::check(name, &world.log);
}

/// A scene's own database: a fresh schema in the throwaway database
/// `MYRIAD_G0_DATABASE_URL` names (never a shared one's default schema).
/// None, and the scene is skipped, when it names none.
fn database(world: &mut World, name: &str) -> Option<g0::Throwaway> {
    let Ok(url) = std::env::var("MYRIAD_G0_DATABASE_URL") else {
        eprintln!("{name}: skipped, MYRIAD_G0_DATABASE_URL names no throwaway database");
        return None;
    };
    let throwaway = g0::Throwaway::open(&url);
    world.db = Some(throwaway.db.clone());
    Some(throwaway)
}

/// Keep `content` in the group `venue` as `source`, said `days` before the
/// scene's start (and last read then).
async fn kept_in_venue(
    db: &sea_orm::DatabaseConnection,
    venue: &str,
    source: &'static str,
    content: &str,
    days: i64,
    base: chrono::DateTime<chrono::Utc>,
) {
    use sea_orm::ConnectionTrait;
    let id = crate::services::agent::memory::unified::remember_in_venue(
        db,
        &format!("group:{venue}"),
        content,
        "{}",
        source,
    )
    .await
    .unwrap()
    .unwrap();
    let at = (base - chrono::Duration::days(days)).fixed_offset();
    db.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "UPDATE agent_memories SET created_at = $1, updated_at = $1, valid_from = $1 WHERE id = $2",
        [at.into(), id.into()],
    ))
    .await
    .unwrap();
}

/// Keep `content` as hers (`source`), with `evidence`, `hours` before the
/// scene's start.
async fn kept_as_hers(
    db: &sea_orm::DatabaseConnection,
    source: &'static str,
    content: &str,
    evidence: &str,
    hours: i64,
    base: chrono::DateTime<chrono::Utc>,
) {
    use sea_orm::ConnectionTrait;
    let id = crate::services::agent::memory::unified::remember_own(
        db,
        content,
        evidence,
        Vec::new(),
        source,
    )
    .await
    .unwrap()
    .unwrap();
    let at = (base - chrono::Duration::hours(hours)).fixed_offset();
    db.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "UPDATE agent_memories SET created_at = $1, updated_at = $1, valid_from = $1 WHERE id = $2",
        [at.into(), id.into()],
    ))
    .await
    .unwrap();
}

/// Keep `content` in the group `venue` (stored form) as `source`, with
/// `evidence`, `hours` before the scene's start.
async fn kept_in_group(
    db: &sea_orm::DatabaseConnection,
    stored: &str,
    source: &'static str,
    content: &str,
    evidence: &str,
    hours: i64,
    base: chrono::DateTime<chrono::Utc>,
) {
    use sea_orm::ConnectionTrait;
    let id = crate::services::agent::memory::unified::remember_in_venue(
        db, stored, content, evidence, source,
    )
    .await
    .unwrap()
    .unwrap();
    let at = (base - chrono::Duration::hours(hours)).fixed_offset();
    db.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "UPDATE agent_memories SET created_at = $1, updated_at = $1, valid_from = $1 WHERE id = $2",
        [at.into(), id.into()],
    ))
    .await
    .unwrap();
}

/// Someone in a group says, as she would answer them from outside.
fn said(world: &mut World, seconds: f64, text: &str) {
    world
        .says
        .entry("group_stranger".into())
        .or_default()
        .push_back((seconds, Ok(text.to_string())));
}

/// The turtle soup a group's table has on, as kept, and how a puzzle of
/// hers went at the tables it was tried at (when they were left out).
async fn soup_kept(db: &sea_orm::DatabaseConnection, venue: &str) {
    let on = crate::services::runtime_registry::get::<serde_json::Value>(
        db,
        "merope_soup",
        &format!("g:{venue}"),
    )
    .await
    .unwrap()
    .map(|mut game| {
        // When it began is the clock's, not the game's.
        if let Some(game) = game.as_object_mut() {
            game.remove("started");
        }
        game
    });
    g0::note("soupOn", json!(on));
    let made: Vec<serde_json::Value> =
        crate::services::agent::memory::unified::own_rows(db, "made_soup", 10)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|row| serde_json::from_str(row.evidence.as_deref()?).ok())
            .collect();
    g0::note("made", json!(made));
}

/// What her turns with a member kept in their group session: who said what.
async fn session_kept(db: &sea_orm::DatabaseConnection, user_id: i32) {
    use sea_orm::ConnectionTrait;
    let rows = db
        .query_all_raw(sea_orm::Statement::from_sql_and_values(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT m.role, m.content FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.user_id = $1 ORDER BY m.created_at, m.id",
            [user_id.into()],
        ))
        .await
        .unwrap();
    let kept: Vec<(String, String)> = rows
        .iter()
        .map(|row| {
            (
                row.try_get::<String>("", "role").unwrap_or_default(),
                row.try_get::<String>("", "content").unwrap_or_default(),
            )
        })
        .collect();
    g0::note("session", json!(kept));
    // And what her memory work has waiting from them.
    let jobs = db
        .query_all_raw(sea_orm::Statement::from_string(
            sea_orm::DatabaseBackend::Postgres,
            "SELECT payload->'data' AS data FROM runtime_registry WHERE namespace = 'merope_memory_jobs' ORDER BY record_id",
        ))
        .await
        .unwrap();
    let jobs: Vec<serde_json::Value> = jobs
        .iter()
        .filter_map(|row| row.try_get::<serde_json::Value>("", "data").ok())
        .collect();
    g0::note("memoryJobs", json!(jobs));
}

/// Her turns with members run on the site's own run lanes, which every
/// scene in the process shares: scenes with such turns take them one at a
/// time, so one scene's turn never waits on another's (on the wall clock,
/// while the scene's clock runs on).
static WHOLE_TURNS: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// What she says in her turn with a member, after the time it takes.
fn her_turn(world: &mut World, seconds: f64, text: &str) {
    world
        .says
        .entry("chat".into())
        .or_default()
        .push_back((seconds, Ok(text.to_string())));
}

/// A member of the site, who may talk to her (the site's permission policy).
async fn site_member(db: &sea_orm::DatabaseConnection, user_id: i32, username: &str, name: &str) {
    use sea_orm::ConnectionTrait;
    db.execute_raw(sea_orm::Statement::from_sql_and_values(
        sea_orm::DatabaseBackend::Postgres,
        "INSERT INTO users (id, username, display_name) VALUES ($1, $2, $3)",
        [user_id.into(), username.into(), name.into()],
    ))
    .await
    .unwrap();
    db.execute_raw(sea_orm::Statement::from_string(
        sea_orm::DatabaseBackend::Postgres,
        "INSERT INTO configurations (key, value) VALUES ('user_perm_ai_chat', 'true'::jsonb) \
         ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
    ))
    .await
    .unwrap();
}
