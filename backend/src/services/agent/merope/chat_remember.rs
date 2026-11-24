//! After a Chat reply, optionally distill one persona-memory fact.
//!
//! What a line gives is a short note: what they said lately, true for now,
//! fading within days unless it keeps coming up. Once the stretch of talk is
//! over (they have gone quiet, or it has run long), it is gone over whole
//! (`settle`): what lasts of the notes, put together, is kept for good, and
//! what it shows has changed replaces what she knew; the rest stays a note.
//!
//! Fail-open: missing Lite, timeout, empty JSON, or a duplicate fact never
//! block the spoken reply. Delivery motion is spawned first so extract does
//! not steal the first Lite slot.

use std::time::{Duration, Instant};

use serde::Deserialize;
use serde_json::json;

use super::ingest::{compact_summary, persona_remember_insert};
use myriad_merope::chat_remember::{
    EXTRACT_SCHEMA_NAME, Examples, MIN_USER_CHARS, extract_schema, extract_schema_asked,
    extract_system_prompt, extract_system_prompt_asked, read_not_to_keep, strip_json_fence,
};

pub fn should_extract_chat_remember(user_text: &str) -> bool {
    should_extract_chat_remember_against(user_text, &[])
}

pub fn should_extract_chat_remember_against(user_text: &str, existing: &[String]) -> bool {
    let Some(compact) = memory_user_text(user_text) else {
        return false;
    };
    if compact.chars().filter(|ch| ch.is_alphanumeric()).count() < MIN_USER_CHARS {
        return false;
    }
    // Do not deduplicate a long utterance by its first 240 characters: a
    // correction can occur at the end, after repeating the previous fact.
    compact.chars().count() > 240 || persona_remember_insert(&compact, existing).is_some()
}

// Nobody waits on this: generous enough to ride out a stalled provider.
const EXTRACT_TIMEOUT: Duration = Duration::from_secs(45);
/// How long a line takes to be written down at most, all it asks: within
/// what a claim of it has (`memory_jobs`).
const EXTRACT_LIMIT: Duration = Duration::from_secs(100);

/// Whether a line that fails is gone over again after this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Tried {
    Again,
    /// Not, unless all that failed is telling what of it they asked her not
    /// to keep.
    Last,
    /// Not for that either (`memory_jobs::GIVES_UP_AFTER`).
    GaveUp,
}

/// What she already said of herself the extraction sees, at most.
const YOURS_SHOWN: usize = 5;

pub(crate) fn memory_user_text(text: &str) -> Option<String> {
    let text = super::ingest::redact_event_text(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    // Refuse an oversized assertion rather than turning a truncated prefix into
    // a fact. A missing final negation must never authorize a memory rewrite.
    (!text.is_empty() && text.chars().count() <= 2_000).then_some(text)
}

/// One bounded interpretation of the user's own assertion. Targets are exact
/// recalled facts, never free-form selectors or model-generated database ids.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ChatMemoryUpdate {
    pub fact: Option<String>,
    pub supersedes: Vec<String>,
    pub evidence: Option<String>,
    /// What `fact` is about, for recall to find it by other names.
    pub concepts: Vec<crate::services::agent::memory::unified::Concept>,
    /// When it would be worth remembering (see `unified::keep_cues`).
    #[serde(default)]
    pub cues: Vec<String>,
    /// Where on her nodes it goes, as going over the stretch decided
    /// (`memory_nodes::facts::settled`).
    #[serde(default)]
    pub topic: Option<super::memory_nodes::facts::GoesUnder>,
    /// When what it tells of happened and where it stands, as going over
    /// the stretch answered (`myriad_merope::happened`): for the entry on
    /// her nodes, which is where a kind of thing is listed whole. Not kept
    /// on the memory row.
    #[serde(default)]
    pub standing: myriad_merope::happened::Standing,
}

/// Something she told them that she would remember having said; or, when
/// `hers`, something she said about herself (what she watched and how far,
/// what she likes, what she did, what she thinks): kept as her own, with
/// what it is about, so she says the same to anyone, anywhere
/// (`myriad_merope::own_said`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Said {
    pub said: String,
    pub evidence: String,
    pub hers: bool,
    pub concepts: Vec<crate::services::agent::memory::unified::Concept>,
}

/// What one message leaves her: single updates applied in order (a
/// withdrawal first, then each new fact), and what she said worth keeping.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ChatMemoryUpdates {
    pub updates: Vec<ChatMemoryUpdate>,
    pub said: Vec<Said>,
    /// Public things they suggested she try herself, each about the thing
    /// with no one named: hers to wonder about (see `merope::heard`).
    pub put_onto: Vec<String>,
    /// Which of `updates` (by place) are what they asked her not to keep
    /// from now on: kept for good as any change of hers is, but under its
    /// own source and off her nodes, since she is always shown it where she
    /// keeps something of them (`unified::asked_not_to_keep`). Taken back or
    /// narrowed as any update is.
    pub asked: std::collections::BTreeSet<usize>,
    /// Whether the line said anything new beside what it let go of: what it
    /// lets go of is then corrected, not taken back.
    pub said_new: SaidNew,
    /// Their message (`agent_messages.id`) that took back what an update
    /// lets go of, by the update's place in `updates`: what the second look
    /// before forgetting on her nodes reads (`memory_nodes::take_back_check`).
    /// None known, it is only let go.
    pub taken_back_in: std::collections::BTreeMap<usize, i32>,
    /// What each thing to keep rests on, by what it quotes (its evidence,
    /// which no check rewrites as it does a fact's words).
    pub said_from: std::collections::BTreeMap<String, Rests>,
}

/// The lines something to keep rests on: where what it quotes is looked for
/// (`memory::origins`), however late it is kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rests {
    /// When the first of them was said.
    pub first: chrono::DateTime<chrono::FixedOffset>,
    /// The messages themselves (`agent_messages.id`), where known.
    pub lines: Vec<i32>,
}

impl ChatMemoryUpdates {
    /// What quotes `evidence` rests on the line `line`, said at `at` too.
    pub(super) fn rests_on(
        &mut self,
        evidence: &str,
        line: Option<i32>,
        at: chrono::DateTime<chrono::FixedOffset>,
    ) {
        let rests = self
            .said_from
            .entry(evidence.to_string())
            .or_insert_with(|| Rests {
                first: at,
                lines: Vec::new(),
            });
        rests.first = rests.first.min(at);
        rests
            .lines
            .extend(line.filter(|line| !rests.lines.contains(line)));
    }

    /// Of a line kept for good: what it corrects is replaced by the new fact
    /// it tells. One that corrects with no fact kept, or with several (which
    /// of them takes its place is not known), changes nothing. What a line that says nothing new lets go of, or only asks
    /// beside it not to be kept, is taken back, as ever.
    pub(super) fn correct_by_what_is_told(&mut self) {
        let asked = &self.asked;
        let told: Vec<usize> = (0..self.updates.len())
            .filter(|index| self.updates[*index].fact.is_some() && !asked.contains(index))
            .collect();
        if self.said_new != SaidNew::Something || (told.is_empty() && !asked.is_empty()) {
            return;
        }
        let corrected: Vec<String> = self
            .updates
            .iter_mut()
            .filter(|update| update.fact.is_none())
            .flat_map(|update| std::mem::take(&mut update.supersedes))
            .collect();
        if let [told] = told[..]
            && let Some(told) = self.updates.get_mut(told)
        {
            for old in corrected {
                if !told.supersedes.contains(&old) && told.fact.as_ref() != Some(&old) {
                    told.supersedes.push(old);
                }
            }
        }
    }

    /// What they asked her not to keep from now on, in place of what it
    /// replaces of what they asked before.
    pub(super) fn ask_not_to_keep(&mut self, asked: myriad_merope::chat_remember::Asked) {
        self.asked.insert(self.updates.len());
        self.updates.push(ChatMemoryUpdate {
            fact: Some(compact_summary(&asked.fact)),
            supersedes: asked.replaces,
            evidence: Some(asked.evidence),
            concepts: Vec::new(),
            cues: Vec::new(),
            topic: None,
            standing: Default::default(),
        });
    }
}

/// What a line said beside what it let go of ([`ChatMemoryUpdates::said_new`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SaidNew {
    /// Nothing: what it lets go of is taken back.
    #[default]
    Nothing,
    /// A new fact, kept or not (one that does not hold up leaves the
    /// correction undone, [`parse_chat_memory_updates`]).
    Something,
    /// Only what she already had (`already`): what it lets go of is
    /// replaced by that, wherever it is kept.
    WhatSheHad,
}

/// Corrections left as they were since the site started, their new fact not
/// holding up ([`parse_chat_memory_updates`]).
static CORRECTIONS_UNKEPT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// [`CORRECTIONS_UNKEPT`] so far, for her status.
pub(crate) fn corrections_unkept() -> u64 {
    CORRECTIONS_UNKEPT.load(std::sync::atomic::Ordering::Relaxed)
}

#[cfg(test)]
impl ChatMemoryUpdates {
    /// Seen as one update, for checks written against one: its first new
    /// fact, and everything it withdraws.
    pub fn combined(&self) -> ChatMemoryUpdate {
        ChatMemoryUpdate {
            standing: Default::default(),
            topic: None,
            cues: Vec::new(),
            fact: self.updates.iter().find_map(|update| update.fact.clone()),
            supersedes: self
                .updates
                .iter()
                .flat_map(|update| update.supersedes.iter().cloned())
                .collect(),
            evidence: None,
            concepts: Vec::new(),
        }
    }
}

/// The extraction's answer (see `extract_schema`), each part held to the
/// same checks as a single update; `reply` is what her said-lines must
/// quote. None when the answer is malformed as a whole.
pub fn parse_chat_memory_updates(
    raw: &str,
    user_text: &str,
    reply: &str,
    existing: &[String],
) -> Option<ChatMemoryUpdates> {
    let value: serde_json::Value = serde_json::from_str(strip_json_fence(raw)).ok()?;
    let facts = value.get("facts")?.as_array()?;
    let supersedes = value.get("supersedes")?.as_array()?;
    let said = value.get("said")?.as_array()?;
    let mut out = ChatMemoryUpdates {
        said_new: if facts.is_empty() {
            SaidNew::Nothing
        } else {
            SaidNew::Something
        },
        ..ChatMemoryUpdates::default()
    };
    // A withdrawal or correction of what she knew comes first, on its own.
    if !supersedes.is_empty() {
        let single = serde_json::json!({
            "fact": null,
            "supersedes": supersedes,
            "evidence": value.get("supersedesEvidence").cloned().unwrap_or_default(),
            "concepts": [],
        });
        out.updates.push(parse_chat_memory_update(
            &single.to_string(),
            user_text,
            existing,
        )?);
    }
    for fact in facts.iter().take(myriad_merope::chat_remember::MAX_FACTS) {
        let single = serde_json::json!({
            "fact": fact.get("fact"),
            "supersedes": [],
            "evidence": fact.get("evidence"),
            "concepts": fact.get("concepts").cloned().unwrap_or_else(|| serde_json::json!([])),
        });
        // One fact that does not hold up does not take the others with it.
        if let Some(update) = parse_chat_memory_update(&single.to_string(), user_text, existing)
            && update.fact.is_some()
            && !out.updates.iter().any(|kept| kept.fact == update.fact)
        {
            out.updates.push(update);
        }
    }
    // A correction none of whose new facts holds up is not made: what it
    // corrects stays as it was (the answer may be what is wrong), rather
    // than going with nothing kept in its place. Counted.
    if !facts.is_empty() && out.updates.iter().all(|update| update.fact.is_none()) {
        if !out.updates.is_empty() {
            let so_far = CORRECTIONS_UNKEPT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) + 1;
            tracing::warn!(
                so_far,
                "[chat-remember] a correction whose new fact did not hold up is left as it was"
            );
        }
        out.updates.clear();
    }
    for item in said.iter().take(myriad_merope::chat_remember::MAX_SAID) {
        let (Some(line), Some(evidence)) = (
            item.get("said").and_then(serde_json::Value::as_str),
            item.get("evidence").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        let line = compact_summary(line);
        let evidence = evidence.trim();
        if line.is_empty()
            || line.chars().count() > 200
            || evidence.chars().filter(|ch| ch.is_alphanumeric()).count() < 2
            || !reply.contains(evidence)
        {
            continue;
        }
        out.said.push(Said {
            said: line,
            evidence: evidence.to_string(),
            hers: false,
            concepts: Vec::new(),
        });
    }
    // What she said about herself, as much as she did: hers. Older answers
    // have none; one that does not quote her reply is dropped.
    let own = value
        .get("own")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for item in &own {
        let (Some(line), Some(evidence)) = (
            item.get("line").and_then(serde_json::Value::as_str),
            item.get("evidence").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        let line = compact_summary(line);
        let evidence = evidence.trim();
        if line.is_empty()
            || line.chars().count() > myriad_merope::chat_remember::MAX_OWN_CHARS
            || evidence.chars().filter(|ch| ch.is_alphanumeric()).count() < 2
            || !reply.contains(evidence)
            || out.said.iter().any(|said| said.hers && said.said == line)
        {
            continue;
        }
        let concepts = item
            .get("concepts")
            .cloned()
            .and_then(|concepts| serde_json::from_value(concepts).ok())
            .map(crate::services::agent::memory::unified::clean_concepts)
            .unwrap_or_default();
        out.said.push(Said {
            said: line,
            evidence: evidence.to_string(),
            hers: true,
            concepts,
        });
    }
    // Older answers have none; one that does not quote them is dropped.
    let put_onto = value
        .get("putOnto")
        .and_then(serde_json::Value::as_array)
        .cloned()
        .unwrap_or_default();
    for item in put_onto
        .iter()
        .take(myriad_merope::chat_remember::MAX_PUT_ONTO)
    {
        let (Some(thing), Some(evidence)) = (
            item.get("thing").and_then(serde_json::Value::as_str),
            item.get("evidence").and_then(serde_json::Value::as_str),
        ) else {
            continue;
        };
        let thing = compact_summary(thing);
        let evidence = evidence.trim();
        if thing.is_empty()
            || thing.chars().count() > 160
            || evidence.chars().filter(|ch| ch.is_alphanumeric()).count() < 2
            || !user_text.contains(evidence)
            || out.put_onto.contains(&thing)
        {
            continue;
        }
        out.put_onto.push(thing);
    }
    Some(out)
}

pub fn parse_chat_memory_update(
    raw: &str,
    user_text: &str,
    existing: &[String],
) -> Option<ChatMemoryUpdate> {
    let stripped = strip_json_fence(raw);
    let value: serde_json::Value = serde_json::from_str(stripped).ok()?;
    // Nullable fields are still required; missing is not an old-format fallback.
    if !["fact", "supersedes", "evidence", "concepts"]
        .iter()
        .all(|key| value.get(key).is_some())
    {
        return None;
    }
    let mut update: ChatMemoryUpdate = serde_json::from_value(value).ok()?;
    update.concepts = if update.fact.is_some() {
        crate::services::agent::memory::unified::clean_concepts(std::mem::take(
            &mut update.concepts,
        ))
    } else {
        Vec::new()
    };
    if let Some(fact) = &update.fact {
        if fact.chars().count() > 240 || compact_summary(fact).is_empty() {
            return None;
        }
        update.fact = Some(compact_summary(fact));
    }
    if update.supersedes.len() > 8
        || update
            .supersedes
            .iter()
            .any(|old| !existing.contains(old) || update.fact.as_ref() == Some(old))
    {
        return None;
    }
    let mut unique = update.supersedes.clone();
    unique.sort();
    unique.dedup();
    if unique.len() != update.supersedes.len() {
        return None;
    }
    if update.fact.is_some() || !update.supersedes.is_empty() {
        let evidence = update.evidence.as_deref()?.trim();
        if evidence.chars().filter(|ch| ch.is_alphanumeric()).count() < 2
            || evidence.chars().count() > 240
            || !user_text.contains(evidence)
        {
            return None;
        }
    } else if update.evidence.is_some() {
        return None;
    }
    Some(update)
}

/// What the extraction reads: their words, her reply, and what surrounded
/// them. Only what they said in `userText` may become a fact.
pub(super) fn extract_input(
    user_text: &str,
    reply: &str,
    turn: &super::TurnContext,
    now: chrono::DateTime<super::clock::Zone>,
) -> String {
    let mut input = json!({
        "userText": user_text,
        "reply": compact_summary(reply),
        "today": now.format("%Y-%m-%d (%A)").to_string(),
    });
    if let Some(before) = &turn.before {
        input["before"] = json!(before);
    }
    if let Some(scene) = &turn.scene {
        input["scene"] = json!(scene);
    }
    if turn.in_game {
        input["inGame"] = json!(true);
    }
    input.to_string()
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn extract(
    db: &sea_orm::DatabaseConnection,
    user_id: i32,
    user_text: &str,
    reply: &str,
    input_at: chrono::DateTime<chrono::FixedOffset>,
    present: &crate::services::agent::memory::unified::Audience,
    turn: &super::TurnContext,
    tried: Tried,
) -> Result<super::memory_jobs::Effect, super::memory_jobs::Failure> {
    use super::memory_jobs::{Effect, Failure};
    let until = Instant::now() + EXTRACT_LIMIT;
    let left = |longest: Duration| until.saturating_duration_since(Instant::now()).min(longest);
    // Failed so, is it gone over again? What they ask her not to keep is
    // not let go with the last try of a line; and what could not be told of
    // that is tried till it is, or given up.
    let again = |failure: &Failure| tried == Tried::Again && failure.retryable();
    let judged_again = tried != Tried::GaveUp;
    let (existing, _) =
        crate::services::agent::memory::unified::quietly(super::store::recall_remembered_primed(
            db,
            user_id,
            present,
            Some(user_text),
            8,
            &crate::services::agent::memory::unified::Priming::default(),
            1.0,
        ))
        .await
        .map_err(|_| Failure::Storage)?;
    // What they told her last, first: "forget what I just said" names
    // nothing that recall by their words would find. In the same plain form
    // as recalled facts, since a correction quotes one to replace it.
    let latest = crate::services::agent::memory::unified::latest_of(
        db,
        user_id,
        crate::services::agent::memory::unified::SHORT_NOTE_SOURCE,
        3,
    )
    .await
    .map_err(|_| Failure::Storage)?;
    let mut known: Vec<String> = latest
        .into_iter()
        .filter(|row| row.venue == present.venue())
        .map(|row| super::ingest::compact_summary(&row.content))
        .filter(|fact| !fact.is_empty())
        .collect();
    for fact in existing {
        if !known.contains(&fact) {
            known.push(fact);
        }
    }
    let existing = known;
    if !should_extract_chat_remember_against(user_text, &existing) {
        return Ok(Effect::NoChange);
    }
    // Recovery retains the date of the original assertion.
    let mut input = extract_input(
        user_text,
        reply,
        turn,
        input_at.with_timezone(&super::clock::zone()),
    );
    // What she already said of herself that this exchange touches: what is
    // there already is not kept again, what changed is (`own`'s `yours`).
    // Looked up by their line and what she said before as well as her
    // reply: a short answer ("it grew on me") does not name what it is about.
    if !reply.trim().is_empty() {
        let exchange = [user_text, turn.before.as_deref().unwrap_or(""), reply]
            .into_iter()
            .filter(|part| !part.trim().is_empty())
            .collect::<Vec<_>>()
            .join("\n");
        let yours: Vec<String> = super::store::said_of_herself(db, present, &exchange, YOURS_SHOWN)
            .await
            .into_iter()
            .map(|said| said.said)
            .collect();
        if !yours.is_empty()
            && let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&input)
        {
            value["yours"] = json!(yours);
            input = value.to_string();
        }
    }
    let examples = Examples::for_words(user_text);
    let mut raw = written(
        user_id,
        &extract_system_prompt_asked(&existing, examples),
        &input,
        &extract_schema_asked(),
        EXTRACT_TIMEOUT,
    )
    .await?;
    let mut asked = serde_json::from_str(strip_json_fence(&raw))
        .map(|answer| read_not_to_keep(&answer, user_text, None, &[]))
        .unwrap_or_default();
    if !asked.is_empty() {
        let read = asked.len();
        match super::asked::really_asked(user_id, user_text, asked.clone(), Some(until)).await {
            Ok(really) => asked = really,
            Err(failure) if judged_again || !failure.of_what_was_shown() => {
                return Err(Failure::NotTold(failure));
            }
            // Not told, time after time: held to all the same.
            Err(_) => {}
        }
        if asked.len() < read {
            // Read as asking what it does not ask: written down once more
            // with nothing of that, so what it tells is kept as ever. What
            // it does ask is held to it below.
            match written(
                user_id,
                &extract_system_prompt(&existing, examples),
                &input,
                &extract_schema(),
                left(EXTRACT_TIMEOUT),
            )
            .await
            {
                Ok(plain) => raw = plain,
                Err(failure) => {
                    let failure = failure.into();
                    if asked.is_empty() || again(&failure) {
                        return Err(failure);
                    }
                }
            }
        }
    }
    let mut updates = match parse_chat_memory_updates(&raw, user_text, reply, &existing) {
        Some(updates) => updates,
        // What they ask, as read, stands on its own words.
        None if !asked.is_empty() => ChatMemoryUpdates::default(),
        None => return Err(super::call::Failure::InvalidOutput.into()),
    };
    already::check_against_unshown(
        db,
        user_id,
        present,
        &existing,
        &mut updates,
        left(already::TIMEOUT),
    )
    .await;
    match super::asked::leave_out_of(db, user_id, &asked, &mut updates, Some(until)).await {
        Ok(()) => {}
        Err(failure) if asked.is_empty() || judged_again || !failure.untold() => {
            return Err(failure.of_judging());
        }
        // What is of it cannot be told, time after time: of what the line
        // gives, only what they ask, and what they take back where it tells
        // nothing (beside something told, it may be what that corrects).
        Err(_) => {
            if updates.updates.iter().any(|update| update.fact.is_some()) {
                super::asked::leave_what_stands(&mut updates);
                updates.updates.clear();
            }
            updates.said.clear();
            updates.put_onto.clear();
        }
    }
    for one in asked {
        updates.ask_not_to_keep(one);
    }
    if updates.updates.is_empty() && updates.said.is_empty() && updates.put_onto.is_empty() {
        return Ok(Effect::NoChange);
    }
    Ok(Effect::Chat(updates))
}

/// What a line gives, as it is asked for.
async fn written(
    user_id: i32,
    system: &str,
    input: &str,
    schema: &serde_json::Value,
    within: Duration,
) -> Result<String, super::call::Failure> {
    super::call::Ask::new(super::call::Voice::Keeping, user_id, "chat_remember")
        .cached(super::call::REMEMBER_CACHE)
        .within(within)
        // At most 2,483 measured, thinking included.
        .output_at_most(6000)
        .json_raw(system, input, EXTRACT_SCHEMA_NAME, schema)
        .await
}

#[cfg(test)]
pub(crate) fn live_probe_contract(
    existing: &[String],
    user_text: &str,
) -> (String, serde_json::Value) {
    (
        extract_system_prompt(existing, Examples::for_words(user_text)),
        extract_schema(),
    )
}

/// The extraction input as production builds it, on a fixed date.
#[cfg(test)]
pub(crate) fn probe_input(user_text: &str, reply: &str, turn: &super::TurnContext) -> String {
    use chrono::TimeZone;
    let now = super::clock::zone()
        .with_ymd_and_hms(2026, 9, 25, 21, 0, 0)
        .single()
        .expect("fixed date");
    extract_input(user_text, reply, turn, now)
}

/// Going over a stretch as production asks it, on a fixed date: `lines`
/// oldest first (`true` on theirs), the short notes taken from it, and what
/// she knows of them for good.
#[cfg(test)]
pub(crate) fn settle_probe(
    lines: &[(bool, String)],
    lately: &[String],
    known: &[String],
) -> (String, serde_json::Value, String) {
    let theirs: Vec<&str> = lines
        .iter()
        .filter(|(theirs, _)| *theirs)
        .map(|(_, line)| line.as_str())
        .collect();
    (
        myriad_merope::chat_remember::settle_system_prompt(
            known,
            Examples::for_words(&theirs.join("\n")),
        ),
        myriad_merope::chat_remember::settle_schema(),
        settle::input(
            lines,
            lately,
            "2026-09-25 (Thursday)",
            &serde_json::json!([]),
        ),
    )
}

#[cfg(test)]
#[path = "chat_remember/asked_tests.rs"]
mod asked_tests;

#[cfg(test)]
mod tests {
    use super::ChatMemoryUpdate;
    use super::already::{Unshown, alike_unshown, settle_already};
    use crate::services::agent::memory::unified::Concept;
    use myriad_merope::chat_remember::Already;

    fn new_fact(fact: &str, subject: &str) -> ChatMemoryUpdate {
        ChatMemoryUpdate {
            standing: Default::default(),
            topic: None,
            cues: Vec::new(),
            fact: Some(fact.into()),
            supersedes: Vec::new(),
            evidence: Some(fact.into()),
            concepts: vec![Concept {
                name: subject.into(),
                aliases: Vec::new(),
            }],
        }
    }

    /// Only what looks alike is checked: said another way, or the same
    /// subject with something changed; not everything about them.
    #[test]
    fn a_new_fact_is_checked_only_against_what_looks_like_it() {
        let updates = vec![
            new_fact("他的猫叫豆豆", "豆豆"),
            new_fact("周六考试", "考试"),
        ];
        let unshown = said_plainly(&["养了一只叫豆豆的猫", "周五要考试", "喜欢喝咖啡"]);
        let pairs = alike_unshown(&updates, &unshown);
        assert_eq!(
            pairs,
            vec![
                (0, "养了一只叫豆豆的猫".to_string()),
                (1, "周五要考试".to_string())
            ]
        );
        assert!(alike_unshown(&[new_fact("在学吉他", "吉他")], &unshown).is_empty());
    }

    fn said_plainly(texts: &[&str]) -> Vec<Unshown> {
        texts
            .iter()
            .map(|text| Unshown {
                text: (*text).into(),
                concepts: Vec::new(),
            })
            .collect()
    }

    fn about(text: &str, subject: &str, aliases: &[&str]) -> Unshown {
        Unshown {
            text: text.into(),
            concepts: vec![Concept {
                name: subject.into(),
                aliases: aliases.iter().map(|alias| (*alias).into()).collect(),
            }],
        }
    }

    /// Many words in common are not enough when both say they are about
    /// different things; a name in common, or one named in the other's
    /// words, still is. A fact that says nothing of its subject is checked
    /// as before.
    #[test]
    fn facts_about_different_things_are_not_asked_about() {
        let update = new_fact("周五要去医院复查", "复查");
        let apart = about("周五要去医院看牙", "看牙", &["牙医"]);
        assert!(alike_unshown(std::slice::from_ref(&update), &[apart]).is_empty());

        let alias = about("周五要去医院看牙", "看牙", &["复查"]);
        assert_eq!(
            alike_unshown(std::slice::from_ref(&update), &[alias]).len(),
            1
        );
        let named = about("周五要去医院复查", "医院", &[]);
        assert_eq!(
            alike_unshown(std::slice::from_ref(&update), &[named]).len(),
            1
        );
        let unsaid = said_plainly(&["周五要去医院看牙"]);
        assert_eq!(
            alike_unshown(std::slice::from_ref(&update), &unsaid).len(),
            1
        );
        let mut silent = update.clone();
        silent.concepts.clear();
        let apart = about("周五要去医院看牙", "看牙", &[]);
        assert_eq!(alike_unshown(&[silent], &[apart]).len(), 1);
    }

    /// Already had: not kept again. How it is now: replaces what she had.
    /// Unsure: both stay.
    #[test]
    fn the_judgment_keeps_once_or_replaces_never_merely_counts() {
        let mut updates = vec![
            new_fact("他的猫叫豆豆", "豆豆"),
            new_fact("考试改到周六", "考试"),
            new_fact("也喜欢茶", "茶"),
        ];
        let pairs = vec![
            (0, "养了一只叫豆豆的猫".to_string()),
            (1, "周五要考试".to_string()),
            (2, "喜欢喝咖啡".to_string()),
        ];
        settle_already(
            &mut updates,
            &pairs,
            &[Already::Same, Already::Replaces, Already::Different],
            false,
        );
        assert_eq!(updates.len(), 2, "the one she had is not kept twice");
        assert_eq!(updates[0].fact.as_deref(), Some("考试改到周六"));
        assert_eq!(updates[0].supersedes, vec!["周五要考试".to_string()]);
        assert_eq!(updates[1].fact.as_deref(), Some("也喜欢茶"));
        assert!(updates[1].supersedes.is_empty());
    }

    /// Two that cannot both be true, with nothing to say which: what she
    /// keeps is one fact holding both, in place of the one she had, so
    /// neither comes to mind without the other. A second such pair for the
    /// same fact stays apart.
    #[test]
    fn two_that_do_not_agree_become_one_holding_both() {
        let mut updates = vec![new_fact("是独生子", "兄弟姐妹")];
        let together = "说过有个姐姐，又说自己是独生子，不清楚哪个对";
        let pairs = vec![
            (0, "有一个姐姐".to_string()),
            (0, "有个弟弟在读高中".to_string()),
        ];
        settle_already(
            &mut updates,
            &pairs,
            &[
                Already::Unclear(together.into()),
                Already::Unclear("说过有个弟弟，又说自己是独生子".into()),
            ],
            false,
        );
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].fact.as_deref(), Some(together));
        assert_eq!(updates[0].supersedes, vec!["有一个姐姐".to_string()]);
        assert_eq!(
            updates[0].evidence.as_deref(),
            Some("是独生子"),
            "still what they said"
        );
    }

    /// A correction is not taken for a taking back: one whose new fact does
    /// not hold up is not made at all (what it corrects stays as it was);
    /// one that only lets go says nothing new.
    #[test]
    fn a_correction_whose_new_fact_does_not_hold_up_is_not_made() {
        let user = "不是周五，改到周六看牙了";
        let corrected = serde_json::json!({
            "facts": [{"fact": "改到周六看牙", "evidence": "周六去看牙", "concepts": []}],
            "supersedes": ["周五去看牙"],
            "supersedesEvidence": "不是周五",
            "said": []
        })
        .to_string();
        let updates =
            parse_chat_memory_updates(&corrected, user, "", &["周五去看牙".into()]).unwrap();
        assert!(updates.updates.is_empty(), "{updates:?}");
        assert_eq!(updates.said_new, SaidNew::Something);
        let let_go = serde_json::json!({
            "facts": [], "supersedes": ["周五去看牙"], "supersedesEvidence": "不是周五", "said": []
        })
        .to_string();
        let updates = parse_chat_memory_updates(&let_go, user, "", &["周五去看牙".into()]).unwrap();
        assert_eq!(updates.updates.len(), 1);
        assert_eq!(updates.said_new, SaidNew::Nothing);
    }

    /// A correction whose new fact she already had replaces what it lets go
    /// of by the one she had; with two she had, which it is goes unsaid.
    #[test]
    fn a_correction_to_what_she_had_is_replaced_by_it() {
        let let_go = ChatMemoryUpdate {
            fact: None,
            supersedes: vec!["周五要考试".into()],
            evidence: Some("不是周五".into()),
            ..new_fact("", "考试")
        };
        let mut updates = vec![let_go.clone(), new_fact("考试改到周六", "考试")];
        let pairs = vec![(1, "考试在周六".to_string())];
        assert_eq!(
            settle_already(&mut updates, &pairs, &[Already::Same], false),
            Some((0, "考试在周六".to_string()))
        );
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].fact.as_deref(), Some("考试在周六"));
        assert_eq!(updates[0].supersedes, vec!["周五要考试".to_string()]);

        let mut updates = vec![let_go, new_fact("考试改到周六", "考试")];
        let pairs = vec![(1, "考试在周六".to_string()), (1, "周六考数学".to_string())];
        assert_eq!(
            settle_already(&mut updates, &pairs, &[Already::Same, Already::Same], false),
            None
        );
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].fact, None);
    }

    /// Going over a whole talk, one line's taking back is not corrected by
    /// what another of its lines said and she already had.
    #[test]
    fn going_over_a_talk_corrects_nothing_by_what_she_had() {
        let let_go = ChatMemoryUpdate {
            fact: None,
            supersedes: vec!["周五要考试".into()],
            evidence: Some("当我没说".into()),
            ..new_fact("", "考试")
        };
        let mut updates = vec![let_go.clone(), new_fact("考试改到周六", "考试")];
        let pairs = vec![(1, "考试在周六".to_string())];
        assert_eq!(
            settle_already(&mut updates, &pairs, &[Already::Same], true),
            None
        );
        assert_eq!(updates, [let_go]);
    }

    /// Of a talk gone over, a fact found already had that says itself what
    /// it replaces still replaces it, by the one she had, with other facts
    /// told beside it too; and what she had is replaced by one fact, a
    /// second that would is kept beside it.
    #[test]
    fn going_over_what_a_fact_replaces_is_never_taken_back() {
        let moved = ChatMemoryUpdate {
            supersedes: vec!["住在杭州".into()],
            ..new_fact("搬到上海了", "住处")
        };
        let mut updates = vec![moved, new_fact("养了一只龟", "龟")];
        let pairs = vec![(0, "住在上海".to_string())];
        assert_eq!(
            settle_already(&mut updates, &pairs, &[Already::Same], true),
            None
        );
        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0].fact.as_deref(), Some("住在上海"));
        assert_eq!(updates[0].supersedes, ["住在杭州"]);

        let mut updates = vec![
            new_fact("考试改到周六", "考试"),
            new_fact("周六在本部考试", "考试"),
        ];
        let pairs = vec![(0, "周五要考试".to_string()), (1, "周五要考试".to_string())];
        settle_already(
            &mut updates,
            &pairs,
            &[Already::Replaces, Already::Replaces],
            true,
        );
        assert_eq!(updates[0].supersedes, ["周五要考试"]);
        assert!(updates[1].supersedes.is_empty() && updates[1].fact.is_some());
    }

    #[test]
    fn what_they_put_her_onto_is_kept_only_when_they_said_it() {
        let user = "你可以去玩玩《Outer Wilds》，千万别看攻略";
        let raw = serde_json::json!({
            "facts": [], "supersedes": [], "supersedesEvidence": null, "said": [],
            "putOnto": [
                {"thing": "有人推荐《Outer Wilds》，说千万别看攻略", "evidence": "你可以去玩玩《Outer Wilds》"},
                {"thing": "有人推荐《星露谷》", "evidence": "星露谷也不错"}
            ]
        })
        .to_string();
        let updates = super::parse_chat_memory_updates(&raw, user, "好", &[]).unwrap();
        assert_eq!(
            updates.put_onto,
            vec!["有人推荐《Outer Wilds》，说千万别看攻略".to_string()]
        );
        // Answers from before there was putOnto still parse.
        let older = r#"{"facts":[],"supersedes":[],"supersedesEvidence":null,"said":[]}"#;
        assert!(
            super::parse_chat_memory_updates(older, user, "好", &[])
                .unwrap()
                .put_onto
                .is_empty()
        );
        let schema = myriad_merope::chat_remember::extract_schema();
        assert_eq!(
            schema["properties"]["putOnto"]["maxItems"],
            myriad_merope::chat_remember::MAX_PUT_ONTO
        );
        assert!(
            myriad_merope::chat_remember::extract_system_prompt(&[], Examples::Chinese)
                .contains("with no names of people")
        );
        assert!(
            myriad_merope::chat_remember::extract_system_prompt(&[], Examples::Chinese)
                .contains("当我没说")
        );
    }

    #[test]
    fn one_message_can_leave_several_facts_and_what_she_said() {
        let user = "上周五把自行车修好了，下周二车要去保养。对了我换工作了";
        let reply = "修好就好！我推荐你试试「夜航」这首歌，下班路上听。";
        let raw = serde_json::json!({
            "facts": [
                {"fact": "2026-09-19 把自行车修好了", "evidence": "上周五把自行车修好了", "concepts": []},
                {"fact": "2026-09-30 车要去保养", "evidence": "下周二车要去保养", "concepts": []},
                // Not what they said: dropped, the others stay.
                {"fact": "他很喜欢骑车", "evidence": "我喜欢骑车", "concepts": []},
                {"fact": "换了工作", "evidence": "我换工作了", "concepts": []}
            ],
            "supersedes": ["在银行上班"],
            "supersedesEvidence": "我换工作了",
            "said": [
                {"said": "我推荐了「夜航」这首歌", "evidence": "我推荐你试试「夜航」这首歌"},
                {"said": "我说过要陪他跑步", "evidence": "陪你跑步"}
            ],
            "own": [
                {"line": "我下班路上听「夜航」", "evidence": "下班路上听", "concepts": [{"name": "夜航", "aliases": []}]},
                // Not in her reply: dropped.
                {"line": "我也修过自行车", "evidence": "我也修过", "concepts": []},
                {"line": "我下班路上听「夜航」", "evidence": "下班路上听", "concepts": []}
            ]
        })
        .to_string();
        let updates = parse_chat_memory_updates(&raw, user, reply, &["在银行上班".into()]).unwrap();
        // The withdrawal first, then each fact that holds up.
        assert_eq!(
            updates.updates[0].supersedes,
            vec!["在银行上班".to_string()]
        );
        assert_eq!(updates.updates[0].fact, None);
        let facts: Vec<&str> = updates.updates[1..]
            .iter()
            .filter_map(|update| update.fact.as_deref())
            .collect();
        assert_eq!(
            facts,
            [
                "2026-09-19 把自行车修好了",
                "2026-09-30 车要去保养",
                "换了工作"
            ]
        );
        // What she said is kept only when her reply says it; what she said
        // about herself is hers, once.
        assert_eq!(updates.said.len(), 2);
        assert_eq!(updates.said[0].said, "我推荐了「夜航」这首歌");
        assert!(!updates.said[0].hers);
        assert_eq!(updates.said[1].said, "我下班路上听「夜航」");
        assert!(updates.said[1].hers);
        assert_eq!(updates.said[1].concepts[0].name, "夜航");
        assert_eq!(
            updates.combined().supersedes,
            vec!["在银行上班".to_string()]
        );
        assert_eq!(
            updates.combined().fact.as_deref(),
            Some("2026-09-19 把自行车修好了")
        );
        // Nothing to keep is an empty answer, not an invalid one.
        let nothing = r#"{"facts":[],"supersedes":[],"supersedesEvidence":null,"said":[]}"#;
        assert_eq!(
            parse_chat_memory_updates(nothing, user, reply, &[]),
            Some(ChatMemoryUpdates::default())
        );
        assert_eq!(
            parse_chat_memory_updates(r#"{"fact":null}"#, user, reply, &[]),
            None
        );
        // A withdrawal of something she never knew spoils the whole answer.
        let unknown =
            r#"{"facts":[],"supersedes":["从没记过"],"supersedesEvidence":"我换工作了","said":[]}"#;
        assert_eq!(parse_chat_memory_updates(unknown, user, reply, &[]), None);
    }

    use super::*;

    #[test]
    fn short_facts_are_evaluated_instead_of_silently_dropped() {
        assert!(should_extract_chat_remember("你好")); // Lite can choose null.
        assert!(!should_extract_chat_remember("好"));
        assert!(!should_extract_chat_remember("……！！"));
        assert!(!should_extract_chat_remember("   "));
        assert!(should_extract_chat_remember("我不喝咖啡了"));
        assert!(should_extract_chat_remember("我吃素"));
        assert!(should_extract_chat_remember("猫が好き"));
        assert!(should_extract_chat_remember("晚上想打独立游戏"));
        assert!(!should_extract_chat_remember_against(
            "晚上想打独立游戏",
            &["晚上想打独立游戏".into()]
        ));
        assert!(should_extract_chat_remember_against(
            "早上只喝美式咖啡",
            &["晚上想打独立游戏".into()]
        ));
    }
    #[test]
    fn the_extraction_sees_what_surrounded_their_words() {
        let turn = crate::services::agent::merope::TurnContext {
            before: Some("你最喜欢什么动物？".into()),
            scene: Some("They are listening to 晴天.".into()),
            in_game: true,
            images: 0,
        };
        let input: serde_json::Value =
            serde_json::from_str(&probe_input("橘猫吧", "好品味", &turn)).unwrap();
        assert_eq!(input["before"], "你最喜欢什么动物？");
        assert_eq!(input["inGame"], true);
        assert_eq!(input["today"], "2026-09-25 (Friday)");
        assert!(input["scene"].as_str().unwrap().contains("晴天"));
        let quiet: serde_json::Value = serde_json::from_str(&probe_input(
            "橘猫吧",
            "好品味",
            &crate::services::agent::merope::TurnContext::default(),
        ))
        .unwrap();
        assert!(quiet.get("before").is_none() && quiet.get("inGame").is_none());
        let prompt = extract_system_prompt(&[], Examples::Chinese);
        assert!(prompt.contains("If inGame is true"));
        assert!(prompt.contains("still taking the fact from userText"));
    }

    #[test]
    fn corrections_at_the_end_of_a_long_utterance_are_not_truncated_or_deduplicated() {
        let prefix = "以前喜欢咖啡".repeat(45);
        let text = format!("{prefix}。但我现在不喝咖啡了。");
        assert!(should_extract_chat_remember_against(
            &text,
            &[compact_summary(&prefix)]
        ));
        let input = memory_user_text(&text).unwrap();
        assert!(input.ends_with("但我现在不喝咖啡了。"));
        let raw = r#"{"fact":"现在不喝咖啡","supersedes":["喜欢咖啡"],"evidence":"但我现在不喝咖啡了","concepts":[]}"#;
        assert!(parse_chat_memory_update(raw, &input, &["喜欢咖啡".into()]).is_some());
        assert!(memory_user_text(&"茶".repeat(2_001)).is_none());
        assert!(!should_extract_chat_remember(&"茶".repeat(2_001)));
    }

    #[test]
    fn parse_accepts_bounded_add_replace_retract_or_no_change() {
        let existing = vec!["喜欢咖啡".into()];
        for (raw, input, fact, targets) in [
            (
                r#"{"fact":null,"supersedes":[],"evidence":null,"concepts":[]}"#,
                "你好",
                None,
                vec![],
            ),
            (
                r#"{"fact":"也喜欢茶","supersedes":[],"evidence":"我也喜欢茶","concepts":[]}"#,
                "我也喜欢茶",
                Some("也喜欢茶"),
                vec![],
            ),
            (
                r#"{"fact":"现在不喝咖啡","supersedes":["喜欢咖啡"],"evidence":"我不喝咖啡了","concepts":[]}"#,
                "我不喝咖啡了",
                Some("现在不喝咖啡"),
                vec!["喜欢咖啡"],
            ),
            (
                r#"{"fact":null,"supersedes":["喜欢咖啡"],"evidence":"咖啡偏好记错了，请撤回","concepts":[]}"#,
                "咖啡偏好记错了，请撤回",
                None,
                vec!["喜欢咖啡"],
            ),
        ] {
            let update = parse_chat_memory_update(raw, input, &existing).unwrap();
            assert_eq!(update.fact.as_deref(), fact);
            assert_eq!(update.supersedes, targets);
            assert_eq!(
                parse_chat_memory_update(&format!("```json\n{raw}\n```"), input, &existing),
                Some(update)
            );
        }
    }

    #[test]
    fn rejects_unknown_targets_missing_evidence_and_old_or_malformed_contracts() {
        let existing = vec!["喜欢咖啡".into()];
        for raw in [
            r#"{"fact":null}"#,
            r#"{"fact":null,"supersedes":[]}"#,
            r#"{"fact":"不喝咖啡","supersedes":[],"evidence":"我不喝咖啡了"}"#,
            r#"{"fact":null,"supersedes":[],"evidence":"我不喝咖啡了","concepts":[]}"#,
            r#"{"fact":"不喝咖啡","supersedes":["其他人的事实"],"evidence":"我不喝咖啡了","concepts":[]}"#,
            r#"{"fact":"不喝咖啡","supersedes":["喜欢咖啡"],"evidence":null,"concepts":[]}"#,
            r#"{"fact":"不喝咖啡","supersedes":["喜欢咖啡"],"evidence":"模型猜测","concepts":[]}"#,
            r#"{"fact":"喜欢咖啡","supersedes":["喜欢咖啡"],"evidence":"我不喝咖啡了","concepts":[]}"#,
            r#"{"fact":"不喝咖啡","supersedes":["喜欢咖啡","喜欢咖啡"],"evidence":"我不喝咖啡了","concepts":[]}"#,
            r#"{"fact":"不喝咖啡","supersedes":[],"evidence":"我不喝咖啡了","concepts":[],"action":"delete_all"}"#,
        ] {
            assert!(
                parse_chat_memory_update(raw, "我不喝咖啡了", &existing).is_none(),
                "{raw}"
            );
        }
        let long = json!({"fact":"茶".repeat(241),"supersedes":[],"evidence":"我不喝咖啡了","concepts":[]});
        assert!(parse_chat_memory_update(&long.to_string(), "我不喝咖啡了", &existing).is_none());
    }

    #[test]
    fn concepts_are_cleaned_and_dropped_without_a_new_fact() {
        let raw = r#"{"fact":"养了一只猫叫年糕","supersedes":[],"evidence":"我养了一只猫叫年糕","concepts":[{"name":" 猫 ","aliases":["喵","猫","x","猫咪"]},{"name":"猫","aliases":[]},{"name":"年糕","aliases":[]}]}"#;
        let update = parse_chat_memory_update(raw, "我养了一只猫叫年糕", &[]).unwrap();
        let names: Vec<(&str, Vec<&str>)> = update
            .concepts
            .iter()
            .map(|concept| {
                (
                    concept.name.as_str(),
                    concept.aliases.iter().map(String::as_str).collect(),
                )
            })
            .collect();
        assert_eq!(names, vec![("猫", vec!["喵", "猫咪"]), ("年糕", vec![])]);
        let retract = r#"{"fact":null,"supersedes":["喜欢咖啡"],"evidence":"咖啡偏好记错了","concepts":[{"name":"咖啡","aliases":[]}]}"#;
        let update =
            parse_chat_memory_update(retract, "咖啡偏好记错了", &["喜欢咖啡".into()]).unwrap();
        assert!(update.concepts.is_empty());
        let unknown = r#"{"fact":"养猫","supersedes":[],"evidence":"我养猫","concepts":[{"name":"猫","kind":"pet"}]}"#;
        assert!(parse_chat_memory_update(unknown, "我养猫", &[]).is_none());
    }

    #[test]
    fn chat_does_not_wait_on_extract_and_starts_motion_first() {
        let src = include_str!("../process_chat.rs");
        let chat = src
            .find("stream_strict_lite_chat_response")
            .expect("streaming chat");
        let after = &src[chat..];
        let reaction = src
            .find("local_directive(&reaction_context)")
            .expect("immediate reaction");
        let director = src
            .find("spawn_chat_motion_refinement(")
            .expect("parallel delivery observer");
        let extract = after
            .find("enqueue_chat_remember")
            .expect("chat remember extract");
        let ret = after.find("return Ok(AgentResponse").expect("chat return");
        assert!(
            reaction < director && director < chat,
            "immediate reaction and delivery observer must start before Chat, not after memory extraction"
        );
        assert!(after.find("response_agent::finish_stream").unwrap() < extract);
        assert!(extract < ret, "extract must not delay the Chat response");
        assert!(!src.contains("extract_and_store("));
    }
}

mod already;
pub(super) mod cues;
mod queue;
pub use queue::enqueue_chat_remember;
pub(super) use queue::hold_chat_words;
#[cfg(test)]
mod replay;

pub(in crate::services::agent::merope) mod settle;
pub(in crate::services::agent::merope) mod settle_raw;
pub(super) use settle::settle;
