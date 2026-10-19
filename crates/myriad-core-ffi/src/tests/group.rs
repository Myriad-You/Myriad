//! The group's mind through the C entry point, as another platform runs it:
//! every trajectory of `myriad-merope` (G1) replayed step by step, the
//! state each step hands back given to the next, as JSON only.

use serde_json::{Value, json};

use super::*;

pub(super) const TESTED: &[&str] = &[
    "group.kept_as_heard",
    "group.ledger_who",
    "group.merge_ledger",
    "group.offer",
    "group.step",
];

macro_rules! trajectory {
    ($name:literal) => {
        (
            $name,
            include_bytes!(concat!(
                "../../../myriad-merope/fixtures/group/",
                $name,
                ".json"
            ))
            .as_slice(),
        )
    };
}

const TRAJECTORIES: &[(&str, &[u8])] = &[
    trajectory!("a_member_calls_her"),
    trajectory!("an_outsider_calls_her"),
    trajectory!("her_line_brings_back_what_a_failed_read_missed"),
    trajectory!("lines_wait_while_she_is_busy"),
    trajectory!("muted_away_and_back"),
    trajectory!("pictures_and_what_she_heard"),
    trajectory!("read_back_after_a_restart"),
    trajectory!("she_brings_something_up_first_and_a_send_is_refused"),
    trajectory!("she_keeps_back_what_no_longer_fits"),
    trajectory!("she_says_nothing_when_what_she_has_cannot_be_read"),
    trajectory!("she_speaks_up_unasked_and_reacts"),
    trajectory!("undercover_goes_on_after_a_restart"),
    trajectory!("undercover_is_played_with_buttons"),
    trajectory!("undercover_she_fills_a_seat"),
    trajectory!("werewolf_goes_on_after_a_restart"),
    trajectory!("werewolf_is_played_with_buttons"),
    trajectory!("werewolf_she_fills_a_seat"),
    trajectory!("avalon_goes_on_after_a_restart"),
    trajectory!("avalon_is_played_with_buttons"),
    trajectory!("avalon_she_fills_a_seat"),
    trajectory!("jubensha_is_played_with_buttons"),
    trajectory!("jubensha_on_qq_a_tie_a_rewrite_and_a_wrong_vote"),
    trajectory!("jubensha_is_given_up_after_ten_minutes"),
    trajectory!("jubensha_stops_when_most_ask"),
    trajectory!("jubensha_goes_on_after_a_restart_while_she_writes"),
    trajectory!("jubensha_goes_on_after_a_restart_mid_game"),
];

#[test]
fn every_trajectory_replays_through_the_core() {
    for (name, bytes) in TRAJECTORIES {
        let trajectory: Value = serde_json::from_slice(bytes).unwrap();
        let mut state = Value::Null;
        let steps = trajectory["steps"].as_array().unwrap();
        assert!(!steps.is_empty(), "{name}");
        for (index, recorded) in steps.iter().enumerate() {
            let mut input = recorded["input"].clone();
            input["state"] = state.clone();
            let mut out = ok("group.step", input);
            state = out["state"].take();
            out.as_object_mut().unwrap().remove("state");
            assert_eq!(out, recorded["output"], "{name} step {index}");
        }
        assert_eq!(state, trajectory["state"], "{name}: the state it ends in");
    }
}

#[test]
fn a_whisper_with_no_game_on_is_not_taken() {
    let out = ok(
        "group.step",
        json!({
            "now": 1_791_957_600_000_000i64,
            "seed": 1,
            "her": { "today": "2026-10-14" },
            "event": { "kind": "whispered", "who": "telegram:11", "text": "1" },
        }),
    );
    assert_eq!(out["taken"], false);
    assert_eq!(out["effects"], json!([]));
}

#[test]
fn a_state_of_another_version_starts_over() {
    let out = ok(
        "group.step",
        json!({
            "state": { "version": 999, "busy": true },
            "now": 1_791_957_600_000_000i64,
            "seed": 1,
            "her": { "today": "2026-10-14" },
            "event": { "kind": "woke", "key": "g1" },
        }),
    );
    assert_eq!(out["state"]["version"], 1);
    assert_eq!(out["state"]["busy"], false);
    assert_eq!(out["effects"], json!([]));
}

#[test]
fn the_ledger_rules_an_end_keeps_it_by() {
    assert_eq!(
        ok("group.ledger_who", json!({ "from": "1" }))["who"],
        "6b86b273ff34"
    );
    let merged = ok(
        "group.merge_ledger",
        json!({
            "stored": null,
            "messages": [{ "at": 1, "by": "her", "chars": 2, "mark": false, "bang": false }],
            "waits": [3.5],
            "theirs": { "messages": 0, "pieces": {} },
            "hers": { "messages": 1, "pieces": { "whole:在": 1 } },
        }),
    );
    assert_eq!(merged["ledger"]["her_waits"], json!([3.5]));
    assert_eq!(merged["ledger"]["hers"]["messages"], 1);
    let past = ok(
        "group.kept_as_heard",
        json!({
            "platform": "telegram",
            "chat": "-1",
            "thread": 9,
            "lines": [{ "at": "2026-10-14T06:00:00Z", "message_id": "3", "name": "阿明",
                        "from": "11", "text": "在吗", "hers": false, "addressed": true }],
        }),
    );
    assert_eq!(past["past"][0]["message"]["thread"], 9);
    assert_eq!(past["past"][0]["at"], 1_791_957_600_000_000i64);
    assert_eq!(
        ok("group.offer", json!({ "state": {}, "now": 0 }))["offer"],
        Value::Null
    );
}

#[test]
fn a_step_that_does_not_parse_is_refused() {
    let (status, out) = ffi("group.step", &json!({ "now": 0 }));
    assert_eq!(status, STATUS_BAD_INPUT);
    assert_eq!(out["error"]["kind"], "bad_input");
}
