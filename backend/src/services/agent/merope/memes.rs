//! Words and memes she learns from the talk around her (the rules are
//! `myriad_merope::memes`): when a judgment of a group's talk names one she
//! is not sure of, she looks it up in the slang and meme dictionaries (or
//! searches), keeps what it means as her own, and from then on knows it
//! wherever it comes up. Looking up is billed to the site's owner, who
//! hosts her in the groups, within the day's allowance for it.

use std::collections::HashSet;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

use chrono::NaiveDate;
use sea_orm::DatabaseConnection;
use serde_json::json;

use super::call::{self, Voice};
use crate::services::agent::memory::unified;
use myriad_merope::memes::{
    DIGEST_SCHEMA, Known, SOURCE, digest_input, digest_schema, digest_system, kept_line, known_in,
    parse_digest, read_kept, search_query, section, used_in,
};

const CALL_TIMEOUT: Duration = Duration::from_secs(30);
/// What she knows, read back: the latest this many.
const READ_BACK: u64 = 500;

/// Words tried today, found or not, so one she could not find is not looked
/// up again all day.
static TRIED: LazyLock<Mutex<(Option<NaiveDate>, HashSet<String>)>> =
    LazyLock::new(|| Mutex::new((None, HashSet::new())));

fn first_try_today(term: &str, today: NaiveDate) -> bool {
    let Ok(mut tried) = TRIED.lock() else {
        return false;
    };
    if tried.0 != Some(today) {
        *tried = (Some(today), HashSet::new());
    }
    tried.1.insert(term.to_lowercase())
}

/// The words and memes she knows.
pub(super) async fn known(db: &DatabaseConnection) -> Vec<Known> {
    unified::own_rows(db, SOURCE, READ_BACK)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| read_kept(&row.content))
        .collect()
}

/// Those she learned since `since`, the latest `limit`.
pub(super) async fn learned_since(
    db: &DatabaseConnection,
    since: chrono::DateTime<chrono::FixedOffset>,
    limit: u64,
) -> Vec<Known> {
    unified::own_rows_since(db, SOURCE, since, limit)
        .await
        .unwrap_or_default()
        .iter()
        .filter_map(|row| read_kept(&row.content))
        .collect()
}

/// What she knows of the words and memes `text` uses, as a prompt section.
pub async fn section_for(db: &DatabaseConnection, text: &str) -> Option<String> {
    let known = known(db).await;
    section(&known_in(text, &known))
}

/// Those she knows that `text` uses, as lines for what she has to draw on.
pub(super) async fn lines_for(db: &DatabaseConnection, text: &str) -> Vec<String> {
    let known = known(db).await;
    known_in(text, &known)
        .into_iter()
        .map(|known| kept_line(&known.term, &known.meaning))
        .collect()
}

/// Look up the words a judgment of the talk (`conversation`) said she is
/// not sure of, in the background: each she does not know yet and has not
/// tried today, within the day's allowance, held to how it was used there.
pub(super) fn learn(owner: i32, terms: Vec<String>, conversation: &[String]) {
    let terms: Vec<(String, Vec<String>)> = terms
        .into_iter()
        .map(|term| {
            let used = used_in(&term, conversation);
            (term, used)
        })
        .collect();
    if terms.is_empty() {
        return;
    }
    super::background::spawn("words and memes", async move {
        let Ok(db) = crate::services::process_db::database() else {
            return;
        };
        let held = known(&db).await;
        let today = super::clock::local_now().date_naive();
        for (term, used) in terms {
            if held
                .iter()
                .any(|known| known.term.to_lowercase() == term.to_lowercase())
                || !first_try_today(&term, today)
            {
                continue;
            }
            if !super::store::curiosity::claim(&db, super::store::curiosity::WORDS, &term, today)
                .await
                .unwrap_or(false)
            {
                tracing::info!("[Merope] no lookups left today for words and memes");
                return;
            }
            learn_one(&db, owner, &term, &used).await;
        }
    });
}

async fn learn_one(db: &DatabaseConnection, owner: i32, term: &str, used: &[String]) {
    // The dictionaries people keep for slang and memes first; else a search
    // for it as a word people use online.
    let found = match super::senses::define(term).await {
        Ok(entry) => Some((format!("{}\n{}", entry.title, entry.text), entry.url)),
        Err(_) => super::curiosity::look_up(&search_query(term), false).await,
    };
    let Some((found, url)) = found else {
        tracing::info!("[Merope] looked up a word and found nothing");
        return;
    };
    keep(db, owner, term, used, &found, &url).await;
}

/// What was found about `term`, made into what it means and kept, if it
/// says what it means as `used` shows it used; what it means, when kept.
pub(super) async fn keep(
    db: &DatabaseConnection,
    owner: i32,
    term: &str,
    used: &[String],
    found: &str,
    url: &str,
) -> Option<String> {
    let raw = call::Ask::new(Voice::Judge, owner, "meme")
        .within(CALL_TIMEOUT)
        .json_raw(
            &digest_system(),
            &digest_input(term, used, found),
            DIGEST_SCHEMA,
            &digest_schema(),
        )
        .await
        .ok()?;
    let Some(Some(meaning)) = parse_digest(&raw) else {
        tracing::info!("[Merope] what she found about a word does not say what it means here");
        return None;
    };
    let evidence = json!({ "term": term, "url": url }).to_string();
    let concepts = vec![unified::Concept {
        name: term.to_string(),
        aliases: Vec::new(),
    }];
    match unified::remember_own(db, &kept_line(term, &meaning), &evidence, concepts, SOURCE).await {
        Ok(Some(_)) => tracing::info!("[Merope] learned a word or meme"),
        Ok(None) => {}
        Err(error) => tracing::warn!(%error, "[Merope] could not keep a word she learned"),
    }
    Some(meaning)
}
