//! Scheduling evaluation: whether what comes in reaches the right place,
//! once, in time. Not whether she answers well.
//!
//! Synthetic message streams play on a simulated clock against Work that
//! only takes time. A private chat goes through the channel's own rules:
//! what is read together (`chat::take_in`), where a line goes
//! (`works::route`, `works::act`), her lines as parsed (`delegate::split`),
//! parked questions (`decide_pending_reply`, `works::stamp`), labels
//! (`works::labelled`), and the chat's one lock, which a reply holds while
//! she writes it. A group goes through the group mind itself
//! (`myriad_merope::group::step`), answered by a small end. What the channel
//! keeps across a restart (each piece, its question and outbox and delivery
//! cursor, what came and was not answered, the claimed inbound ids) is kept
//! here the same way; the rest is dropped.
//!
//! Deterministic by default: her lines are scripted from what each message
//! means, so only scheduling is measured; it runs with the other tests.
//! `MYRIAD_SCHEDULING_LIVE=1 cargo test … scheduling_eval::live -- --ignored`
//! asks the configured chat model for her lines instead (read-only, as the
//! semantic evaluation does) and scores which piece she gives each message
//! to. A report is written only where `MYRIAD_SCHEDULING_REPORT` points.

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};

use myriad_agent_rules::channel::{
    PendingDecision, PendingKind, PendingOption, PendingPrompt, decide_pending_reply,
    format_pending_prompt,
};
use serde_json::{Value, json};

use super::works::{self, Act, Facts, Piece, Route, Standing};
use crate::services::agent::delegate::{self, HandOff};

/// She sees a message this soon while in talk.
const LOOK: f64 = 2.0;
/// Her reply takes this long; she holds the chat while she writes it.
const TURN: f64 = 6.0;
/// The recovery worker's tick: a piece's delivery is picked up again.
const RECOVER: f64 = 10.0;
/// A refused send is tried again after this.
const RETRY: f64 = 1.0;

fn ms(seconds: f64) -> i64 {
    (seconds * 1000.0).round() as i64
}

mod group;
mod play;
mod scenes;
mod sim;

use play::Run;
use scenes::*;
use sim::*;

// ------------------------------------------------------------ the report

fn percentile(values: &[f64], p: f64) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let rank = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
    sorted[rank]
}

fn add(total: &mut Tally, tally: &Tally) {
    total.messages += tally.messages;
    total.missed += tally.missed;
    total.duplicated += tally.duplicated;
    total.heard_again += tally.heard_again;
    total.results += tally.results;
    total.results_missed += tally.results_missed;
    total.results_duplicated += tally.results_duplicated;
    total.more_right += tally.more_right;
    total.more_wrong += tally.more_wrong;
    total.stops_right += tally.stops_right;
    total.stops_wrong += tally.stops_wrong;
    total.unlabelled_while_others += tally.unlabelled_while_others;
    total.placed += tally.placed;
    total.misplaced.extend(tally.misplaced.iter().cloned());
    total.latencies.extend(tally.latencies.iter().copied());
}

fn summary(total: &Tally) -> Value {
    json!({
        "messages": total.messages,
        "missed": total.missed,
        "duplicated": total.duplicated,
        "heardAgain": total.heard_again,
        "results": total.results,
        "resultsMissed": total.results_missed,
        "resultsDuplicated": total.results_duplicated,
        "moreRight": total.more_right,
        "moreWrong": total.more_wrong,
        "stopsRight": total.stops_right,
        "stopsWrong": total.stops_wrong,
        "unlabelledWhileOthers": total.unlabelled_while_others,
        "turnsPlaced": total.placed,
        "misplaced": total.misplaced,
        "resultP50": percentile(&total.latencies, 50.0),
        "resultP95": percentile(&total.latencies, 95.0),
    })
}

/// Every scene played with `her`, and the numbers: an IM private chat and
/// the web panel apart.
pub(super) fn evaluate(her: &mut dyn Her) -> Value {
    let mut scenes = Vec::new();
    let mut private = Tally::default();
    let mut panel = Tally::default();
    for (run, scene) in private_scenes().iter().enumerate() {
        let (tally, log) = Run::new(scene, run).play(her);
        add(
            if scene.panel {
                &mut panel
            } else {
                &mut private
            },
            &tally,
        );
        scenes.push(json!({ "name": scene.name, "tally": tally, "log": log }));
    }
    let mut group_latencies = Vec::new();
    let mut groups = Vec::new();
    let mut group_total = GroupTally::default();
    for scene in group_scenes() {
        let (tally, log) = group::play(&scene);
        group_latencies.extend(tally.latencies.iter().copied());
        group_total.called += tally.called;
        group_total.missed += tally.missed;
        group_total.duplicated += tally.duplicated;
        group_total.again_ignored += tally.again_ignored;
        groups.push(json!({ "name": scene.name, "tally": tally, "log": log }));
    }
    json!({
        "private": summary(&private),
        "panel": summary(&panel),
        "group": {
            "called": group_total.called,
            "missed": group_total.missed,
            "duplicated": group_total.duplicated,
            "againIgnored": group_total.again_ignored,
            "replyP50": percentile(&group_latencies, 50.0),
            "replyP95": percentile(&group_latencies, 95.0),
        },
        "scenes": scenes,
        "groups": groups,
    })
}

fn write_report(report: &Value) {
    if let Ok(path) = std::env::var("MYRIAD_SCHEDULING_REPORT") {
        std::fs::write(&path, serde_json::to_string_pretty(report).unwrap()).unwrap();
        eprintln!("scheduling report written to {path}");
    }
}

#[test]
fn scheduling_holds_with_scripted_judgment() {
    let report = evaluate(&mut Scripted);
    write_report(&report);
    let private = &report["private"];
    let panel = &report["panel"];
    let group = &report["group"];
    eprintln!(
        "private {}\npanel {}\ngroup {}",
        serde_json::to_string(private).unwrap(),
        serde_json::to_string(panel).unwrap(),
        serde_json::to_string(group).unwrap()
    );
    for chat in [private, panel] {
        assert_eq!(chat["missed"], 0, "{report:#}");
        assert_eq!(chat["duplicated"], 0, "{report:#}");
        assert_eq!(chat["heardAgain"], 0, "{report:#}");
        assert_eq!(chat["resultsMissed"], 0, "{report:#}");
        assert_eq!(chat["resultsDuplicated"], 0, "{report:#}");
        assert_eq!(chat["moreWrong"], 0, "{report:#}");
        assert_eq!(chat["stopsWrong"], 0, "{report:#}");
        assert_eq!(chat["unlabelledWhileOthers"], 0, "{report:#}");
    }
    assert!(private["moreRight"].as_u64().unwrap() >= 4);
    assert_eq!(private["stopsRight"], 1);
    assert_eq!(panel["moreRight"], 5, "{report:#}");
    assert_eq!(panel["stopsRight"], 1, "{report:#}");
    assert_eq!(group["missed"], 0, "{report:#}");
    assert_eq!(group["duplicated"], 0, "{report:#}");
}

// ------------------------------------------------------------ live

/// Her lines from the configured chat model, given what she sees of the
/// pieces as the channel would show it.
struct Live {
    analyzer: std::sync::Arc<crate::services::analyzer::AiAnalyzer>,
    soul: String,
    runtime: tokio::runtime::Handle,
    history: Vec<String>,
}

impl Her for Live {
    fn lines(&mut self, read: &[&Said], pieces: &[Piece], waiting: usize) -> String {
        let section = delegate::section(&crate::services::agent::types::ChannelChat {
            works: works::shown(pieces.to_vec(), 0)
                .iter()
                .map(|piece| works::line(piece, 0))
                .collect(),
            waiting,
            came_while_away: None,
            ..Default::default()
        });
        let system = format!(
            "{}\n\nYou are talking with someone in a private chat app conversation. Reply in their language, briefly.\n\n{section}",
            self.soul
        );
        let input = read
            .iter()
            .map(|said| said.text)
            .collect::<Vec<_>>()
            .join("\n");
        let mut context = self.history.join("\n");
        if !context.is_empty() {
            context = format!("Earlier in this chat:\n{context}\n\nThey now say:\n");
        }
        let analyzer = self.analyzer.clone();
        let prompt = format!("{context}{input}");
        let raw = tokio::task::block_in_place(|| {
            self.runtime
                .block_on(async { analyzer.analyze_with_system(&system, &prompt).await })
        })
        .unwrap_or_default();
        self.history.push(format!("them: {input}"));
        self.history
            .push(format!("you: {}", delegate::split(&raw).0));
        raw
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "asks the configured chat model; MYRIAD_SCHEDULING_LIVE=1"]
async fn live() {
    if std::env::var("MYRIAD_SCHEDULING_LIVE").as_deref() != Ok("1") {
        eprintln!("live: set MYRIAD_SCHEDULING_LIVE=1");
        return;
    }
    let mut soul = crate::services::agent::semantic_eval::configured_soul().await;
    if soul.is_empty() {
        soul = "你是她。".into();
    }
    let analyzer = std::sync::Arc::new(
        crate::services::ai::create_strict_lite_ai_analyzer_with_timeout(Some(
            std::time::Duration::from_secs(60),
        ))
        .await
        .expect("configured Lite unavailable")
        .with_light_thinking(),
    );
    let runtime = tokio::runtime::Handle::current();
    let mut placed = 0;
    let mut turns = 0;
    let mut misplaced = Vec::new();
    let mut reports = Vec::new();
    let repeats: usize = std::env::var("MYRIAD_SCHEDULING_REPEAT")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1);
    for round in 0..repeats {
        for (run, scene) in private_scenes().iter().enumerate() {
            let mut her = Live {
                analyzer: analyzer.clone(),
                soul: soul.clone(),
                runtime: runtime.clone(),
                history: Vec::new(),
            };
            let (tally, log) = Run::new(scene, 1000 + round * 100 + run).play(&mut her);
            turns += tally.placed + tally.misplaced.len();
            placed += tally.placed;
            misplaced.extend(
                tally
                    .misplaced
                    .iter()
                    .map(|line| format!("{}: {line}", scene.name)),
            );
            reports.push(json!({ "name": scene.name, "tally": tally, "log": log }));
        }
    }
    let report = json!({
        "turns": turns,
        "placed": placed,
        "misplaced": misplaced,
        "scenes": reports,
    });
    write_report(&report);
    eprintln!(
        "live: {placed}/{turns} turns placed as meant\n{}",
        misplaced.join("\n")
    );
}
