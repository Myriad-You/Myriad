//! The contract's protocol-2 examples (`shared/her_link_fixtures.json`,
//! `next.frames`) all pass; the shared conformance cases
//! (`crates/conformance/v1/link/`) refuse what they name, with the code they
//! name. `myriad-core-ffi` runs the same cases through the C entry point.
//!
//! `MYRIAD_CONFORMANCE_BLESS=1 cargo test -p myriad-her-link` rewrites the
//! cases' `output`.

use std::{collections::BTreeSet, path::PathBuf};

use serde_json::{Value, json};

use super::*;

const FIXTURES_JSON: &str = include_str!("../../../shared/her_link_fixtures.json");

fn next_fixtures() -> Value {
    serde_json::from_str::<Value>(FIXTURES_JSON).unwrap()["next"].clone()
}

fn roles(of: &str) -> Vec<String> {
    match of {
        "device" => vec!["device".into()],
        "host" => vec!["device".into(), "host".into()],
        "mind" => vec!["device".into(), "host".into(), "mind".into()],
        "server" => vec!["host".into(), "server".into()],
        _ => Vec::new(),
    }
}

/// One `link.check` as the shared core answers it.
pub(crate) fn answer(input: &Value) -> Value {
    let roles: Vec<String> = input
        .get("roles")
        .map(|roles| serde_json::from_value(roles.clone()).unwrap())
        .unwrap_or_default();
    match check(&input["frame"], &roles) {
        Ok(checked) => json!({ "accepted": checked }),
        Err(refusal) => json!({ "refused": { "code": refusal.code } }),
    }
}

#[test]
fn the_contract_reads_and_agrees_with_these_rules() {
    let raw: Value = serde_json::from_str(CONTRACT_JSON).unwrap();
    assert_eq!(raw["schemaVersion"], 2);
    assert_eq!(raw["protocol"], 1, "no host speaks protocol 2 yet");
    assert_eq!(protocol(), 2);
    assert_eq!(raw["next"]["status"], "planned");
    assert_eq!(CONTRACT.next.limits.id_pattern, "^[A-Za-z0-9_.:-]{1,64}$");
    for id in ["t_1", "A.b:c-D_9", &"x".repeat(64)] {
        assert!(valid_id(id), "{id}");
    }
    for id in ["", "has space", "turn/1", "表", &"x".repeat(65)] {
        assert!(!valid_id(id), "{id}");
    }
    // Every frame's family is declared, and every family has frames.
    let declared: BTreeSet<&str> = families().keys().map(String::as_str).collect();
    let used: BTreeSet<&str> = CONTRACT
        .next
        .frames
        .keys()
        .map(|t| t.split('.').next().unwrap())
        .collect();
    assert_eq!(declared, used);
    for (t, spec) in &CONTRACT.next.frames {
        assert!(raw["deliveries"].get(&spec.delivery).is_some(), "{t}");
        assert!(
            raw["next"]["roles"].get(&spec.from).is_some() || spec.from == "either",
            "{t}"
        );
        for pair in &spec.one_of {
            for field in pair {
                assert!(spec.optional.contains(field), "{t}.{field}");
            }
        }
        for field in spec.sealed.iter().chain(spec.max_items.keys()) {
            assert!(
                spec.optional.contains(field) || spec.required.contains(field),
                "{t}.{field}"
            );
        }
        assert!(!CONTRACT.one.frames.contains_key(t), "{t} is protocol 1's");
    }
    // New close codes do not reuse protocol 1's numbers.
    let mut numbers = BTreeSet::new();
    for spec in raw["closeCodes"]
        .as_object()
        .unwrap()
        .values()
        .chain(raw["next"]["closeCodes"].as_object().unwrap().values())
    {
        let code = spec["code"].as_u64().unwrap();
        assert!((4000..5000).contains(&code));
        assert!(numbers.insert(code), "close code {code} twice");
    }
    assert_eq!(raw["next"]["wake"]["payload"], next_fixtures()["wake"]);
    assert_eq!(seal_info("fact.up"), "myriad-link/2 fact.up");
    assert_eq!(
        String::from_utf8(seal_aad("dev_phone", "fact.up", "dev_mac")).unwrap(),
        r#"{"from":"dev_phone","t":"fact.up","to":"dev_mac"}"#
    );
}

#[test]
fn every_protocol_two_example_passes_and_every_frame_has_one() {
    let mut seen = BTreeSet::new();
    for frame in next_fixtures()["frames"].as_array().unwrap() {
        let t = frame["t"].as_str().unwrap();
        let from = &CONTRACT.next.frames[t].from;
        let checked =
            check(frame, &roles(from)).unwrap_or_else(|refusal| panic!("{t}: {refusal:?}"));
        // Its one byte form reads back to itself.
        let again = check_bytes(checked.canonical.as_bytes(), &[]).unwrap();
        assert_eq!(again, checked);
        seen.insert(t.to_string());
    }
    let declared: BTreeSet<String> = CONTRACT.next.frames.keys().cloned().collect();
    assert_eq!(seen, declared, "one example per frame at least");
}

#[test]
fn a_frame_over_the_limit_is_too_large() {
    let mut frame = next_fixtures()["frames"][0].clone();
    frame["body"]["facts"][0]["data"]["label"] = json!("x".repeat(270_000));
    assert_eq!(check(&frame, &[]).unwrap_err().code, "too_large");
    let bytes = serde_json::to_vec(&frame).unwrap();
    assert_eq!(check_bytes(&bytes, &[]).unwrap_err().code, "too_large");
    assert_eq!(
        check_bytes(b"{not json", &[]).unwrap_err().code,
        "bad_frame"
    );
}

fn conformance() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../conformance/v1/link")
}

#[test]
fn every_link_case_answers_as_recorded() {
    let path = conformance().join("check.json");
    let mut file: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(file["call"], "link.check");
    let bless = std::env::var_os("MYRIAD_CONFORMANCE_BLESS").is_some();
    let mut codes = BTreeSet::new();
    for case in file["cases"].as_array_mut().unwrap() {
        let got = answer(&case["input"]);
        if let Some(code) = got["refused"]["code"].as_str() {
            codes.insert(code.to_string());
            // A refused case is named after its code.
            assert!(
                case["name"]
                    .as_str()
                    .unwrap()
                    .starts_with(&format!("{code}:")),
                "{}: {got}",
                case["name"]
            );
        }
        if bless {
            case["output"] = got;
        } else {
            assert_eq!(case["output"], got, "{}", case["name"]);
        }
    }
    if bless {
        let mut text = serde_json::to_string_pretty(&file).unwrap();
        text.push('\n');
        std::fs::write(&path, text).unwrap();
    }
    // Every code a frame can be refused with but too_large (tested above,
    // too big for a file) has a case.
    let all: BTreeSet<String> = CONTRACT
        .next
        .error_codes
        .keys()
        .filter(|code| {
            ![
                "too_large",
                "not_for_me",
                "not_holder",
                "stale_epoch",
                "clock_ahead",
            ]
            .contains(&code.as_str())
        })
        .cloned()
        .collect();
    assert_eq!(codes, all);
}
