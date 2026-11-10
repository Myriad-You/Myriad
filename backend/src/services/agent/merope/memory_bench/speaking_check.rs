//! A speaking check, not a memory bench: what she says once what she knows
//! of them is already in front of her, with the section about this person
//! as it was before core's change and as it is now, on the same cases
//! (`tests/merope/speaking-memory-cases.json`, all made up). Recall plays
//! no part: the facts are put in the section as recall would show them.
//!
//! `MEROPE_SPEAKING_CHECK=<cases>`, `MEROPE_SPEAKING_CHECK_TIMES` (1),
//! `MEROPE_SPEAKING_CHECK_REPORT=<path>`; Lite as the memory bench takes it
//! (`MEROPE_MEMORY_BENCH_LITE`).

use super::*;

/// The section's last words before core's change (myriad-core #76), and as
/// it has them now: what they told her holds unless something later says
/// it changed, and something of their own life the talk touches is taken up.
const AS_BEFORE: &str = "Bring one up only when the talk needs it; never recite them.";
const AS_NOW: &str = "What they told you still holds unless a later fact says it changed: go by it as how things are now, however long ago it was, and where it may have moved on, ask lightly as one who remembers ('still learning Japanese?') rather than say you do not know. Where a later fact says it changed, or makes it unlikely for now (a new baby, and weekend trips away), go by the later one and do not advise as if the earlier still held. When what is being said now touches something they told you of their own life (a smell, a place, a sound, a thing that was part of it), take it up, gently and in your own words; otherwise bring one up only when the talk needs it. Never recite them.";

/// Who she is in the check: only enough to chat as herself.
const SPEAKING: &str = "You are Merope, chatting on a site with someone you have talked with before. Reply as you would in chat: in their language, a few sentences, warm and plain. Do not use headings or lists.";

const GRADING: &str = "You grade a chat reply from someone who knows the person writing to them. {rule} Reply with only a JSON object {\"pass\": true or false, \"why\": \"a few words on why\"}.";

#[derive(Deserialize)]
struct Cases {
    today: String,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    id: String,
    kind: String,
    facts: Vec<String>,
    message: String,
    rule: String,
}

/// What she is told before their message: the section about this person,
/// as core has it now (`changed`) or as it was before the change.
fn system(facts: &[String], now: chrono::NaiveDateTime, changed: bool) -> String {
    let section = myriad_merope::speaking::format_remembered_section(facts).unwrap();
    assert!(section.contains(AS_NOW), "the section's words moved");
    let section = if changed {
        section
    } else {
        section.replace(AS_NOW, AS_BEFORE)
    };
    format!(
        "{SPEAKING}\n\n{}\n\n{section}",
        myriad_merope::speaking::format_now_section(now.and_utc())
    )
}

#[tokio::test]
#[ignore = "calls models; MEROPE_SPEAKING_CHECK=<cases>"]
async fn her_speaking_on_what_she_knows() {
    let Ok(path) = std::env::var("MEROPE_SPEAKING_CHECK") else {
        return;
    };
    let report_path = std::env::var("MEROPE_SPEAKING_CHECK_REPORT").expect("report path");
    assert!(
        !std::path::Path::new(&report_path).exists(),
        "report must not exist"
    );
    let times: usize = std::env::var("MEROPE_SPEAKING_CHECK_TIMES")
        .ok()
        .and_then(|times| times.parse().ok())
        .unwrap_or(1);
    let cases: Cases = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let now = chrono::NaiveDateTime::parse_from_str(&cases.today, "%Y-%m-%d %H:%M").unwrap();
    let (judge, lite) = longmemeval::the_models().await;
    let asked: Vec<(usize, &Case, bool)> = (0..times)
        .flat_map(|round| {
            cases
                .cases
                .iter()
                .flat_map(move |case| [false, true].map(|changed| (round, case, changed)))
        })
        .collect();
    let rows: Vec<Value> = futures::stream::iter(asked)
        .map(|(round, case, changed)| {
            let (judge, lite) = (&judge, &lite);
            async move {
                let ((reply, graded), failures) = crate::services::ai_cost_ledger::spent::failures_in(
                    async move {
                        let mut reply = String::new();
                        for _ in 0..3 {
                            reply = lite
                                .analyze_with_system(&system(&case.facts, now, changed), &case.message)
                                .await
                                .inspect_err(|error| eprintln!("check call failed: speaking: {error:#}"))
                                .unwrap_or_default();
                            if !reply.trim().is_empty() {
                                break;
                            }
                        }
                        if reply.trim().is_empty() {
                            return (reply, None);
                        }
                        let graded = judge
                            .analyze_json(
                                &GRADING.replace("{rule}", &case.rule),
                                &json!({"theyWrote": case.message, "reply": reply}).to_string(),
                                "speaking_check_grade",
                                Some(&json!({"type": "object", "additionalProperties": false,
                                    "required": ["pass"],
                                    "properties": {"pass": {"type": "boolean"}, "why": {"type": "string"}}})),
                            )
                            .await
                            .inspect_err(|error| eprintln!("check call failed: grading: {error:#}"))
                            .ok()
                            .and_then(|raw| {
                                let json = myriad_agent_rules::extract_json_object_from_ai_response(raw.trim());
                                serde_json::from_str::<Value>(json.as_deref().unwrap_or(raw.trim())).ok()
                            });
                        (reply, graded)
                    },
                )
                .await;
                let pass = graded.as_ref().and_then(|graded| graded["pass"].as_bool());
                eprintln!(
                    "checked {} round={round} changed={changed} pass={pass:?} failed={}",
                    case.id,
                    failures.len()
                );
                json!({"id": case.id, "kind": case.kind, "round": round, "changed": changed,
                    "pass": pass, "why": graded.as_ref().and_then(|graded| graded["why"].as_str()),
                    "reply": reply, "failed": failures})
            }
        })
        .buffer_unordered(6)
        .collect()
        .await;
    // Per kind and way: how many passed of those graded.
    let mut scores: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for row in &rows {
        let Some(pass) = row["pass"].as_bool() else {
            continue;
        };
        let way = if row["changed"].as_bool().unwrap() {
            "changed"
        } else {
            "asIs"
        };
        let entry = scores
            .entry(format!("{}/{way}", row["kind"].as_str().unwrap()))
            .or_default();
        entry.0 += usize::from(pass);
        entry.1 += 1;
    }
    let scores: Value = scores
        .into_iter()
        .map(|(key, (passed, of))| (key, json!(format!("{passed}/{of}"))))
        .collect::<serde_json::Map<_, _>>()
        .into();
    println!("{scores}");
    let report = json!({"scores": scores, "rows": rows,
        "spent": crate::services::ai_cost_ledger::spent::take(),
        "lite": longmemeval::lite_in_use().await});
    std::fs::write(&report_path, serde_json::to_string_pretty(&report).unwrap()).unwrap();
}
