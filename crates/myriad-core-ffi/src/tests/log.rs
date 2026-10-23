//! Her log across host cores: every case in the shared conformance files
//! (`crates/conformance/v1/log`, `…/lease`) through the C entry point, as
//! another platform runs them. `myriad-persona-log` runs the same files
//! through the crate and blesses them.

use std::path::PathBuf;

use myriad_persona_sync::canonical::canonical_bytes;
use serde_json::{Value, json};

use super::ffi;
use crate::{STATUS_BAD_INPUT, STATUS_OK};

pub(super) const TESTED: &[&str] = &[
    "lease.here",
    "lease.move",
    "lease.newer",
    "log.admit",
    "log.answer",
    "log.canonical",
    "log.clock_next",
    "log.clock_observe",
    "log.clock_offset",
    "log.clock_repair",
    "log.fold",
    "log.holdings",
    "log.merge",
    "log.missing",
];

fn conformance() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../conformance/v1")
}

#[test]
fn every_conformance_case_answers_through_the_c_entry_point() {
    let mut calls = std::collections::BTreeSet::new();
    let mut cases = 0;
    for area in ["log", "lease"] {
        let mut files: Vec<PathBuf> = std::fs::read_dir(conformance().join(area))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
        files.sort();
        for path in files {
            let file: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            let call = file["call"].as_str().unwrap();
            calls.insert(call.to_string());
            for case in file["cases"].as_array().unwrap() {
                let (status, out) = ffi(call, &case["input"]);
                let got = match status {
                    STATUS_OK => out,
                    STATUS_BAD_INPUT => json!({ "error": "bad_input" }),
                    other => panic!("{call}: status {other}: {out}"),
                };
                assert_eq!(
                    canonical_bytes(&got),
                    canonical_bytes(&case["output"]),
                    "{} › {}",
                    path.display(),
                    case["name"]
                );
                cases += 1;
            }
        }
    }
    assert_eq!(calls.into_iter().collect::<Vec<_>>(), TESTED);
    assert!(cases >= 40, "{cases}");
}
