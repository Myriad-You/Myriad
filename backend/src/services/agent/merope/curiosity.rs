//! Finding things out on her own.
//!
//! After a chat turn, she may notice something in what they said that she
//! does not actually know — a name, a thing, an event — and want to find out.
//! Whether she does is the model's judgment, given the exchange and the facts
//! of her day. If so she searches, and writes down what she found and what
//! she makes of it in her own words: notes, never a copy (an AI that reads
//! search results out verbatim stops thinking for itself). The note becomes
//! her knowledge, heard only where that conversation's audience is present,
//! and if she wants to tell them, it goes through the same live-only decision
//! as a passing thought.
//!
//! Private matters are not searched: their life and their people stay theirs.
//! Only someone whose granted permissions include `ai:search` can set her
//! looking, and the budget is capped per person and per site each day.
//! Search results are untrusted data from the web.

use std::time::Duration;

use serde_json::json;

use super::call::{self, Voice};
use crate::services::agent::memory::unified;
use myriad_merope::curiosity::{
    DIGEST_SCHEMA, FoundOut, MAX_QUERY_CHARS, WONDER_SCHEMA, Wonder, clip, digest_schema,
    digest_system, wonder_schema, wonder_system,
};
#[cfg(test)]
use serde_json::Value;

pub const FOUND_OUT_EVENT: &str = "agent.merope.found_out";
const MIN_USER_CHARS: usize = 6;
const CALL_TIMEOUT: Duration = Duration::from_secs(30);

pub fn spawn_curiosity(
    user_id: i32,
    user_text: String,
    reply: String,
    present: unified::Audience,
    turn: super::TurnContext,
) {
    if user_id <= 0 || user_text.trim().chars().count() < MIN_USER_CHARS {
        return;
    }
    crate::services::agent::merope::background::spawn("curiosity", async move {
        if tokio::time::timeout(
            Duration::from_secs(120),
            wonder_and_find_out(user_id, &user_text, &reply, &present, &turn),
        )
        .await
        .is_err()
        {
            tracing::info!(user_id, "[Merope] curiosity ran out of time");
        }
    });
}

/// The search payload, flattened for the digest. Only text and addresses.
/// What her senses turn up for it, and where: the slang dictionaries for a
/// term, else a search and its first result read with the question in mind
/// (the results alone when the page will not load).
async fn look_up(query: &str, slang: bool) -> Option<(String, String)> {
    if slang && let Ok(entry) = super::senses::define(query).await {
        let text = format!("{}\n{}", entry.title, entry.text);
        return Some((clip(&text), entry.url));
    }
    let hits = super::senses::search(query).await.ok()?;
    let first = hits.first()?.url.clone();
    let results = hits_text(&hits);
    match super::senses::read(&first, Some(query)).await {
        Ok(page) => Some((
            clip(&format!("{results}\n\n{}\n{}", page.title, page.text)),
            page.url,
        )),
        Err(_) => Some((clip(&results), first)),
    }
}

fn hits_text(hits: &[super::senses::Hit]) -> String {
    hits.iter()
        .take(5)
        .map(|hit| format!("- {} ({}): {}", hit.title, hit.url, hit.snippet))
        .collect::<Vec<_>>()
        .join("\n")
}

async fn wonder_and_find_out(
    user_id: i32,
    user_text: &str,
    reply: &str,
    present: &unified::Audience,
    turn: &super::TurnContext,
) {
    // A move in a game is not something to go and look up.
    if turn.in_game {
        return;
    }
    if !super::is_logged_in_addressee(user_id) || !super::is_enabled().await {
        return;
    }
    let Ok(db) = crate::services::process_db::database() else {
        return;
    };
    match super::store::curiosity::available(&db, user_id, chrono::Local::now().date_naive()).await
    {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            tracing::warn!(user_id, %error, "[Merope] could not read curiosity allowance");
            return;
        }
    }
    // Only someone who may search can set her searching on the site's key.
    if !crate::services::agent::get_user_permissions(&db, user_id)
        .await
        .contains("ai:search")
    {
        return;
    }
    let soul = crate::services::agent::identity::get_speaking_soul()
        .await
        .unwrap_or_default();
    let myself = super::self_state::current(&db).await.facts_view();
    let Ok(wonder) = call::Ask::new(Voice::Judge, user_id, "wonder")
        .within(CALL_TIMEOUT)
        .json::<Wonder>(
            &wonder_system(&soul),
            &json!({
                "userText": user_text.chars().take(1000).collect::<String>(),
                "reply": super::ingest::compact_summary(reply),
                "scene": turn.scene,
                "myself": myself,
            })
            .to_string(),
            WONDER_SCHEMA,
            &wonder_schema(),
        )
        .await
    else {
        return;
    };
    let Some(query) = wonder
        .query
        .map(|query| {
            query
                .trim()
                .chars()
                .take(MAX_QUERY_CHARS)
                .collect::<String>()
        })
        .filter(|query| !query.is_empty())
    else {
        return;
    };
    // Re-read grants after the model wait and claim against the current day.
    if !super::is_enabled().await
        || !crate::services::agent::get_user_permissions(&db, user_id)
            .await
            .contains("ai:search")
    {
        return;
    }
    match super::store::curiosity::claim(&db, user_id, &query, chrono::Local::now().date_naive())
        .await
    {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            tracing::warn!(user_id, %error, "[Merope] could not claim curiosity allowance");
            return;
        }
    }
    tracing::info!(user_id, "[Merope] curious enough to look something up");
    let Some((text, found_at)) = look_up(&query, wonder.slang).await else {
        tracing::info!(user_id, "[Merope] lookup turned up nothing");
        return;
    };
    if text.trim().is_empty() {
        return;
    }
    let why = wonder.why.unwrap_or_default();
    let Ok(found) = call::Ask::new(Voice::Hers, user_id, "found_out")
        .within(CALL_TIMEOUT)
        .json::<FoundOut>(
            &digest_system(&soul, why.trim()),
            &myriad_agent_rules::untrusted_block("search_results", &text),
            DIGEST_SCHEMA,
            &digest_schema(),
        )
        .await
    else {
        return;
    };
    let learned = super::ingest::compact_summary(&found.learned);
    if learned.is_empty() {
        return;
    }
    let evidence = format!("{query} — {found_at}");
    let kept = unified::remember(
        &db,
        unified::NewMemory {
            user_id,
            kind: unified::MemoryKind::Knowledge,
            content: learned.clone(),
            evidence: Some(evidence),
            speaker: unified::Speaker::Agent,
            source: "lookup",
            // Found out because of this conversation: heard where it was.
            audience: present.clone(),
            importance: 0.5,
            concepts: found.concepts,
        },
    )
    .await;
    // Telling is said in person to one person; a group hears it next time
    // the topic comes up there.
    if matches!(kept, Ok(Some(_))) && found.tell && !present.is_group() {
        super::spawn_ingest(
            user_id,
            FOUND_OUT_EVENT,
            format!("你刚才自己去查了「{query}」，了解到：{learned}"),
        );
    }
}

/// The wonder call as production sends it, for the semantic suite.
#[cfg(test)]
pub(crate) fn wonder_probe_contract(soul: &str) -> (String, Value) {
    (wonder_system(soul), wonder_schema())
}

/// The digest call as production sends it, for the semantic suite.
#[cfg(test)]
pub(crate) fn digest_probe_contract(soul: &str, why: &str) -> (String, Value) {
    (digest_system(soul, why), digest_schema())
}

/// `Some(query)` when she would look something up, `Some(None)` when not.
#[cfg(test)]
pub(crate) fn parse_wonder(raw: &str) -> Option<Option<String>> {
    call::parse::<Wonder>(raw).map(|wonder| wonder.query.filter(|query| !query.trim().is_empty()))
}

/// Whether a digest honors the contract.
#[cfg(test)]
pub(crate) fn parse_found_out(raw: &str) -> bool {
    call::parse::<FoundOut>(raw).is_some_and(|found| !found.learned.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;
    use myriad_merope::curiosity::MAX_RESULTS_CHARS;
    #[test]
    fn search_results_are_flattened_to_text_and_addresses() {
        let hits = vec![super::super::senses::Hit {
            title: "Tame Impala".into(),
            url: "https://example.com/ti".into(),
            snippet: "Psych rock".into(),
        }];
        assert_eq!(
            hits_text(&hits),
            "- Tame Impala (https://example.com/ti): Psych rock"
        );
        assert_eq!(
            clip(&"字".repeat(MAX_RESULTS_CHARS + 5)).chars().count(),
            MAX_RESULTS_CHARS
        );
        let wonder =
            call::parse::<Wonder>(r#"{"query":"芝士雪豹","why":"没听过这个梗","slang":true}"#)
                .unwrap();
        assert!(wonder.slang);
    }
}
