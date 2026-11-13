//! The looks the site kept in the owner's ledger on 11-12, looked at again
//! the way she is asked now (spends on the site's model; see the module
//! docs of `trial`).

use myriad_nodes::session_look::Piece;

use super::*;

/// The looks the site kept in the owner's ledger on 11-12 after 15:27
/// (`agent_session_look`, exported as `[{at, look: {material, answers,
/// …}}]` to the file `MYRIAD_SESSION_LOOK_REPLAY` names: kept out of the
/// repository), each looked at again with the two facts it lacked: when the
/// owner last spoke to the session themselves (`owner_said`, minutes before
/// the look, by the look's time `HH:MM:SS`), and that its choices are not in
/// the material.
#[tokio::test]
#[ignore = "spends on the site's model; see the module docs"]
async fn the_ledger_looks_again_with_what_they_lacked() {
    let Ok(path) = std::env::var("MYRIAD_SESSION_LOOK_REPLAY") else {
        panic!("MYRIAD_SESSION_LOOK_REPLAY names the exported looks");
    };
    // When the owner last spoke to the video script's session, by look.
    let owner_said: std::collections::HashMap<&str, i64> = [
        ("15:59:17", 16),
        ("16:08:33", 0),
        ("16:09:09", 0),
        ("16:12:38", 4),
    ]
    .into_iter()
    .collect();
    let soul = crate::services::agent::semantic_eval::configured_soul().await;
    let looks_kept: Vec<Value> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let mut said = Vec::new();
    for kept in looks_kept {
        let at = kept["at"].as_str().unwrap_or_default().to_string();
        let Some(minutes) = owner_said.get(at.as_str()) else {
            continue;
        };
        println!("\n=== {at} (said then: {})", kept["look"]["answers"][0]);
        let material = with_what_it_lacked(
            kept["look"]["material"].as_str().unwrap_or_default(),
            *minutes,
        );
        let waits = vec![true; material.changes.len()];
        if let Some(answer) = ask_of(&soul, material, waits).await
            && let Some(line) = answer.line
        {
            said.push((at, line.text));
        }
    }
    println!("\n=== said: {said:?}");
}

/// The same four looks again, each with the card it puts before the owner
/// (`nodes.look_card`, in `MYRIAD_SESSION_LOOK_LANGUAGE`, zh-CN by
/// default) beside her line: the card is of what she judged, its facts the
/// session's as the device told them, her view her judgment's, and her
/// line, when she would say one, carries that card. Two calls a look at
/// most.
#[tokio::test]
#[ignore = "spends on the site's model; see the module docs"]
async fn the_four_looks_with_their_cards() {
    let path = std::env::var("MYRIAD_SESSION_LOOK_REPLAY").expect("the exported looks");
    let language =
        std::env::var("MYRIAD_SESSION_LOOK_LANGUAGE").unwrap_or_else(|_| "zh-CN".to_string());
    let owner_said: std::collections::HashMap<&str, i64> = [
        ("15:59:17", 16),
        ("16:08:33", 0),
        ("16:09:09", 0),
        ("16:12:38", 4),
    ]
    .into_iter()
    .collect();
    let soul = crate::services::agent::semantic_eval::configured_soul().await;
    let looks_kept: Vec<Value> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let mut shown = Vec::new();
    for kept in looks_kept {
        let at = kept["at"].as_str().unwrap_or_default().to_string();
        let Some(minutes) = owner_said.get(at.as_str()) else {
            continue;
        };
        println!("\n=== {at}");
        let material = with_what_it_lacked(
            kept["look"]["material"].as_str().unwrap_or_default(),
            *minutes,
        );
        let told = told_of(&material);
        let Some(answer) = ask_of(&soul, material.clone(), vec![true]).await else {
            shown.push((at, "no answer".to_string(), None));
            continue;
        };
        let Some(judgment) = answer.judgments[0].clone() else {
            shown.push((at, "no judgment".to_string(), None));
            continue;
        };
        let card = myriad_nodes::look_cards::decision(myriad_nodes::look_cards::DecisionInput {
            task: "n_trial_video".into(),
            told,
            judgment: judgment.clone(),
            material,
            change: 1,
            at: 1_794_470_000_000,
            language: language.clone(),
        });
        match &card {
            Ok(card) => {
                myriad_nodes::cards::check_call(myriad_nodes::cards::CardCheckInput {
                    card: serde_json::to_value(card).unwrap(),
                })
                .expect("the wire takes it");
                println!("card: {}", serde_json::to_string_pretty(card).unwrap());
                println!(
                    "leans {:?}, answers {}, buttons {:?}",
                    judgment.leans,
                    judgment.answers,
                    card.actions
                        .iter()
                        .map(|action| action.label.as_str())
                        .collect::<Vec<_>>()
                );
                let kind_matches = matches!(
                    (judgment.verdict, card.kind.as_str()),
                    (Verdict::Decide, "decide") | (Verdict::Nod, "nod")
                );
                assert!(kind_matches, "{:?} {}", judgment.verdict, card.kind);
            }
            Err(error) => println!("no card: {error}"),
        }
        let line = answer.line.map(|line| {
            assert_eq!(line.change, 1, "her line is of the change the card is");
            line.text
        });
        println!("says (carrying the card): {line:?}");
        shown.push((at, format!("{:?}", judgment.verdict), line));
    }
    println!("\n=== the four: {shown:#?}");
}

/// The choices a question ends with, `(A / B)`, as the device tells them.
fn choices_in(asked: &str) -> Vec<String> {
    asked
        .trim_end()
        .strip_suffix(')')
        .and_then(|rest| rest.rsplit_once(" ("))
        .map(|(_, choices)| {
            choices
                .split(" / ")
                .map(str::trim)
                .filter(|choice| !choice.is_empty())
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_default()
}

/// The session a kept material was of, as the device told it: its title,
/// what the owner last told it and what it asked, waiting on the owner.
fn told_of(material: &Material) -> Told {
    let piece = |what: &str| {
        material
            .pieces
            .iter()
            .find(|piece| piece.change == Some(1) && piece.what == what)
            .map(|piece| piece.text.clone())
    };
    let title = piece("the session")
        .and_then(|text| text.split(" (").next().map(str::to_string))
        .unwrap_or_default();
    Told {
        device: DEVICE.into(),
        session: myriad_her_link::now::read_session(&json!({
            "grant": GRANT, "session": "s-video", "change": "asked",
            "occurredAt": 1_794_470_000_000_i64, "tool": "claude_code", "folder": "myriad",
            "title": title, "askedBy": "owner",
            "fromOwner": piece("what the owner last told it").unwrap_or_default(),
            "fromAgent": piece("what it last said or asked").unwrap_or_default(),
            "waitingOn": "owner",
            "choices": material.choices.first().cloned().unwrap_or_default(),
        }))
        .expect("a question"),
    }
}

/// One kept look (`MYRIAD_SESSION_LOOK_AT`, its time `HH:MM:SS`; the
/// owner spoke `MYRIAD_SESSION_LOOK_OWNER_SAID` minutes before it, 16 by
/// default) asked five times as she is asked now: for each, why the model
/// stopped, how many tokens it wrote, and whether what it gave was empty.
/// Five calls.
#[tokio::test]
#[ignore = "spends on the site's model; see the module docs"]
async fn one_look_five_times() {
    let path = std::env::var("MYRIAD_SESSION_LOOK_REPLAY").expect("the exported looks");
    let at = std::env::var("MYRIAD_SESSION_LOOK_AT").expect("which look, HH:MM:SS");
    let minutes: i64 = std::env::var("MYRIAD_SESSION_LOOK_OWNER_SAID")
        .ok()
        .and_then(|minutes| minutes.parse().ok())
        .unwrap_or(16);
    let looks_kept: Vec<Value> = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let kept = looks_kept
        .iter()
        .find(|kept| kept["at"] == at.as_str())
        .expect("that look is in the export");
    let material = with_what_it_lacked(
        kept["look"]["material"].as_str().unwrap_or_default(),
        minutes,
    );
    let soul = crate::services::agent::semantic_eval::configured_soul().await;
    let system = looks::system(&soul);
    let analyzer = model().await;
    let prompt = looks::prompt(&material);
    for time in 1..=5 {
        let (raw, reported) = crate::services::ai_cost_ledger::reporting(
            analyzer.analyze_json_strict(&system, &prompt, "session_look", &looks::schema()),
        )
        .await;
        let raw = raw.unwrap_or_else(|error| format!("error: {error:#}"));
        let answer: Value = serde_json::from_str(&raw).unwrap_or(Value::Null);
        let first = &answer["changes"][0];
        println!(
            "{time}: stop={:?} output={:?} about_empty={} because_empty={} say={} read={:?}",
            reported.as_ref().map(|reported| reported.stops.clone()),
            reported.as_ref().map(|reported| reported.usage.output),
            first["about"]
                .as_str()
                .is_none_or(|about| about.trim().is_empty()),
            first["because"].as_array().is_none_or(Vec::is_empty),
            !answer["say"].is_null(),
            match looks::read(&raw, &material) {
                Read::Good(_) => "good",
                Read::BadGrounds => "bad_grounds",
                Read::Unreadable => "unreadable",
            },
        );
        println!("{time} raw: {raw}");
    }
}

/// A kept material read back, with the owner's last word and that the
/// choices are not here added to its first change.
fn with_what_it_lacked(numbered: &str, minutes: i64) -> Material {
    let mut material = Material::default();
    let mut change = None;
    for line in numbered.lines() {
        if let Some(rest) = line.strip_prefix("Change ") {
            let heading = rest.split_once(": ").map_or("", |(_, heading)| heading);
            material.changes.push(heading.to_string());
            change = Some(material.changes.len());
            continue;
        }
        if line == "Around them:" {
            change = None;
            continue;
        }
        let Some((_, piece)) = line.split_once("] ") else {
            if let Some(last) = material.pieces.last_mut() {
                last.text.push('\n');
                last.text.push_str(line);
            }
            continue;
        };
        let (what, text) = piece.split_once(": ").unwrap_or((piece, ""));
        material.pieces.push(Piece {
            change,
            what: what.to_string(),
            text: text.to_string(),
            evidence: "session:replayed".into(),
        });
        if change == Some(1) && what == "what the owner last told it" {
            material.pieces.push(Piece {
                change,
                what: "when the owner last spoke to it themselves".into(),
                // In each of these the question came after the owner spoke
                // (the session's own history says so): they spoke `minutes`
                // and a half before the look, the question came as the look
                // waited its quiet.
                text: {
                    let at = 1_760_000_000_000_i64;
                    let asked = myriad_her_link::now::read_session(&serde_json::json!({
                        "grant": "g_0123456789", "session": "s-1", "change": "asked",
                        "occurredAt": at - 20_000, "tool": "claude_code", "folder": "myriad",
                    }))
                    .expect("a question");
                    looks::owner_said(at - minutes * 60_000 - 30_000, &asked, at)
                },
                evidence: "session:replayed".into(),
            });
        }
        if change == Some(1) && what == "what it last said or asked" {
            // The choices the device will tell (`now.choices`), as the
            // session gave them at the end of its question.
            let choices = choices_in(text);
            if !choices.is_empty() {
                material.pieces.push(Piece {
                    change,
                    what: "its choices".into(),
                    text: choices
                        .iter()
                        .enumerate()
                        .map(|(at, choice)| format!("choice {}: «{choice}»", at + 1))
                        .collect::<Vec<_>>()
                        .join("; "),
                    evidence: "session:replayed".into(),
                });
                material.choices = vec![choices];
            }
            material.pieces.push(Piece {
                change,
                what: "what its choices hold".into(),
                text: "only its question and the names of its choices come up from the device; what each choice is (written in a file, or earlier in its conversation with the owner) is not in this material".into(),
                evidence: "session:replayed".into(),
            });
        }
    }
    material
}
