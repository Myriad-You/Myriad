//! Her real talk, gone over again in other ways, to see which keeps what a
//! friend would: line by line as production does now (their line, her
//! reply, her line before it); line by line with the talk so far in view; a
//! whole stretch of talk at once, once they have gone quiet; and line by line
//! into short notes that, once the stretch is over, are gone over for what
//! lasts (settle).
//!
//! Read only (the site's database), and it asks the judgment model once per
//! line or stretch. What each way keeps is its own: nothing is written, and
//! the facts one way keeps never reach another. Counted in code (calls,
//! facts proposed, facts whose evidence is not what they said, facts
//! replaced) and every fact kept is in the report beside the talk it came
//! from, for reading.
//!
//! `MEROPE_REMEMBER_REPLAY_REPORT=<new file> DATABASE_URL=… cargo test -p
//! myriad-backend --bin myriad-backend -- --ignored
//! her_talk_gone_over_three_ways --nocapture`; `MEROPE_REMEMBER_REPLAY_WAYS`
//! a comma list of `line`, `window`, `talk`, `settle` (default the first
//! three); `MEROPE_REMEMBER_REPLAY_NOTES=<earlier report>` has settle go
//! over the short notes that report took instead of taking its own.

use std::time::Duration;

use futures::StreamExt;
use sea_orm::{ConnectionTrait, DatabaseBackend, Statement};
use serde_json::{Value, json};

use myriad_merope::chat_remember::{
    EXTRACT_SCHEMA_NAME, Examples, extract_schema, extract_system_prompt, strip_json_fence,
};

/// Quiet this long, and a stretch of talk is over.
const QUIET: chrono::Duration = chrono::Duration::minutes(3);
/// Talk that never goes quiet is still gone over this often.
const AT_MOST: chrono::Duration = chrono::Duration::minutes(20);
/// Lines of the talk so far shown with each line, in the window way.
const WINDOW: usize = 12;
const LINE_CHARS: usize = 300;
const CALL_TIMEOUT: Duration = Duration::from_secs(40);

#[derive(Clone)]
struct Line {
    theirs: bool,
    text: String,
    at: chrono::DateTime<chrono::Utc>,
}

/// One person in one place, every line in order.
struct Stream {
    user_id: i32,
    venue: String,
    lines: Vec<Line>,
}

#[derive(Default)]
struct Tally {
    calls: usize,
    failed: usize,
    proposed: usize,
    not_what_they_said: usize,
    already_known: usize,
    replaced: usize,
    /// Short notes taken line by line, in the settle way.
    short_notes: usize,
}

fn clipped(text: &str) -> String {
    text.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .take(LINE_CHARS)
        .collect()
}

/// Production's prompt with one passage swapped, failing loudly if the
/// passage is no longer there (the comparison would silently drift).
fn swapped(prompt: String, swaps: &[(&str, &str)]) -> String {
    swaps.iter().fold(prompt, |prompt, (old, new)| {
        assert!(prompt.contains(old), "prompt changed: {old}");
        prompt.replacen(old, new, 1)
    })
}

const EVIDENCE_LINE: &str =
    "must be a contiguous verbatim excerpt from userText where they stated it. Do not cite reply.";

/// The window way: their line still, with the talk before it in view
/// (`theirs`, their words in it, for the examples' language).
fn window_prompt(known: &[String], theirs: &str) -> String {
    swapped(
        extract_system_prompt(known, Examples::for_words(theirs)),
        &[
            (
                "before is what you said just before their message: use it only to understand what userText answers (a short reply to your question), still taking the fact from userText.",
                "conversation is the talk just before userText, oldest first (who: them or you): use it to understand userText; a short line often completes something they said a moment ago, and the fact may then rest on their earlier lines too. Take facts only from what they stated, never from your lines.",
            ),
            (
                EVIDENCE_LINE,
                "must be verbatim from their words where they stated it (userText or their lines in conversation): one contiguous excerpt, or several from different lines joined with ' / '. Do not cite your lines.",
            ),
        ],
    )
}

/// The talk way: a whole stretch, once it is over (`theirs`, their words
/// in it, for the examples' language).
fn talk_prompt(known: &[String], theirs: &str) -> String {
    swapped(
        extract_system_prompt(known, Examples::for_words(theirs)),
        &[
            (
                "from this message of theirs.",
                "from this stretch of talk with them.",
            ),
            (
                "taken only from what they explicitly stated in userText;",
                "taken only from what they explicitly stated in their lines (who: them);",
            ),
            (
                "reply is context only; never treat your guesses as their facts. \
before is what you said just before their message: use it only to understand what userText answers (a short reply to your question), still taking the fact from userText.",
                "conversation is the stretch in order, oldest first; your lines (who: you) are context only, never treat your guesses as their facts. A fact is often spread over several of their short lines; put it together.",
            ),
            (
                EVIDENCE_LINE,
                "must be verbatim from their lines where they stated it: one contiguous excerpt, or several from different lines joined with ' / '. Do not cite your lines.",
            ),
            (
                "with evidence a contiguous verbatim excerpt from reply.",
                "with evidence a contiguous verbatim excerpt from your lines.",
            ),
            (
                "with evidence a contiguous verbatim excerpt from userText.",
                "with evidence a contiguous verbatim excerpt from their lines.",
            ),
            (
                "If inGame is true, userText is a move",
                "If inGame is true, their lines are moves",
            ),
        ],
    )
}

/// Whether `evidence` is what they said: each part (joined with ` / ` when
/// `parts` allows) verbatim in `theirs`.
fn what_they_said(evidence: &str, theirs: &str, parts: bool) -> bool {
    let evidence = evidence.trim();
    if evidence.chars().filter(|ch| ch.is_alphanumeric()).count() < 2 {
        return false;
    }
    if !parts {
        return theirs.contains(evidence);
    }
    evidence
        .split(" / ")
        .map(str::trim)
        .all(|part| !part.is_empty() && theirs.contains(part))
}

/// Keep what one answer gives: facts whose evidence is what they said, not
/// already known; what it replaces goes.
fn keep(
    raw: &str,
    theirs: &str,
    parts: bool,
    known: &mut Vec<String>,
    kept: &mut Vec<Value>,
    tally: &mut Tally,
    from: &Value,
) {
    let Ok(answer) = serde_json::from_str::<Value>(strip_json_fence(raw)) else {
        tally.failed += 1;
        return;
    };
    for old in answer["supersedes"].as_array().into_iter().flatten() {
        if let Some(old) = old.as_str()
            && let Some(at) = known.iter().position(|fact| fact == old)
        {
            known.remove(at);
            tally.replaced += 1;
            for fact in kept.iter_mut() {
                if fact["fact"] == old {
                    fact["replaced"] = json!(true);
                }
            }
        }
    }
    for item in answer["facts"].as_array().into_iter().flatten() {
        let (Some(fact), Some(evidence)) = (item["fact"].as_str(), item["evidence"].as_str())
        else {
            continue;
        };
        tally.proposed += 1;
        if !what_they_said(evidence, theirs, parts) {
            tally.not_what_they_said += 1;
            kept.push(
                json!({"fact": fact, "evidence": evidence, "rejected": "evidence", "from": from}),
            );
            continue;
        }
        if known.iter().any(|old| old == fact) {
            tally.already_known += 1;
            continue;
        }
        known.push(fact.to_string());
        kept.push(json!({"fact": fact, "evidence": evidence, "from": from}));
    }
}

fn today(at: chrono::DateTime<chrono::Utc>) -> String {
    at.with_timezone(&super::super::clock::zone())
        .format("%Y-%m-%d (%A)")
        .to_string()
}

fn talk_lines(lines: &[Line]) -> Vec<Value> {
    lines
        .iter()
        .map(|line| json!({"who": if line.theirs { "them" } else { "you" }, "text": clipped(&line.text)}))
        .collect()
}

/// Stretches of talk: a new one after a quiet spell, or once one has run
/// long without one.
fn stretches(lines: &[Line]) -> Vec<std::ops::Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for index in 1..=lines.len() {
        let over = index == lines.len()
            || lines[index].at - lines[index - 1].at > QUIET
            || lines[index].at - lines[start].at > AT_MOST;
        if over {
            out.push(start..index);
            start = index;
        }
    }
    out
}

async fn ask(
    analyzer: &crate::services::analyzer::AiAnalyzer,
    system: &str,
    input: &str,
    tally: &mut Tally,
) -> Option<String> {
    tally.calls += 1;
    let answer = tokio::time::timeout(
        CALL_TIMEOUT,
        analyzer.analyze_json(system, input, EXTRACT_SCHEMA_NAME, Some(&extract_schema())),
    )
    .await;
    match answer {
        Ok(Ok(raw)) => Some(raw),
        _ => {
            tally.failed += 1;
            None
        }
    }
}

/// One of their lines gone over as the line or window way asks: the raw
/// answer, their words its evidence must come from, and where it came from.
/// None for her lines, lines too slight to go over, and failed calls.
async fn line_ask(
    analyzer: &crate::services::analyzer::AiAnalyzer,
    stream: &Stream,
    index: usize,
    way: &str,
    known: &[String],
    tally: &mut Tally,
) -> Option<(String, String, Value)> {
    let line = &stream.lines[index];
    if !line.theirs || !super::should_extract_chat_remember(&line.text) {
        return None;
    }
    let reply = stream.lines[index + 1..]
        .iter()
        .take_while(|next| !next.theirs)
        .map(|next| next.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let before = stream.lines[..index].iter().rev().find(|line| !line.theirs);
    let from = json!({"at": line.at, "line": clipped(&line.text)});
    let (system, input, theirs) = if way == "window" {
        let earlier = &stream.lines[index.saturating_sub(WINDOW)..index];
        let theirs = earlier
            .iter()
            .chain(std::iter::once(line))
            .filter(|line| line.theirs)
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let input = json!({
            "userText": line.text,
            "reply": super::super::ingest::compact_summary(&reply),
            "conversation": talk_lines(earlier),
            "today": today(line.at),
        })
        .to_string();
        (window_prompt(known, &theirs), input, theirs)
    } else {
        let turn = super::super::TurnContext {
            before: before.map(|line| line.text.clone()),
            ..Default::default()
        };
        let input = super::extract_input(
            &line.text,
            &reply,
            &turn,
            line.at.with_timezone(&super::super::clock::zone()),
        );
        (
            extract_system_prompt(known, Examples::for_words(&line.text)),
            input,
            line.text.clone(),
        )
    };
    let raw = ask(analyzer, &system, &input, tally).await?;
    Some((raw, theirs, from))
}

/// `recorded`: short notes an earlier settle run took from this stream
/// (marked `short` in its report). Given, the settle way goes over them
/// instead of taking its own, so a change to how a stretch is gone over is
/// seen apart from how line extraction happened to go this time.
async fn go_over(
    analyzer: &crate::services::analyzer::AiAnalyzer,
    stream: &Stream,
    way: &str,
    recorded: Option<&[Value]>,
) -> (Tally, Vec<Value>) {
    let mut tally = Tally::default();
    let mut known: Vec<String> = Vec::new();
    let mut kept = Vec::new();
    match way {
        "talk" => {
            for range in stretches(&stream.lines) {
                let stretch = &stream.lines[range];
                let theirs: Vec<&str> = stretch
                    .iter()
                    .filter(|line| line.theirs)
                    .map(|line| line.text.as_str())
                    .collect();
                if theirs.is_empty() {
                    continue;
                }
                let theirs = theirs.join("\n");
                let input = json!({
                    "conversation": talk_lines(stretch),
                    "today": today(stretch[0].at),
                })
                .to_string();
                let from = json!({"at": stretch[0].at, "lines": stretch.len()});
                if let Some(raw) =
                    ask(analyzer, &talk_prompt(&known, &theirs), &input, &mut tally).await
                {
                    keep(
                        &raw, &theirs, true, &mut known, &mut kept, &mut tally, &from,
                    );
                }
            }
        }
        "settle" => {
            // Lasting facts are `known`/`kept`; what they said lately, the
            // short notes, are taken line by line and gone over once the
            // stretch is over.
            let mut lately: Vec<String> = Vec::new();
            for range in stretches(&stream.lines) {
                let mut notes: Vec<Value> = Vec::new();
                if let Some(recorded) = recorded {
                    let (first, last) =
                        (stream.lines[range.start].at, stream.lines[range.end - 1].at);
                    notes.extend(
                        recorded
                            .iter()
                            .filter(|note| {
                                note["from"]["at"]
                                    .as_str()
                                    .and_then(|at| chrono::DateTime::parse_from_rfc3339(at).ok())
                                    .is_some_and(|at| at >= first && at <= last)
                            })
                            .map(|note| {
                                let mut note = note.clone();
                                if let Some(map) = note.as_object_mut() {
                                    map.remove("short");
                                }
                                note
                            }),
                    );
                }
                for index in range.clone().filter(|_| recorded.is_none()) {
                    let mut seen: Vec<String> = known.iter().chain(&lately).cloned().collect();
                    let Some((raw, theirs, from)) =
                        line_ask(analyzer, stream, index, "line", &seen, &mut tally).await
                    else {
                        continue;
                    };
                    let before = notes.len();
                    keep(
                        &raw, &theirs, false, &mut seen, &mut notes, &mut tally, &from,
                    );
                    for note in &notes[before..] {
                        if note.get("rejected").is_none() {
                            lately.push(note["fact"].as_str().unwrap_or_default().to_string());
                        }
                    }
                }
                // The short notes go in the report too, marked, beside what lasts.
                kept.extend(notes.iter().cloned().map(|mut note| {
                    note["short"] = json!(true);
                    note
                }));
                let stretch = &stream.lines[range];
                let theirs: Vec<&str> = stretch
                    .iter()
                    .filter(|line| line.theirs)
                    .map(|line| line.text.as_str())
                    .collect();
                let fresh: Vec<&str> = notes
                    .iter()
                    .filter(|note| note.get("rejected").is_none())
                    .filter_map(|note| note["fact"].as_str())
                    .collect();
                if theirs.is_empty() || fresh.is_empty() {
                    continue;
                }
                let theirs = theirs.join("\n");
                let input = json!({
                    "conversation": talk_lines(stretch),
                    "lately": fresh,
                    "today": today(stretch[0].at),
                })
                .to_string();
                let from = json!({"at": stretch[0].at, "lines": stretch.len(), "lately": fresh});
                tally.calls += 1;
                let answer = tokio::time::timeout(
                    CALL_TIMEOUT,
                    analyzer.analyze_json(
                        &myriad_merope::chat_remember::settle_system_prompt(
                            &known,
                            Examples::for_words(&theirs),
                        ),
                        &input,
                        myriad_merope::chat_remember::SETTLE_SCHEMA_NAME,
                        Some(&myriad_merope::chat_remember::settle_schema()),
                    ),
                )
                .await;
                let Ok(Ok(raw)) = answer else {
                    tally.failed += 1;
                    continue;
                };
                // The settle answer in the extraction's shape, so one keeper
                // checks both.
                let Ok(answer) = serde_json::from_str::<Value>(strip_json_fence(&raw)) else {
                    tally.failed += 1;
                    continue;
                };
                let shaped = json!({
                    "facts": answer["lasting"],
                    "supersedes": answer["replaces"],
                });
                keep(
                    &shaped.to_string(),
                    &theirs,
                    true,
                    &mut known,
                    &mut kept,
                    &mut tally,
                    &from,
                );
                tally.short_notes += fresh.len();
            }
        }
        way => {
            for index in 0..stream.lines.len() {
                if let Some((raw, theirs, from)) =
                    line_ask(analyzer, stream, index, way, &known, &mut tally).await
                {
                    keep(
                        &raw,
                        &theirs,
                        way == "window",
                        &mut known,
                        &mut kept,
                        &mut tally,
                        &from,
                    );
                }
            }
        }
    }
    (tally, kept)
}

#[tokio::test]
#[ignore = "reads the site's database and asks its judgment model"]
async fn her_talk_gone_over_three_ways() {
    let db = crate::services::agent::semantic_eval::load_configured_lite().await;
    let ways: Vec<String> = std::env::var("MEROPE_REMEMBER_REPLAY_WAYS")
        .unwrap_or_else(|_| "line,window,talk".into())
        .split(',')
        .map(|way| way.trim().to_string())
        .filter(|way| matches!(way.as_str(), "line" | "window" | "talk" | "settle"))
        .collect();
    let rows = db
        .query_all_raw(Statement::from_string(
            DatabaseBackend::Postgres,
            "SELECT s.user_id, COALESCE(s.context->>'venue', 'private') AS venue, m.role, \
               m.content, m.created_at \
             FROM agent_messages m JOIN agent_sessions s ON s.id = m.session_id \
             WHERE s.context->>'mode' = 'chat' AND s.user_id > 0 \
               AND m.role IN ('user', 'assistant') \
             ORDER BY s.user_id, venue, m.created_at",
        ))
        .await
        .unwrap();
    let mut streams: Vec<Stream> = Vec::new();
    for row in &rows {
        let user_id: i32 = row.try_get("", "user_id").unwrap();
        let venue: String = row.try_get("", "venue").unwrap();
        let line = Line {
            theirs: row.try_get::<String>("", "role").unwrap() == "user",
            text: row.try_get("", "content").unwrap(),
            at: row
                .try_get::<chrono::DateTime<chrono::FixedOffset>>("", "created_at")
                .unwrap()
                .with_timezone(&chrono::Utc),
        };
        match streams.last_mut() {
            Some(stream) if stream.user_id == user_id && stream.venue == venue => {
                stream.lines.push(line)
            }
            _ => streams.push(Stream {
                user_id,
                venue,
                lines: vec![line],
            }),
        }
    }
    let analyzer =
        crate::services::ai::create_lite_judge_ai_analyzer_with_timeout(Some(CALL_TIMEOUT))
            .await
            .expect("judgment model");
    let model = crate::GLOBAL_DYNAMIC_CONFIG
        .read()
        .await
        .resolve_lite_judge_ai_config()
        .map(|resolved| resolved.model);
    let mut report = json!({"model": model, "ways": {}, "streams": []});
    // Short notes from an earlier settle report, gone over again.
    let recorded: Option<Value> = std::env::var("MEROPE_REMEMBER_REPLAY_NOTES")
        .ok()
        .map(|path| serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap());
    let mut per_stream: Vec<Value> = streams
        .iter()
        .map(|stream| {
            json!({
                "userId": stream.user_id,
                "venue": stream.venue,
                "theirLines": stream.lines.iter().filter(|line| line.theirs).count(),
                "talk": stream.lines.iter().map(|line| json!({
                    "who": if line.theirs { "them" } else { "her" },
                    "at": line.at,
                    "text": clipped(&line.text),
                })).collect::<Vec<_>>(),
                "kept": {},
            })
        })
        .collect();
    let analyzer = &analyzer;
    for way in &ways {
        let results: Vec<(Tally, Vec<Value>)> = futures::stream::iter(streams.iter())
            .map(|stream| {
                let notes = recorded
                    .iter()
                    .flat_map(|report| report["streams"].as_array().into_iter().flatten())
                    .find(|earlier| {
                        earlier["userId"] == json!(stream.user_id)
                            && earlier["venue"] == json!(stream.venue)
                    })
                    .and_then(|earlier| earlier["kept"]["settle"].as_array())
                    .map(|kept| {
                        kept.iter()
                            .filter(|note| note.get("short").is_some())
                            .cloned()
                            .collect::<Vec<_>>()
                    });
                async move { go_over(analyzer, stream, way, notes.as_deref()).await }
            })
            .buffered(4)
            .collect()
            .await;
        let mut total = Tally::default();
        let mut facts = 0;
        for (index, (tally, kept)) in results.into_iter().enumerate() {
            total.calls += tally.calls;
            total.failed += tally.failed;
            total.proposed += tally.proposed;
            total.not_what_they_said += tally.not_what_they_said;
            total.already_known += tally.already_known;
            total.replaced += tally.replaced;
            total.short_notes += tally.short_notes;
            facts += kept
                .iter()
                .filter(|fact| {
                    fact.get("rejected").is_none()
                        && fact.get("replaced").is_none()
                        && fact.get("short").is_none()
                })
                .count();
            per_stream[index]["kept"][way] = json!(kept);
        }
        let line = json!({
            "calls": total.calls,
            "failed": total.failed,
            "proposed": total.proposed,
            "notWhatTheySaid": total.not_what_they_said,
            "alreadyKnown": total.already_known,
            "replaced": total.replaced,
            "keptAtTheEnd": facts,
            "shortNotes": total.short_notes,
        });
        println!("{way}: {line}");
        report["ways"][way] = line;
    }
    report["streams"] = json!(per_stream);
    if let Ok(path) = std::env::var("MEROPE_REMEMBER_REPLAY_REPORT") {
        assert!(
            !std::path::Path::new(&path).exists(),
            "report exists: {path}"
        );
        std::fs::write(&path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
        println!("report: {path}");
    }
}

#[test]
fn both_ways_still_swap_into_productions_prompt() {
    let known = vec!["喜欢咖啡".to_string()];
    for prompt in [
        window_prompt(&known, "我不喝咖啡了"),
        talk_prompt(&known, "我不喝咖啡了"),
    ] {
        assert!(prompt.contains("' / '"));
        assert!(prompt.contains("- 喜欢咖啡"));
    }
    assert!(talk_prompt(&[], "我不喝咖啡了").contains("from this stretch of talk with them."));
}

#[test]
fn evidence_is_what_they_said_in_one_piece_or_several() {
    let theirs = "明天考试\n数学\n好紧张";
    assert!(what_they_said("明天考试", theirs, false));
    assert!(!what_they_said("明天考试 / 数学", theirs, false));
    assert!(what_they_said("明天考试 / 数学", theirs, true));
    assert!(!what_they_said("明天考物理", theirs, true));
    assert!(!what_they_said("明天考试 / ", theirs, true));
}
